//! The desktop app: Tauri commands over the shared engine in `vivy-core`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{Emitter, Manager, State};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use uuid::Uuid;

use vivy_core::events::Events;
use vivy_core::phone::commands::{DeviceState, PhoneCommand};
use vivy_core::security::{load_credentials_securely, load_saved_credentials, save_credentials_securely};
use vivy_core::sessions::{Session, SessionMeta};
use vivy_core::state::{get_local_lan_ip, AppState};
use vivy_core::PHONE_PORT;

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
        ws_port: PHONE_PORT,
        local_ip,
    }
}

#[tauri::command]
fn save_settings(payload: SaveSettingsPayload) -> Result<(), String> {
    let mut c = load_saved_credentials();
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

fn pairing_of(state: &AppState) -> Result<std::sync::Arc<vivy_core::phone::pairing::Pairing>, String> {
    state
        .phone
        .pairing
        .clone()
        .ok_or_else(|| "Secure pairing isn't available. Restart Vivy.".to_string())
}

/// The paired phone (if any) and the current pairing code.
#[tauri::command]
fn get_pairing(state: State<'_, AppState>) -> Result<vivy_core::phone::pairing::PairingView, String> {
    Ok(pairing_of(&state)?.view())
}

/// Show a fresh code, e.g. to pair a different phone.
#[tauri::command]
fn start_pairing(state: State<'_, AppState>) -> Result<vivy_core::phone::pairing::PairingView, String> {
    Ok(pairing_of(&state)?.start())
}

#[tauri::command]
fn cancel_pairing(state: State<'_, AppState>) -> Result<vivy_core::phone::pairing::PairingView, String> {
    Ok(pairing_of(&state)?.cancel())
}

/// Unpair the phone and close its connection.
#[tauri::command]
async fn forget_phone(state: State<'_, AppState>) -> Result<vivy_core::phone::pairing::PairingView, String> {
    let view = pairing_of(&state)?.forget();
    state.phone.disconnect().await;
    Ok(view)
}

/// Whether Windows Firewall lets the phone reach this app.
#[tauri::command]
async fn check_firewall() -> Result<vivy_core::firewall::FirewallStatus, String> {
    tauri::async_runtime::spawn_blocking(|| vivy_core::firewall::check(PHONE_PORT))
        .await
        .map_err(|e| e.to_string())
}

/// Allow this app through Windows Firewall (shows the Windows admin prompt).
#[tauri::command]
async fn allow_firewall() -> Result<vivy_core::firewall::FirewallStatus, String> {
    tauri::async_runtime::spawn_blocking(|| vivy_core::firewall::allow(PHONE_PORT))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn get_connection_info() -> serde_json::Value {
    let ip = get_local_lan_ip();
    json!({
        "ip": ip,
        "port": PHONE_PORT,
        "ws_url": format!("wss://{ip}:{PHONE_PORT}")
    })
}

#[tauri::command]
async fn start_agent_task(
    prompt: String,
    rewind_from: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    vivy_core::runner::start_task(&state, prompt, rewind_from).map(|_| ())
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
fn set_agent_paused(paused: bool, state: State<'_, AppState>) -> bool {
    state.set_paused(paused)
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
        paused: state.is_paused(),
    }
}

/// The master switch. Turning it off kills the running task on the spot,
/// denies any open permission prompt and forgets granted permissions.
#[tauri::command]
async fn set_agent_enabled(enabled: bool, state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.set_enabled(enabled).await)
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
            // Engine events go straight to the web UI.
            let events = Events::new(move |name: &str, payload: Value| {
                let _ = handle.emit(name, payload);
            });
            let state = AppState::new(events);

            // The phone server needs the async runtime, which setup doesn't run on.
            let phone = state.phone.clone();
            tauri::async_runtime::spawn(async move {
                vivy_core::phone::connection::start_websocket_server(phone, PHONE_PORT);
            });

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
            check_firewall,
            allow_firewall,
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
