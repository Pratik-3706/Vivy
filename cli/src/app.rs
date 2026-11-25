//! The interactive chat and single-task mode.

use std::time::Duration;

use serde_json::Value;
use tokio::sync::mpsc;

use vivy_core::events::Events;
use vivy_core::security::load_credentials_securely;
use vivy_core::state::AppState;
use vivy_core::{providers, PHONE_PORT};

use crate::commands;
use crate::config::{CliConfig, Look};
use crate::input::{Answer, Input};
use crate::matsumoto;
use crate::render::{phone_notice, Outcome, TaskView};
use crate::term;

pub type EventRx = mpsc::UnboundedReceiver<(String, Value)>;

pub struct App {
    pub state: AppState,
    pub events: EventRx,
    pub cfg: CliConfig,
    pub look: Look,
    pub input: Input,
    /// The last message sent, for /retry.
    last: Option<(String, String)>,
}

impl App {
    /// Start the engine and the phone server.
    pub fn start(cfg: CliConfig) -> Self {
        let (tx, events) = mpsc::unbounded_channel::<(String, Value)>();
        let sink = Events::new(move |name: &str, payload: Value| {
            // Screen frames are for the desktop mirror; the terminal has no use for them.
            if name != "phone-frame" {
                let _ = tx.send((name.to_string(), payload));
            }
        });
        let state = AppState::new(sink);
        state.start_phone_server(PHONE_PORT);
        let look = cfg.matsumoto.look();
        let input = Input::start(term::tint("›", look.palette.eye));
        Self { state, events, cfg, look, input, last: None }
    }

    /// Run one message through the agent, showing progress until it ends.
    pub async fn run_task(&mut self, prompt: String, rewind_from: Option<String>) -> Outcome {
        // Drop notices that arrived while idle so they don't print mid-task out of order.
        while let Ok((name, payload)) = self.events.try_recv() {
            if let Some(n) = phone_notice(&name, &payload) {
                println!("{n}");
            }
        }
        let handle = match vivy_core::runner::start_task(&self.state, prompt.clone(), rewind_from) {
            Ok(h) => h,
            Err(e) => {
                println!("{}", term::red(&format!("✗ {e}")));
                return Outcome::Failed;
            }
        };
        let mut view = TaskView::new(self.look);
        let mut tick = tokio::time::interval(Duration::from_millis(125));
        let mut stopping = false;
        tokio::pin!(handle);

        loop {
            tokio::select! {
                biased;
                ev = self.events.recv() => {
                    let Some((name, payload)) = ev else { break };
                    if name == "agent-action" && payload["action_type"] == "task" {
                        if let Some(id) = payload["id"].as_str() {
                            self.last = Some((id.to_string(), prompt.clone()));
                        }
                        continue;
                    }
                    if name == "agent-permission-request" {
                        view.pause_status();
                        let allowed = self.ask_permission(&payload).await;
                        if allowed.is_none() {
                            stopping = true;
                            self.state.cancel_active_task();
                        }
                        if let Some(id) = payload["id"].as_str() {
                            self.state.permissions.respond_to_request(id, allowed.unwrap_or(false)).await;
                        }
                        continue;
                    }
                    view.handle(&name, &payload);
                }
                _ = &mut handle => {
                    // Show whatever was emitted right before the task ended.
                    while let Ok((name, payload)) = self.events.try_recv() {
                        view.handle(&name, &payload);
                    }
                    break;
                }
                _ = tick.tick() => view.tick(),
                _ = tokio::signal::ctrl_c() => {
                    if stopping {
                        view.finish();
                        println!("{}", term::yellow("Force quit."));
                        std::process::exit(130);
                    }
                    stopping = true;
                    view.pause_status();
                    println!("{}", term::yellow("■ Stopping… (Ctrl+C again to force quit)"));
                    self.state.cancel_active_task();
                }
            }
        }
        view.finish();
        view.outcome.unwrap_or(if stopping { Outcome::Stopped } else { Outcome::Done })
    }

