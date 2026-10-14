use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, Mutex, Notify, RwLock};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use super::commands::{DeviceState, PhoneCommand, PhoneMessage};
use super::pairing::{self, Pairing};

type WsSink<S> = futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<S>, Message>;
type WsSource<S> = futures_util::stream::SplitStream<tokio_tungstenite::WebSocketStream<S>>;

/// A phone gets this long to finish TLS and prove who it is.
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);
/// Typing the pairing code on the phone happens before connecting, so the
/// exchange itself is quick; this only bounds a stalled attempt.
const PAIR_TIMEOUT: Duration = Duration::from_secs(30);

type CommandReply = Result<Option<String>, String>;
type PendingResponses = Arc<Mutex<HashMap<String, oneshot::Sender<CommandReply>>>>;

#[derive(Clone)]
pub struct PhoneManager {
    app: AppHandle,
    /// Sender for the active connection, tagged with that connection's id so a
    /// stale connection closing never tears down a newer one. The Notify closes it.
    tx: Arc<Mutex<Option<(u64, mpsc::Sender<PhoneCommand>, Arc<Notify>)>>>,
    next_conn_id: Arc<AtomicU64>,
    pending: PendingResponses,
    latest_frame: Arc<RwLock<Option<String>>>,
    device_state: Arc<RwLock<DeviceState>>,
    /// TLS identity and the paired phone. None if TLS couldn't be set up, in
    /// which case no phone can connect at all (never falls back to plain text).
    pub pairing: Option<Arc<Pairing>>,
}

impl PhoneManager {
    pub fn new(app: AppHandle) -> Self {
        let pairing = match Pairing::load() {
            Ok(p) => Some(Arc::new(p)),
            Err(e) => {
                eprintln!("[Vivy] Secure pairing unavailable: {e}");
                None
            }
        };
        Self {
            pairing,
            app,
            tx: Arc::new(Mutex::new(None)),
            next_conn_id: Arc::new(AtomicU64::new(1)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            latest_frame: Arc::new(RwLock::new(None)),
            device_state: Arc::new(RwLock::new(DeviceState::default())),
        }
    }

    pub async fn get_device_state(&self) -> DeviceState {
        self.device_state.read().await.clone()
    }

    /// Retrieves the most recent screen frame held strictly in memory.
    /// Never touches disk or records video.
    pub async fn get_latest_frame(&self) -> Option<String> {
        self.latest_frame.read().await.clone()
    }

    pub async fn is_connected(&self) -> bool {
        self.tx.lock().await.is_some()
    }

    /// Send a command to the phone and wait for acknowledgment.
    pub async fn execute_command(&self, cmd: PhoneCommand, timeout_secs: u64) -> Result<(), String> {
        self.request(cmd, timeout_secs).await.map(|_| ())
    }

    /// Send a command and return the optional data payload the phone replies with.
    pub async fn request(&self, cmd: PhoneCommand, timeout_secs: u64) -> CommandReply {
        let cmd_id = cmd.id().to_string();

        let sender = {
            let guard = self.tx.lock().await;
            guard.as_ref().map(|(_, s, _)| s.clone())
        };
        let Some(sender) = sender else {
            return Err("Phone is not connected".to_string());
        };

        let (resp_tx, resp_rx) = oneshot::channel();
        self.pending.lock().await.insert(cmd_id.clone(), resp_tx);

        if let Err(e) = sender.send(cmd).await {
            self.pending.lock().await.remove(&cmd_id);
            return Err(format!("Failed to send command to phone: {e}"));
        }

        match tokio::time::timeout(Duration::from_secs(timeout_secs), resp_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("Phone disconnected before replying".to_string()),
            Err(_) => {
                self.pending.lock().await.remove(&cmd_id);
                Err(format!("Phone did not reply within {timeout_secs}s"))
            }
        }
    }

    /// Close the current phone connection, if any (used after "Forget phone").
    pub async fn disconnect(&self) {
        if let Some((_, _, kick)) = self.tx.lock().await.as_ref() {
            kick.notify_one();
        }
    }

    async fn store_frame(&self, b64: String) {
        *self.latest_frame.write().await = Some(b64.clone());
        {
            let mut state = self.device_state.write().await;
            state.last_seen = chrono::Utc::now().timestamp_millis() as u64;
            if !state.streaming {
                state.streaming = true;
                let _ = self.app.emit("phone-status", state.clone());
            }
        }
        // Live frame for the UI (memory only, never written to disk)
        let _ = self.app.emit("phone-frame", json!({ "frame": b64 }));
    }
}

/// Starts the background WebSocket server
pub fn start_websocket_server(manager: PhoneManager, port: u16) {
    tauri::async_runtime::spawn(async move {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        if manager.pairing.is_none() {
            let _ = manager.app.emit(
                "phone-error",
                json!({ "error": "Secure connection setup failed, so phones can't connect. Restart Vivy to try again." }),
            );
            return;
        }
        let listener = match TcpListener::bind(addr).await {
            Ok(l) => {
                println!("[Vivy WebSocket] Server listening on wss://0.0.0.0:{port}");
                l
            }
            Err(e) => {
                eprintln!("[Vivy WebSocket] Failed to bind port {port}: {e}");
                let _ = manager.app.emit(
                    "phone-error",
                    json!({ "error": format!("Couldn't open port {port}: {e}. Is another copy of Vivy running?") }),
                );
                return;
            }
        };

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    println!("[Vivy WebSocket] Incoming connection from {peer_addr}");
                    let _ = stream.set_nodelay(true);
                    let mgr_clone = manager.clone();
                    tauri::async_runtime::spawn(async move {
                        handle_connection(stream, mgr_clone, peer_addr).await;
                    });
                }
                Err(e) => {
                    eprintln!("[Vivy WebSocket] Accept error: {e}");
                }
            }
        }
    });
}

