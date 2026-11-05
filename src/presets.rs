//! The providers and models eco ships. A new config starts with them as
//! ordinary entries (`seed`), which the user edits or deletes; a config read
//! later is taken as it is, so a deleted preset stays deleted.

use crate::config::{Config, ModelConfig, ModelType, ProviderConfig, SecretConfig};

/// A preset provider and the variable its key is read from; its secret and
/// the provider carry the same name.
struct Provider {
    name: &'static str,
    base_url: &'static str,
    variable: &'static str,
}

/// Every preset provider, in the order the window shows them.
const PROVIDERS: [Provider; 4] = [
    Provider {
        name: "OPENROUTER",
        base_url: "https://openrouter.ai/api/v1",
        variable: "OPENROUTER_API_KEY",
    },
    Provider {
        name: "ELEVENLABS",
        base_url: "wss://api.elevenlabs.io/v1/speech-to-text/realtime",
        variable: "ELEVEN_LABS_API_KEY",
    },
    Provider {
        name: "DEEPGRAM",
        base_url: "wss://api.deepgram.com/v1/listen",
        variable: "DEEPGRAM_API_KEY",
    },
    Provider {
        name: "GROQ",
        base_url: "https://api.groq.com/openai/v1",
        variable: "GROQ_API_KEY",
    },
];

/// A preset model: one per provider and type, picked from `docs/benchmarks.md`
/// and reviewed each release. The first usable one of a type is its default.
struct Model {
    name: &'static str,
    kind: ModelType,
    provider: &'static str,
    model: &'static str,
    /// Chat models only, as the benchmark ran them.
    reasoning: Option<&'static str>,
}

const MODELS: [Model; 4] = [
    // Answer benchmark: best score (4.34), 95% pass, $0.10 per 1000 answers.
    Model {
        name: "gpt-6-luna",
        kind: ModelType::Chat,
        provider: "OPENROUTER",
        model: "openai/gpt-6-luna",
        reasoning: Some("off"),
    },
    // Streaming test: p50 1.05 s, p90 1.45 s.
    Model {
        name: "scribe-v2",
        kind: ModelType::Transcription,
        provider: "ELEVENLABS",
        model: "scribe_v2_realtime",
        reasoning: None,
    },
    // Streaming test: p50 1.58 s, p90 2.83 s.
    Model {
        name: "nova-3",
        kind: ModelType::Transcription,
        provider: "DEEPGRAM",
        model: "nova-3",
        reasoning: None,
    },
    // Not measured on Groq; whisper large-v3-turbo took ~990 ms a segment on the LAN.
    Model {
        name: "whisper-turbo",
        kind: ModelType::Transcription,
        provider: "GROQ",
        model: "whisper-large-v3-turbo",
        reasoning: None,
    },
];

impl Provider {
    fn secret(&self) -> SecretConfig {
        SecretConfig {
            name: self.name.into(),
            env: Some(self.variable.into()),
            omapass: None,
            credential: None,
        }
    }

    fn config(&self) -> ProviderConfig {
        ProviderConfig {
            name: self.name.into(),
            base_url: self.base_url.into(),
            secret: Some(self.name.into()),
        }
    }
}

impl Model {
    fn config(&self) -> ModelConfig {
        ModelConfig {
            name: self.name.into(),
            kind: self.kind,
            provider: self.provider.into(),
            model: self.model.into(),
            extra: serde_json::Map::new(),
            reasoning: self.reasoning.map(String::from),
            kinds: Vec::new(),
            translates: Vec::new(),
            price_per_minute: None,
        }
    }
}

/// The name of the preset secret that reads `variable`.
pub fn secret_named(variable: &str) -> Option<&'static str> {
    PROVIDERS
        .iter()
        .find(|p| p.variable == variable)
        .map(|p| p.name)
}

/// The name of the preset provider at `base_url`.
pub fn provider_named(base_url: &str) -> Option<&'static str> {
    PROVIDERS
        .iter()
        .find(|p| p.base_url == base_url)
        .map(|p| p.name)
}

/// The variables the presets' keys are read from.
pub fn variables() -> impl Iterator<Item = &'static str> {
    PROVIDERS.iter().map(|p| p.variable)
}