    /// Allow / deny for sensitive steps. None = the user pressed Ctrl+C (stop).
    async fn ask_permission(&self, req: &Value) -> Option<bool> {
        let title = req["title"].as_str().unwrap_or("Vivy needs your OK");
        let reason = req["reason"].as_str().unwrap_or("");
        let every_time = matches!(req["category"].as_str(), Some("destructive" | "financial"));
        println!();
        println!("  {} {}", term::yellow("?"), term::bold(title));
        if !reason.is_empty() {
            println!("    {reason}");
        }
        println!(
            "    {}",
            term::dim(if every_time {
                "Asked every time for this kind of action."
            } else {
                "Allowing covers this kind of action until you quit."
            })
        );
        loop {
            match self.input.ask("    Allow? [y/N] ", false).await {
                Answer::Line(l) => match l.trim().to_ascii_lowercase().as_str() {
                    "y" | "yes" => return Some(true),
                    "" | "n" | "no" => return Some(false),
                    _ => println!("    {}", term::dim("Type y or n.")),
                },
                Answer::Interrupted => return None,
                Answer::Eof => return Some(false),
            }
        }
    }

    /// Notices that arrived while idle, printed above the prompt.
    fn pump_notices(&mut self) {
        while let Ok((name, payload)) = self.events.try_recv() {
            if let Some(n) = phone_notice(&name, &payload) {
                self.input.notice(n);
            }
        }
    }
}

// ---- Interactive chat -----------------------------------------------------------

pub async fn chat(cfg: CliConfig, resume: bool) -> i32 {
    let mut app = App::start(cfg);
    if resume {
        if let Some(latest) = app.state.sessions.list().first() {
            let _ = app.state.sessions.open(&latest.id);
        }
    }
    if app.cfg.terminal.banner {
        banner(&app).await;
    }
    if resume {
        print_history(&app, 6);
    }

    loop {
        // Plain text: the line editor adds the colour (see input.rs).
        let prompt = "› ".to_string();
        let answer = {
            let ask = app.input.ask(&prompt, true);
            tokio::pin!(ask);
            loop {
                tokio::select! {
                    a = &mut ask => break a,
                    ev = app.events.recv() => {
                        if let Some((name, payload)) = ev {
                            if let Some(n) = phone_notice(&name, &payload) {
                                app.input.notice(n);
                            }
                        }
                    }
                }
            }
        };
        let line = match answer {
            Answer::Line(l) => l,
            Answer::Interrupted => {
                println!("{}", term::dim("(Ctrl+D or /quit to leave)"));
                continue;
            }
            Answer::Eof => break,
        };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        if let Some(cmd) = line.strip_prefix('/') {
            match slash(&mut app, cmd).await {
                Flow::Continue => continue,
                Flow::Quit => break,
                Flow::Send(prompt, rewind) => {
                    app.run_task(prompt, rewind).await;
                }
            }
            continue;
        }
        app.run_task(line, None).await;
        app.pump_notices();
    }
    println!("{}", term::dim("Bye."));
    0
}

enum Flow {
    Continue,
    Quit,
    Send(String, Option<String>),
}

const HELP: &[(&str, &str)] = &[
    ("/new", "start a new chat"),
    ("/chats", "list saved chats"),
    ("/open N", "open chat N from /chats"),
    ("/delete N", "delete chat N from /chats"),
    ("/rename TITLE", "rename this chat"),
    ("/retry", "send your last message again, replacing the answer"),
    ("/model [N|ID]", "list or switch models"),
    ("/use PROVIDER", "switch provider"),
    ("/effort [LEVEL]", "how hard the model thinks"),
    ("/key [PROVIDER]", "save an API key"),
    ("/pair", "show the pairing code for your phone"),
    ("/phone", "phone status"),
    ("/forget", "unpair the phone"),
    ("/matsumoto", "say hi to Matsumoto"),
    ("/status", "settings at a glance"),
    ("/clear", "clear the screen"),
    ("/quit", "leave (or Ctrl+D)"),
];

