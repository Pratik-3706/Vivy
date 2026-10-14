//! Chat sessions: each one keeps its activity log (what the UI shows) and the
//! conversation the model remembers (turns + a compacted summary).
//! Stored as one JSON file per session in %LOCALAPPDATA%\Vivy\sessions.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Long tool output (screen dumps) is trimmed when saved; the UI only needs a preview.
const MAX_SAVED_DESCRIPTION: usize = 4000;
/// Oldest activity entries are dropped beyond this, to keep files small.
const MAX_SAVED_LOGS: usize = 3000;
const UNTITLED: &str = "New chat";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub created_at: u64,
    pub updated_at: u64,
    /// Model-written summary of turns that were compacted away.
    #[serde(default)]
    pub summary: Option<String>,
    /// Remembered conversation: alternating user / assistant messages.
    #[serde(default)]
    pub turns: Vec<Value>,
    /// Activity entries as shown in the UI.
    #[serde(default)]
    pub logs: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionMeta {
    pub id: String,
    pub title: String,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Session {
    fn new() -> Self {
        let now = now_ms();
        Self {
            id: Uuid::new_v4().to_string(),
            title: UNTITLED.to_string(),
            created_at: now,
            updated_at: now,
            summary: None,
            turns: Vec::new(),
            logs: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.logs.is_empty() && self.turns.is_empty()
    }

    fn meta(&self) -> SessionMeta {
        SessionMeta {
            id: self.id.clone(),
            title: self.title.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

pub struct SessionStore {
    dir: Option<PathBuf>,
    active: Mutex<Session>,
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis() as u64
}

/// First line of the first message, shortened, as the chat title.
fn title_from(prompt: &str) -> String {
    let line = prompt.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let mut title: String = line.chars().take(48).collect();
    if line.chars().count() > 48 {
        title = format!("{}…", title.trim_end());
    }
    if title.is_empty() {
        UNTITLED.to_string()
    } else {
        title
    }
}

impl SessionStore {
    /// Opens the store on a fresh chat; earlier chats stay in the list.
    pub fn load() -> Self {
        let dir = std::env::var("LOCALAPPDATA").ok().map(|d| PathBuf::from(d).join("Vivy").join("sessions"));
        if let Some(d) = &dir {
            let _ = std::fs::create_dir_all(d);
        }
        Self { dir, active: Mutex::new(Session::new()) }
    }

    fn path(&self, id: &str) -> Option<PathBuf> {
        // ids are UUIDs we generated; refuse anything that could escape the folder
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return None;
        }
        self.dir.as_ref().map(|d| d.join(format!("{id}.json")))
    }

    fn read(&self, id: &str) -> Option<Session> {
        let data = std::fs::read_to_string(self.path(id)?).ok()?;
        serde_json::from_str(&data).ok()
    }

    fn write(&self, s: &Session) {
        if s.is_empty() {
            return; // don't litter the list with untouched chats
        }
        let Some(path) = self.path(&s.id) else { return };
        if let Ok(json) = serde_json::to_string(s) {
            // Write then rename, so a crash mid-save never leaves a half-written file.
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }

    fn with_active<R>(&self, f: impl FnOnce(&mut Session) -> R) -> R {
        let mut s = self.active.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut s)
    }

    pub fn active(&self) -> Session {
        self.with_active(|s| s.clone())
    }

    /// All saved chats plus the current one if it has content, newest first.
    pub fn list(&self) -> Vec<SessionMeta> {
        let mut out: Vec<SessionMeta> = Vec::new();
        if let Some(dir) = &self.dir {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("json") {
                        continue;
                    }
                    if let Ok(data) = std::fs::read_to_string(&path) {
                        if let Ok(s) = serde_json::from_str::<Session>(&data) {
                            out.push(s.meta());
                        }
                    }
                }
            }
        }
        // The in-memory copy of the active chat is the freshest.
        if let Ok(active) = self.active.lock() {
            out.retain(|m| m.id != active.id);
            if !active.is_empty() {
                out.push(active.meta());
            }
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        out
    }

    pub fn create(&self) -> Session {
        self.with_active(|s| {
            self.write(s);
            *s = Session::new();
            s.clone()
        })
    }

    pub fn open(&self, id: &str) -> Result<Session, String> {
        if self.with_active(|s| s.id == id) {
            return Ok(self.active());
        }
        let loaded = self.read(id).ok_or_else(|| "That chat no longer exists.".to_string())?;
        Ok(self.with_active(|s| {
            self.write(s);
            *s = loaded;
            s.clone()
        }))
    }

    /// Deletes a chat. Returns the chat that is active afterwards.
    pub fn delete(&self, id: &str) -> Session {
        if let Some(path) = self.path(id) {
            let _ = std::fs::remove_file(path);
        }
        self.with_active(|s| {
            if s.id == id {
                *s = Session::new();
            }
            s.clone()
        })
    }

    pub fn rename(&self, id: &str, title: &str) {
        let title = title.trim();
        if title.is_empty() {
            return;
        }
        let title: String = title.chars().take(80).collect();
        let renamed_active = self.with_active(|s| {
            if s.id == id {
                s.title = title.clone();
                self.write(s);
                true
            } else {
                false
            }
        });
        if !renamed_active {
            if let Some(mut s) = self.read(id) {
                s.title = title;
                self.write(&s);
            }
        }
    }

    /// Record an activity entry in the active chat and save it.
    pub fn append_log(&self, mut log: Value) {
        if let Some(desc) = log["description"].as_str() {
            if desc.chars().count() > MAX_SAVED_DESCRIPTION {
                let cut: String = desc.chars().take(MAX_SAVED_DESCRIPTION).collect();
                log["description"] = Value::String(format!("{cut}…"));
            }
        }
        self.with_active(|s| {
            if log["action_type"] == "task" && s.title == UNTITLED {
                s.title = title_from(log["description"].as_str().unwrap_or(""));
            }
            s.logs.push(log);
            let excess = s.logs.len().saturating_sub(MAX_SAVED_LOGS);
            s.logs.drain(..excess);
            s.updated_at = now_ms();
            self.write(s);
        });
    }

    /// What the model should remember: (summary of compacted turns, recent turns).
    pub fn context(&self) -> (Option<String>, Vec<Value>) {
        self.with_active(|s| {
            // The task tag is ours; models only get role and content.
            let turns = s
                .turns
                .iter()
                .map(|t| serde_json::json!({ "role": t["role"], "content": t["content"] }))
                .collect();
            (s.summary.clone(), turns)
        })
    }

    /// `task_id` is the id of the message's "task" log, so the turn can be
    /// removed again if that message is resent.
    pub fn push_turn(&self, task_id: &str, user: String, assistant: String) {
        self.with_active(|s| {
            s.turns.push(serde_json::json!({ "role": "user", "content": user, "task": task_id }));
            s.turns.push(serde_json::json!({ "role": "assistant", "content": assistant }));
            s.updated_at = now_ms();
            self.write(s);
        });
    }

    /// Forget a message and everything after it, both on screen and in the
    /// model's memory, so it can be sent again as if for the first time.
    pub fn rewind(&self, task_id: &str) -> Result<(), String> {
        self.with_active(|s| {
            let cut = s
                .logs
                .iter()
                .position(|l| l["id"] == task_id && l["action_type"] == "task")
                .ok_or_else(|| "That message is no longer in this chat.".to_string())?;
            let removed: Vec<String> = s.logs[cut..]
                .iter()
                .filter(|l| l["action_type"] == "task")
                .filter_map(|l| l["id"].as_str().map(str::to_string))
                .collect();
            s.logs.truncate(cut);

            let first_removed = s
                .turns
                .iter()
                .position(|t| t["task"].as_str().is_some_and(|id| removed.iter().any(|r| r == id)));
            match first_removed {
                Some(i) => s.turns.truncate(i),
                // Chats saved before turns were tagged: one user/assistant pair per message.
                None if !s.turns.iter().any(|t| t.get("task").is_some()) => {
                    let keep = s.turns.len().saturating_sub(removed.len() * 2);
                    s.turns.truncate(keep);
                }
                // The removed turns were already folded into the summary.
                None => {}
            }
            s.updated_at = now_ms();
            self.write(s);
            Ok(())
        })
    }

    /// Replace the oldest `folded` turns with a summary, if the chat is still the
    /// one that was compacted (the user may have switched meanwhile).
    pub fn apply_compaction(&self, session_id: &str, summary: String, folded: usize) {
        self.with_active(|s| {
            if s.id != session_id || folded > s.turns.len() {
                return;
            }
            s.turns.drain(..folded);
            s.summary = Some(summary);
            self.write(s);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_come_from_the_first_line() {
        assert_eq!(title_from("open youtube\nand play music"), "open youtube");
        assert_eq!(title_from("   "), UNTITLED);
        let long = "a".repeat(80);
        assert!(title_from(&long).ends_with('…'));
    }

    #[test]
    fn rewind_forgets_the_message_and_everything_after() {
        let store = SessionStore { dir: None, active: Mutex::new(Session::new()) };
        let task = |id: &str| serde_json::json!({ "id": id, "action_type": "task", "description": id });
        let reply = |id: &str| serde_json::json!({ "id": id, "action_type": "finished", "description": "ok" });
        for (t, r) in [("t1", "r1"), ("t2", "r2"), ("t3", "r3")] {
            store.append_log(task(t));
            store.append_log(reply(r));
            store.push_turn(t, t.into(), "ok".into());
        }
        store.rewind("t2").unwrap();
        let s = store.active();
        assert_eq!(s.logs.len(), 2);
        assert_eq!(s.turns.len(), 2);
        assert_eq!(store.context().1[0], serde_json::json!({ "role": "user", "content": "t1" }));
        assert!(store.rewind("t3").is_err());
    }

    #[test]
    fn rejects_path_like_ids() {
        let store = SessionStore { dir: Some(PathBuf::from("x")), active: Mutex::new(Session::new()) };
        assert!(store.path("../evil").is_none());
        assert!(store.path("3f2b-11aa").is_some());
    }
}
