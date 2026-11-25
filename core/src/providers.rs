//! The providers and suggested models, for front ends that list them.
//! Keep in sync with desktop/src/lib/providers.ts. Endpoints and request
//! details live in `security`.

pub struct ModelOption {
    pub id: &'static str,
    pub name: &'static str,
    /// What it's good for.
    pub note: &'static str,
    /// The provider's everyday pick.
    pub everyday: bool,
    /// The provider's top model, for complex tasks.
    pub complex: bool,
}

pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    /// Where to create a key.
    pub key_url: &'static str,
    /// Thinking-effort levels the provider accepts, fastest first. Empty = no setting.
    pub efforts: &'static [&'static str],
    pub models: &'static [ModelOption],
}

const fn m(id: &'static str, name: &'static str, note: &'static str) -> ModelOption {
    ModelOption { id, name, note, everyday: false, complex: false }
}
const fn everyday(id: &'static str, name: &'static str) -> ModelOption {
    ModelOption { id, name, note: "Everyday", everyday: true, complex: false }
}
const fn complex(id: &'static str, name: &'static str) -> ModelOption {
    ModelOption { id, name, note: "Complex tasks", everyday: false, complex: true }
}

pub const PROVIDERS: &[Provider] = &[
    Provider {
        id: "aicredits",
        name: "AICredits",
        key_url: "https://aicredits.in",
        efforts: &["low", "medium", "high"],
        models: &[
            everyday("z-ai/glm-5.3-flash", "GLM 5.3 Flash"),
            m("anthropic/claude-haiku-5.5", "Claude Haiku 5.5", "Quick tasks"),
            m("openai/gpt-6-luna", "GPT-6 Luna", "Quick tasks"),
            m("deepseek/deepseek-v4.1-flash", "DeepSeek V4.1 Flash", "Everyday"),
            m("moonshotai/kimi-k2.6", "Kimi K2.6", "Everyday"),
            complex("anthropic/claude-opus-5.5", "Claude Opus 5.5"),
        ],
    },
    Provider {
        id: "openai",
        name: "OpenAI",
        key_url: "https://platform.openai.com/api-keys",
        efforts: &["none", "minimal", "low", "medium", "high", "xhigh", "max"],
        models: &[
            everyday("gpt-6.1-sol", "GPT-6.1 Sol"),
            m("gpt-6-luna", "GPT-6 Luna", "Quick tasks"),
            m("gpt-5-mini", "GPT-5 Mini", "Simple tasks"),
            complex("gpt-6-astra", "GPT-6 Astra"),
        ],
    },
    Provider {
        id: "anthropic",
        name: "Anthropic",
        key_url: "https://console.anthropic.com/settings/keys",
        efforts: &[],
        models: &[
            everyday("claude-sonnet-5-5", "Claude Sonnet 5.5"),
            m("claude-sonnet-5", "Claude Sonnet 5", "Everyday"),
            m("claude-haiku-5-5", "Claude Haiku 5.5", "Quick tasks"),
            complex("claude-opus-5-5", "Claude Opus 5.5"),
        ],
    },
    Provider {
        id: "google",
        name: "Google Gemini",
        key_url: "https://aistudio.google.com/apikey",
        efforts: &["minimal", "low", "medium", "high"],
        models: &[
            everyday("gemini-3.8-flash", "Gemini 3.8 Flash"),
            m("gemini-3.6-flash", "Gemini 3.6 Flash", "Everyday"),
            m("gemini-3.5-flash-lite", "Gemini 3.5 Flash-Lite", "Simple tasks"),
            complex("gemini-4-argon", "Gemini 4 Argon"),
        ],
    },
    Provider {
        id: "custom",
        name: "Custom",
        key_url: "",
        efforts: &[],
        models: &[],
    },
];

/// The provider with this id, or AICredits (the default) for anything else.
pub fn provider(id: &str) -> &'static Provider {
    PROVIDERS.iter().find(|p| p.id == id).unwrap_or(&PROVIDERS[0])
}

/// Accepts an id or a name, any case: "google", "Google Gemini", "gemini".
pub fn find_provider(query: &str) -> Option<&'static Provider> {
    let q = query.trim().to_ascii_lowercase();
    PROVIDERS.iter().find(|p| {
        p.id == q || p.name.to_ascii_lowercase() == q || p.name.to_ascii_lowercase().split(' ').any(|w| w == q)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_provider_has_one_everyday_and_one_complex_model() {
        for p in PROVIDERS.iter().filter(|p| !p.models.is_empty()) {
            assert_eq!(p.models.iter().filter(|m| m.everyday).count(), 1, "{}", p.id);
            assert_eq!(p.models.iter().filter(|m| m.complex).count(), 1, "{}", p.id);
            assert!(p.models[0].everyday, "{}: the everyday pick comes first", p.id);
        }
    }

    #[test]
    fn finds_providers_by_id_or_name() {
        assert_eq!(find_provider("gemini").unwrap().id, "google");
        assert_eq!(find_provider("OpenAI").unwrap().id, "openai");
        assert!(find_provider("nope").is_none());
    }
}