async fn slash(app: &mut App, cmd: &str) -> Flow {
    let (name, arg) = match cmd.split_once(char::is_whitespace) {
        Some((n, a)) => (n, a.trim()),
        None => (cmd, ""),
    };
    let arg_opt = (!arg.is_empty()).then_some(arg);
    match name.to_ascii_lowercase().as_str() {
        "help" | "?" => {
            println!();
            for (c, d) in HELP {
                println!("  {} {}", term::bold(&format!("{c:<16}")), term::dim(d));
            }
            println!("\n  {}", term::dim("Ctrl+C stops a running task. Anything else you type goes to Vivy."));
        }
        "quit" | "exit" | "q" => return Flow::Quit,
        "clear" | "cls" => {
            // Screen and scrollback, flushed now (print! alone stays buffered).
            use crossterm::{cursor::MoveTo, execute, terminal::{Clear, ClearType}};
            let _ = execute!(std::io::stdout(), Clear(ClearType::All), Clear(ClearType::Purge), MoveTo(0, 0));
        }
        "new" => {
            app.state.sessions.create();
            app.last = None;
            println!("{}", term::dim("New chat."));
        }
        "chats" => {
            commands::chats();
        }
        "open" => match pick_chat(app, arg) {
            Some(id) => match app.state.sessions.open(&id) {
                Ok(s) => {
                    app.last = None;
                    println!("{} {}", term::dim("Opened"), term::bold(&s.title));
                    print_history(app, 6);
                }
                Err(e) => println!("{}", term::red(&e)),
            },
            None => println!("{}", term::dim("Usage: /open N (numbers from /chats)")),
        },
        "delete" => match pick_chat(app, arg) {
            Some(id) => {
                app.state.sessions.delete(&id);
                println!("{}", term::dim("Deleted."));
            }
            None => println!("{}", term::dim("Usage: /delete N (numbers from /chats)")),
        },
        "rename" => {
            if arg.is_empty() {
                println!("{}", term::dim("Usage: /rename new title"));
            } else {
                let id = app.state.sessions.active().id;
                app.state.sessions.rename(&id, arg);
                println!("{}", term::dim("Renamed."));
            }
        }
        "retry" => match app.last.clone() {
            Some((id, prompt)) => return Flow::Send(prompt, Some(id)),
            None => println!("{}", term::dim("Nothing to retry yet in this chat.")),
        },
        "model" | "models" => {
            commands::model(arg_opt);
        }
        "use" | "provider" => match arg_opt {
            Some(p) => {
                let mut parts = p.split_whitespace();
                let provider = parts.next().unwrap_or("");
                commands::use_provider(provider, parts.next(), None);
            }
            None => println!("{}", term::dim("Usage: /use aicredits | openai | anthropic | google | custom")),
        },
        "effort" => {
            commands::effort(arg_opt);
        }
        "key" => {
            commands::key(arg_opt);
        }
        "pair" => {
            commands::show_pairing(&app.state, arg == "new");
        }
        "forget" => {
            commands::forget();
            app.state.phone.disconnect().await;
        }
        "phone" => {
            commands::phone_status(&app.state).await;
        }
        "status" => {
            commands::status();
        }
        "matsumoto" | "m" => {
            commands::preview(app.look, Duration::from_secs(4)).await;
        }
        other => println!("{}", term::dim(&format!("Unknown command /{other}. Try /help."))),
    }
    Flow::Continue
}

fn pick_chat(app: &App, arg: &str) -> Option<String> {
    let n: usize = arg.parse().ok()?;
    app.state.sessions.list().get(n.checked_sub(1)?).map(|m| m.id.clone())
}

/// The last few exchanges of the open chat.
fn print_history(app: &App, max: usize) {
    let logs = app.state.sessions.active().logs;
    let mut lines: Vec<String> = Vec::new();
    for log in &logs {
        let text = log["description"].as_str().unwrap_or("").trim();
        let first: String = text.lines().next().unwrap_or("").chars().take(100).collect();
        match log["action_type"].as_str() {
            Some("task") => lines.push(format!("{} {}", term::dim("you  ›"), first)),
            Some("finished") | Some("thought") => lines.push(format!("{} {}", term::dim("vivy ›"), term::dim(&first))),
            _ => {}
        }
    }
    if lines.is_empty() {
        return;
    }
    let skip = lines.len().saturating_sub(max * 2);
    println!();
    for l in &lines[skip..] {
        println!("  {l}");
    }
    println!();
}

// ---- Single task ------------------------------------------------------------------

