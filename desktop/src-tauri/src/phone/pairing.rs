//! Secure pairing between this PC and one phone.
//!
//! The connection is TLS with a certificate the PC makes for itself. The first
//! time, the user types a one-time code from the PC into the phone. Both sides
//! then prove they know the code, bound to the certificate the phone actually
//! sees, so nobody else on the network can pose as either end. The phone keeps
//! the certificate's fingerprint and a secret token; later connections check
//! both, and anything else is dropped before it sees a frame or a command.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_rustls::rustls;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio_rustls::TlsAcceptor;

/// How long a pairing code stays valid.
pub const CODE_LIFETIME: Duration = Duration::from_secs(5 * 60);
/// Wrong attempts allowed before a code is thrown away.
const MAX_CODE_FAILURES: u32 = 5;
/// Crockford base32: no I, L, O or U, so codes are easy to read and type.
const CODE_ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// 12 characters = 60 bits, far too many to guess in a code's lifetime.
const CODE_LEN: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedPhone {
    pub device_name: String,
    /// SHA-256 of the phone's token; the token itself is never stored here.
    token_sha256: String,
    pub paired_at: u64,
}

struct ActiveCode {
    code: String,
    expires_at: u64,
    failures: u32,
}

#[derive(Default)]
struct State {
    paired: Option<PairedPhone>,
    code: Option<ActiveCode>,
}

/// What the UI shows about pairing.
#[derive(Debug, Clone, Serialize)]
pub struct PairingView {
    pub paired: Option<PairedPhone>,
    /// Formatted like `7KQ4-M2XD-9HPA`.
    pub code: Option<String>,
    pub code_expires_at: Option<u64>,
}

pub struct Pairing {
    dir: Option<PathBuf>,
    state: Mutex<State>,
    /// SHA-256 of our certificate, lowercase hex. Bound into every proof.
    pub fingerprint: String,
    pub acceptor: TlsAcceptor,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    hex(&buf)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// Compare without leaking how many leading characters matched.
fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Uppercase, drop separators, and fix look-alike characters people type by mistake.
pub fn normalize_code(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        })
        .collect()
}

pub fn format_code(code: &str) -> String {
    code.as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join("-")
}

fn new_code() -> String {
    let mut rng = rand::thread_rng();
    (0..CODE_LEN)
        .map(|_| CODE_ALPHABET[(rng.next_u32() % CODE_ALPHABET.len() as u32) as usize] as char)
        .collect()
}

/// Proof that the sender knows the code, tied to the certificate in use and to
/// this exchange's nonces. `role` is "pc" or "phone" so one can't be replayed as the other.
pub fn proof(code: &str, role: &str, fingerprint: &str, phone_nonce: &str, pc_nonce: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(normalize_code(code).as_bytes()).expect("any key length works");
    mac.update(format!("vivy-pair|{role}|{fingerprint}|{phone_nonce}|{pc_nonce}").as_bytes());
    hex(&mac.finalize().into_bytes())
}

pub fn new_nonce() -> String {
    random_hex(16)
}

impl Pairing {
    /// Loads (or creates) the TLS identity and the paired phone, if any.
    pub fn load() -> Result<Self, String> {
        Self::load_in(std::env::var("LOCALAPPDATA").ok().map(|d| PathBuf::from(d).join("Vivy")))
    }

