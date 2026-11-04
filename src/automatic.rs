//! The roles the user left without a model. An empty default model of a type
//! (transcription, chat, translation, review) is filled in when the config is
//! composed and never written: it is the first model of the type whose key is
//! usable, else the first of the type. A model whose variable is not set is
//! told as disabled with the variable it waits for.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::config::{Config, Key, ModelConfig, ModelType, Resolved, ReviewerConfig, env_key};

/// A model whose key variable is not set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disabled {
    pub name: String,
    pub variable: String,
}

/// The variable `model` waits for: its key is read from a variable that is not set.
pub fn missing(model: &Resolved) -> Option<&str> {
    match model.key() {
        Key::Env(variable) if env_key(variable).is_err() => Some(variable),
        _ => None,
    }
}

/// The roles whose model the user chose none for, so `filled` fills them:
/// "transcription", "chat", "translation" and "review".
pub fn roles(config: &Config) -> Vec<&'static str> {
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

/// The models of `config` that wait for a variable, in registration order.
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

/// `config` with an empty default model of a type set to the first model of
/// that type whose key is usable, or else the first one, which sessions then
/// tell as disabled.
pub fn filled(config: &Config) -> Config {
    let mut config = config.clone();
    let usable = |model: &ModelConfig| {
        config
            .resolve(model)
            .is_some_and(|resolved| missing(&resolved).is_none())
    };
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

/// What a snapshot says of the models that wait for a variable:
/// `{name: {code, params}}`.
pub fn disabled_value(disabled: &[Disabled]) -> Value {
    let entries: BTreeMap<&str, Value> = disabled
        .iter()
        .map(|d| {
            (
                d.name.as_str(),
                json!({"code": "model.disabled", "params": {"name": d.name, "variable": d.variable}}),
            )
        })
        .collect();
    json!(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::testing::set_env;

    fn config(value: Value) -> Config {
        Config::from_value(value).unwrap()
    }

    fn lan_models() -> Value {
        json!({
            "providers": [{"name": "lan", "base_url": "http://lan"}],
            "models": [
                {"name": "chat-a", "type": "chat", "provider": "lan", "model": "m"},
                {"name": "stt-a", "type": "transcription", "provider": "lan", "model": "w"},
            ],
        })
    }

    fn keyed_models() -> Value {
        json!({
            "secrets": [{"name": "K1", "env": "ECO_AUTO_UNSET_1"},
                        {"name": "K2", "env": "ECO_AUTO_SET_2"}],
            "providers": [{"name": "p1", "base_url": "http://p1", "secret": "K1"},
                          {"name": "p2", "base_url": "http://p2", "secret": "K2"}],
            "models": [
                {"name": "first", "type": "chat", "provider": "p1", "model": "m"},
                {"name": "second", "type": "chat", "provider": "p2", "model": "m"},
            ],
        })
    }

    #[test]
    fn every_empty_role_is_named_and_filled_with_the_first_model_of_its_type() {
        let own = config(lan_models());
        assert_eq!(
            roles(&own),
            ["transcription", "chat", "translation", "review"]
        );
        let filled = filled(&own);
        assert_eq!(filled.llm.model, "chat-a");
        assert_eq!(filled.stt.model, "stt-a");
        assert_eq!(filled.translation.model, "chat-a");
        let reviewer = filled.reviewer.unwrap();
        assert_eq!(
            (reviewer.model.as_str(), reviewer.enabled),
            ("chat-a", false)
        );
        assert!(own.llm.model.is_empty() && own.models.len() == 2);
    }

    #[test]
    fn a_chosen_role_is_kept_and_a_reviewer_stays_as_set() {
        let mut value = lan_models();
        value["translation"] = json!({"model": "chat-a"});
        value["reviewer"] = json!({"enabled": true});
        let mut own = config(value);
        own.stt.model = "stt-a".into();
        assert_eq!(roles(&own), ["chat", "review"]);
        let filled = filled(&own);
        assert_eq!(filled.stt.model, "stt-a");
        let reviewer = filled.reviewer.unwrap();
        assert_eq!(
            (reviewer.model.as_str(), reviewer.enabled),
            ("chat-a", true)
        );
    }

    #[test]
    fn the_first_usable_model_wins_else_the_first_of_the_type() {
        set_env("ECO_AUTO_SET_2", "k");
        let own = config(keyed_models());
        assert_eq!(filled(&own).llm.model, "second");
        let mut none = keyed_models();
        none["secrets"][1]["env"] = json!("ECO_AUTO_UNSET_3");
        assert_eq!(filled(&config(none)).llm.model, "first");
    }

    #[test]
    fn any_model_waiting_for_an_unset_variable_is_disabled_with_it() {
        set_env("ECO_AUTO_SET_2", "k");
        let own = config(keyed_models());
        let waiting = disabled(&own);
        assert_eq!(
            waiting,
            [Disabled {
                name: "first".into(),
                variable: "ECO_AUTO_UNSET_1".into()
            }]
        );
        let resolved = own.resolved();
        assert_eq!(missing(&resolved[0]), Some("ECO_AUTO_UNSET_1"));
        assert_eq!(missing(&resolved[1]), None);
        assert_eq!(
            disabled_value(&waiting),
            json!({"first": {"code": "model.disabled",
                             "params": {"name": "first", "variable": "ECO_AUTO_UNSET_1"}}})
        );
        assert!(disabled(&config(lan_models())).is_empty());
    }
}
