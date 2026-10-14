pub mod agent;
pub mod phone;
pub mod security;
pub mod sessions;
pub mod state;

use std::sync::atomic::Ordering;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use uuid::Uuid;

use crate::agent::brain::{record_log, AgentSession};
use crate::agent::memory::{compact, Compaction};
use crate::sessions::{Session, SessionMeta};
use crate::agent::permissions::PermissionManager;
use crate::phone::commands::{DeviceState, PhoneCommand};
use crate::phone::connection::{start_websocket_server, PhoneManager};
use crate::security::{load_credentials_securely, save_credentials_securely};
use crate::state::{get_local_lan_ip, AppState};

#[derive(Serialize, Deserialize)]
pub struct SettingsView {
    pub provider: String,
    pub aicredits_key: String,
    pub openai_key: String,
    pub anthropic_key: String,
    pub google_key: String,
    pub custom_key: String,
    pub tavily_key: String,
    pub model_name: String,
    pub base_url: String,
    pub vision_model: String,
    pub reasoning_effort: String,
    /// A key is saved for the provider in use.
    pub has_key: bool,
    pub has_tavily_key: bool,
    pub ws_port: u16,
    pub local_ip: String,
}

#[derive(Serialize, Deserialize)]
pub struct SaveSettingsPayload {
    pub provider: Option<String>,
    pub aicredits_key: Option<String>,
    pub openai_key: Option<String>,
    pub anthropic_key: Option<String>,
    pub google_key: Option<String>,
    pub custom_key: Option<String>,
    pub tavily_key: Option<String>,
    pub model_name: Option<String>,
    pub base_url: Option<String>,
    pub vision_model: Option<String>,
    pub reasoning_effort: Option<String>,
}

#[tauri::command]
fn get_settings() -> SettingsView {
    let creds = load_credentials_securely();
    let local_ip = get_local_lan_ip();
    SettingsView {
        has_key: !creds.api_key().is_empty(),
        has_tavily_key: !creds.tavily_key.trim().is_empty(),
        provider: creds.provider,
        aicredits_key: creds.aicredits_key,
        openai_key: creds.openai_key,
        anthropic_key: creds.anthropic_key,
        google_key: creds.google_key,
        custom_key: creds.custom_key,
        tavily_key: creds.tavily_key,
        model_name: creds.model_name,
        base_url: creds.base_url,
        vision_model: creds.vision_model,
        reasoning_effort: creds.reasoning_effort,
        ws_port: 8765,
        local_ip,
    }
}

#[tauri::command]
fn save_settings(payload: SaveSettingsPayload) -> Result<(), String> {
    let mut c = load_credentials_securely();
    let SaveSettingsPayload {
        provider,
        aicredits_key,
        openai_key,
        anthropic_key,
        google_key,
        custom_key,
        tavily_key,
        model_name,
        base_url,
        vision_model,
        reasoning_effort,
    } = payload;
    // Only the fields that were sent change; the rest keep their saved values.
    for (value, slot) in [
        (provider, &mut c.provider),
        (aicredits_key, &mut c.aicredits_key),
        (openai_key, &mut c.openai_key),
        (anthropic_key, &mut c.anthropic_key),
        (google_key, &mut c.google_key),
        (custom_key, &mut c.custom_key),
        (tavily_key, &mut c.tavily_key),
        (model_name, &mut c.model_name),
        (base_url, &mut c.base_url),
        (vision_model, &mut c.vision_model),
        (reasoning_effort, &mut c.reasoning_effort),
    ] {
        if let Some(v) = value {
            *slot = v;
        }
    }
    save_credentials_securely(&c)
}

#[tauri::command]
async fn get_device_status(state: State<'_, AppState>) -> Result<DeviceState, String> {
    Ok(state.phone.get_device_state().await)
}

#[tauri::command]
async fn get_latest_frame(state: State<'_, AppState>) -> Result<Option<String>, String> {
    Ok(state.phone.get_latest_frame().await)
}

// ---- Phone pairing ------------------------------------------------------------

fn pairing_of(state: &AppState) -> Result<std::sync::Arc<crate::phone::pairing::Pairing>, String> {
    state
        .phone
        .pairing
        .clone()
        .ok_or_else(|| "Secure pairing isn't available. Restart Vivy.".to_string())
}

/// The paired phone (if any) and the current pairing code.
#[tauri::command]
fn get_pairing(state: State<'_, AppState>) -> Result<crate::phone::pairing::PairingView, String> {
    Ok(pairing_of(&state)?.view())
}

/// Show a fresh code, e.g. to pair a different phone.
#[tauri::command]
fn start_pairing(state: State<'_, AppState>) -> Result<crate::phone::pairing::PairingView, String> {
    Ok(pairing_of(&state)?.start())
}

#[tauri::command]
fn cancel_pairing(state: State<'_, AppState>) -> Result<crate::phone::pairing::PairingView, String> {
    Ok(pairing_of(&state)?.cancel())
}

