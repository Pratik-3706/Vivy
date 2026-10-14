use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::watch;
use uuid::Uuid;

use crate::agent::permissions::{PermissionCategory, PermissionManager};
use crate::agent::search::execute_tavily_search;
use crate::agent::tools::get_agent_tools_schema;
use crate::agent::vision::analyze_screen_frame;
use crate::phone::commands::PhoneCommand;
use crate::phone::connection::PhoneManager;
use crate::security::ApiCredentials;
use crate::sessions::SessionStore;

const MAX_STEPS: usize = 40;
/// Screen snapshots are big; only the most recent ones stay in the prompt verbatim.
const KEEP_SCREEN_SNAPSHOTS: usize = 2;
/// Fewer UI elements than this means the accessibility tree is not much use
/// (games, video, canvas-drawn apps), so we fall back to looking at pixels.
const SPARSE_UI_THRESHOLD: usize = 4;
/// Optional request fields; a 4xx that names one means "retry without them".
const THINKING_FIELDS: &[&str] = &[
    "include_reasoning",
    "stream_options",
    "reasoning_effort",
    "extra_body",
    "thinking_config",
    "thinking_level",
    "include_thoughts",
];
/// Streamed tokens are batched into UI events at most this often.
const STREAM_FLUSH: Duration = Duration::from_millis(45);
/// A reply whose text arrived over at least this long was shown live, so the
/// UI shouldn't type it out a second time. Buffered replies land within ~0.2s.
const LIVE_STREAM_SPREAD: Duration = Duration::from_millis(600);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentActionLog {
    pub id: String,
    pub timestamp: u64,
    pub action_type: String, // "thought", "reasoning", "tool_call", "tool_result", "permission_request", "finished", "error"
    pub description: String,
    pub details: Option<Value>,
}

struct ToolCall {
    id: String,
    name: String,
    args: Value,
    /// Provider data that must be sent back with the call (Gemini's thought
    /// signature lives here). Null for most providers.
    extra: Value,
}

enum ToolOutcome {
    Continue(String),
    Finish(String),
}

pub struct AgentSession {
    app: AppHandle,
    phone: PhoneManager,
    permissions: PermissionManager,
    creds: ApiCredentials,
    client: reqwest::Client,
    pause_rx: watch::Receiver<bool>,
    /// Turned off for the rest of the task if the provider rejects `stream: true`.
    streaming: AtomicBool,
    /// Thinking fields (reasoning, effort, thought summaries); turned off if the provider rejects them.
    extras: AtomicBool,
    /// Whether the last reply really arrived piece by piece (the user already
    /// watched it), as opposed to all at once after a silence.
    live: AtomicBool,
    /// Short record of what Matsumoto did this task, kept for the conversation memory.
    actions: std::sync::Mutex<Vec<String>>,
    /// The chat this task belongs to; every activity entry is saved there.
    store: std::sync::Arc<SessionStore>,
}

/// Show an activity entry in the UI and save it in the active chat. Returns its id.
pub fn record_log(app: &AppHandle, store: &SessionStore, action_type: &str, description: &str, details: Option<Value>) -> String {
    let log = AgentActionLog {
        id: Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().timestamp_millis() as u64,
        action_type: action_type.to_string(),
        description: description.to_string(),
        details,
    };
    if let Ok(v) = serde_json::to_value(&log) {
        store.append_log(v);
    }
    let _ = app.emit("agent-action", &log);
    log.id
}

