//! Running one message through the agent: compact the chat's memory if it has
//! grown large, run the agent loop, honour Stop, and remember the turn.

use serde_json::json;

use crate::agent::brain::{record_log, AgentSession};
use crate::agent::memory::{compact, Compaction};
use crate::security::load_credentials_securely;
use crate::sessions::SessionStore;
use crate::state::AppState;

/// Start working on `prompt` in the active chat. Returns once the task is
/// running; progress arrives as events, ending with `agent-complete`,
/// `agent-error` or `agent-stopped`, then `sessions-changed`.
///
/// `rewind_from` replaces an earlier message (and everything after it).
/// Must be called from inside a Tokio runtime.
pub fn start_task(
    state: &AppState,
    prompt: String,
    rewind_from: Option<String>,
) -> Result<tokio::task::JoinHandle<()>, String> {
    if !state.is_agent_enabled() {
        return Err("The agent is switched off. Turn it on to run tasks.".to_string());
    }
    let guard = state
        .running_task_lock
        .clone()
        .try_lock_owned()
        .map_err(|_| "A task is already running. Stop it first.".to_string())?;

    let events = state.events.clone();
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
    let task_id = record_log(&events, &store, "task", &prompt, None);

    Ok(tokio::spawn(async move {
        let _guard = guard;

        // Fold old turns into a summary first if this chat's memory has grown large.
        let (mut summary, mut history) = store.context();
        let session_id = store.active().id;
        let compaction = tokio::select! {
            c = compact(&creds, summary.as_deref(), &history) => c,
            _ = cancel_rx.changed() => {
                record_log(&events, &store, "stopped", "Stopped", None);
                events.emit("agent-stopped", &());
                return;
            }
        };
        match compaction {
            Compaction::None => {}
            Compaction::Summarized { summary: s, folded } => {
                store.apply_compaction(&session_id, s.clone(), folded);
                history.drain(..folded);
                summary = Some(s);
                record_log(&events, &store, "compacted",
                    &format!("Condensed {folded} older messages into a summary to keep Vivy's memory short."), None);
            }
            Compaction::Dropped { folded } => {
                store.apply_compaction(&session_id, summary.clone().unwrap_or_default(), folded);
                history.drain(..folded);
                record_log(&events, &store, "compacted",
                    &format!("Dropped {folded} of the oldest messages from Vivy's memory."), None);
            }
        }

        let session = AgentSession::new(events.clone(), phone, permissions.clone(), creds, pause_rx, store.clone());

        let outcome = tokio::select! {
            result = session.run_task(prompt.clone(), summary, history) => match result {
                Ok(summary) => {
                    events.emit("agent-complete", &json!({ "summary": summary }));
                    summary
                }
                Err(err) => {
                    events.emit("agent-error", &json!({ "error": err }));
                    format!("(This attempt failed: {err})")
                }
            },
            // Dropping run_task's future aborts whatever it was awaiting:
            // the model request, a phone command, a wait, or a permission prompt.
            _ = cancel_rx.changed() => {
                permissions.cancel_pending().await;
                record_log(&events, &store, "stopped", "Stopped", None);
                events.emit("agent-stopped", &());
                "(The user stopped this before it finished.)".to_string()
            }
        };

        remember_turn(&store, &task_id, prompt, outcome, session.actions());
        events.emit("sessions-changed", &());
    }))
}

/// Store one finished turn in the chat's memory: the user's message and Vivy's
/// reply, with a short list of what was done on the phone so follow-ups like
/// "now search it" work. Large memories are compacted before the next task.
fn remember_turn(store: &SessionStore, task_id: &str, prompt: String, reply: String, actions: Vec<String>) {
    let mut reply = reply;
    if !actions.is_empty() {
        let shown: Vec<_> = actions.iter().rev().take(25).rev().cloned().collect();
        reply.push_str(&format!("\n\n[Phone actions taken: {}]", shown.join("; ")));
    }
    store.push_turn(task_id, prompt, reply);
}