/// Unpair the phone and close its connection.
#[tauri::command]
async fn forget_phone(state: State<'_, AppState>) -> Result<crate::phone::pairing::PairingView, String> {
    let view = pairing_of(&state)?.forget();
    state.phone.disconnect().await;
    Ok(view)
}

#[tauri::command]
fn get_connection_info() -> serde_json::Value {
    let ip = get_local_lan_ip();
    json!({
        "ip": ip,
        "port": 8765,
        "ws_url": format!("wss://{}:8765", ip)
    })
}

#[tauri::command]
async fn start_agent_task(
    prompt: String,
    rewind_from: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if !state.is_agent_enabled() {
        return Err("The agent is switched off. Turn it on to run tasks.".to_string());
    }
    let guard = state
        .running_task_lock
        .clone()
        .try_lock_owned()
        .map_err(|_| "A task is already running. Stop it first.".to_string())?;

    let phone = state.phone.clone();
    let permissions = state.permissions.clone();
    let creds = load_credentials_securely();
    // Subscribe before spawning so a stop pressed right away is not missed.
    let mut cancel_rx = state.cancel_tx.subscribe();
    state.pause_tx.send_replace(false);
    let pause_rx = state.pause_tx.subscribe();
    let store = state.sessions.clone();
    // Resending an earlier message replaces it and everything after it.
    if let Some(id) = rewind_from.as_deref() {
        store.rewind(id)?;
    }
    let task_id = record_log(&app, &store, "task", &prompt, None);

    tauri::async_runtime::spawn(async move {
        let _guard = guard;

        // Fold old turns into a summary first if this chat's memory has grown large.
        let (mut summary, mut history) = store.context();
        let session_id = store.active().id;
        let compaction = tokio::select! {
            c = compact(&creds, summary.as_deref(), &history) => c,
            _ = cancel_rx.changed() => {
                let _ = app.emit("agent-stopped", ());
                record_log(&app, &store, "stopped", "Stopped", None);
                return;
            }
        };
        match compaction {
            Compaction::None => {}
            Compaction::Summarized { summary: s, folded } => {
                store.apply_compaction(&session_id, s.clone(), folded);
                history.drain(..folded);
                summary = Some(s);
                record_log(&app, &store, "compacted",
                    &format!("Condensed {} older messages into a summary to keep Vivy's memory short.", folded), None);
            }
            Compaction::Dropped { folded } => {
                store.apply_compaction(&session_id, summary.clone().unwrap_or_default(), folded);
                history.drain(..folded);
                record_log(&app, &store, "compacted",
                    &format!("Dropped {} of the oldest messages from Vivy's memory.", folded), None);
            }
        }

        let session = AgentSession::new(app.clone(), phone, permissions.clone(), creds, pause_rx, store.clone());

        let outcome = tokio::select! {
            result = session.run_task(prompt.clone(), summary, history) => match result {
                Ok(summary) => {
                    let _ = app.emit("agent-complete", json!({ "summary": summary }));
                    summary
                }
                Err(err) => {
                    let _ = app.emit("agent-error", json!({ "error": err }));
                    format!("(This attempt failed: {err})")
                }
            },
            // Dropping run_task's future aborts whatever it was awaiting:
            // the model request, a phone command, a wait, or a permission prompt.
            _ = cancel_rx.changed() => {
                permissions.cancel_pending().await;
                record_log(&app, &store, "stopped", "Stopped", None);
                let _ = app.emit("agent-stopped", ());
                "(The user stopped this before it finished.)".to_string()
            }
        };

        remember_turn(&store, &task_id, prompt, outcome, session.actions());
        let _ = app.emit("sessions-changed", ());
    });

    Ok(())
}

/// Store one finished turn in the chat's memory: the user's message and Vivy's
/// reply, with a short list of what was done on the phone so follow-ups like
/// "now search it" work. Large memories are compacted before the next task.
fn remember_turn(store: &crate::sessions::SessionStore, task_id: &str, prompt: String, reply: String, actions: Vec<String>) {
    let mut reply = reply;
    if !actions.is_empty() {
        let shown: Vec<_> = actions.iter().rev().take(25).rev().cloned().collect();
        reply.push_str(&format!("\n\n[Phone actions taken: {}]", shown.join("; ")));
    }
    store.push_turn(task_id, prompt, reply);
}

// ---- Chat sessions -----------------------------------------------------------

fn ensure_idle(state: &AppState) -> Result<(), String> {
    if state.is_task_running() {
        Err("Wait for the current task to finish, or stop it, before switching chats.".to_string())
    } else {
        Ok(())
    }
}

#[tauri::command]
fn list_sessions(state: State<'_, AppState>) -> Vec<SessionMeta> {
    state.sessions.list()
}

#[tauri::command]
fn get_active_session(state: State<'_, AppState>) -> Session {
    state.sessions.active()
}