impl AgentSession {
    pub fn new(
        app: AppHandle,
        phone: PhoneManager,
        permissions: PermissionManager,
        creds: ApiCredentials,
        pause_rx: watch::Receiver<bool>,
        store: std::sync::Arc<SessionStore>,
    ) -> Self {
        // No total timeout: a streamed reply can legitimately take a while.
        // read_timeout catches a stalled connection instead.
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(90))
            .build()
            .unwrap_or_default();
        Self {
            app,
            phone,
            permissions,
            creds,
            client,
            pause_rx,
            streaming: AtomicBool::new(true),
            extras: AtomicBool::new(true),
            live: AtomicBool::new(false),
            actions: std::sync::Mutex::new(Vec::new()),
            store,
        }
    }

    /// One-line summaries of the tool calls made so far, e.g. `open_app {"app_name":"Google"}`.
    pub fn actions(&self) -> Vec<String> {
        self.actions.lock().map(|a| a.clone()).unwrap_or_default()
    }

    fn emit_log(&self, action_type: &str, description: &str, details: Option<Value>) {
        record_log(&self.app, &self.store, action_type, description, details);
    }

    fn emit_stream(&self, step: usize, kind: &str, text: &str) {
        let _ = self.app.emit("agent-stream", json!({ "step": step, "kind": kind, "text": text }));
    }

    /// Blocks while the user has the task paused. Returns true if it waited.
    async fn wait_if_paused(&self) -> bool {
        let mut rx = self.pause_rx.clone();
        if !*rx.borrow_and_update() {
            return false;
        }
        while *rx.borrow_and_update() {
            if rx.changed().await.is_err() {
                break;
            }
        }
        true
    }

    /// `summary` is the compacted memory of older turns; `history` holds the
    /// recent turns of this chat (user/assistant pairs).
    pub async fn run_task(
        &self,
        user_prompt: String,
        summary: Option<String>,
        history: Vec<Value>,
    ) -> Result<String, String> {
        if self.creds.api_key().is_empty() {
            return Err("No API key set. Add one in Settings.".to_string());
        }

        let device_line = if self.phone.is_connected().await {
            let d = self.phone.get_device_state().await;
            let mut line = format!(
                "Connected phone: {} {} (Android {}), screen {}x{} px, battery {}%.",
                d.brand,
                d.model,
                if d.android_version.is_empty() { "?" } else { &d.android_version },
                d.screen_width,
                d.screen_height,
                d.battery_level,
            );
            if d.accessibility == Some(false) {
                line.push_str(
                    "\nIMPORTANT: Accessibility for Vivy is OFF on the phone right now, so no phone tool will work \
                     (no tapping, typing, reading the screen or opening apps). Don't try. Tell the user to turn it \
                     on in Settings > Accessibility > Vivy on the phone, then try again.",
                );
            }
            line
        } else {
            "No phone is connected right now. You can still talk with the user, but phone tools will fail; \
             if they ask for a phone task, tell them to connect the phone first."
                .to_string()
        };

        let system_prompt = format!(
            "You are Vivy, an AI assistant living in the Vivy desktop app on the user's PC. You can see and \
             operate the user's Android phone. Your name is a nod to the anime songstress AI Vivy.\n\
             You do the thinking and deciding. Your hands are Matsumoto: a small, white, cube-shaped helper \
             program (named after the cube AI from the same anime) who runs your tools on the phone and reports \
             back. In the app, your messages appear under \"Vivy\" and every tool call appears under \
             \"Matsumoto\". If the user asks about you or Matsumoto, answer from this description. They are not \
             things to look for on the phone.\n\n\
             {device_line}\n\n\
             Not every message is a phone task. If the user is chatting, asking about you, or asking something \
             you can answer from your own knowledge, just reply in plain text without calling any tools. Use \
             tools only when the request needs something done or checked on the phone, or web_search for facts \
             you need.\n\n\
             For phone tasks:\n\
             - Before acting, call inspect_screen. It lists what is on screen with each element's center as \
               percentages of the screen (x from the left edge, y from the top edge, 0-100).\n\
             - Prefer tap_element with an element's visible text or description. Use tap(x, y) with coordinates \
               from the most recent inspect_screen only when the element has no text.\n\
             - To launch an app call open_app with its name; add the package name if you are sure of it.\n\
             - After anything that changes the screen, call inspect_screen again before acting on the new screen. \
               Never reuse coordinates from an older screen.\n\
             - Make one tool call at a time and wait for its result.\n\
             - To enter text, tap the input field first, then call type_text. Submit with press_key ENTER.\n\
             - If the same action fails twice, change approach (scroll, go back, search, or use another route).\n\
             - When the goal is done, or clearly cannot be done, call finish_task with a short summary written \
               to the user.\n\n\
             Privacy rules (mandatory):\n\
             - Before opening or reading photos/gallery, files, messages/chats, or email, call ask_permission with \
               the matching category (photos, files, messages, mails).\n\
             - Before deleting anything call ask_permission with category destructive. Before any purchase, \
               payment or money transfer call ask_permission with category financial. These are asked every time.\n\
             - If permission is denied, do not do it; call finish_task and explain."
        );
        let system_prompt = match summary {
            Some(s) if !s.trim().is_empty() => format!(
                "{system_prompt}\n\nSummary of the earlier part of this conversation (older messages were \
                 condensed to save space):\n{s}"
            ),
            _ => system_prompt,
        };

        let mut messages: Vec<Value> = vec![json!({ "role": "system", "content": system_prompt })];
        messages.extend(history);
        messages.push(json!({ "role": "user", "content": user_prompt }));
        let tools = get_agent_tools_schema();
        let endpoint = format!("{}/chat/completions", self.creds.api_base());
        let mut snapshot_indices: Vec<usize> = Vec::new();
        let mut empty_replies = 0;
        let mut tools_used = 0usize;
        let mut resumed = false;

        for step in 1..=MAX_STEPS {
            // Messages can only be appended between steps (never between a tool call and its result).
            if self.wait_if_paused().await || resumed {
                resumed = false;
                messages.push(json!({
                    "role": "user",
                    "content": "(I paused you and may have used the phone myself. Check the screen again before continuing.)"
                }));
            }

            let body = json!({
                "model": self.creds.model_name,
                "messages": messages,
                "tools": tools,
                "tool_choice": "auto",
                "temperature": 0.2
            });

            let started = Instant::now();
            self.live.store(false, Ordering::Relaxed);
            let message = match self.call_llm(&endpoint, &body, step).await {
                Ok(m) => m,
                Err(e) => {
                    self.emit_log("error", &e, None);
                    return Err(e);
                }
            };
            let think_ms = started.elapsed().as_millis() as u64;
            let live = self.live.load(Ordering::Relaxed);

            let (tag_reasoning, content) = split_think_tags(&message_text(&message));
            let mut reasoning = reasoning_texts(&message).join("");
            if reasoning.trim().is_empty() {
                reasoning = tag_reasoning;
            }
            let reasoning = reasoning.trim().to_string();
            if !reasoning.is_empty() {
                self.emit_log("reasoning", &reasoning, Some(json!({ "step": step, "ms": think_ms, "live": live })));
            }

            let calls = parse_tool_calls(&message);

            // Re-serialize a clean assistant message: some gateways reject the
            // extra fields (reasoning, refusal, etc.) that others return.
            let mut assistant = json!({
                "role": "assistant",
                "content": if content.is_empty() { Value::Null } else { Value::String(content.clone()) },
            });
            if !calls.is_empty() {
                assistant["tool_calls"] = Value::Array(
                    calls
                        .iter()
                        .map(|c| {
                            let mut call = json!({
                                "id": c.id,
                                "type": "function",
                                "function": { "name": c.name, "arguments": c.args.to_string() }
                            });
                            if !c.extra.is_null() {
                                call["extra_content"] = c.extra.clone();
                            }
                            call
                        })
                        .collect(),
                );
            }
            messages.push(assistant);

            if calls.is_empty() {
                if !content.is_empty() {
                    // A plain answer with no phone work is a conversational reply, not a finished task.
                    self.emit_log("finished", &content, Some(json!({ "reply": tools_used == 0, "live": live })));
                    return Ok(content);
                }
                // Empty reply with no tool call: nudge the model a couple of times before giving up.
                empty_replies += 1;
                if empty_replies > 2 {
                    let msg = "The model stopped responding without finishing the task.".to_string();
                    self.emit_log("error", &msg, None);
                    return Err(msg);
                }
                messages.push(json!({
                    "role": "user",
                    "content": "Continue with the task by calling a tool, or call finish_task if it is done."
                }));
                continue;
            }

            if !content.is_empty() {
                self.emit_log("thought", &content, Some(json!({ "step": step, "live": live })));
            }

            for call in calls {
                if self.wait_if_paused().await {
                    resumed = true;
                }
                tools_used += 1;
                if let Ok(mut actions) = self.actions.lock() {
                    let args = call.args.to_string();
                    let args = if args == "{}" { String::new() } else { format!(" {}", truncate(&args, 120)) };
                    actions.push(format!("{}{}", call.name, args));
                }
                self.emit_log(
                    "tool_call",
                    &call.name,
                    Some(json!({ "tool": call.name, "arguments": call.args, "call_id": call.id, "step": step })),
                );

                let started = Instant::now();
                let (result_text, ok, finished) = match self.execute_tool(&call.name, &call.args).await {
                    Ok(ToolOutcome::Continue(text)) => (text, true, false),
                    Ok(ToolOutcome::Finish(text)) => (text, true, true),
                    // Hand failures back to the model so it can recover instead of aborting the run.
                    Err(err) => (format!("ERROR: {err}"), false, false),
                };

                self.emit_log(
                    "tool_result",
                    &result_text,
                    Some(json!({
                        "tool": call.name,
                        "call_id": call.id,
                        "ok": ok,
                        "ms": started.elapsed().as_millis() as u64,
                    })),
                );

                if call.name == "inspect_screen" && ok {
                    snapshot_indices.push(messages.len());
                }
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call.id,
                    "content": result_text,
                }));

                if finished {
                    self.emit_log("finished", &result_text, None);
                    return Ok(result_text);
                }
            }

            // Collapse old screen snapshots to keep the prompt small.
            if snapshot_indices.len() > KEEP_SCREEN_SNAPSHOTS {
                let stale = snapshot_indices.len() - KEEP_SCREEN_SNAPSHOTS;
                for idx in snapshot_indices.drain(..stale) {
                    messages[idx]["content"] =
                        Value::String("[Older screen snapshot removed. The screen has changed since.]".into());
                }
            }
        }

        let msg = format!("Stopped after {MAX_STEPS} steps without finishing. Try a more specific instruction.");
        self.emit_log("error", &msg, None);
        Err(msg)
    }

    /// POST to the chat completions endpoint, streaming tokens to the UI when the
    /// provider supports it. Retries rate limits, server errors and dropped connections.
    async fn call_llm(&self, endpoint: &str, body: &Value, step: usize) -> Result<Value, String> {
        let mut last_err = String::new();
        for attempt in 0..3u32 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(1200 * attempt as u64)).await;
            }

            let stream = self.streaming.load(Ordering::Relaxed);
            let extras = self.extras.load(Ordering::Relaxed);
            let mut request_body = body.clone();
            if stream {
                request_body["stream"] = json!(true);
            }
            if extras {
                // Most providers only show their thinking when asked.
                self.creds.add_thinking(&mut request_body, stream);
            }
            self.creds.fit_request(&mut request_body);

            let resp = match self
                .client
                .post(endpoint)
                .bearer_auth(self.creds.api_key())
                .header(
                    reqwest::header::ACCEPT,
                    if stream { "text/event-stream" } else { "application/json" },
                )
                .json(&request_body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    last_err = format!("Couldn't reach the model API: {e}");
                    if e.is_timeout() || e.is_connect() || e.is_request() {
                        continue;
                    }
                    return Err(last_err);
                }
            };

            let status = resp.status();
            if !status.is_success() {
                let text = resp.text().await.unwrap_or_default();
                let lower = text.to_lowercase();
                if extras
                    && status.is_client_error()
                    && status.as_u16() != 429
                    && THINKING_FIELDS.iter().any(|f| lower.contains(f))
                {
                    // Provider doesn't know these optional fields; retry without them.
                    self.extras.store(false, Ordering::Relaxed);
                    last_err = format!("Model API returned {status}: {}", truncate(&text, 300));
                    continue;
                }
                if stream && status.is_client_error() && status.as_u16() != 429 && text.to_lowercase().contains("stream") {
                    // Provider doesn't support streaming here; fall back to plain requests.
                    self.streaming.store(false, Ordering::Relaxed);
                    last_err = format!("Model API returned {status}: {}", truncate(&text, 300));
                    continue;
                }
                last_err = format!("Model API returned {status}: {}", truncate(&text, 500));
                if status.as_u16() == 429 || status.is_server_error() {
                    continue;
                }
                return Err(last_err);
            }

            // Don't trust Content-Type: some gateways stream SSE under another
            // label, others ignore `stream` and send plain JSON. Look at the bytes.
            match self.read_body(resp, step).await {
                Ok(message) => return Ok(message),
                Err(BodyError::Retry(e)) => {
                    last_err = e;
                    continue;
                }
                Err(BodyError::Fatal(e)) => return Err(e),
            }
        }
        Err(last_err)
    }

    /// Read the response body. SSE streams are forwarded to the UI token by
    /// token; a plain JSON body is parsed as a normal completion.
    async fn read_body(&self, resp: reqwest::Response, step: usize) -> Result<Value, BodyError> {
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("?")
            .to_string();
        let mut trace = StreamTrace::new(step, &content_type);
        let mut bytes = resp.bytes_stream();
        let mut acc = StreamAccumulator::default();
        let mut json_body: Option<Vec<u8>> = None;
        let mut pending_content = String::new();
        let mut pending_reasoning = String::new();
        let mut last_flush = Instant::now();
        let mut started = false;
        // When the first and latest text arrived; a real stream spreads out.
        let mut text_span: Option<(Instant, Instant)> = None;

        while let Some(chunk) = bytes.next().await {
            let chunk = chunk.map_err(|e| BodyError::Retry(format!("Model stream interrupted: {e}")))?;
            trace.chunk(&chunk);

            if let Some(buf) = json_body.as_mut() {
                buf.extend_from_slice(&chunk);
                continue;
            }
            if !started {
                // Decide the format from the first non-whitespace byte.
                match chunk.iter().find(|b| !b.is_ascii_whitespace()) {
                    None => continue,
                    Some(b'{') | Some(b'[') => {
                        json_body = Some(chunk.to_vec());
                        continue;
                    }
                    Some(_) => {
                        started = true;
                        self.emit_stream(step, "start", "");
                    }
                }
            }

            for (kind, text) in acc.push(&chunk).map_err(BodyError::Fatal)? {
                let now = Instant::now();
                text_span = Some((text_span.map_or(now, |(first, _)| first), now));
                if kind == DeltaKind::Reasoning {
                    pending_reasoning.push_str(&text);
                } else {
                    pending_content.push_str(&text);
                }
            }
            if last_flush.elapsed() >= STREAM_FLUSH {
                self.flush_stream(step, &mut pending_content, &mut pending_reasoning);
                last_flush = Instant::now();
            }
            if acc.done {
                break;
            }
        }

        if let Some(buf) = json_body {
            let text = String::from_utf8_lossy(&buf).to_string();
            let parsed: Value = serde_json::from_str(&text).map_err(|e| {
                BodyError::Fatal(format!("Model API sent invalid JSON ({e}): {}", truncate(&text, 200)))
            })?;
            if let Some(err) = parsed.get("error").filter(|e| !e.is_null()) {
                return Err(BodyError::Fatal(format!("Model API error: {err}")));
            }
            let message = parsed["choices"][0]["message"].clone();
            if message.is_null() {
                return Err(BodyError::Fatal(format!("Model API reply had no message: {}", truncate(&text, 300))));
            }
            return Ok(message);
        }

        // A final line without a trailing newline still counts.
        for (kind, text) in acc.finish().map_err(BodyError::Fatal)? {
            if kind == DeltaKind::Reasoning {
                pending_reasoning.push_str(&text);
            } else {
                pending_content.push_str(&text);
            }
        }
        self.flush_stream(step, &mut pending_content, &mut pending_reasoning);
        let spread = text_span.map_or(Duration::ZERO, |(first, last)| last - first);
        self.live.store(spread >= LIVE_STREAM_SPREAD, Ordering::Relaxed);
        if started {
            self.emit_stream(step, "end", "");
        }
        if !started && acc.is_empty() {
            return Err(BodyError::Retry("Model API sent an empty reply".into()));
        }
        Ok(acc.into_message())
    }

    fn flush_stream(&self, step: usize, content: &mut String, reasoning: &mut String) {
        if !reasoning.is_empty() {
            self.emit_stream(step, "reasoning", reasoning);
            reasoning.clear();
        }
        if !content.is_empty() {
            self.emit_stream(step, "content", content);
            content.clear();
        }
    }

    async fn execute_tool(&self, name: &str, args: &Value) -> Result<ToolOutcome, String> {
        let new_id = || Uuid::new_v4().to_string();
        let num = |key: &str| -> Result<f32, String> {
            args[key]
                .as_f64()
                .or_else(|| args[key].as_str().and_then(|s| s.trim().trim_end_matches('%').parse().ok()))
                .map(|v| v.clamp(0.0, 100.0) as f32)
                .ok_or_else(|| format!("Missing number argument '{key}'"))
        };
        let text = |key: &str| args[key].as_str().map(str::trim).unwrap_or("").to_string();

        match name {
            "inspect_screen" => self.inspect_screen(args).await.map(ToolOutcome::Continue),

            "tap_element" => {
                let query = text("text");
                if query.is_empty() {
                    return Err("tap_element needs a non-empty 'text'".into());
                }
                let data = self
                    .phone
                    .request(PhoneCommand::TapElement { id: new_id(), query: query.clone() }, 6)
                    .await?;
                settle(700).await;
                Ok(ToolOutcome::Continue(data.unwrap_or_else(|| format!("Tapped \"{query}\""))))
            }

            "tap" => {
                let (x, y) = (num("x")?, num("y")?);
                self.phone.execute_command(PhoneCommand::Tap { id: new_id(), x, y }, 6).await?;
                settle(700).await;
                let label = text("element_label");
                Ok(ToolOutcome::Continue(if label.is_empty() {
                    format!("Tapped ({x:.1}%, {y:.1}%)")
                } else {
                    format!("Tapped \"{label}\" at ({x:.1}%, {y:.1}%)")
                }))
            }

            "long_press" => {
                let (x, y) = (num("x")?, num("y")?);
                let duration_ms = args["duration_ms"].as_u64().unwrap_or(800);
                self.phone
                    .execute_command(PhoneCommand::LongPress { id: new_id(), x, y, duration_ms }, 6)
                    .await?;
                settle(700).await;
                Ok(ToolOutcome::Continue(format!("Long-pressed ({x:.1}%, {y:.1}%) for {duration_ms}ms")))
            }

            "swipe" => {
                let (x1, y1, x2, y2) = (num("x1")?, num("y1")?, num("x2")?, num("y2")?);
                let duration_ms = args["duration_ms"].as_u64().unwrap_or(300);
                self.phone
                    .execute_command(PhoneCommand::Swipe { id: new_id(), x1, y1, x2, y2, duration_ms }, 6)
                    .await?;
                settle(800).await;
                Ok(ToolOutcome::Continue(format!(
                    "Swiped ({x1:.1}%, {y1:.1}%) to ({x2:.1}%, {y2:.1}%)"
                )))
            }

            "scroll" => {
                let direction = text("direction").to_uppercase();
                let direction = if direction.is_empty() { "DOWN".to_string() } else { direction };
                self.phone
                    .execute_command(
                        PhoneCommand::Scroll { id: new_id(), direction: direction.clone(), distance: None },
                        6,
                    )
                    .await?;
                settle(800).await;
                Ok(ToolOutcome::Continue(format!("Scrolled {direction}")))
            }

            "type_text" => {
                let value = args["text"].as_str().unwrap_or("").to_string();
                self.phone
                    .execute_command(PhoneCommand::TypeText { id: new_id(), text: value.clone() }, 6)
                    .await?;
                settle(400).await;
                Ok(ToolOutcome::Continue(format!("Typed \"{value}\"")))
            }

            "press_key" => {
                let key = text("key").to_uppercase();
                let key = if key.is_empty() { "BACK".to_string() } else { key };
                self.phone
                    .execute_command(PhoneCommand::PressKey { id: new_id(), key: key.clone() }, 6)
                    .await?;
                settle(700).await;
                Ok(ToolOutcome::Continue(format!("Pressed {key}")))
            }

            "open_app" => {
                let app_name = text("app_name");
                let package_name = text("package_name");
                if app_name.is_empty() && package_name.is_empty() {
                    return Err("open_app needs 'app_name' or 'package_name'".into());
                }
                let data = self
                    .phone
                    .request(
                        PhoneCommand::OpenApp {
                            id: new_id(),
                            package_name: package_name.clone(),
                            app_name: if app_name.is_empty() { None } else { Some(app_name.clone()) },
                        },
                        8,
                    )
                    .await?;
                // Cold app starts take a moment before the first screen is useful.
                settle(2000).await;
                let shown = if app_name.is_empty() { &package_name } else { &app_name };
                Ok(ToolOutcome::Continue(data.unwrap_or_else(|| format!("Opened {shown}"))))
            }

            "wait" => {
                let seconds = args["seconds"].as_f64().unwrap_or(2.0).clamp(0.5, 10.0);
                tokio::time::sleep(Duration::from_secs_f64(seconds)).await;
                Ok(ToolOutcome::Continue(format!("Waited {seconds:.1}s")))
            }

            "web_search" => {
                let query = text("query");
                let res = execute_tavily_search(&self.creds.tavily_key, &query, 4).await?;
                let mut out = String::new();
                if let Some(ans) = res.answer {
                    out.push_str(&format!("Answer: {ans}\n\n"));
                }
                for (i, r) in res.results.iter().enumerate() {
                    out.push_str(&format!(
                        "{}. {}\n   {}\n   {}\n\n",
                        i + 1,
                        r.title,
                        r.url,
                        truncate(&r.content, 600)
                    ));
                }
                if out.is_empty() {
                    out = "No results.".into();
                }
                Ok(ToolOutcome::Continue(out))
            }

            "ask_permission" => {
                let category = match text("category").to_lowercase().as_str() {
                    "photos" => PermissionCategory::Photos,
                    "files" => PermissionCategory::Files,
                    "messages" => PermissionCategory::Messages,
                    "mails" | "mail" | "email" => PermissionCategory::Mails,
                    "destructive" => PermissionCategory::Destructive,
                    "financial" => PermissionCategory::Financial,
                    other => {
                        return Err(format!(
                            "Unknown category '{other}'. Use photos, files, messages, mails, destructive or financial."
                        ))
                    }
                };
                let reason = text("reason");
                let reason = if reason.is_empty() { "The agent needs your approval to continue.".into() } else { reason };

                self.emit_log(
                    "permission_request",
                    &reason,
                    Some(json!({ "category": category })),
                );
                let allowed = self.permissions.check_or_request(category.clone(), reason, None).await?;
                Ok(ToolOutcome::Continue(if allowed {
                    format!("Permission granted for {:?}.", category)
                } else {
                    format!("Permission DENIED for {:?}. Do not do this; finish the task and explain.", category)
                }))
            }

            "finish_task" => {
                let summary = text("summary");
                Ok(ToolOutcome::Finish(if summary.is_empty() { "Done.".into() } else { summary }))
            }

            other => Err(format!("Unknown tool '{other}'")),
        }
    }

    /// Accessibility tree first (exact, cheap); a vision pass only when asked
    /// for or when the tree doesn't describe the screen well.
    async fn inspect_screen(&self, args: &Value) -> Result<String, String> {
        let target = args["target"].as_str().map(str::trim).filter(|s| !s.is_empty());
        let want_visual = args["visual"].as_bool().unwrap_or(false);

        let ui = self
            .phone
            .request(PhoneCommand::DumpUi { id: Uuid::new_v4().to_string() }, 6)
            .await;

        let (ui_text, element_count) = match &ui {
            Ok(Some(dump)) => (dump.clone(), dump.lines().filter(|l| l.starts_with("- ")).count()),
            Ok(None) => (String::new(), 0),
            Err(e) => (format!("(Couldn't read UI elements: {e})"), 0),
        };

        let mut out = String::new();
        if !ui_text.is_empty() {
            out.push_str("Screen elements (center x%, y%):\n");
            out.push_str(&ui_text);
        }

        if want_visual || element_count < SPARSE_UI_THRESHOLD {
            match self.phone.get_latest_frame().await {
                Some(frame) => {
                    let model = if self.creds.vision_model.trim().is_empty() {
                        &self.creds.model_name
                    } else {
                        &self.creds.vision_model
                    };
                    match analyze_screen_frame(&self.creds, model, &frame, target).await {
                        Ok(analysis) => {
                            out.push_str("\n\nVisual analysis of the screenshot:\n");
                            out.push_str(&analysis);
                        }
                        Err(e) => out.push_str(&format!("\n\n(Visual analysis unavailable: {e})")),
                    }
                }
                None => out.push_str("\n\n(No screenshot available: screen sharing is off on the phone.)"),
            }
        }

        if out.trim().is_empty() {
            return Err("Couldn't read the screen. Make sure accessibility and screen sharing are on.".into());
        }
        if let Some(t) = target {
            out.push_str(&format!("\n\nYou were looking for: \"{t}\"."));
        }
        Ok(out)
    }
}