    /// Same as [`Pairing::load`], storing everything under `dir`.
    pub fn load_in(dir: Option<PathBuf>) -> Result<Self, String> {
        if let Some(d) = &dir {
            let _ = std::fs::create_dir_all(d.join("tls"));
        }
        let (cert, key) = load_or_create_identity(dir.as_deref())?;
        let fingerprint = sha256_hex(&cert);

        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| format!("TLS setup failed: {e}"))?
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(cert)],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)),
            )
            .map_err(|e| format!("TLS certificate rejected: {e}"))?;

        let paired = dir
            .as_ref()
            .and_then(|d| std::fs::read_to_string(d.join("pairing.json")).ok())
            .and_then(|s| serde_json::from_str::<PairedPhone>(&s).ok());

        Ok(Self {
            dir,
            state: Mutex::new(State { paired, code: None }),
            fingerprint,
            acceptor: TlsAcceptor::from(Arc::new(config)),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Current state. While no phone is paired, a fresh code is always ready.
    pub fn view(&self) -> PairingView {
        let mut s = self.lock();
        let now = now_ms();
        if s.code.as_ref().is_some_and(|c| c.expires_at <= now) {
            s.code = None;
        }
        if s.paired.is_none() && s.code.is_none() {
            s.code = Some(ActiveCode {
                code: new_code(),
                expires_at: now + CODE_LIFETIME.as_millis() as u64,
                failures: 0,
            });
        }
        PairingView {
            paired: s.paired.clone(),
            code: s.code.as_ref().map(|c| format_code(&c.code)),
            code_expires_at: s.code.as_ref().map(|c| c.expires_at),
        }
    }

    /// Show a new code (to pair a different phone, or after one expired).
    pub fn start(&self) -> PairingView {
        self.lock().code = Some(ActiveCode {
            code: new_code(),
            expires_at: now_ms() + CODE_LIFETIME.as_millis() as u64,
            failures: 0,
        });
        self.view()
    }

    /// Stop offering a code (only meaningful while a phone is already paired).
    pub fn cancel(&self) -> PairingView {
        let mut s = self.lock();
        if s.paired.is_some() {
            s.code = None;
        }
        drop(s);
        self.view()
    }

    /// Unpair the phone. It has to be paired again with a new code.
    pub fn forget(&self) -> PairingView {
        self.lock().paired = None;
        if let Some(d) = &self.dir {
            let _ = std::fs::remove_file(d.join("pairing.json"));
        }
        self.view()
    }

    /// The code, if one is open right now.
    pub fn active_code(&self) -> Option<String> {
        let s = self.lock();
        s.code.as_ref().filter(|c| c.expires_at > now_ms()).map(|c| c.code.clone())
    }

    /// Count a wrong pairing attempt; too many and the code is thrown away.
    pub fn record_failure(&self) {
        let mut s = self.lock();
        if let Some(c) = s.code.as_mut() {
            c.failures += 1;
            if c.failures >= MAX_CODE_FAILURES {
                s.code = None;
            }
        }
    }

    /// Pairing succeeded: remember the phone and return the token it must
    /// present from now on. Replaces any previously paired phone.
    pub fn complete(&self, device_name: &str) -> String {
        let token = random_hex(32);
        let phone = PairedPhone {
            device_name: device_name.chars().take(80).collect(),
            token_sha256: sha256_hex(token.as_bytes()),
            paired_at: now_ms(),
        };
        if let Some(d) = &self.dir {
            if let Ok(json) = serde_json::to_string(&phone) {
                let _ = std::fs::write(d.join("pairing.json"), json);
            }
        }
        let mut s = self.lock();
        s.paired = Some(phone);
        s.code = None;
        token
    }

    /// Whether this token belongs to the paired phone.
    pub fn check_token(&self, token: &str) -> bool {
        let s = self.lock();
        s.paired
            .as_ref()
            .is_some_and(|p| constant_time_eq(&sha256_hex(token.as_bytes()), &p.token_sha256))
    }
}

/// The PC's certificate and private key (DER), made once and kept in
/// %LOCALAPPDATA%\Vivy\tls so the phone's pinned fingerprint stays valid.
fn load_or_create_identity(dir: Option<&std::path::Path>) -> Result<(Vec<u8>, Vec<u8>), String> {
    let paths = dir.map(|d| (d.join("tls").join("cert.der"), d.join("tls").join("key.der")));
    if let Some((cert_path, key_path)) = &paths {
        if let (Ok(cert), Ok(key)) = (std::fs::read(cert_path), std::fs::read(key_path)) {
            if !cert.is_empty() && !key.is_empty() {
                return Ok((cert, key));
            }
        }
    }
    let generated = rcgen::generate_simple_self_signed(vec!["vivy.local".to_string()])
        .map_err(|e| format!("Couldn't create the TLS certificate: {e}"))?;
    let cert = generated.cert.der().to_vec();
    let key = generated.key_pair.serialize_der();
    if let Some((cert_path, key_path)) = &paths {
        let _ = std::fs::write(cert_path, &cert);
        let _ = std::fs::write(key_path, &key);
    }
    Ok((cert, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_readable_and_forgiving() {
        let code = new_code();
        assert_eq!(code.len(), CODE_LEN);
        assert!(code.bytes().all(|b| CODE_ALPHABET.contains(&b)));
        assert_eq!(normalize_code("7kq4-m2xd 9hpa"), "7KQ4M2XD9HPA");
        assert_eq!(normalize_code("o0-il"), "0011");
        assert_eq!(format_code("7KQ4M2XD9HPA"), "7KQ4-M2XD-9HPA");
    }

    #[test]
    fn proofs_bind_code_role_and_certificate() {
        let p = proof("7KQ4-M2XD-9HPA", "pc", "abc", "n1", "n2");
        assert_eq!(p, proof("7kq4m2xd9hpa", "pc", "abc", "n1", "n2"));
        assert_ne!(p, proof("7KQ4-M2XD-9HPB", "pc", "abc", "n1", "n2"));
        assert_ne!(p, proof("7KQ4-M2XD-9HPA", "phone", "abc", "n1", "n2"));
        assert_ne!(p, proof("7KQ4-M2XD-9HPA", "pc", "evil", "n1", "n2"));
        // Known value, checked against the phone's Java HMAC and Python's hmac module.
        assert_eq!(p, "cc790a15478303e6bbbe9f3d93985bf76094b16f193db481837bc4bb7d417601");
    }

    #[test]
    fn constant_time_compare() {
        assert!(constant_time_eq("abcd", "abcd"));
        assert!(!constant_time_eq("abcd", "abce"));
        assert!(!constant_time_eq("abc", "abcd"));
    }
}