#[tauri::command]
fn new_session(state: State<'_, AppState>) -> Result<Session, String> {
    ensure_idle(&state)?;
    Ok(state.sessions.create())
}

#[tauri::command]
fn open_session(id: String, state: State<'_, AppState>) -> Result<Session, String> {
    ensure_idle(&state)?;
    state.sessions.open(&id)
}

#[tauri::command]
fn delete_session(id: String, state: State<'_, AppState>) -> Result<Session, String> {
    if state.sessions.active().id == id {
        ensure_idle(&state)?;
    }
    Ok(state.sessions.delete(&id))
}

#[tauri::command]
fn rename_session(id: String, title: String, state: State<'_, AppState>) {
    state.sessions.rename(&id, &title);
}

#[tauri::command]
fn stop_agent_task(state: State<'_, AppState>) {
    state.cancel_active_task();
}

/// Pause between actions so the user can use the phone by hand, then resume.
#[tauri::command]
fn set_agent_paused(paused: bool, app: AppHandle, state: State<'_, AppState>) -> bool {
    let paused = paused && state.is_task_running();
    state.pause_tx.send_replace(paused);
    let _ = app.emit("agent-paused", paused);
    paused
}

#[derive(Serialize)]
struct AgentStatus {
    enabled: bool,
    running: bool,
    paused: bool,
}

#[tauri::command]
fn get_agent_status(state: State<'_, AppState>) -> AgentStatus {
    AgentStatus {
        enabled: state.is_agent_enabled(),
        running: state.is_task_running(),
        paused: *state.pause_tx.borrow(),
    }
}

/// The master switch. Turning it off kills the running task on the spot,
/// denies any open permission prompt and forgets granted permissions.
#[tauri::command]
async fn set_agent_enabled(enabled: bool, app: AppHandle, state: State<'_, AppState>) -> Result<bool, String> {
    state.agent_enabled.store(enabled, Ordering::SeqCst);
    if !enabled {
        state.cancel_active_task();
        state.permissions.cancel_pending().await;
        state.permissions.revoke_all().await;
    }
    let _ = app.emit("agent-enabled-changed", enabled);
    Ok(enabled)
}

#[tauri::command]
async fn respond_to_permission(
    request_id: String,
    allowed: bool,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    Ok(state.permissions.respond_to_request(&request_id, allowed).await)
}

#[tauri::command]
async fn manual_touch(x: f32, y: f32, state: State<'_, AppState>) -> Result<(), String> {
    state
        .phone
        .execute_command(
            PhoneCommand::Tap {
                id: Uuid::new_v4().to_string(),
                x,
                y,
            },
            4,
        )
        .await
}

#[tauri::command]
async fn manual_swipe(
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    duration_ms: Option<u64>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .phone
        .execute_command(
            PhoneCommand::Swipe {
                id: Uuid::new_v4().to_string(),
                x1,
                y1,
                x2,
                y2,
                duration_ms: duration_ms.unwrap_or(300),
            },
            4,
        )
        .await
}

#[tauri::command]
async fn manual_key(key: String, state: State<'_, AppState>) -> Result<(), String> {
    state
        .phone
        .execute_command(
            PhoneCommand::PressKey {
                id: Uuid::new_v4().to_string(),
                key,
            },
            4,
        )
        .await
}

#[tauri::command]
async fn manual_type(text: String, state: State<'_, AppState>) -> Result<(), String> {
    state
        .phone
        .execute_command(
            PhoneCommand::TypeText {
                id: Uuid::new_v4().to_string(),
                text,
            },
            4,
        )
        .await
}

fn stop_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyX)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            // Ctrl+Shift+X stops the running task even when Vivy isn't focused.
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == ShortcutState::Pressed && *shortcut == stop_shortcut() {
                        if let Some(state) = app.try_state::<AppState>() {
                            if state.is_task_running() {
                                state.cancel_active_task();
                            }
                        }
                    }
                })
                .build(),
        )
        .setup(|app| {
            if let Err(e) = app.global_shortcut().register(stop_shortcut()) {
                eprintln!("[Vivy] Couldn't register the stop hotkey (Ctrl+Shift+X): {e}");
            }
            let handle = app.handle().clone();
            let phone = PhoneManager::new(handle.clone());
            let permissions = PermissionManager::new(handle.clone());

            // Start WebSocket server on 8765
            start_websocket_server(phone.clone(), 8765);

            let state = AppState::new(phone, permissions);
            app.manage(state);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_device_status,
            get_latest_frame,
            get_connection_info,
            get_pairing,
            start_pairing,
            cancel_pairing,
            forget_phone,
            start_agent_task,
            stop_agent_task,
            list_sessions,
            get_active_session,
            new_session,
            open_session,
            delete_session,
            rename_session,
            get_agent_status,
            set_agent_enabled,
            set_agent_paused,
            respond_to_permission,
            manual_touch,
            manual_swipe,
            manual_key,
            manual_type
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
