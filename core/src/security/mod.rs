use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiCredentials {
    /// Which service answers model calls: aicredits, openai, anthropic, google or custom.
    #[serde(default = "default_provider")]
    pub provider: String,
    pub aicredits_key: String,
    #[serde(default)]
    pub openai_key: String,
    #[serde(default)]
    pub anthropic_key: String,
    #[serde(default)]
    pub google_key: String,
    /// Key for the custom endpoint in `base_url`.
    #[serde(default)]
    pub custom_key: String,
    pub tavily_key: String,
    pub model_name: String,
    /// Endpoint for the custom provider.
    pub base_url: String,
    /// Optional separate model for screenshot analysis. Empty = use model_name.
    #[serde(default)]
    pub vision_model: String,
    /// How hard the model should think: "low", "medium", "high", or empty for
    /// the model's default. Only sent to providers that accept it.
    #[serde(default)]
    pub reasoning_effort: String,
}

fn default_provider() -> String {
    "aicredits".to_string()
}

impl Default for ApiCredentials {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            aicredits_key: String::new(),
            openai_key: String::new(),
            anthropic_key: String::new(),
            google_key: String::new(),
            custom_key: String::new(),
            tavily_key: String::new(),
            model_name: DEFAULT_MODEL.to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
            vision_model: String::new(),
            reasoning_effort: String::new(),
        }
    }
}

impl ApiCredentials {
    /// The OpenAI-compatible endpoint for the chosen provider. OpenAI,
    /// Anthropic and Google all serve the same chat-completions API.
    pub fn api_base(&self) -> String {
        match self.provider.as_str() {
            "openai" => "https://api.openai.com/v1".to_string(),
            "anthropic" => "https://api.anthropic.com/v1".to_string(),
            "google" => "https://generativelanguage.googleapis.com/v1beta/openai".to_string(),
            "custom" => self.base_url.trim().trim_end_matches('/').to_string(),
            _ => DEFAULT_BASE_URL.to_string(),
        }
    }

    /// The saved key for the chosen provider.
    pub fn api_key(&self) -> &str {
        match self.provider.as_str() {
            "openai" => self.openai_key.trim(),
            "anthropic" => self.anthropic_key.trim(),
            "google" => self.google_key.trim(),
            "custom" => self.custom_key.trim(),
            _ => self.aicredits_key.trim(),
        }
    }

    /// Gateways (AICredits, OpenRouter-style) accept extra fields such as
    /// `include_reasoning`; first-party APIs may reject unknown fields.
    pub fn is_gateway(&self) -> bool {
        matches!(self.provider.as_str(), "aicredits" | "custom")
    }

    /// The chosen effort, if this provider accepts that level. Some models
    /// take only part of the range; a rejected level is retried without it.
    fn effort(&self) -> Option<&str> {
        let e = self.reasoning_effort.trim();
        let ok = match self.provider.as_str() {
            "openai" => matches!(e, "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"),
            "google" => {
                // Gemini 4 has no "minimal" level; its nearest is "low".
                if e == "minimal" && self.model_name.contains("gemini-4") {
                    return Some("low");
                }
                matches!(e, "minimal" | "low" | "medium" | "high")
            }
            _ => matches!(e, "low" | "medium" | "high"),
        };
        ok.then_some(e)
    }

    /// Fix fields a provider rejects. Applies to every request.
    pub fn fit_request(&self, body: &mut Value) {
        let Some(obj) = body.as_object_mut() else { return };
        match self.provider.as_str() {
            // Reasoning models take only the default temperature and the newer token field.
            "openai" => {
                obj.remove("temperature");
                if let Some(max) = obj.remove("max_tokens") {
                    obj.insert("max_completion_tokens".into(), max);
                }
            }
            // Current Claude models return an error for any temperature below 1.
            "anthropic" => {
                obj.remove("temperature");
            }
            _ => {}
        }
    }

    /// Ask for visible thinking and the chosen effort, in each provider's own
    /// terms. Optional: the agent drops these if a provider rejects them.
    /// OpenAI and Anthropic don't return their thinking on this API, and
    /// Claude picks its own effort, so they get only what they accept.
    pub fn add_thinking(&self, body: &mut Value, stream: bool) {
        let effort = self.effort();
        match self.provider.as_str() {
            "aicredits" => {
                body["include_reasoning"] = json!(true);
                if stream {
                    body["stream_options"] = json!({ "include_usage": true });
                }
                if let Some(e) = effort {
                    body["reasoning_effort"] = json!(e);
                }
            }
            "custom" => {
                body["include_reasoning"] = json!(true);
                if stream {
                    body["stream_options"] = json!({ "include_usage": true });
                }
            }
            "openai" => {
                if let Some(e) = effort {
                    body["reasoning_effort"] = json!(e);
                }
            }
            "google" => {
                // Gemini returns thought summaries only when asked. Its thinking
                // level can't be combined with reasoning_effort, so set it here.
                let mut config = json!({ "include_thoughts": true });
                if let Some(e) = effort {
                    config["thinking_level"] = json!(e);
                }
                body["extra_body"] = json!({ "google": { "thinking_config": config } });
            }
            _ => {}
        }
    }
}