pub async fn run_once(cfg: CliConfig, task: String, wait_secs: u64, resume: bool) -> i32 {
    let mut app = App::start(cfg);
    if resume {
        if let Some(latest) = app.state.sessions.list().first() {
            let _ = app.state.sessions.open(&latest.id);
        }
    }
    let paired = app.state.phone.pairing.as_ref().is_some_and(|p| p.view().paired.is_some());
    if paired && wait_secs > 0 {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(wait_secs);
        let mut announced = false;
        while !app.state.phone.is_connected().await && tokio::time::Instant::now() < deadline {
            if !announced {
                eprintln!("{}", term::dim("Waiting for your phone to connect…"));
                announced = true;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        if announced && !app.state.phone.is_connected().await {
            eprintln!("{}", term::yellow("Phone didn't connect; Vivy can still answer, but can't use the phone."));
        }
    }
    println!("{} {}", term::tint("›", app.look.palette.eye), task);
    match app.run_task(task, None).await {
        Outcome::Done => 0,
        Outcome::Failed => 1,
        Outcome::Stopped => 130,
    }
}

// ---- Banner -------------------------------------------------------------------------

async fn banner(app: &App) {
    let creds = load_credentials_securely();
    let provider = providers::provider(&creds.provider);
    let model_name = provider
        .models
        .iter()
        .find(|m| m.id == creds.model_name)
        .map(|m| m.name.to_string())
        .unwrap_or(creds.model_name.clone());
    let pairing = app.state.phone.pairing.as_ref().map(|p| p.view());
    let phone = match pairing.as_ref().and_then(|v| v.paired.as_ref()) {
        Some(p) => format!("{} {}", p.device_name, term::dim("(paired; connects on its own)")),
        None => format!("not paired yet {}", term::dim("· type /pair")),
    };
    let key_line = if creds.api_key().is_empty() {
        term::yellow(&format!("no {} key yet · type /key", provider.name))
    } else {
        format!("{model_name} {}", term::dim(&format!("· {}", provider.name)))
    };
    let info = vec![
        String::new(),
        format!("{}  {}", term::bold(&term::tint("Vivy", app.look.palette.eye)), term::dim("your phone, by conversation")),
        String::new(),
        format!("{}  {key_line}", term::dim("model ")),
        format!("{}  {phone}", term::dim("phone ")),
        format!("{}  {}:{PHONE_PORT}", term::dim("address"), vivy_core::state::get_local_lan_ip()),
        String::new(),
        term::dim("Ask anything, or tell Vivy what to do on the phone. /help for commands."),
    ];

    // He wakes up: a few frames of floating and a blink, then settles.
    let edge = app.look.edge.min(16);
    let frames: Vec<matsumoto::Pose> = if app.look.animate && term::is_tty() && term::mode() != term::ColorMode::None {
        (0..10u64).map(|t| matsumoto::Pose { blink: t == 6, lift: (t / 2 % 2) as u32, ..Default::default() }).collect()
    } else {
        vec![matsumoto::Pose::default()]
    };
    let mut drawn = 0usize;
    for (i, pose) in frames.iter().enumerate() {
        let sprite = matsumoto::to_lines(&matsumoto::draw(&app.look.palette, edge, *pose), term::mode(), app.look.pixel);
        let lines = side_by_side(&sprite, &info);
        if drawn > 0 {
            print!("\x1b[{drawn}A");
        }
        for l in &lines {
            println!("\x1b[2K{l}");
        }
        drawn = lines.len();
        if i + 1 < frames.len() {
            tokio::time::sleep(Duration::from_millis(90)).await;
        }
    }
    println!();
}

fn side_by_side(left: &[String], right: &[String]) -> Vec<String> {
    let lw = left.iter().map(|l| term::visible_width(l)).max().unwrap_or(0);
    let rows = left.len().max(right.len());
    let top = rows.saturating_sub(right.len()) / 2;
    (0..rows)
        .map(|i| {
            let l = left.get(i).cloned().unwrap_or_default();
            let pad = lw - term::visible_width(&l);
            let r = i.checked_sub(top).and_then(|j| right.get(j)).cloned().unwrap_or_default();
            format!("  {l}{}   {r}", " ".repeat(pad))
        })
        .collect()
}
