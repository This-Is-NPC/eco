//! The providers and models eco ships. They are laid over the user's config
//! when it is composed and never written back to it: an entry of the user's
//! with the same name replaces the preset's. A preset whose key is not set is
//! still there, told as disabled with the variable it waits for.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::config::{
    Config, Key, ModelConfig, ModelType, ProviderConfig, Resolved, ReviewerConfig, SecretConfig,
    env_key,
};

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

/// A preset model whose key is not set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disabled {
    pub name: String,
    pub variable: String,
}

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

/// The variable `model` waits for when it is a preset whose key is not set.
pub fn missing(model: &Resolved) -> Option<&str> {
    let preset = MODELS.iter().find(|p| p.name == model.name)?;
    let provider = PROVIDERS.iter().find(|p| p.name == preset.provider)?;
    let same = model.entry == preset.config()
        && model.base_url == provider.base_url
        && model.key() == Key::Env(provider.variable);
    let variable = provider.variable;
    (same && env_key(variable).is_err()).then_some(variable)
}

/// Whether `model` can be used now: it needs no key, or its key resolves.
/// A key in the keyring or the credential manager is read when the model is
/// built, so it counts as usable.
fn is_usable(model: &Resolved) -> bool {
    match model.key() {
        Key::Env(variable) => env_key(variable).is_ok(),
        Key::None | Key::Omapass(_) | Key::Credential(_) => true,
    }
}

/// The roles whose model the user chose none for, so `overlay` fills them:
/// "transcription", "chat", "translation" and "review".
pub fn automatic(config: &Config) -> Vec<&'static str> {
    let empty = |model: &str| model.is_empty();
    [
        ("transcription", empty(&config.stt.model)),
        ("chat", empty(&config.llm.model)),
        ("translation", empty(&config.translation.model)),
        (
            "review",
            config.reviewer.as_ref().is_none_or(|r| empty(&r.model)),
        ),
    ]
    .into_iter()
    .filter_map(|(role, left)| left.then_some(role))
    .collect()
}

/// The preset models of `config` that wait for a key, in the order they are shown.
pub fn disabled(config: &Config) -> Vec<Disabled> {
    config
        .resolved()
        .iter()
        .filter_map(|model| {
            Some(Disabled {
                name: model.name.clone(),
                variable: missing(model)?.into(),
            })
        })
        .collect()
}

/// `config` with the presets laid under it: those a user entry does not
/// replace by name. An empty default model of a type becomes the first preset
/// of that type whose key is set, or else the first one, which sessions then
/// tell as disabled.
pub fn overlay(config: &Config) -> Config {
    let mut config = config.clone();
    let old = config.clone();
    for provider in &PROVIDERS {
        // A secret of the user's that reads the same variable takes the
        // preset's place: the preset provider names it.
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
    let usable = |model: &ModelConfig| config.resolve(model).is_some_and(|r| is_usable(&r));
    // The user's models come first, then the presets, so the first usable one
    // is the user's when it works; with none usable, the first of the type.
    let first = |kind: ModelType| {
        let of = || config.models.iter().filter(move |m| m.kind == kind);
        of().find(|m| usable(m))
            .or_else(|| of().next())
            .map(|m| m.name.clone())
    };
    let (chat, stt) = (first(ModelType::Chat), first(ModelType::Transcription));
    let fill = |model: &mut String, pick: &Option<String>| {
        if let Some(pick) = pick.as_ref().filter(|_| model.is_empty()) {
            model.clone_from(pick);
        }
    };
    fill(&mut config.llm.model, &chat);
    fill(&mut config.stt.model, &stt);
    fill(&mut config.translation.model, &chat);
    fill(
        &mut config
            .reviewer
            .get_or_insert_with(ReviewerConfig::default)
            .model,
        &chat,
    );
    config
}

/// The variables the presets' keys are read from.
pub fn variables() -> impl Iterator<Item = &'static str> {
    PROVIDERS.iter().map(|p| p.variable)
}

