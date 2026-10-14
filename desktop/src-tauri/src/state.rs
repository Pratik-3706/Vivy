use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{watch, Mutex};

use crate::agent::permissions::PermissionManager;
use crate::phone::connection::PhoneManager;
use crate::sessions::SessionStore;

pub struct AppState {
    pub phone: PhoneManager,
    pub permissions: PermissionManager,
    /// Master switch for the AI. When off, no task may start and any running one is killed.
    pub agent_enabled: Arc<AtomicBool>,
    /// Bumped on every stop request; running tasks watch it and abort immediately.
    pub cancel_tx: watch::Sender<u64>,
    /// While true the running task waits between actions so the user can take over.
    pub pause_tx: watch::Sender<bool>,
    pub running_task_lock: Arc<Mutex<()>>,
    /// Saved chats; the active one holds the activity log and the model's memory.
    pub sessions: Arc<SessionStore>,
}

impl AppState {
    pub fn new(phone: PhoneManager, permissions: PermissionManager) -> Self {
        let (cancel_tx, _) = watch::channel(0u64);
        let (pause_tx, _) = watch::channel(false);
        Self {
            phone,
            permissions,
            agent_enabled: Arc::new(AtomicBool::new(true)),
            cancel_tx,
            pause_tx,
            running_task_lock: Arc::new(Mutex::new(())),
            sessions: Arc::new(SessionStore::load()),
        }
    }

    pub fn cancel_active_task(&self) {
        self.pause_tx.send_replace(false);
        self.cancel_tx.send_modify(|gen| *gen = gen.wrapping_add(1));
    }

    pub fn is_agent_enabled(&self) -> bool {
        self.agent_enabled.load(Ordering::SeqCst)
    }

    pub fn is_task_running(&self) -> bool {
        self.running_task_lock.try_lock().is_err()
    }
}

pub fn get_local_lan_ip() -> String {
    // 1. Quick local IP lookup via route socket probe (does not send network packets)
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        for probe_target in &["8.8.8.8:80", "1.1.1.1:80", "192.168.1.1:80", "10.0.0.1:80"] {
            if socket.connect(probe_target).is_ok() {
                if let Ok(addr) = socket.local_addr() {
                    let ip = addr.ip();
                    if !ip.is_loopback() {
                        return ip.to_string();
                    }
                }
            }
        }
    }

    // 2. Fallback: Query system hostname addresses
    if let Ok(hostname) = std::env::var("COMPUTERNAME") {
        use std::net::ToSocketAddrs;
        if let Ok(addrs) = format!("{}:0", hostname).to_socket_addrs() {
            for addr in addrs {
                if let std::net::SocketAddr::V4(v4) = addr {
                    let ip = v4.ip();
                    if !ip.is_loopback() && !ip.is_link_local() {
                        return ip.to_string();
                    }
                }
            }
        }
    }

    "127.0.0.1".to_string()
}