/// The API host streams replies as they are generated. The website host
/// (`aicredits.in/v1`) also answers API calls but buffers every streamed
/// reply until it is complete, so thinking never showed live.
pub const DEFAULT_BASE_URL: &str = "https://api.aicredits.in/v1";
const LEGACY_BASE_URL: &str = "https://aicredits.in/v1";
pub const DEFAULT_MODEL: &str = "z-ai/glm-5.3-flash";

const SERVICE_NAME: &str = "VivyAIPhoneController";

/// Every saved setting: (credential name, getter, setter).
type Field = (&'static str, fn(&ApiCredentials) -> &String, fn(&mut ApiCredentials, String));
const FIELDS: &[Field] = &[
    ("provider", |c| &c.provider, |c, v| c.provider = v),
    ("aicredits_api_key", |c| &c.aicredits_key, |c, v| c.aicredits_key = v),
    ("openai_api_key", |c| &c.openai_key, |c, v| c.openai_key = v),
    ("anthropic_api_key", |c| &c.anthropic_key, |c, v| c.anthropic_key = v),
    ("google_api_key", |c| &c.google_key, |c, v| c.google_key = v),
    ("custom_api_key", |c| &c.custom_key, |c, v| c.custom_key = v),
    ("tavily_api_key", |c| &c.tavily_key, |c, v| c.tavily_key = v),
    ("model_name", |c| &c.model_name, |c, v| c.model_name = v),
    ("base_url", |c| &c.base_url, |c, v| c.base_url = v),
    ("vision_model", |c| &c.vision_model, |c, v| c.vision_model = v),
    ("reasoning_effort", |c| &c.reasoning_effort, |c, v| c.reasoning_effort = v),
];

/// Saves settings in the OS keyring (Windows Credential Manager, Secret Service on
/// Linux), or in a private file if the keyring is unavailable.
pub fn save_credentials_securely(creds: &ApiCredentials) -> Result<(), String> {
    let written = FIELDS
        .iter()
        .all(|(name, get, _)| save_keyring_entry(name, get(creds)).is_ok());

    // Only trust the vault if every value reads back exactly as written.
    let verified = written
        && FIELDS
            .iter()
            .all(|(name, get, _)| read_keyring_entry(name).as_deref() == Some(get(creds).as_str()));

    if verified {
        // The vault has everything; don't leave a plain-text copy of the keys around.
        if let Some(path) = get_fallback_path() {
            let _ = fs::remove_file(path);
        }
        Ok(())
    } else {
        // Credential vault unavailable: fall back to a local file so settings still persist.
        save_fallback_creds(creds)
    }
}

/// Settings for running the agent: the saved ones, with environment
/// overrides applied (see [`apply_env_overrides`]).
pub fn load_credentials_securely() -> ApiCredentials {
    let mut creds = load_saved_credentials();
    apply_env_overrides(&mut creds, |k| std::env::var(k).ok());
    creds
}

/// Settings exactly as saved, for editing and saving back, so a temporary
/// environment override is never written into the keyring.
pub fn load_saved_credentials() -> ApiCredentials {
    let fallback = load_fallback_creds();
    let mut creds = ApiCredentials::default();
    let mut has_provider = false;
    for (name, get, set) in FIELDS {
        let value = read_keyring_entry(name).or_else(|| fallback.as_ref().map(|f| get(f).clone()));
        if let Some(v) = value {
            has_provider |= *name == "provider";
            set(&mut creds, v);
        }
    }

    // The old website host buffered streams; the API host doesn't.
    if creds.base_url.trim().trim_end_matches('/') == LEGACY_BASE_URL {
        creds.base_url = DEFAULT_BASE_URL.to_string();
    }
    // Settings saved before providers existed: a non-AICredits URL was a custom endpoint.
    if !has_provider && creds.base_url.trim().trim_end_matches('/') != DEFAULT_BASE_URL {
        creds.provider = "custom".to_string();
        creds.custom_key = creds.aicredits_key.clone();
    }
    if creds.provider.trim().is_empty() {
        creds.provider = default_provider();
    }
    if creds.model_name.trim().is_empty() {
        creds.model_name = DEFAULT_MODEL.to_string();
    }
    creds
}

/// Environment variables win over saved settings, for scripts, CI and
/// headless machines without a keyring:
/// `VIVY_PROVIDER`, `VIVY_MODEL`, `VIVY_API_KEY` (for that provider),
/// `VIVY_BASE_URL` (custom provider), `VIVY_EFFORT`, `TAVILY_API_KEY`.
fn apply_env_overrides(creds: &mut ApiCredentials, var: impl Fn(&str) -> Option<String>) {
    let var = |k: &str| var(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    if let Some(p) = var("VIVY_PROVIDER") {
        creds.provider = p.to_ascii_lowercase();
    }
    if let Some(url) = var("VIVY_BASE_URL") {
        creds.base_url = url;
        if var("VIVY_PROVIDER").is_none() {
            creds.provider = "custom".to_string();
        }
    }
    if let Some(m) = var("VIVY_MODEL") {
        creds.model_name = m;
    }
    if let Some(e) = var("VIVY_EFFORT") {
        creds.reasoning_effort = e;
    }
    if let Some(k) = var("VIVY_API_KEY") {
        match creds.provider.as_str() {
            "openai" => creds.openai_key = k,
            "anthropic" => creds.anthropic_key = k,
            "google" => creds.google_key = k,
            "custom" => creds.custom_key = k,
            _ => creds.aicredits_key = k,
        }
    }
    if let Some(k) = var("TAVILY_API_KEY") {
        creds.tavily_key = k;
    }
}

fn save_keyring_entry(account: &str, secret: &str) -> Result<(), String> {
    std::panic::catch_unwind(|| {
        let entry = keyring::Entry::new(SERVICE_NAME, account)
            .map_err(|e| format!("Keyring access: {e}"))?;
        entry.set_password(secret)
            .map_err(|e| format!("Keyring write: {e}"))?;
        Ok::<(), String>(())
    })
    .map_err(|_| "Keyring panic caught".to_string())?
}

fn read_keyring_entry(account: &str) -> Option<String> {
    std::panic::catch_unwind(|| {
        let entry = keyring::Entry::new(SERVICE_NAME, account).ok()?;
        entry.get_password().ok()
    })
    .ok()
    .flatten()
}

/// Used only when the system keyring is unavailable (e.g. a Linux machine
/// without a Secret Service). Readable by the current user only.
fn get_fallback_path() -> Option<PathBuf> {
    crate::paths::data_path("config.json")
}

fn save_fallback_creds(creds: &ApiCredentials) -> Result<(), String> {
    if let Some(path) = get_fallback_path() {
        let json = serde_json::to_string(creds).map_err(|e| e.to_string())?;
        crate::paths::write_private(&path, json.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Where settings and keys currently live, for display.
pub fn credential_store_label() -> String {
    match get_fallback_path().filter(|p| p.exists()) {
        Some(p) => format!("{} (system keyring unavailable)", p.display()),
        None if cfg!(windows) => "Windows Credential Manager".to_string(),
        None if cfg!(target_os = "macos") => "macOS Keychain".to_string(),
        None => "system keyring (Secret Service)".to_string(),
    }
}

fn load_fallback_creds() -> Option<ApiCredentials> {
    let path = get_fallback_path()?;
    let data = fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creds(provider: &str, effort: &str) -> ApiCredentials {
        ApiCredentials { provider: provider.into(), reasoning_effort: effort.into(), ..Default::default() }
    }

    #[test]
    fn environment_overrides_saved_settings() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        };
        let mut c = ApiCredentials::default();
        apply_env_overrides(&mut c, env(&[("VIVY_BASE_URL", "http://localhost:9/v1"), ("VIVY_API_KEY", "k1")]));
        assert_eq!((c.provider.as_str(), c.custom_key.as_str(), c.api_base().as_str()), ("custom", "k1", "http://localhost:9/v1"));

        let mut c = ApiCredentials::default();
        apply_env_overrides(&mut c, env(&[("VIVY_PROVIDER", "Google"), ("VIVY_API_KEY", "g"), ("VIVY_MODEL", "m")]));
        assert_eq!((c.provider.as_str(), c.api_key(), c.model_name.as_str()), ("google", "g", "m"));

        let mut c = ApiCredentials::default();
        apply_env_overrides(&mut c, env(&[("VIVY_MODEL", "  ")]));
        assert_eq!(c.model_name, DEFAULT_MODEL);
    }

    #[test]
    fn requests_fit_each_provider() {
        let body = || json!({ "model": "m", "temperature": 0.2, "max_tokens": 10 });

        let mut b = body();
        creds("openai", "high").fit_request(&mut b);
        creds("openai", "high").add_thinking(&mut b, true);
        assert!(b.get("temperature").is_none());
        assert_eq!(b["max_completion_tokens"], 10);
        assert_eq!(b["reasoning_effort"], "high");

        let mut b = body();
        creds("anthropic", "high").fit_request(&mut b);
        creds("anthropic", "high").add_thinking(&mut b, true);
        assert!(b.get("temperature").is_none());
        assert!(b.get("reasoning_effort").is_none());

        let mut b = body();
        creds("google", "low").add_thinking(&mut b, true);
        assert_eq!(b["extra_body"]["google"]["thinking_config"]["include_thoughts"], true);
        assert_eq!(b["extra_body"]["google"]["thinking_config"]["thinking_level"], "low");
        assert!(b.get("reasoning_effort").is_none());

        let mut b = body();
        creds("openai", "max").add_thinking(&mut b, true);
        assert_eq!(b["reasoning_effort"], "max");

        // A level the provider doesn't take falls back to its default.
        let mut b = body();
        creds("google", "max").add_thinking(&mut b, true);
        assert!(b["extra_body"]["google"]["thinking_config"].get("thinking_level").is_none());

        let mut b = body();
        creds("aicredits", "").add_thinking(&mut b, true);
        assert_eq!(b["include_reasoning"], true);
        assert!(b.get("reasoning_effort").is_none());
        assert_eq!(b["temperature"], 0.2);
    }
}