async fn settle(ms: u64) {
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

/// Message content may be a string or an array of typed parts depending on the provider.
fn message_text(message: &Value) -> String {
    match &message["content"] {
        Value::String(s) => s.trim().to_string(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string(),
        _ => String::new(),
    }
}

fn parse_tool_calls(message: &Value) -> Vec<ToolCall> {
    let Some(calls) = message["tool_calls"].as_array() else {
        return Vec::new();
    };
    calls
        .iter()
        .filter_map(|call| {
            let function = &call["function"];
            let name = function["name"].as_str()?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            let id = call["id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("call_{}", Uuid::new_v4().simple()));
            let args = match &function["arguments"] {
                Value::String(s) => parse_arguments(s),
                Value::Object(_) => function["arguments"].clone(),
                _ => json!({}),
            };
            Some(ToolCall { id, name, args, extra: call["extra_content"].clone() })
        })
        .collect()
}

/// Arguments arrive as a JSON string; tolerate empty strings and code fences.
fn parse_arguments(raw: &str) -> Value {
    let trimmed = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if trimmed.is_empty() {
        return json!({});
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(v @ Value::Object(_)) => v,
        // Some models double-encode: "{\"x\": 1}"
        Ok(Value::String(inner)) => serde_json::from_str(&inner).unwrap_or_else(|_| json!({})),
        _ => json!({}),
    }
}

enum BodyError {
    /// Worth retrying (dropped connection, empty reply).
    Retry(String),
    /// The provider answered with an error; retrying won't help.
    Fatal(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeltaKind {
    Content,
    Reasoning,
}

/// Incrementally parses an OpenAI-style SSE body and rebuilds the final
/// assistant message. Pure, so it can be tested without a network.
#[derive(Default)]
struct StreamAccumulator {
    raw: Vec<u8>,
    content: String,
    reasoning: String,
    // (id, name, arguments, extra_content) per tool-call index
    calls: Vec<(String, String, String, Value)>,
    done: bool,
    think: ThinkSplitter,
}

impl StreamAccumulator {
    fn push(&mut self, chunk: &[u8]) -> Result<Vec<(DeltaKind, String)>, String> {
        self.raw.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(pos) = self.raw.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.raw.drain(..=pos).collect();
            self.line(&String::from_utf8_lossy(&line), &mut out)?;
            if self.done {
                break;
            }
        }
        Ok(out)
    }

    fn finish(&mut self) -> Result<Vec<(DeltaKind, String)>, String> {
        let mut out = Vec::new();
        if !self.raw.is_empty() && !self.done {
            let rest = std::mem::take(&mut self.raw);
            self.line(&String::from_utf8_lossy(&rest), &mut out)?;
        }
        for (kind, text) in self.think.flush() {
            self.record(kind, text, &mut out);
        }
        Ok(out)
    }

    fn is_empty(&self) -> bool {
        self.content.is_empty() && self.reasoning.is_empty() && self.calls.is_empty()
    }

    fn line(&mut self, line: &str, out: &mut Vec<(DeltaKind, String)>) -> Result<(), String> {
        let line = line.trim();
        // Accept "data: {...}" and, from looser gateways, bare JSON lines.
        let data = line.strip_prefix("data:").map(str::trim).unwrap_or(line);
        if data.is_empty() || data.starts_with(':') || line.starts_with("event:") || line.starts_with("id:") {
            return Ok(());
        }
        if data == "[DONE]" {
            self.done = true;
            return Ok(());
        }
        let Ok(event) = serde_json::from_str::<Value>(data) else { return Ok(()) };
        if let Some(err) = event.get("error").filter(|e| !e.is_null()) {
            return Err(format!("Model API error: {err}"));
        }

        let choice = &event["choices"][0];
        // Most send `delta`; a few send the whole `message` in each chunk.
        let delta = if choice["delta"].is_object() { &choice["delta"] } else { &choice["message"] };

        if let Some(t) = delta["content"].as_str().filter(|t| !t.is_empty()) {
            for (kind, text) in self.think.push(t) {
                self.record(kind, text, out);
            }
        }
        for t in reasoning_texts(delta) {
            self.reasoning.push_str(&t);
            out.push((DeltaKind::Reasoning, t));
        }

        if let Some(tool_deltas) = delta["tool_calls"].as_array() {
            for tc in tool_deltas {
                let idx = tc["index"].as_u64().map(|i| i as usize).unwrap_or_else(|| {
                    if tc["id"].as_str().is_some_and(|s| !s.is_empty()) || self.calls.is_empty() {
                        self.calls.len()
                    } else {
                        self.calls.len() - 1
                    }
                });
                while self.calls.len() <= idx {
                    self.calls.push(Default::default());
                }
                let slot = &mut self.calls[idx];
                if let Some(id) = tc["id"].as_str().filter(|s| !s.is_empty()) {
                    slot.0 = id.to_string();
                }
                if let Some(name) = tc["function"]["name"].as_str() {
                    slot.1.push_str(name);
                }
                match &tc["function"]["arguments"] {
                    Value::String(s) => slot.2.push_str(s),
                    v @ Value::Object(_) => slot.2 = v.to_string(),
                    _ => {}
                }
                if tc["extra_content"].is_object() {
                    slot.3 = tc["extra_content"].clone();
                }
            }
        }
        Ok(())
    }

    fn record(&mut self, kind: DeltaKind, text: String, out: &mut Vec<(DeltaKind, String)>) {
        if text.is_empty() {
            return;
        }
        match kind {
            DeltaKind::Content => self.content.push_str(&text),
            DeltaKind::Reasoning => self.reasoning.push_str(&text),
        }
        out.push((kind, text));
    }

    fn into_message(self) -> Value {
        let mut message = json!({ "role": "assistant", "content": self.content });
        if !self.reasoning.is_empty() {
            message["reasoning_content"] = Value::String(self.reasoning);
        }
        let tool_calls: Vec<Value> = self
            .calls
            .into_iter()
            .filter(|(_, name, _, _)| !name.is_empty())
            .map(|(id, name, args, extra)| {
                let mut call = json!({ "id": id, "type": "function", "function": { "name": name, "arguments": args } });
                if !extra.is_null() {
                    call["extra_content"] = extra;
                }
                call
            })
            .collect();
        if !tool_calls.is_empty() {
            message["tool_calls"] = Value::Array(tool_calls);
        }
        message
    }
}

/// Reasoning arrives under different names depending on the provider.
fn reasoning_texts(delta: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for key in ["reasoning_content", "reasoning", "thinking", "thinking_content", "thought", "thoughts"] {
        if let Some(t) = delta[key].as_str().filter(|t| !t.is_empty()) {
            out.push(t.to_string());
        }
    }
    // OpenRouter-style: reasoning_details: [{ type: "reasoning.text", text: "..." }]
    // Only used when no plain field carried the same text.
    if out.is_empty() {
        if let Some(details) = delta["reasoning_details"].as_array() {
            for d in details {
                if let Some(t) = d["text"].as_str().or_else(|| d["summary"].as_str()).filter(|t| !t.is_empty()) {
                    out.push(t.to_string());
                }
            }
        }
    }
    out
}

/// Splits `<think>...</think>` (or Gemini-style `<thought>...</thought>`) out
/// of streamed content, chunk by chunk. A tag may be cut across chunks, so the
/// tail that could start a tag is held back.
#[derive(Default)]
struct ThinkSplitter {
    buf: String,
    inside: bool,
    /// Closing tag of the block we're inside.
    close: &'static str,
    /// Once a think block has closed (or real text arrived first), stop scanning.
    finished: bool,
}

impl ThinkSplitter {
    const TAGS: [(&'static str, &'static str); 2] = [("<think>", "</think>"), ("<thought>", "</thought>")];

    fn push(&mut self, text: &str) -> Vec<(DeltaKind, String)> {
        if self.finished {
            return vec![(DeltaKind::Content, text.to_string())];
        }
        self.buf.push_str(text);
        let mut out = Vec::new();
        loop {
            if !self.inside {
                let trimmed = self.buf.trim_start();
                if let Some(&(open, close)) = Self::TAGS.iter().find(|(open, _)| trimmed.starts_with(open)) {
                    let start = self.buf.len() - trimmed.len();
                    self.buf.drain(..start + open.len());
                    self.inside = true;
                    self.close = close;
                    continue;
                }
                if Self::TAGS.iter().any(|(open, _)| open.starts_with(trimmed)) {
                    break; // could still become an opening tag; wait for more
                }
                // Real text before any think block: no tags to look for.
                self.finished = true;
                out.push((DeltaKind::Content, std::mem::take(&mut self.buf)));
                break;
            }
            if let Some(pos) = self.buf.find(self.close) {
                out.push((DeltaKind::Reasoning, self.buf[..pos].to_string()));
                let rest = self.buf[pos + self.close.len()..].trim_start().to_string();
                self.buf.clear();
                self.inside = false;
                self.finished = true;
                if !rest.is_empty() {
                    out.push((DeltaKind::Content, rest));
                }
                break;
            }
            // Emit all but a tail that might be the start of the closing tag.
            let keep = self.close.len() - 1;
            if self.buf.len() > keep {
                let mut cut = self.buf.len() - keep;
                while !self.buf.is_char_boundary(cut) {
                    cut -= 1;
                }
                out.push((DeltaKind::Reasoning, self.buf[..cut].to_string()));
                self.buf.drain(..cut);
            }
            break;
        }
        out
    }

    fn flush(&mut self) -> Vec<(DeltaKind, String)> {
        if self.buf.is_empty() {
            return Vec::new();
        }
        let kind = if self.inside { DeltaKind::Reasoning } else { DeltaKind::Content };
        vec![(kind, std::mem::take(&mut self.buf))]
    }
}

/// Non-streamed version: returns (reasoning, content).
fn split_think_tags(text: &str) -> (String, String) {
    let mut splitter = ThinkSplitter::default();
    let (mut reasoning, mut content) = (String::new(), String::new());
    for (kind, t) in splitter.push(text).into_iter().chain(splitter.flush()) {
        match kind {
            DeltaKind::Reasoning => reasoning.push_str(&t),
            DeltaKind::Content => content.push_str(&t),
        }
    }
    (reasoning, content.trim().to_string())
}

/// Timing log for model replies, kept so streaming problems can be diagnosed:
/// %LOCALAPPDATA%\\Vivy\\stream-debug.log. Records when each chunk arrived and a
/// short preview of it. Never includes the API key or request headers.
struct StreamTrace {
    started: Instant,
    lines: Vec<String>,
}

impl StreamTrace {
    const MAX_CHUNKS: usize = 400;
    const MAX_FILE: u64 = 512 * 1024;

    fn new(step: usize, content_type: &str) -> Self {
        Self {
            started: Instant::now(),
            lines: vec![format!(
                "=== {} step {step} content-type: {content_type}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
            )],
        }
    }

    fn chunk(&mut self, bytes: &[u8]) {
        if self.lines.len() > Self::MAX_CHUNKS {
            return;
        }
        let preview: String = String::from_utf8_lossy(bytes)
            .chars()
            .take(160)
            .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
            .collect();
        self.lines.push(format!(
            "+{:>6}ms {:>6}B {preview}",
            self.started.elapsed().as_millis(),
            bytes.len()
        ));
    }
}

impl Drop for StreamTrace {
    fn drop(&mut self) {
        let Ok(dir) = std::env::var("LOCALAPPDATA") else { return };
        let dir = std::path::PathBuf::from(dir).join("Vivy");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("stream-debug.log");
        // Start over once the file gets big; only recent requests matter.
        let append = std::fs::metadata(&path).map(|m| m.len() < Self::MAX_FILE).unwrap_or(false);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(&path);
        if let Ok(mut f) = file {
            use std::io::Write;
            self.lines.push(format!("--- done after {}ms", self.started.elapsed().as_millis()));
            let _ = writeln!(f, "{}", self.lines.join("\n"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_string_and_object_arguments() {
        let msg = json!({
            "tool_calls": [
                { "id": "a", "function": { "name": "tap", "arguments": "{\"x\": 10, \"y\": 20}" } },
                { "function": { "name": "scroll", "arguments": { "direction": "UP" } } },
                { "id": "c", "function": { "name": "inspect_screen", "arguments": "" } },
                { "id": "d", "function": { "name": "tap", "arguments": "\"{\\\"x\\\": 1, \\\"y\\\": 2}\"" } }
            ]
        });
        let calls = parse_tool_calls(&msg);
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[0].args["x"], 10);
        assert!(calls[1].id.starts_with("call_"));
        assert_eq!(calls[1].args["direction"], "UP");
        assert_eq!(calls[2].args, json!({}));
        assert_eq!(calls[3].args["y"], 2);
    }

    #[test]
    fn reads_array_content() {
        let msg = json!({ "content": [{ "type": "text", "text": "hi" }] });
        assert_eq!(message_text(&msg), "hi");
        assert_eq!(message_text(&json!({ "content": null })), "");
    }

    #[test]
    fn streams_reasoning_content_and_tool_calls() {
        let mut acc = StreamAccumulator::default();
        let body = concat!(
            ": keep-alive\n",
            "data: {\"choices\":[{\"delta\":{\"reasoning\":\"Need \"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"reasoning_details\":[{\"type\":\"reasoning.text\",\"text\":\"chrome.\"}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Opening\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"open_app\",\"arguments\":\"{\\\"app_\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"name\\\":\\\"Chrome\\\"}\"}}]}}]}\n\n",
            "data: [DONE]\n\n"
        );
        // Feed in awkward slices to exercise line buffering.
        let mut deltas = Vec::new();
        for piece in body.as_bytes().chunks(7) {
            deltas.extend(acc.push(piece).unwrap());
        }
        assert!(acc.done);
        let reasoning: String = deltas.iter().filter(|d| d.0 == DeltaKind::Reasoning).map(|d| d.1.clone()).collect();
        assert_eq!(reasoning, "Need chrome.");
        let msg = acc.into_message();
        assert_eq!(msg["content"], "Opening");
        let calls = parse_tool_calls(&msg);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "open_app");
        assert_eq!(calls[0].args["app_name"], "Chrome");
    }

    #[test]
    fn surfaces_stream_errors() {
        let mut acc = StreamAccumulator::default();
        assert!(acc.push(b"data: {\"error\":{\"message\":\"bad key\"}}\n").is_err());
    }

    #[test]
    fn splits_think_tags_across_chunks() {
        let mut acc = StreamAccumulator::default();
        let mut deltas = Vec::new();
        for piece in ["data: {\"choices\":[{\"delta\":{\"content\":\"<thi\"}}]}\n",
                      "data: {\"choices\":[{\"delta\":{\"content\":\"nk>open chrome</th\"}}]}\n",
                      "data: {\"choices\":[{\"delta\":{\"content\":\"ink>Opening Chrome.\"}}]}\n",
                      "data: [DONE]\n"] {
            deltas.extend(acc.push(piece.as_bytes()).unwrap());
        }
        deltas.extend(acc.finish().unwrap());
        let reasoning: String = deltas.iter().filter(|d| d.0 == DeltaKind::Reasoning).map(|d| d.1.clone()).collect();
        assert_eq!(reasoning, "open chrome");
        let msg = acc.into_message();
        assert_eq!(msg["content"], "Opening Chrome.");
    }

    #[test]
    fn splits_gemini_thought_tags() {
        let (reasoning, content) = split_think_tags("<thought>check battery</thought>It is 80%.");
        assert_eq!(reasoning, "check battery");
        assert_eq!(content, "It is 80%.");
    }

    #[test]
    fn split_think_tags_plain() {
        assert_eq!(split_think_tags("<think>plan</think>\nHello"), ("plan".into(), "Hello".into()));
        assert_eq!(split_think_tags("Just text"), (String::new(), "Just text".into()));
    }
}
