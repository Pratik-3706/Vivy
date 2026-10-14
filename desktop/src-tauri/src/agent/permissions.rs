use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::{oneshot, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionCategory {
    Photos,      // Viewing photos, gallery, camera roll
    Files,       // Viewing local files, downloads, documents
    Messages,    // SMS, WhatsApp, Telegram, chat contents
    Mails,       // Gmail, Outlook, mail contents
    Destructive, // Deleting anything (files, messages, uninstalling apps)
    Financial,   // Making purchases, payment gateways, banking
}

impl PermissionCategory {
    pub fn description(&self) -> &'static str {
        match self {
            PermissionCategory::Photos => "Access Photos or Gallery",
            PermissionCategory::Files => "Access Storage Files or Documents",
            PermissionCategory::Messages => "Read or Access Chat/SMS Messages",
            PermissionCategory::Mails => "Read or Access Email Messages",
            PermissionCategory::Destructive => "Delete Files, Messages, or Apps",
            PermissionCategory::Financial => "Make Purchases or Access Banking",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub id: String,
    pub category: PermissionCategory,
    pub title: String,
    pub reason: String,
    pub target_detail: Option<String>,
}

#[derive(Clone)]
pub struct PermissionManager {
    app: AppHandle,
    granted_categories: Arc<Mutex<HashSet<PermissionCategory>>>,
    pending_requests: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
}

impl PermissionManager {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            granted_categories: Arc::new(Mutex::new(HashSet::new())),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Checks if category is pre-approved or requires asking user
    pub async fn check_or_request(
        &self,
        category: PermissionCategory,
        reason: String,
        target_detail: Option<String>,
    ) -> Result<bool, String> {
        // Destructive and Financial ALWAYS require fresh confirmation every single time
        if category != PermissionCategory::Destructive && category != PermissionCategory::Financial {
            let granted = self.granted_categories.lock().await;
            if granted.contains(&category) {
                return Ok(true);
            }
        }

        // Need user confirmation via UI modal
        let request_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel::<bool>();

        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(request_id.clone(), tx);
        }

        let req = PermissionRequest {
            id: request_id.clone(),
            title: category.description().to_string(),
            reason: reason.clone(),
            category: category.clone(),
            target_detail,
        };

        // Notify UI to display interactive permission modal
        let _ = self.app.emit("agent-permission-request", &req);

        // Await user's choice in desktop dashboard (up to 120 seconds timeout)
        match tokio::time::timeout(std::time::Duration::from_secs(120), rx).await {
            Ok(Ok(allowed)) => {
                if allowed && category != PermissionCategory::Destructive && category != PermissionCategory::Financial {
                    let mut granted = self.granted_categories.lock().await;
                    granted.insert(category);
                }
                Ok(allowed)
            }
            Ok(Err(_)) => Err("Permission channel closed".to_string()),
            Err(_) => {
                let mut pending = self.pending_requests.lock().await;
                pending.remove(&request_id);
                Err("Permission request timed out waiting for user confirmation".to_string())
            }
        }
    }

    /// Called by Tauri command when user clicks "Allow" or "Deny" in dashboard
    pub async fn respond_to_request(&self, request_id: &str, allowed: bool) -> bool {
        let mut pending = self.pending_requests.lock().await;
        if let Some(sender) = pending.remove(request_id) {
            let _ = sender.send(allowed);
            true
        } else {
            false
        }
    }

    /// Reset session permissions
    pub async fn revoke_all(&self) {
        let mut granted = self.granted_categories.lock().await;
        granted.clear();
    }

    /// Deny every open request (used when the agent is stopped mid-prompt).
    pub async fn cancel_pending(&self) {
        let drained: Vec<_> = self.pending_requests.lock().await.drain().collect();
        for (_, sender) in drained {
            let _ = sender.send(false);
        }
        let _ = self.app.emit("agent-permission-cancelled", ());
    }
}