/// The presets the `config` event lists, for the settings window to show them
/// as presets: `{secrets: [{name, env}], providers: [{name, base_url, secret}],
/// models: [{name, type, provider, model, reasoning?}]}`, in the order shown.
pub fn listing() -> Value {
    let secrets: Vec<Value> = PROVIDERS
        .iter()
        .map(|p| json!({"name": p.name, "env": p.variable}))
        .collect();
    let providers: Vec<Value> = PROVIDERS
        .iter()
        .map(|p| json!({"name": p.name, "base_url": p.base_url, "secret": p.name}))
        .collect();
    let models: Vec<Value> = MODELS
        .iter()
        .map(|m| serde_json::to_value(m.config()).expect("a model always serializes"))
        .collect();
    json!({"secrets": secrets, "providers": providers, "models": models})
}

/// What a snapshot says of the preset models that wait for a key:
/// `{name: {code, params}}`.
pub fn disabled_value(disabled: &[Disabled]) -> Value {
    let entries: BTreeMap<&str, Value> = disabled
        .iter()
        .map(|d| {
            (
                d.name.as_str(),
                json!({"code": "preset.key_missing", "params": {"name": d.name, "variable": d.variable}}),
            )
        })
        .collect();
    json!(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{self, testing::set_env};

    fn user(models: Value) -> Config {
        Config::from_value(json!({"models": models})).unwrap()
    }

    fn names(config: &Config) -> Vec<&str> {
        config.models.iter().map(|m| m.name.as_str()).collect()
    }

    #[test]
    fn an_empty_config_gets_every_preset_and_the_first_usable_as_default() {
        set_env("DEEPGRAM_API_KEY", "dg");
        let config = overlay(&user(json!([])));
        assert_eq!(
            names(&config),
            ["gpt-6-luna", "scribe-v2", "nova-3", "whisper-turbo"]
        );
        assert_eq!(config.llm.model, "gpt-6-luna");
        assert_eq!(config.stt.model, "nova-3");
        let chat = config.resolve(config.model("gpt-6-luna").unwrap()).unwrap();
        assert_eq!(chat.base_url, "https://openrouter.ai/api/v1");
        assert_eq!(chat.key(), Key::Env("OPENROUTER_API_KEY"));
        assert_eq!(
            chat.request_fields()["reasoning"],
            json!({"enabled": false})
        );
        let waiting: Vec<_> = disabled(&config).into_iter().map(|d| d.name).collect();
        assert_eq!(waiting, ["gpt-6-luna", "scribe-v2", "whisper-turbo"]);
    }

    #[test]
    fn every_role_takes_the_first_usable_model_of_its_type() {
        set_env("OPENROUTER_API_KEY", "or");
        let config = overlay(&user(json!([])));
        assert_eq!(config.llm.model, "gpt-6-luna");
        assert_eq!(config.translation.model, "gpt-6-luna");
        let reviewer = config.reviewer.unwrap();
        assert_eq!(
            (reviewer.model.as_str(), reviewer.enabled),
            ("gpt-6-luna", false)
        );
        assert_eq!(
            automatic(&user(json!([]))),
            ["transcription", "chat", "translation", "review"]
        );
    }

    #[test]
    fn a_chosen_role_is_kept_and_a_reviewer_stays_as_set() {
        set_env("OPENROUTER_API_KEY", "or");
        let lan = json!([{"name": "mine", "type": "chat", "provider": "lan", "model": "m"}]);
        let mut own = Config::from_value(json!({
            "models": lan,
            "providers": [{"name": "lan", "base_url": "http://lan"}],
            "translation": {"model": "gpt-6-luna"},
            "reviewer": {"enabled": true},
        }))
        .unwrap();
        own.stt.model = "whisper-turbo".into();
        let config = overlay(&own);
        assert_eq!(config.translation.model, "gpt-6-luna");
        assert_eq!(config.stt.model, "whisper-turbo");
        assert_eq!(config.llm.model, "mine");
        let reviewer = config.reviewer.unwrap();
        assert_eq!((reviewer.model.as_str(), reviewer.enabled), ("mine", true));
        assert_eq!(automatic(&own), ["chat", "review"]);
    }

    #[test]
    fn with_no_key_set_the_first_preset_is_the_default_and_is_disabled() {
        let config = overlay(&user(json!([])));
        assert_eq!(config.stt.model, "scribe-v2");
        assert_eq!(
            disabled(&config)[1],
            Disabled {
                name: "scribe-v2".into(),
                variable: "ELEVEN_LABS_API_KEY".into()
            }
        );
    }

    #[test]
    fn a_key_set_later_enables_the_preset_on_the_next_look() {
        let config = overlay(&user(json!([])));
        assert_eq!(disabled(&config).len(), 4);
        set_env("GROQ_API_KEY", "gq");
        assert_eq!(disabled(&config).len(), 3);
    }

    #[test]
    fn a_user_entry_with_a_preset_name_wins() {
        let lan = json!({"name": "nova-3", "type": "transcription",
                         "provider": "lan", "model": "w"});
        let own = Config::from_value(json!({
            "providers": [{"name": "lan", "base_url": "http://lan:9000"}],
            "models": [lan],
        }))
        .unwrap();
        let config = overlay(&own);
        assert_eq!(names(&config).iter().filter(|n| **n == "nova-3").count(), 1);
        let nova = config.resolve(config.model("nova-3").unwrap()).unwrap();
        assert_eq!(
            (nova.base_url.as_str(), nova.model.as_str()),
            ("http://lan:9000", "w")
        );
        assert!(disabled(&config).iter().all(|d| d.name != "nova-3"));
        assert_eq!(config.stt.model, "nova-3");
        // A secret of the user's with a preset's name replaces its key source.
        let mut own = user(json!([]));
        own.secrets.push(SecretConfig {
            name: "DEEPGRAM".into(),
            env: None,
            omapass: Some("deepgram".into()),
            credential: None,
        });
        let config = overlay(&own);
        let nova = config.resolve(config.model("nova-3").unwrap()).unwrap();
        assert_eq!(nova.key(), Key::Omapass("deepgram"));
        assert!(disabled(&config).iter().all(|d| d.name != "nova-3"));
    }

    #[test]
    fn presets_are_never_written_to_the_saved_file() {
        let mut chosen = user(json!([]));
        chosen.llm.model = "gpt-6-luna".into();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        config::save(&chosen, &path).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        for preset in [
            "scribe-v2",
            "nova-3",
            "whisper-turbo",
            "OPENROUTER",
            "DEEPGRAM",
        ] {
            assert!(!written.contains(preset), "{preset} in {written}");
        }
        assert!(written.contains("gpt-6-luna"));
        let loaded = config::load(&path).unwrap();
        assert_eq!(loaded, chosen);
        assert!(
            loaded.models.is_empty() && loaded.providers.is_empty() && loaded.secrets.is_empty()
        );
    }

    #[test]
    fn a_user_secret_reading_a_preset_variable_takes_the_presets_place() {
        let own = Config::from_value(json!({
            "secrets": [{"name": "OPENROUTER_API_KEY", "env": "OPENROUTER_API_KEY"}],
        }))
        .unwrap();
        let config = overlay(&own);
        let names: Vec<&str> = config.secrets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            ["OPENROUTER_API_KEY", "ELEVENLABS", "DEEPGRAM", "GROQ"]
        );
        let luna = config.resolve(config.model("gpt-6-luna").unwrap()).unwrap();
        assert_eq!(luna.secret.unwrap().name, "OPENROUTER_API_KEY");
        assert_eq!(disabled(&config)[0].name, "gpt-6-luna");
    }

    #[test]
    fn the_presets_are_found_by_variable_and_by_address() {
        assert_eq!(secret_named("GROQ_API_KEY"), Some("GROQ"));
        assert_eq!(secret_named("MINE"), None);
        assert_eq!(
            provider_named("https://api.groq.com/openai/v1"),
            Some("GROQ")
        );
    }

    #[test]
    fn the_window_gets_the_presets_to_show_them_as_presets() {
        let listing = listing();
        assert_eq!(
            listing["secrets"][2],
            json!({"name": "DEEPGRAM", "env": "DEEPGRAM_API_KEY"})
        );
        assert_eq!(
            listing["providers"][0],
            json!({"name": "OPENROUTER", "base_url": "https://openrouter.ai/api/v1",
                   "secret": "OPENROUTER"})
        );
        assert_eq!(
            listing["models"][0],
            json!({"name": "gpt-6-luna", "type": "chat", "provider": "OPENROUTER",
                   "model": "openai/gpt-6-luna", "reasoning": "off"})
        );
        assert_eq!(listing["models"].as_array().unwrap().len(), 4);
    }
}