async fn handle_connection(stream: TcpStream, manager: PhoneManager, peer_addr: SocketAddr) {
    let Some(pairing) = manager.pairing.clone() else { return };

    // Everything is encrypted. A plain ws:// client fails right here.
    let tls = match tokio::time::timeout(AUTH_TIMEOUT, pairing.acceptor.accept(stream)).await {
        Ok(Ok(tls)) => tls,
        _ => {
            eprintln!("[Vivy WebSocket] TLS handshake failed with {peer_addr}");
            return;
        }
    };
    let ws = match tokio::time::timeout(AUTH_TIMEOUT, tokio_tungstenite::accept_async(tls)).await {
        Ok(Ok(ws)) => ws,
        _ => {
            eprintln!("[Vivy WebSocket] WebSocket handshake failed with {peer_addr}");
            return;
        }
    };
    let (mut ws_sink, mut ws_stream) = ws.split();

    // Nothing is shared with a connection until it proves it's the paired phone.
    let app = manager.app.clone();
    let on_change = move |view: pairing::PairingView| {
        let _ = app.emit("pairing-changed", view);
    };
    if let Err(reason) = authenticate(&mut ws_sink, &mut ws_stream, &pairing, &on_change).await {
        eprintln!("[Vivy WebSocket] Rejected {peer_addr}: {reason}");
        let _ = ws_sink.send(Message::Close(None)).await;
        return;
    }

    let conn_id = manager.next_conn_id.fetch_add(1, Ordering::Relaxed);
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<PhoneCommand>(32);
    let kick = Arc::new(Notify::new());

    // The newest connection of the paired phone wins; an older one is closed.
    if let Some((_, _, old_kick)) = manager.tx.lock().await.replace((conn_id, cmd_tx, kick.clone())) {
        old_kick.notify_one();
    }
    {
        let mut state = manager.device_state.write().await;
        state.connected = true;
        state.streaming = false;
        state.accessibility = None;
        state.screen_share = None;
        state.last_seen = chrono::Utc::now().timestamp_millis() as u64;
        let _ = manager.app.emit("phone-connected", state.clone());
    }

    let write_task = tauri::async_runtime::spawn(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            let json_str = match serde_json::to_string(&cmd) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[Vivy WebSocket] Command serialize error: {e}");
                    continue;
                }
            };
            if let Err(e) = ws_sink.send(Message::Text(json_str.into())).await {
                eprintln!("[Vivy WebSocket] Send command error: {e}");
                break;
            }
        }
    });

    loop {
        let msg_res = tokio::select! {
            m = ws_stream.next() => match m {
                Some(m) => m,
                None => break,
            },
            _ = kick.notified() => {
                println!("[Vivy WebSocket] Closing connection from {peer_addr}");
                break;
            }
        };
        match msg_res {
            Ok(Message::Text(text)) => {
                process_incoming_message(&text, &manager).await;
            }
            Ok(Message::Binary(bin)) => {
                // Raw JPEG frame bytes (the phone's default, cheaper than base64 JSON)
                let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bin);
                manager.store_frame(b64).await;
            }
            Ok(Message::Close(_)) => {
                println!("[Vivy WebSocket] Client requested close: {peer_addr}");
                break;
            }
            Err(e) => {
                eprintln!("[Vivy WebSocket] Stream error from {peer_addr}: {e}");
                break;
            }
            _ => {}
        }
    }

    write_task.abort();

    // Only tear down shared state if this is still the active connection.
    let was_active = {
        let mut guard = manager.tx.lock().await;
        if matches!(guard.as_ref(), Some((id, _, _)) if *id == conn_id) {
            *guard = None;
            true
        } else {
            false
        }
    };

    if was_active {
        // Fail anything still waiting on this phone instead of letting it time out.
        let drained: Vec<_> = manager.pending.lock().await.drain().collect();
        for (_, ch) in drained {
            let _ = ch.send(Err("Phone disconnected".to_string()));
        }
        *manager.latest_frame.write().await = None;
        {
            let mut state = manager.device_state.write().await;
            state.connected = false;
            state.streaming = false;
        }
        let _ = manager.app.emit("phone-disconnected", ());
    }
    println!("[Vivy WebSocket] Phone disconnected: {peer_addr}");
}