/// Adds every preset `config` does not have, after the user's entries and in
/// catalogue order. A secret that reads the same variable counts as the
/// preset's, and the preset provider then names it.
pub fn seed(config: &mut Config) {
    let old = config.clone();
    for provider in &PROVIDERS {
        let user = old
            .secrets
            .iter()
            .find(|s| s.name == provider.name || s.env.as_deref() == Some(provider.variable));
        if user.is_none() {
            config.secrets.push(provider.secret());
        }
        if !old.providers.iter().any(|p| p.name == provider.name) {
            let mut entry = provider.config();
            if let Some(user) = user {
                entry.secret = Some(user.name.clone());
            }
            config.providers.push(entry);
        }
    }
    config.models.extend(
        MODELS
            .iter()
            .filter(|p| !old.models.iter().any(|m| m.name == p.name))
            .map(Model::config),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn user(value: serde_json::Value) -> Config {
        Config::from_value(value).unwrap()
    }

    fn names<T>(list: &[T], name: impl Fn(&T) -> &str) -> Vec<&str> {
        list.iter().map(name).collect()
    }

    #[test]
    fn an_empty_config_gets_every_preset_in_catalogue_order() {
        let mut config = user(json!({}));
        seed(&mut config);
        assert_eq!(
            names(&config.secrets, |s| &s.name),
            ["OPENROUTER", "ELEVENLABS", "DEEPGRAM", "GROQ"]
        );
        assert_eq!(
            names(&config.providers, |p| &p.name),
            ["OPENROUTER", "ELEVENLABS", "DEEPGRAM", "GROQ"]
        );
        assert_eq!(
            names(&config.models, |m| &m.name),
            ["gpt-6-luna", "scribe-v2", "nova-3", "whisper-turbo"]
        );
        let chat = config.resolve(config.model("gpt-6-luna").unwrap()).unwrap();
        assert_eq!(chat.base_url, "https://openrouter.ai/api/v1");
        assert_eq!(
            chat.request_fields()["reasoning"],
            json!({"enabled": false})
        );
        assert!(config.llm.model.is_empty() && config.stt.model.is_empty());
    }

    #[test]
    fn the_users_entries_come_first_and_one_with_a_preset_name_wins() {
        let mut config = user(json!({
            "providers": [{"name": "lan", "base_url": "http://lan:9000"}],
            "models": [{"name": "nova-3", "type": "transcription",
                        "provider": "lan", "model": "w"}],
        }));
        seed(&mut config);
        assert_eq!(
            names(&config.models, |m| &m.name),
            ["nova-3", "gpt-6-luna", "scribe-v2", "whisper-turbo"]
        );
        let nova = config.resolve(config.model("nova-3").unwrap()).unwrap();
        assert_eq!(
            (nova.base_url.as_str(), nova.model.as_str()),
            ("http://lan:9000", "w")
        );
        let mut config = user(json!({
            "providers": [{"name": "GROQ", "base_url": "http://mine"}],
        }));
        seed(&mut config);
        let groq: Vec<_> = config
            .providers
            .iter()
            .filter(|p| p.name == "GROQ")
            .collect();
        assert_eq!(groq.len(), 1);
        assert_eq!(groq[0].base_url, "http://mine");
    }

    #[test]
    fn a_secret_reading_a_preset_variable_takes_the_presets_place() {
        let mut config = user(json!({
            "secrets": [{"name": "MINE", "env": "OPENROUTER_API_KEY"},
                        {"name": "DEEPGRAM", "omapass": "deepgram"}],
        }));
        seed(&mut config);
        assert_eq!(
            names(&config.secrets, |s| &s.name),
            ["MINE", "DEEPGRAM", "ELEVENLABS", "GROQ"]
        );
        let secret = |provider: &str| config.provider(provider).unwrap().0.secret.clone();
        assert_eq!(secret("OPENROUTER").as_deref(), Some("MINE"));
        assert_eq!(secret("DEEPGRAM").as_deref(), Some("DEEPGRAM"));
        assert_eq!(secret("GROQ").as_deref(), Some("GROQ"));
    }

    #[test]
    fn the_presets_are_found_by_variable_and_by_address() {
        assert_eq!(secret_named("GROQ_API_KEY"), Some("GROQ"));
        assert_eq!(secret_named("MINE"), None);
        assert_eq!(
            provider_named("https://api.groq.com/openai/v1"),
            Some("GROQ")
        );
        assert_eq!(variables().count(), 4);
    }
}