/// Next text message as JSON, within `limit`.
async fn next_json<S>(stream: &mut WsSource<S>, limit: Duration) -> Result<serde_json::Value, String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let deadline = tokio::time::Instant::now() + limit;
    loop {
        let msg = tokio::time::timeout_at(deadline, stream.next())
            .await
            .map_err(|_| "timed out".to_string())?
            .ok_or("closed")?
            .map_err(|e| e.to_string())?;
        match msg {
            Message::Text(t) => return serde_json::from_str(&t).map_err(|_| "not JSON".to_string()),
            Message::Ping(_) | Message::Pong(_) => continue,
            _ => return Err("unexpected message".into()),
        }
    }
}

async fn send_json<S>(sink: &mut WsSink<S>, value: serde_json::Value) -> Result<(), String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    sink.send(Message::Text(value.to_string().into())).await.map_err(|e| e.to_string())
}

/// The phone's first message must be either `auth` (already paired) or
/// `pair_hello` (pairing with a code shown on this PC).
pub(crate) async fn authenticate<S>(
    sink: &mut WsSink<S>,
    stream: &mut WsSource<S>,
    pairing: &Pairing,
    on_change: &(dyn Fn(pairing::PairingView) + Send + Sync),
) -> Result<(), String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let pc_name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".to_string());
    let first = next_json(stream, AUTH_TIMEOUT).await?;
    match first["type"].as_str().unwrap_or("") {
        "auth" => {
            let token = first["token"].as_str().unwrap_or("");
            if pairing.check_token(token) {
                send_json(sink, json!({ "type": "auth_ok", "pc_name": pc_name })).await
            } else {
                let _ = send_json(
                    sink,
                    json!({
                        "type": "auth_failed",
                        "message": "This phone isn't paired with this PC anymore. Pair it again with the code shown on the PC."
                    }),
                )
                .await;
                Err("unknown token".into())
            }
        }
        "pair_hello" => {
            let Some(code) = pairing.active_code() else {
                let _ = send_json(
                    sink,
                    json!({
                        "type": "pair_error",
                        "message": "Pairing isn't open on the PC. Click \"Pair a phone\" in Vivy on the PC to get a code."
                    }),
                )
                .await;
                return Err("pairing not open".into());
            };
            let phone_nonce = first["nonce"].as_str().unwrap_or("").to_string();
            if phone_nonce.len() < 16 {
                return Err("bad nonce".into());
            }
            let pc_nonce = pairing::new_nonce();
            // We prove first, so the phone can check it reached the right PC
            // before it reveals anything about the code.
            let pc_proof = pairing::proof(&code, "pc", &pairing.fingerprint, &phone_nonce, &pc_nonce);
            send_json(sink, json!({ "type": "pair_challenge", "nonce": pc_nonce, "proof": pc_proof })).await?;

            let confirm = next_json(stream, PAIR_TIMEOUT).await?;
            if confirm["type"] != "pair_confirm" {
                return Err("expected pair_confirm".into());
            }
            let expected = pairing::proof(&code, "phone", &pairing.fingerprint, &phone_nonce, &pc_nonce);
            if confirm["proof"].as_str() != Some(expected.as_str()) {
                pairing.record_failure();
                on_change(pairing.view());
                let _ = send_json(
                    sink,
                    json!({ "type": "pair_error", "message": "That code is wrong or has expired. Check the code on the PC and try again." }),
                )
                .await;
                return Err("wrong pairing code".into());
            }
            let device_name = confirm["device_name"].as_str().unwrap_or("Phone").to_string();
            let token = pairing.complete(&device_name);
            on_change(pairing.view());
            send_json(sink, json!({ "type": "paired", "token": token, "pc_name": pc_name })).await
        }
        _ => Err("first message wasn't auth or pairing".into()),
    }
}

async fn process_incoming_message(text: &str, manager: &PhoneManager) {
    let msg: PhoneMessage = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            let sample: String = text.chars().take(80).collect();
            eprintln!("[Vivy WebSocket] Parse error: {e}. Prefix: {sample}");
            return;
        }
    };

    match msg {
        PhoneMessage::Connected {
            device_name,
            brand,
            model,
            android_version,
            screen_width,
            screen_height,
            battery_level,
            is_charging,
            accessibility,
            screen_share,
        } => {
            {
                let mut state = manager.device_state.write().await;
                state.connected = true;
                state.accessibility = accessibility;
                state.screen_share = screen_share;
                state.device_name = device_name;
                state.brand = brand;
                state.model = model;
                state.android_version = android_version;
                state.screen_width = screen_width;
                state.screen_height = screen_height;
                state.battery_level = battery_level;
                state.is_charging = is_charging;
                state.last_seen = chrono::Utc::now().timestamp_millis() as u64;
                let _ = manager.app.emit("phone-status", state.clone());
            }

            // Ask for the stream from a separate task so the reader loop keeps
            // draining messages (the reply arrives through this same loop).
            let mgr_cmd = manager.clone();
            tauri::async_runtime::spawn(async move {
                let res = mgr_cmd
                    .execute_command(
                        PhoneCommand::StartStream {
                            id: Uuid::new_v4().to_string(),
                            fps: 12,
                            quality: 65,
                            max_dimension: 1280,
                        },
                        5,
                    )
                    .await;
                if let Err(e) = res {
                    let _ = mgr_cmd.app.emit("phone-error", json!({ "error": format!("Screen stream didn't start: {e}") }));
                }
            });
        }
        PhoneMessage::DeviceStatus { accessibility, screen_share } => {
            let mut state = manager.device_state.write().await;
            if accessibility.is_some() {
                state.accessibility = accessibility;
            }
            if screen_share.is_some() {
                state.screen_share = screen_share;
            }
            let _ = manager.app.emit("phone-status", state.clone());
        }
        PhoneMessage::Frame { data, .. } => {
            manager.store_frame(data).await;
        }
        PhoneMessage::CommandResult { id, success, error, data } => {
            if let Some(ch) = manager.pending.lock().await.remove(&id) {
                let _ = ch.send(if success {
                    Ok(data)
                } else {
                    Err(error.unwrap_or_else(|| "Phone reported an unknown error".to_string()))
                });
            }
        }
        PhoneMessage::DeviceInfo {
            device_name,
            brand,
            model,
            screen_width,
            screen_height,
            battery_level,
            is_charging,
        } => {
            let mut state = manager.device_state.write().await;
            state.device_name = device_name;
            state.brand = brand;
            state.model = model;
            state.screen_width = screen_width;
            state.screen_height = screen_height;
            state.battery_level = battery_level;
            state.is_charging = is_charging;
            state.last_seen = chrono::Utc::now().timestamp_millis() as u64;
            let _ = manager.app.emit("phone-status", state.clone());
        }
        PhoneMessage::Heartbeat { .. } => {
            manager.device_state.write().await.last_seen = chrono::Utc::now().timestamp_millis() as u64;
        }
        PhoneMessage::Error { message } => {
            let _ = manager.app.emit("phone-error", json!({ "error": message }));
        }
        PhoneMessage::CrashReport { report } => {
            // Keep the full trace on disk for diagnosis; the UI gets a short note.
            if let Ok(dir) = std::env::var("LOCALAPPDATA") {
                let dir = std::path::PathBuf::from(dir).join("Vivy");
                let _ = std::fs::create_dir_all(&dir);
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join("phone-crash.log"))
                {
                    use std::io::Write;
                    let _ = writeln!(f, "{report}
----");
                }
            }
            let first = report.lines().nth(1).unwrap_or("").trim().to_string();
            let _ = manager.app.emit(
                "phone-error",
                json!({ "error": format!("The phone app crashed earlier ({first}). Details saved in %LOCALAPPDATA%\\Vivy\\phone-crash.log.") }),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    //! The whole pairing exchange over real TLS, with a fake phone that does
    //! what the Android app does.
    use super::*;
    use sha2::{Digest, Sha256};
    use tokio_rustls::rustls::{
        self,
        client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
        pki_types::{CertificateDer, ServerName, UnixTime},
        DigitallySignedStruct, SignatureScheme,
    };

    /// Accepts any certificate but records its fingerprint, like the phone while pairing.
    #[derive(Debug)]
    struct Capture {
        seen: std::sync::Mutex<Option<String>>,
        provider: Arc<rustls::crypto::CryptoProvider>,
    }

    impl ServerCertVerifier for Capture {
        fn verify_server_cert(
            &self,
            end_entity: &CertificateDer<'_>,
            _: &[CertificateDer<'_>],
            _: &ServerName<'_>,
            _: &[u8],
            _: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            let fp: String = Sha256::digest(end_entity.as_ref()).iter().map(|b| format!("{b:02x}")).collect();
            *self.seen.lock().unwrap() = Some(fp);
            Ok(ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
        }
        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
        }
        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            self.provider.signature_verification_algorithms.supported_schemes()
        }
    }

    type ClientWs = tokio_tungstenite::WebSocketStream<tokio_rustls::client::TlsStream<tokio::io::DuplexStream>>;

    /// Opens a TLS WebSocket to a server task running `authenticate`.
    async fn open(pairing: Arc<Pairing>) -> (ClientWs, Arc<Capture>, tokio::task::JoinHandle<Result<(), String>>) {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(async move {
            let tls = pairing.acceptor.accept(server_io).await.map_err(|e| e.to_string())?;
            let ws = tokio_tungstenite::accept_async(tls).await.map_err(|e| e.to_string())?;
            let (mut sink, mut stream) = ws.split();
            authenticate(&mut sink, &mut stream, &pairing, &|_| {}).await
        });

        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let capture = Arc::new(Capture { seen: Default::default(), provider: provider.clone() });
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(capture.clone())
            .with_no_client_auth();
        let tls = tokio_rustls::TlsConnector::from(Arc::new(config))
            .connect(ServerName::try_from("vivy.local").unwrap(), client_io)
            .await
            .unwrap();
        let (ws, _) = tokio_tungstenite::client_async("wss://vivy.local/", tls).await.unwrap();
        (ws, capture, server)
    }

    async fn send(ws: &mut ClientWs, v: serde_json::Value) {
        ws.send(Message::Text(v.to_string().into())).await.unwrap();
    }

    async fn recv(ws: &mut ClientWs) -> serde_json::Value {
        loop {
            if let Message::Text(t) = ws.next().await.unwrap().unwrap() {
                return serde_json::from_str(&t).unwrap();
            }
        }
    }

    /// Runs the phone's side of pairing with `code`.
    async fn pair(pairing: Arc<Pairing>, code: &str) -> (serde_json::Value, Result<(), String>, String) {
        let (mut ws, capture, server) = open(pairing).await;
        let phone_nonce = pairing::new_nonce();
        send(&mut ws, json!({ "type": "pair_hello", "nonce": phone_nonce })).await;
        let challenge = recv(&mut ws).await;
        if challenge["type"] != "pair_challenge" {
            return (challenge, server.await.unwrap(), String::new());
        }
        let fp = capture.seen.lock().unwrap().clone().unwrap();
        let pc_nonce = challenge["nonce"].as_str().unwrap();
        let pc_proved = challenge["proof"] == pairing::proof(code, "pc", &fp, &phone_nonce, pc_nonce);
        send(
            &mut ws,
            json!({
                "type": "pair_confirm",
                "proof": pairing::proof(code, "phone", &fp, &phone_nonce, pc_nonce),
                "device_name": "Test Phone"
            }),
        )
        .await;
        let reply = recv(&mut ws).await;
        (json!({ "reply": reply, "pc_proved": pc_proved }), server.await.unwrap(), fp)
    }

    #[tokio::test]
    async fn pairs_then_only_the_paired_phone_gets_in() {
        let dir = std::env::temp_dir().join(format!("vivy-pair-test-{}", Uuid::new_v4()));
        let pairing = Arc::new(Pairing::load_in(Some(dir.clone())).unwrap());
        let code = pairing.view().code.unwrap();

        // A wrong code is refused, and the phone can tell the PC's proof didn't match.
        let (out, result, _) = pair(pairing.clone(), "0000-0000-0000").await;
        assert_eq!(out["pc_proved"], false);
        assert_eq!(out["reply"]["type"], "pair_error");
        assert!(result.is_err());
        assert!(pairing.view().paired.is_none());

        // The right code pairs, and the PC proved itself with this certificate.
        let (out, result, fp) = pair(pairing.clone(), &code).await;
        assert_eq!(out["pc_proved"], true);
        assert_eq!(out["reply"]["type"], "paired");
        assert!(result.is_ok());
        assert_eq!(fp, pairing.fingerprint);
        let token = out["reply"]["token"].as_str().unwrap().to_string();
        assert_eq!(pairing.view().paired.unwrap().device_name, "Test Phone");
        assert!(pairing.view().code.is_none(), "code is used up");

        // Reconnecting with the token works.
        let (mut ws, _, server) = open(pairing.clone()).await;
        send(&mut ws, json!({ "type": "auth", "token": token })).await;
        assert_eq!(recv(&mut ws).await["type"], "auth_ok");
        assert!(server.await.unwrap().is_ok());

        // Anyone else is turned away.
        let (mut ws, _, server) = open(pairing.clone()).await;
        send(&mut ws, json!({ "type": "auth", "token": "f".repeat(64) })).await;
        assert_eq!(recv(&mut ws).await["type"], "auth_failed");
        assert!(server.await.unwrap().is_err());

        // Pairing is closed once a phone is paired, so the old code can't be reused.
        let (out, result, _) = pair(pairing.clone(), &code).await;
        assert_eq!(out["type"], "pair_error");
        assert!(result.is_err());

        // Sending anything other than auth or pairing first gets nothing.
        let (mut ws, _, server) = open(pairing.clone()).await;
        send(&mut ws, json!({ "type": "connected", "device_name": "x" })).await;
        assert!(server.await.unwrap().is_err());
        drop(ws);

        // The pairing survives a restart (same certificate, same phone).
        let reloaded = Pairing::load_in(Some(dir.clone())).unwrap();
        assert_eq!(reloaded.fingerprint, pairing.fingerprint);
        assert!(reloaded.check_token(&token));

        // After forgetting, the old token stops working.
        pairing.forget();
        assert!(!pairing.check_token(&token));

        let _ = std::fs::remove_dir_all(dir);
    }
}
