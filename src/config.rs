//! `~/.config/eco/config.toml`: its schema, validation, and atomic save.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::adapters::credentials;
use crate::adapters::devices::{DEFAULT_INPUT, DEFAULT_OUTPUT};
use crate::domain::action::Action;
use crate::domain::prompts::{self, DEFAULT_REVIEW, DEFAULT_RULES};
use crate::paths;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ConfigError(pub String);

/// The key held by the environment variable `name`.
pub fn env_key(name: &str) -> Result<String, ConfigError> {
    variable(name)
        .filter(|key| !key.is_empty())
        .ok_or_else(|| ConfigError(format!("{name} is not set")))
}

/// A test reads the variables it sets with `testing::set_env`, on its own
/// thread, and none of the process's, unless it asks for them with
/// `testing::read_process_env`.
fn variable(name: &str) -> Option<String> {
    #[cfg(test)]
    if !testing::PROCESS.get() {
        return testing::ENV.with_borrow(|vars| vars.get(name).cloned());
    }
    std::env::var(name).ok()
}

#[cfg(test)]
pub mod testing {
    use std::cell::RefCell;
    use std::collections::HashMap;

    thread_local! {
        pub(super) static ENV: RefCell<HashMap<String, String>> = RefCell::default();
        pub(super) static PROCESS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// Make the calling test's key lookups read the process's variables.
    pub fn read_process_env() {
        PROCESS.set(true);
    }

    /// Set `name` for the key lookups of the calling test only.
    pub fn set_env(name: &str, value: &str) {
        ENV.with_borrow_mut(|vars| vars.insert(name.into(), value.into()));
    }
}

/// A language code the STT accepts: ISO 639-1/2 letters, or "auto" to detect it.
pub fn valid_language(code: &str) -> bool {
    code == "auto"
        || ((2..=3).contains(&code.len()) && code.chars().all(|c| c.is_ascii_alphabetic()))
}

/// What a session can be; the overlay translates these four, others show as typed.
fn default_kinds() -> Vec<String> {
    ["meeting", "conversation", "other", "idea"]
        .map(String::from)
        .to_vec()
}

fn default_rules() -> String {
    DEFAULT_RULES.into()
}

fn default_languages() -> Vec<String> {
    ["auto", "pt", "en", "es"].map(String::from).to_vec()
}

/// Transcription: the model that transcribes sessions whose kind names none, and
/// the language it hears.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SttConfig {
    /// The name of a registered transcription model; empty takes the first usable one.
    #[serde(default)]
    pub model: String,
    /// Code from `languages` used now; "auto" lets the server detect it.
    #[serde(default = "SttConfig::default_language")]
    pub language: String,
    /// Languages offered by the overlay's picker.
    #[serde(default = "default_languages")]
    pub languages: Vec<String>,
}

/// Where a provider's key comes from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Key<'a> {
    /// The provider needs none (a LAN server).
    None,
    Env(&'a str),
    Omapass(&'a str),
    /// The Windows Credential Manager's generic credential `eco:<name>`.
    Credential(&'a str),
}

impl<'a> Key<'a> {
    /// The source `env`, `omapass` and `credential` name; more than one set is
    /// the caller's to refuse.
    pub fn of(env: Option<&'a str>, omapass: Option<&'a str>, credential: Option<&'a str>) -> Self {
        let given = |name: Option<&'a str>| name.filter(|n| !n.trim().is_empty());
        match (given(env), given(omapass), given(credential)) {
            (_, _, Some(name)) => Self::Credential(name),
            (_, Some(account), None) => Self::Omapass(account),
            (Some(variable), None, None) => Self::Env(variable),
            (None, None, None) => Self::None,
        }
    }

    /// The `(env, omapass, credential)` names `of` takes this key from.
    pub fn fields(self) -> (Option<&'a str>, Option<&'a str>, Option<&'a str>) {
        match self {
            Self::None => (None, None, None),
            Self::Env(name) => (Some(name), None, None),
            Self::Omapass(account) => (None, Some(account), None),
            Self::Credential(name) => (None, None, Some(name)),
        }
    }
}

impl SttConfig {
    fn default_language() -> String {
        "pt".into()
    }
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
            language: Self::default_language(),
            languages: default_languages(),
        }
    }
}

/// What a registered model does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelType {
    /// Answers: the assistant, the reviewer, actions.
    Chat,
    /// Turns speech into lines: an OpenAI-compatible endpoint, or a streaming one
    /// by its wss:// URL.
    Transcription,
}

/// Where a key lives: a reference only, eco never stores or creates a key.
/// Exactly one of `env`, `omapass` and `credential` is set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretConfig {
    pub name: String,
    /// The environment variable holding the key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<String>,
    /// The omapass account holding the key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omapass: Option<String>,
    /// The name of the Windows Credential Manager's credential `eco:<name>`
    /// holding the key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}

impl SecretConfig {
    pub fn key(&self) -> Key<'_> {
        Key::of(
            self.env.as_deref(),
            self.omapass.as_deref(),
            self.credential.as_deref(),
        )
    }
}

/// A service eco reaches: an address, and the secret its requests carry, if any.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub name: String,
    pub base_url: String,
    /// The name of its secret; a server on the LAN has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
}

/// A model the user registered once, named so roles, actions and session kinds pick it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ModelType,
    /// The name of the provider that serves it.
    pub provider: String,
    /// The provider's id for it.
    pub model: String,
    /// Provider-specific request fields, sent as-is; chat models only.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
    /// How much a chat model reasons before answering: "off", "low", "medium" or
    /// "high"; the provider decides when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    /// The session kinds it serves instead of the default model of its type.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<String>,
    /// The session kinds it translates instead of the default translator; chat models only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub translates: Vec<String>,
    /// What a minute of audio costs it, in USD, if the user gives it: it stands
    /// in where the provider reports no cost; transcription models only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_per_minute: Option<f64>,
}

/// A model with the address and key source of its provider, what building it needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub entry: ModelConfig,
    pub base_url: String,
    pub secret: Option<SecretConfig>,
}

impl std::ops::Deref for Resolved {
    type Target = ModelConfig;

    fn deref(&self) -> &ModelConfig {
        &self.entry
    }
}

impl Resolved {
    pub fn key(&self) -> Key<'_> {
        self.secret.as_ref().map_or(Key::None, SecretConfig::key)
    }
}

impl ModelConfig {
    /// The fields sent with every request: `extra`, with the reasoning chosen in
    /// OpenRouter's form, which wins over a `reasoning` written in `extra`.
    pub fn request_fields(&self) -> Map<String, Value> {
        let mut fields = self.extra.clone();
        match self.reasoning.as_deref() {
            Some("off") => {
                fields.insert("reasoning".into(), serde_json::json!({"enabled": false}));
            }
            Some(effort) => {
                fields.insert("reasoning".into(), serde_json::json!({"effort": effort}));
            }
            None => {}
        }
        fields
    }
}

/// The assistant: the model that answers unless an action names another.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmConfig {
    /// The name of a registered chat model; empty takes the first usable one.
    #[serde(default)]
    pub model: String,
    /// Characters of transcript and earlier answers a request may carry (~4 per token).
    #[serde(default = "LlmConfig::default_max_context_chars")]
    pub max_context_chars: usize,
    /// How many answers stream at once, across sessions.
    #[serde(default = "LlmConfig::default_concurrency")]
    pub concurrency: usize,
}

impl LlmConfig {
    fn default_max_context_chars() -> usize {
        60_000
    }

    fn default_concurrency() -> usize {
        8
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
            max_context_chars: Self::default_max_context_chars(),
            concurrency: Self::default_concurrency(),
        }
    }
}

/// The model that translates each line and answer, below it, in a session that
/// turned translation on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranslationConfig {
    /// The name of a registered chat model; empty takes the first usable one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
}

/// A second model every answer passes through before the user sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewerConfig {
    #[serde(default)]
    pub enabled: bool,
    /// The name of a registered chat model; empty takes the first usable one.
    #[serde(default)]
    pub model: String,
    /// What the reviewer is asked after the draft.
    #[serde(default = "ReviewerConfig::default_prompt")]
    pub prompt: String,
    /// The draft is shown and kept beside the reviewed answer, to inspect.
    #[serde(default)]
    pub verbose: bool,
}

impl ReviewerConfig {
    fn default_prompt() -> String {
        DEFAULT_REVIEW.into()
    }
}

impl Default for ReviewerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: String::new(),
            prompt: Self::default_prompt(),
            verbose: false,
        }
    }
}

/// A speaker label in the transcript, heard through one or more audio devices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Participant {
    pub name: String,
    pub devices: Vec<String>,
    /// The participant whose speech is the user's own.
    #[serde(default)]
    pub user: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextTable {
    #[serde(default)]
    files: Vec<String>,
}

/// A named slot of the user's context: files sent only while a session has it
/// on; sessions of `kinds` start with it on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSlot {
    pub name: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub kinds: Vec<String>,
}

/// How the user's microphone is kept apart from what the others say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioConfig {
    /// PipeWire's echo cancellation on the user's microphone while a session records.
    #[serde(default)]
    pub echo_cancel: bool,
    /// The user's lines that repeat the others' are dropped as leaked audio.
    #[serde(default = "AudioConfig::on")]
    pub drop_echoes: bool,
}

impl AudioConfig {
    fn on() -> bool {
        true
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            echo_cancel: false,
            drop_echoes: true,
        }
    }
}

/// The interface preset and the custom palette kept when another preset is active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppearanceConfig {
    pub preset: String,
    pub colors: BTreeMap<String, String>,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            preset: "system".into(),
            colors: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UiTable {
    #[serde(default = "UiTable::auto")]
    language: String,
    #[serde(default)]
    hide_from_share: bool,
    #[serde(default)]
    tray: bool,
    #[serde(default)]
    notifications: bool,
    #[serde(default)]
    appearance: AppearanceConfig,
}

impl UiTable {
    fn auto() -> String {
        "auto".into()
    }
}

impl Default for UiTable {
    fn default() -> Self {
        Self {
            language: Self::auto(),
            hide_from_share: false,
            tray: false,
            notifications: false,
            appearance: AppearanceConfig::default(),
        }
    }
}

/// Whether the daemon asks GitHub once a day for a newer release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateConfig {
    #[serde(default = "UpdateConfig::on")]
    pub check: bool,
}

impl UpdateConfig {
    fn on() -> bool {
        true
    }
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self { check: true }
    }
}

/// The file's shape, field for field, in the order it is written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    #[serde(default = "default_kinds")]
    kinds: Vec<String>,
    #[serde(default = "default_rules")]
    rules: String,
    #[serde(default)]
    stt: SttConfig,
    #[serde(default)]
    llm: LlmConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reviewer: Option<ReviewerConfig>,
    #[serde(default)]
    translation: TranslationConfig,
    #[serde(default)]
    secrets: Vec<SecretConfig>,
    #[serde(default)]
    providers: Vec<ProviderConfig>,
    #[serde(default)]
    models: Vec<ModelConfig>,
    #[serde(default)]
    context: ContextTable,
    #[serde(default)]
    contexts: Vec<ContextSlot>,
    #[serde(default)]
    participants: Vec<Participant>,
    #[serde(default)]
    actions: Vec<Action>,
    #[serde(default)]
    colors: BTreeMap<String, String>,
    #[serde(default)]
    audio: AudioConfig,
    #[serde(default)]
    ui: UiTable,
    #[serde(default)]
    update: UpdateConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shortcuts: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// The kinds a session can be given, in the order they are offered.
    pub kinds: Vec<String>,
    /// The rules every answer follows unless its request says otherwise.
    pub rules: String,
    pub stt: SttConfig,
    pub llm: LlmConfig,
    pub reviewer: Option<ReviewerConfig>,
    pub translation: TranslationConfig,
    /// Where the keys live.
    pub secrets: Vec<SecretConfig>,
    /// The services models are served by.
    pub providers: Vec<ProviderConfig>,
    /// The chat and transcription models roles, actions and kinds name.
    pub models: Vec<ModelConfig>,
    pub participants: Vec<Participant>,
    pub actions: Vec<Action>,
    pub context_files: Vec<String>,
    /// Named context slots, turned on per session.
    pub contexts: Vec<ContextSlot>,
    /// Device id -> "#rrggbb": the colour of that input's trace everywhere.
    pub colors: BTreeMap<String, String>,
    pub audio: AudioConfig,
    /// Language of the interface: a language pack's code (pt-BR, en-US, ja-JP…) or "auto".
    pub ui_language: String,
    /// eco's windows are left out of screen sharing (and so of screenshots and recordings).
    pub hide_from_share: bool,
    /// eco shows an icon in the system tray, run as its own window process.
    pub tray: bool,
    /// The daemon tells the desktop's notifications what the windows would
    /// show while no eco window is active.
    pub notifications: bool,
    pub appearance: AppearanceConfig,
    pub update: UpdateConfig,
    /// Keys -> the `eco window` command they run, registered on Windows;
    /// `None` keeps the defaults of `shortcuts::DEFAULT`.
    pub shortcuts: Option<BTreeMap<String, String>>,
}

impl Config {
    /// The user's global context files, joined, `~` expanded.
    pub fn context(&self) -> std::io::Result<String> {
        read_all(&self.context_files)
    }

    /// The text of each context slot, in order, with its name and kinds; a slot
    /// whose files cannot be read gives why instead.
    pub fn slot_texts(&self) -> Vec<(&ContextSlot, std::io::Result<String>)> {
        let read = |slot: &ContextSlot| read_all(&slot.files);
        self.contexts
            .iter()
            .map(|slot| (slot, read(slot)))
            .collect()
    }

    /// The language pack the interface reads: the one chosen, or, on "auto",
    /// the system language `locale`'s.
    pub fn interface_pack(&self, locale: Option<&str>) -> String {
        match self.ui_language.as_str() {
            "auto" => system_language(locale).0.to_string(),
            chosen => chosen.to_string(),
        }
    }

    /// The registered model called `name`.
    pub fn model(&self, name: &str) -> Option<&ModelConfig> {
        self.models.iter().find(|model| model.name == name)
    }

    /// The model of type `kind` that serves sessions of `session_kind` in place of
    /// the default one, if any.
    pub fn model_of_kind(&self, kind: ModelType, session_kind: &str) -> Option<&ModelConfig> {
        self.models
            .iter()
            .find(|model| model.kind == kind && model.kinds.iter().any(|k| k == session_kind))
    }

    /// The model that translates sessions of `session_kind`: its own, the
    /// translation default, or the assistant's; `None` when there is none.
    pub fn translator(&self, session_kind: &str) -> Option<&ModelConfig> {
        let own = self
            .models
            .iter()
            .find(|m| m.translates.iter().any(|k| k == session_kind));
        let default = Some(&self.translation.model).filter(|name| !name.is_empty());
        own.or_else(|| self.model(default.unwrap_or(&self.llm.model)))
    }

    /// The model that transcribes sessions of `session_kind`: its own, or the
    /// default; `None` when there is none.
    pub fn transcriber(&self, session_kind: &str) -> Option<&ModelConfig> {
        self.model_of_kind(ModelType::Transcription, session_kind)
            .or_else(|| self.model(&self.stt.model))
    }

    /// Build a Config from its JSON shape (what the overlay sends); validate it.
    /// An empty default model stays empty: it is automatic (see `automatic::filled`).
    pub fn from_value(value: Value) -> Result<Self, ConfigError> {
        let raw: Raw = serde_json::from_value(value)
            .map_err(|error| ConfigError(format!("invalid config: {error}")))?;
        Self::from_raw(raw)
    }

    /// The JSON shape of this Config; `Config::from_value(c.to_value())` gives `c` back.
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self.to_raw()).expect("a config always serializes")
    }

    /// `model` with its provider's address and its secret; `None` when its provider is unknown.
    pub fn resolve(&self, model: &ModelConfig) -> Option<Resolved> {
        let (provider, secret) = self.provider(&model.provider)?;
        Some(Resolved {
            entry: model.clone(),
            base_url: provider.base_url.clone(),
            secret: secret.cloned(),
        })
    }

    /// The provider called `name` and its secret, if it has one.
    pub fn provider(&self, name: &str) -> Option<(&ProviderConfig, Option<&SecretConfig>)> {
        let provider = self.providers.iter().find(|p| p.name == name)?;
        let secret = provider
            .secret
            .as_ref()
            .and_then(|name| self.secrets.iter().find(|s| &s.name == name));
        Some((provider, secret))
    }

    /// Every model resolved, in registration order.
    pub fn resolved(&self) -> Vec<Resolved> {
        self.models.iter().filter_map(|m| self.resolve(m)).collect()
    }

    fn from_raw(raw: Raw) -> Result<Self, ConfigError> {
        let config = Self {
            kinds: raw.kinds,
            rules: raw.rules,
            stt: raw.stt,
            llm: raw.llm,
            reviewer: raw.reviewer,
            translation: raw.translation,
            secrets: raw.secrets,
            providers: raw.providers,
            models: raw.models,
            participants: raw.participants,
            actions: raw.actions,
            context_files: raw.context.files,
            contexts: raw.contexts,
            colors: raw.colors,
            audio: raw.audio,
            ui_language: raw.ui.language,
            hide_from_share: raw.ui.hide_from_share,
            tray: raw.ui.tray,
            notifications: raw.ui.notifications,
            appearance: raw.ui.appearance,
            update: raw.update,
            shortcuts: raw.shortcuts,
        };
        crate::automatic::filled(&config).validate()?;
        Ok(config)
    }

    fn to_raw(&self) -> Raw {
        Raw {
            kinds: self.kinds.clone(),
            rules: self.rules.clone(),
            stt: self.stt.clone(),
            llm: self.llm.clone(),
            reviewer: self.reviewer.clone(),
            translation: self.translation.clone(),
            secrets: self.secrets.clone(),
            providers: self.providers.clone(),
            models: self.models.clone(),
            context: ContextTable {
                files: self.context_files.clone(),
            },
            contexts: self.contexts.clone(),
            participants: self.participants.clone(),
            actions: self.actions.clone(),
            colors: self.colors.clone(),
            audio: self.audio.clone(),
            ui: UiTable {
                language: self.ui_language.clone(),
                hide_from_share: self.hide_from_share,
                tray: self.tray,
                notifications: self.notifications,
                appearance: self.appearance.clone(),
            },
            update: self.update.clone(),
            shortcuts: self.shortcuts.clone(),
        }
    }

    fn validate(&self) -> Result<(), ConfigError> {
        let fail = |message: &str| Err(ConfigError(message.into()));
        if self.llm.concurrency == 0 {
            return fail("llm concurrency is at least 1");
        }
        let kinds = &self.kinds;
        if kinds.is_empty()
            || !distinct(kinds)
            || kinds.iter().any(|k| k.trim() != k || k.is_empty())
        {
            return fail("kinds must be unique, trimmed and not empty");
        }
        let slots: Vec<String> = self.contexts.iter().map(|slot| slot.name.clone()).collect();
        if !distinct(&slots) || slots.iter().any(|n| n.trim() != n || n.is_empty()) {
            return fail("context names must be unique, trimmed and not empty");
        }
        let secrets: Vec<&String> = self.secrets.iter().map(|s| &s.name).collect();
        if !distinct(&secrets) || secrets.iter().any(|n| n.trim() != *n || n.is_empty()) {
            return fail("secret names must be unique, trimmed and not empty");
        }
        let set = |value: &Option<String>| value.as_ref().is_some_and(|v| !v.trim().is_empty());
        if self.secrets.iter().any(|s| {
            [&s.env, &s.omapass, &s.credential]
                .into_iter()
                .filter(|source| set(source))
                .count()
                != 1
        }) {
            return fail("a secret's key comes from exactly one of env, omapass or credential");
        }
        if self.secrets.iter().any(|s| {
            set(&s.credential) && !s.credential.as_deref().is_some_and(credentials::valid_name)
        }) {
            return fail("a secret's credential is a word with no spaces, like deepgram");
        }
        let providers: Vec<&String> = self.providers.iter().map(|p| &p.name).collect();
        if !distinct(&providers) || providers.iter().any(|n| n.trim() != *n || n.is_empty()) {
            return fail("provider names must be unique, trimmed and not empty");
        }
        if self.providers.iter().any(|p| p.base_url.trim().is_empty()) {
            return fail("a provider needs a base URL");
        }
        if self.providers.iter().any(|p| {
            p.secret
                .as_ref()
                .is_some_and(|name| !secrets.contains(&name))
        }) {
            return fail("a provider's secret must be a registered secret");
        }
        let models: Vec<&String> = self.models.iter().map(|m| &m.name).collect();
        if !distinct(&models) || models.iter().any(|n| n.trim() != *n || n.is_empty()) {
            return fail("model names must be unique, trimmed and not empty");
        }
        if self
            .models
            .iter()
            .any(|m| m.model.trim().is_empty() || !providers.contains(&&m.provider))
        {
            return fail("a model needs a registered provider and the provider's model id");
        }
        if self.models.iter().any(|m| {
            m.kind == ModelType::Transcription && (!m.extra.is_empty() || m.reasoning.is_some())
        }) {
            return fail("only chat models take extra fields or a reasoning effort");
        }
        if self.models.iter().any(|m| {
            m.price_per_minute.is_some_and(|price| {
                m.kind != ModelType::Transcription || !price.is_finite() || price < 0.0
            })
        }) {
            return fail("only transcription models take a price per minute, of at least 0");
        }
        if self.models.iter().any(|m| {
            m.reasoning
                .as_deref()
                .is_some_and(|r| !["off", "low", "medium", "high"].contains(&r))
        }) {
            return fail("reasoning is off, low, medium or high");
        }
        for kind in [ModelType::Chat, ModelType::Transcription] {
            let served: Vec<&String> = self
                .models
                .iter()
                .filter(|m| m.kind == kind)
                .flat_map(|m| &m.kinds)
                .collect();
            if !distinct(&served) {
                return fail("a session kind has at most one model of each type");
            }
        }
        if self
            .models
            .iter()
            .flat_map(|m| m.kinds.iter().chain(&m.translates))
            .any(|kind| !self.kinds.contains(kind))
        {
            return fail("a model serves only configured session kinds");
        }
        let translated: Vec<&String> = self.models.iter().flat_map(|m| &m.translates).collect();
        if !distinct(&translated) {
            return fail("a session kind has at most one translation model");
        }
        if self
            .models
            .iter()
            .any(|m| m.kind != ModelType::Chat && !m.translates.is_empty())
        {
            return fail("only chat models translate");
        }
        let translation = &self.translation;
        if !translation.model.is_empty()
            && self
                .model(&translation.model)
                .is_none_or(|model| model.kind != ModelType::Chat)
        {
            return fail("translation must name a registered chat model");
        }
        let unknown = |name: &str| {
            self.model(name)
                .is_none_or(|model| model.kind != ModelType::Chat)
        };
        // An empty default names no model: sessions say so until one is chosen.
        if !self.stt.model.is_empty()
            && self
                .model(&self.stt.model)
                .is_none_or(|model| model.kind != ModelType::Transcription)
        {
            return fail("transcription must name a registered transcription model");
        }
        if !self.llm.model.is_empty() && unknown(&self.llm.model) {
            return fail("the assistant must name a registered chat model");
        }
        if let Some(reviewer) = &self.reviewer
            && (reviewer.enabled || !reviewer.model.is_empty())
            && unknown(&reviewer.model)
        {
            return fail("a reviewer that is on must name a registered chat model");
        }
        if self
            .actions
            .iter()
            .any(|a| a.model.as_deref().is_some_and(unknown))
        {
            return fail("an action's model must be a registered chat model");
        }
        let stt = &self.stt;
        if stt.languages.is_empty() || !stt.languages.iter().all(|code| valid_language(code)) {
            return fail("languages must be language codes (pt, en, ja…) or auto");
        }
        if !distinct(&stt.languages) {
            return fail("languages must not repeat");
        }
        if !stt.languages.contains(&stt.language) {
            return Err(ConfigError(format!(
                "language {:?} is not in languages",
                stt.language
            )));
        }
        let names: Vec<&String> = self.participants.iter().map(|p| &p.name).collect();
        if !distinct(&names) || names.iter().any(|name| name.trim().is_empty()) {
            return fail("participant names must be unique and not empty");
        }
        if self.participants.iter().filter(|p| p.user).count() > 1 {
            return fail("only one participant can be the user");
        }
        let devices: Vec<&String> = self.participants.iter().flat_map(|p| &p.devices).collect();
        if !distinct(&devices) {
            return fail("a device can belong to one participant only");
        }
        if !valid_ui_language(&self.ui_language) {
            return fail("ui language must be auto or a code like pt-BR");
        }
        if !["system", "dark", "light", "custom"].contains(&self.appearance.preset.as_str()) {
            return fail("appearance preset must be system, dark, light or custom");
        }
        if !self.appearance.colors.iter().all(|(key, color)| {
            [
                "background",
                "foreground",
                "accent",
                "cyan",
                "muted",
                "green",
                "yellow",
                "red",
                "magenta",
            ]
            .contains(&key.as_str())
                && valid_color(color)
        }) {
            return fail("appearance colors must name a theme color and use #rrggbb");
        }
        if !self.colors.values().all(|color| valid_color(color)) {
            return fail("colors must be #rrggbb");
        }
        let actions: Vec<&String> = self.actions.iter().map(|a| &a.name).collect();
        if !distinct(&actions) {
            return fail("action names must be unique");
        }
        crate::shortcuts::bindings(self.shortcuts.as_ref()).map_err(ConfigError)?;
        Ok(())
    }
}

/// The files, read and joined, `~` expanded.
fn read_all(files: &[String]) -> std::io::Result<String> {
    let texts = files
        .iter()
        .map(|file| fs::read_to_string(paths::expand(file)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(texts.join("\n\n"))
}

fn distinct<T: Eq + std::hash::Hash>(items: &[T]) -> bool {
    items.iter().collect::<HashSet<_>>().len() == items.len()
}

/// "auto", or a language and an optional region: `ja`, `pt-BR`.
fn valid_ui_language(code: &str) -> bool {
    if code == "auto" {
        return true;
    }
    let (language, region) = code
        .split_once('-')
        .map_or((code, None), |(l, r)| (l, Some(r)));
    let lower =
        (2..=3).contains(&language.len()) && language.chars().all(|c| c.is_ascii_lowercase());
    let upper = region.is_none_or(|r| r.len() == 2 && r.chars().all(|c| c.is_ascii_uppercase()));
    lower && upper
}

pub(crate) fn valid_color(color: &str) -> bool {
    color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// The interface pack and transcription language a first config starts in, by
/// the system language: (pack, transcription code). English is the first, for
/// any language without a pack.
const FIRST_LANGUAGES: [(&str, &str); 3] = [("en-US", "en"), ("pt-BR", "pt"), ("ja-JP", "ja")];

/// The (pack, transcription code) of the system language `locale`
/// (`pt_BR.UTF-8`, `ja-JP`); English for a language eco has no pack for, or none.
fn system_language(locale: Option<&str>) -> &'static (&'static str, &'static str) {
    let language = locale
        .map(|locale| locale.split(['_', '-', '.', '@']).next().unwrap_or(""))
        .unwrap_or("")
        .to_ascii_lowercase();
    FIRST_LANGUAGES
        .iter()
        .find(|(_, code)| *code == language)
        .unwrap_or(&FIRST_LANGUAGES[0])
}

/// The config of a first run, with the presets, in the system language `locale` (`pt_BR.UTF-8`, `ja-JP`):
/// its interface, its transcription language and its rules; English for a
/// language eco has no pack for, or none. Its audio sources are `Me` on the
/// default input, marked as the user, and `Them` on the default output.
fn first_run(locale: Option<&str>) -> Config {
    let (pack, code) = system_language(locale);
    let languages = ["auto", code]
        .into_iter()
        .map(String::from)
        .chain(
            default_languages()
                .into_iter()
                .filter(|l| l != "auto" && l != code),
        )
        .collect();
    let raw = Raw {
        participants: vec![
            Participant {
                name: "Me".into(),
                devices: vec![DEFAULT_INPUT.into()],
                user: true,
            },
            Participant {
                name: "Them".into(),
                devices: vec![DEFAULT_OUTPUT.into()],
                user: false,
            },
        ],
        rules: prompts::default_rules(code).into(),
        stt: SttConfig {
            language: (*code).into(),
            languages,
            ..SttConfig::default()
        },
        ui: UiTable {
            language: (*pack).into(),
            ..UiTable::default()
        },
        ..toml::from_str("").expect("the empty file is a config")
    };
    let mut config = Config::from_raw(raw).expect("a first config is valid");
    crate::presets::seed(&mut config);
    config
}

/// The config at `path`; no file there is the first-run config in the system
/// language with the presets, until the first save writes it. An old file is
/// converted and given the presets in memory only (see `convert`).
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    read(path, sys_locale::get_locale().as_deref()).map(|(config, _)| config)
}

/// `load` for the daemon, which alone writes the config: an old file is
/// converted, kept beside itself as `config.toml.bak` and saved in the new
/// shape. When that fails the converted config is still returned, with why.
pub fn load_converting(path: &Path) -> Result<(Config, Option<ConfigError>), ConfigError> {
    let (config, old) = read(path, sys_locale::get_locale().as_deref())?;
    let failure = old.and_then(|old| {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(".bak");
        write_atomically(&path.with_file_name(name), old.as_bytes())
            .map_err(|error| ConfigError(format!("{}.bak: {error}", path.display())))
            .and_then(|()| save(&config, path))
            .err()
    });
    Ok((config, failure))
}

/// The config at `path` and, when it was in the old shape, its text.
fn read(path: &Path, locale: Option<&str>) -> Result<(Config, Option<String>), ConfigError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((first_run(locale), None));
        }
        Err(error) => return Err(ConfigError(format!("{}: {error}", path.display()))),
    };
    let mut value: Value = toml::from_str(&text).map_err(|error| {
        ConfigError(format!("{}: {}", path.display(), error.to_string().trim()))
    })?;
    let converted = convert(&mut value);
    let raw: Raw = serde_json::from_value(value)
        .map_err(|error| ConfigError(format!("invalid config: {error}")))?;
    let mut config = Config::from_raw(raw)?;
    if converted {
        crate::presets::seed(&mut config);
    }
    Ok((config, converted.then_some(text)))
}

/// The fields a model had before providers and secrets: its address and its key source.
const OLD_FIELDS: [&str; 4] = [
    "base_url",
    "api_key_env",
    "api_key_omapass",
    "api_key_credential",
];

/// Point every model that still carries `base_url` or `api_key_*` fields at a
/// provider and a secret, in `value`'s `providers` and `secrets` lists: one
/// provider per distinct (base URL, key source), one secret per distinct key
/// source, reused when the list already has them, named after the preset with the same variable or base URL, else from the host and the
/// source and numbered when the name is taken. Every other field of the model
/// stays. Where an old model names several key sources, the credential wins,
/// then omapass, then the variable. A provider or secret the conversion left
/// unused is dropped. Whether any model was converted.
fn convert(value: &mut Value) -> bool {
    let Some(root) = value.as_object_mut() else {
        return false;
    };
    let old = root
        .get("models")
        .and_then(Value::as_array)
        .is_some_and(|models| {
            models
                .iter()
                .any(|model| OLD_FIELDS.iter().any(|field| model.get(field).is_some()))
        });
    if !old {
        return false;
    }
    let mut take = |name: &str| match root.remove(name) {
        Some(Value::Array(items)) => items,
        _ => Vec::new(),
    };
    let (mut secrets, mut providers, mut models) =
        (take("secrets"), take("providers"), take("models"));
    let text =
        |item: &Value, field: &str| item.get(field).and_then(Value::as_str).map(String::from);
    let providers_used = |models: &[Value]| -> BTreeSet<String> {
        models.iter().filter_map(|m| text(m, "provider")).collect()
    };
    let before = providers_used(&models);
    for model in &mut models {
        let Some(fields) = model.as_object_mut() else {
            continue;
        };
        if !OLD_FIELDS.iter().any(|field| fields.contains_key(*field)) {
            continue;
        }
        let mut old = |field: &str| match fields.remove(field) {
            Some(Value::String(text)) => Some(text),
            _ => None,
        };
        let (url, env, omapass, credential) = (
            old("base_url"),
            old("api_key_env"),
            old("api_key_omapass"),
            old("api_key_credential"),
        );
        let named = fields.get("provider").and_then(Value::as_str);
        let url = url
            .or_else(|| {
                let provider = providers
                    .iter()
                    .find(|p| text(p, "name").as_deref() == named)?;
                text(provider, "base_url")
            })
            .unwrap_or_default();
        let mut secret_of = |field: &str, source: &str| {
            let found = secrets
                .iter()
                .find(|s| text(s, field).as_deref() == Some(source));
            match found.and_then(|s| text(s, "name")) {
                Some(name) => name,
                None => {
                    let preset = (field == "env")
                        .then(|| crate::presets::secret_named(source))
                        .flatten();
                    let name = unique(&secrets, preset.unwrap_or(source));
                    secrets.push(serde_json::json!({"name": name, field: source}));
                    name
                }
            }
        };
        let secret = match Key::of(env.as_deref(), omapass.as_deref(), credential.as_deref()) {
            Key::None => None,
            Key::Env(name) => Some(secret_of("env", name)),
            Key::Omapass(name) => Some(secret_of("omapass", name)),
            Key::Credential(name) => Some(secret_of("credential", name)),
        };
        let found = providers.iter().find(|p| {
            text(p, "base_url").as_deref() == Some(url.as_str()) && text(p, "secret") == secret
        });
        let provider = match found.and_then(|p| text(p, "name")) {
            Some(name) => name,
            None => {
                let preset = crate::presets::provider_named(&url);
                let name = unique(
                    &providers,
                    &preset.map_or_else(|| host_label(&url), String::from),
                );
                let mut provider = serde_json::json!({"name": name, "base_url": url});
                if let Some(secret) = &secret {
                    provider["secret"] = secret.clone().into();
                }
                providers.push(provider);
                name
            }
        };
        fields.insert("provider".into(), provider.into());
    }
    let after = providers_used(&models);
    let dropped: Vec<Value> = providers
        .extract_if(.., |p| {
            text(p, "name").is_some_and(|name| before.contains(&name) && !after.contains(&name))
        })
        .collect();
    for gone in dropped.iter().filter_map(|p| text(p, "secret")) {
        if !providers
            .iter()
            .any(|p| text(p, "secret").as_deref() == Some(gone.as_str()))
        {
            secrets.retain(|s| text(s, "name").as_deref() != Some(gone.as_str()));
        }
    }
    root.insert("models".into(), models.into());
    for (name, list) in [("secrets", secrets), ("providers", providers)] {
        if !list.is_empty() {
            root.insert(name.into(), list.into());
        }
    }
    true
}

/// `name`, or `name-2`, `name-3`… when an item of `items` already has that name.
fn unique(items: &[Value], name: &str) -> String {
    let taken = |candidate: &str| {
        items
            .iter()
            .any(|item| item.get("name").and_then(Value::as_str) == Some(candidate))
    };
    (1..)
        .map(|n| {
            if n == 1 {
                name.to_string()
            } else {
                format!("{name}-{n}")
            }
        })
        .find(|candidate| !taken(candidate))
        .expect("an unbounded count finds a free name")
}

/// What names the provider at `base_url`: its host without `api.` and the
/// top-level domain (`openrouter` for https://openrouter.ai/api/v1); an IP
/// address or a bare host stays whole.
fn host_label(base_url: &str) -> String {
    let rest = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest);
    let host = rest.split(['/', ':', '?']).next().unwrap_or_default();
    let host = host.strip_prefix("api.").unwrap_or(host);
    let labels: Vec<&str> = host.split('.').collect();
    let address = labels.iter().all(|l| l.chars().all(|c| c.is_ascii_digit()));
    match labels.as_slice() {
        [.., name, _] if !address && !name.is_empty() => (*name).into(),
        _ if host.is_empty() => "provider".into(),
        _ => host.into(),
    }
}

/// Options that create a file only its owner may read or write (0600).
#[cfg(unix)]
pub fn private_file() -> fs::OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = fs::OpenOptions::new();
    options.mode(0o600);
    options
}

/// Options that create a file only its owner may read or write: under the
/// user's profile, a new file inherits an access list that admits only the
/// user, the system and administrators.
#[cfg(windows)]
pub fn private_file() -> fs::OpenOptions {
    fs::OpenOptions::new()
}

/// Create `path` and its missing parents, each only its owner may enter (0700).
#[cfg(unix)]
pub fn private_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

/// Create `path` and its missing parents, each only its owner may enter: under
/// the user's profile, through the access list it inherits.
#[cfg(windows)]
pub fn private_dir(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)
}

/// Replace `path` with `bytes` through a new private temporary file, so a crash
/// never leaves half a file; its directory is created private when missing.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        private_dir(parent)?;
    }
    replace_file(path, bytes, 0o600)
}

/// Create `path`, which must not exist, with `mode`.
#[cfg(unix)]
fn create_new(path: &Path, mode: u32) -> std::io::Result<fs::File> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)?;
    // The umask may have cleared bits of `mode`.
    file.set_permissions(fs::Permissions::from_mode(mode))?;
    Ok(file)
}

/// Create `path`, which must not exist; Windows has no modes, and the file
/// takes the access list of its directory.
#[cfg(windows)]
fn create_new(path: &Path, _mode: u32) -> std::io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// Replace `path` with `bytes` through `<name>.tmp` beside it, created with
/// `mode` and synced before the rename, so a crash never leaves half a file.
pub fn replace_file(path: &Path, bytes: &[u8], mode: u32) -> std::io::Result<()> {
    use std::io::Write;
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    let temporary = path.with_file_name(name);
    // A temporary left by a crash would keep its mode, so it goes first.
    match fs::remove_file(&temporary) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let written = (|| {
        let mut file = create_new(&temporary, mode)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// Write the config atomically.
pub fn save(config: &Config, path: &Path) -> Result<(), ConfigError> {
    let text = toml::to_string_pretty(&config.to_raw())
        .map_err(|error| ConfigError(format!("cannot write the config: {error}")))?;
    write_atomically(path, text.as_bytes())
        .map_err(|error| ConfigError(format!("{}: {error}", path.display())))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A config in the shape before providers and secrets, or in the new one, as the file is read.
    fn from_old(mut value: Value) -> Result<Config, ConfigError> {
        convert(&mut value);
        Config::from_value(value)
    }

    #[test]
    fn context_slots_are_read_and_their_names_must_differ() {
        let directory = tempfile::tempdir().unwrap();
        let cv = directory.path().join("cv.md");
        fs::write(&cv, "Currículo").unwrap();
        let mut value = raw();
        value["contexts"] = json!([
            {"name": "entrevista", "files": [cv.to_string_lossy()], "kinds": ["meeting"]},
            {"name": "faltando", "files": [directory.path().join("nada.md").to_string_lossy()]},
        ]);
        let config = from_old(value.clone()).unwrap();
        let texts = config.slot_texts();
        assert_eq!(texts[0].0.kinds, ["meeting"]);
        assert_eq!(texts[0].1.as_ref().unwrap(), "Currículo");
        assert!(texts[1].1.is_err());
        assert_eq!(from_old(config.to_value()).unwrap(), config);
        value["contexts"][1]["name"] = json!("entrevista");
        assert!(from_old(value).is_err());
    }

    #[test]
    fn a_reviewer_is_optional_and_names_a_model_when_on() {
        assert_eq!(from_old(raw()).unwrap().reviewer, None);
        let reviewer = json!({"enabled": true, "model": "grok"});
        let config = from_old(with("reviewer", reviewer.clone())).unwrap();
        let on = config.reviewer.as_ref().unwrap();
        assert_eq!((on.prompt.as_str(), on.verbose), (DEFAULT_REVIEW, false));
        assert_eq!(from_old(config.to_value()).unwrap(), config);
        let mut other = reviewer;
        other["model"] = json!("whisper");
        assert!(error_of(with("reviewer", other)).contains("reviewer"));
        // A reviewer with no model takes the first usable one, on or off.
        let unnamed = json!({"enabled": true, "model": ""});
        assert!(from_old(with("reviewer", unnamed)).is_ok());
    }

    #[test]
    fn roles_and_actions_name_registered_models() {
        let config = from_old(raw()).unwrap();
        assert_eq!(config.model("grok").unwrap().model, "x-ai/grok-4.20");
        let mut value = raw();
        value["actions"][0]["model"] = json!("nenhum");
        assert!(error_of(value).contains("action"));
        let mut value = raw();
        value["models"][2]["name"] = json!("gemini");
        assert!(error_of(value).contains("unique"));
        let mut value = raw();
        value["models"][1]["model"] = json!(" ");
        assert!(error_of(value).contains("model id"));
    }

    #[test]
    fn each_role_names_a_model_of_its_type() {
        let mut value = raw();
        value["actions"][0]["model"] = json!("whisper");
        assert!(error_of(value).contains("chat"));
        let mut value = raw();
        value["models"][0]["extra"] = json!({"temperature": 0});
        assert!(error_of(value).contains("extra"));
    }

    #[test]
    fn only_a_transcription_model_takes_a_price_per_minute_of_at_least_zero() {
        let mut value = raw();
        value["models"][0]["price_per_minute"] = json!(0.0077);
        let config = from_old(value.clone()).unwrap();
        assert_eq!(
            config.model("whisper").unwrap().price_per_minute,
            Some(0.0077)
        );
        assert_eq!(from_old(config.to_value()).unwrap(), config);
        value["models"][0]["price_per_minute"] = json!(-0.01);
        assert!(error_of(value).contains("price"));
        let mut value = raw();
        value["models"][1]["price_per_minute"] = json!(0.01);
        assert!(error_of(value).contains("price"));
    }

    #[test]
    fn a_session_kind_may_have_its_own_models() {
        let mut value = raw();
        value["models"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "local", "type": "transcription", "base_url": "http://lan:8000/v1", "model": "whisper-large-v3", "kinds": ["idea"]}));
        value["models"][2]["kinds"] = json!(["idea"]);
        let config = from_old(value.clone()).unwrap();
        assert_eq!(config.transcriber("idea").unwrap().name, "local");
        assert_eq!(config.transcriber("meeting").unwrap().name, "whisper");
        assert_eq!(
            config.model_of_kind(ModelType::Chat, "idea").unwrap().name,
            "grok"
        );
        assert!(config.model_of_kind(ModelType::Chat, "meeting").is_none());
        assert_eq!(from_old(config.to_value()).unwrap(), config);
        let mut twice = value.clone();
        twice["models"][1]["kinds"] = json!(["idea"]);
        assert!(error_of(twice).contains("at most one"));
        let mut unknown = value;
        unknown["models"][1]["kinds"] = json!(["party"]);
        assert!(error_of(unknown).contains("configured session kinds"));
    }

    #[test]
    fn translation_names_a_chat_model_only() {
        let config = from_old(raw()).unwrap();
        assert_eq!(config.translator("meeting").unwrap().name, "gemini");
        let mut value = with("translation", json!({"model": "grok"}));
        let config = from_old(value.clone()).unwrap();
        assert_eq!(config.translator("meeting").unwrap().name, "grok");
        assert_eq!(from_old(config.to_value()).unwrap(), config);
        value["models"][1]["translates"] = json!(["idea"]);
        assert_eq!(
            from_old(value.clone())
                .unwrap()
                .translator("idea")
                .unwrap()
                .name,
            "gemini"
        );
        assert!(error_of(with("translation", json!({"model": "whisper"}))).contains("chat"));
        assert!(error_of(with("translation", json!({"language": "en"}))).contains("unknown field"));
        value["models"][0]["translates"] = json!(["meeting"]);
        assert!(error_of(value).contains("only chat"));
    }

    #[test]
    fn answers_at_once_default_to_eight_and_are_at_least_one() {
        assert_eq!(from_old(raw()).unwrap().llm.concurrency, 8);
        let mut value = raw();
        value["llm"]["concurrency"] = json!(0);
        assert!(error_of(value).contains("concurrency"));
    }

    #[test]
    fn reasoning_is_sent_in_openrouter_form() {
        let mut value = raw();
        value["models"][1]["reasoning"] = json!("off");
        value["models"][2]["reasoning"] = json!("low");
        let config = from_old(value.clone()).unwrap();
        assert_eq!(
            config.models[1].request_fields()["reasoning"],
            json!({"enabled": false})
        );
        assert_eq!(
            config.models[2].request_fields()["reasoning"],
            json!({"effort": "low"})
        );
        let unchosen = from_old(raw()).unwrap();
        assert_eq!(
            unchosen.models[1].request_fields()["reasoning"],
            json!({"effort": "minimal"}),
            "extra is sent as written"
        );
        assert_eq!(from_old(config.to_value()).unwrap(), config);
        value["models"][2]["reasoning"] = json!("max");
        assert!(error_of(value.clone()).contains("reasoning"));
        value["models"][2]["reasoning"] = json!("low");
        value["models"][0]["reasoning"] = json!("low");
        assert!(error_of(value).contains("only chat"));
    }

    #[test]
    fn rules_start_as_the_defaults_and_keep_what_the_user_writes() {
        let config = from_old(raw()).unwrap();
        assert_eq!(config.rules, DEFAULT_RULES);
        let config = from_old(with("rules", json!("- Fale como eu."))).unwrap();
        assert_eq!(config.rules, "- Fale como eu.");
        assert_eq!(from_old(config.to_value()).unwrap(), config);
    }

    fn raw() -> Value {
        json!({
            "stt": {"model": "whisper", "language": "pt", "languages": ["auto", "pt", "ja"]},
            "llm": {"model": "gemini"},
            "models": [
                {"name": "whisper", "type": "transcription", "base_url": "http://lan:8081/v1", "model": "whisper-1"},
                {
                    "name": "gemini",
                    "type": "chat",
                    "base_url": "https://openrouter.ai/api/v1",
                    "model": "google/gemini-3.5-flash-lite",
                    "api_key_env": "OPENROUTER_API_KEY",
                    "extra": {"reasoning": {"effort": "minimal"}},
                },
                {
                    "name": "grok",
                    "type": "chat",
                    "base_url": "https://openrouter.ai/api/v1",
                    "model": "x-ai/grok-4.20",
                    "api_key_omapass": "openrouter",
                },
            ],
            "context": {"files": ["~/ctx.md"]},
            "participants": [
                {"name": "Eu", "user": true, "devices": ["@default-input"]},
                {"name": "Recrutador", "devices": ["@default-output", "alsa_output.usb-G522"]},
            ],
            "colors": {"@default-input": "#5EF2FF"},
            "actions": [
                {"name": "probe", "prompt": "Pergunte.\nCom porquê.", "format": "Lista.", "model": "grok"},
            ],
        })
    }

    fn with(key: &str, value: Value) -> Value {
        let mut raw = raw();
        raw[key] = value;
        raw
    }

    fn error_of(value: Value) -> String {
        from_old(value).expect_err("must be rejected").0
    }

    #[test]
    fn round_trips_through_toml() {
        let config = from_old(raw()).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("eco").join("config.toml");
        save(&config, &path).unwrap();
        assert_eq!(load(&path).unwrap(), config);
        assert_eq!(from_old(config.to_value()).unwrap(), config);
        let written: toml::Table = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            written["participants"][1]["devices"][1].as_str(),
            Some("alsa_output.usb-G522")
        );
        assert!(!path.with_file_name("config.toml.tmp").exists());
    }

    #[test]
    fn participants_keep_their_devices() {
        let config = from_old(raw()).unwrap();
        assert_eq!(
            config.participants[1],
            Participant {
                name: "Recrutador".into(),
                devices: vec!["@default-output".into(), "alsa_output.usb-G522".into()],
                user: false,
            }
        );
    }

    #[test]
    fn rejects_invalid_participants() {
        let cases = [
            (
                json!([{"name": "A", "devices": []}, {"name": "A", "devices": []}]),
                "unique",
            ),
            (json!([{"name": " ", "devices": []}]), "unique"),
            (
                json!([{"name": "A", "devices": [], "user": true}, {"name": "B", "devices": [], "user": true}]),
                "one participant",
            ),
            (
                json!([{"name": "A", "devices": ["d"]}, {"name": "B", "devices": ["d"]}]),
                "one participant only",
            ),
            (json!([{"name": "A"}]), "invalid"),
        ];
        for (participants, message) in cases {
            let error = error_of(with("participants", participants));
            assert!(error.contains(message), "{error:?} lacks {message:?}");
        }
    }

    #[test]
    fn a_missing_file_is_the_empty_config() {
        let directory = tempfile::tempdir().unwrap();
        let empty = read(&directory.path().join("nope.toml"), Some("pt_BR.UTF-8"))
            .unwrap()
            .0;
        assert_eq!(model_names(&empty), PRESET_MODELS);
        assert!(empty.actions.is_empty());
        assert_eq!(
            (empty.stt.model.as_str(), empty.llm.model.as_str()),
            ("", "")
        );
        assert_eq!(empty.kinds, default_kinds());
        assert_eq!(empty.transcriber("meeting"), None);
        assert_eq!(empty.translator("meeting"), None);
        let mut written_empty = from_old(json!({})).unwrap();
        crate::presets::seed(&mut written_empty);
        written_empty.participants = empty.participants.clone();
        written_empty.ui_language = "pt-BR".into();
        assert_eq!(written_empty, empty);
        assert!(!directory.path().join("nope.toml").exists());
    }

    #[test]
    fn a_fresh_install_hears_me_and_them_and_the_first_save_writes_them() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let config = load(&path).unwrap();
        let sources: Vec<_> = config
            .participants
            .iter()
            .map(|p| (p.name.as_str(), p.devices.clone(), p.user))
            .collect();
        assert_eq!(
            sources,
            [
                ("Me", vec!["@default-input".to_string()], true),
                ("Them", vec!["@default-output".to_string()], false),
            ]
        );
        assert_eq!(config.to_value()["participants"][1]["name"], "Them");
        save(&config, &path).unwrap();
        assert_eq!(load(&path).unwrap().participants, config.participants);
    }

    #[test]
    fn a_config_without_participants_keeps_none() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, "kinds = [\"meeting\"]\n").unwrap();
        assert!(load(&path).unwrap().participants.is_empty());
    }

    #[test]
    fn a_first_config_follows_the_system_language_and_falls_back_to_english() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("config.toml");
        let first = |locale: Option<&str>| {
            let config = read(&missing, locale).unwrap().0;
            (
                config.ui_language,
                config.stt.language,
                config.stt.languages,
                config.rules,
            )
        };
        let (ui, stt, languages, rules) = first(Some("pt_BR.UTF-8"));
        assert_eq!((ui.as_str(), stt.as_str()), ("pt-BR", "pt"));
        assert_eq!(languages, ["auto", "pt", "en", "es"]);
        assert_eq!(rules, DEFAULT_RULES);
        let (ui, stt, languages, rules) = first(Some("ja-JP"));
        assert_eq!((ui.as_str(), stt.as_str()), ("ja-JP", "ja"));
        assert_eq!(languages, ["auto", "ja", "pt", "en", "es"]);
        assert!(
            rules.starts_with("- 文字起こしと同じ言語で答えて"),
            "{rules}"
        );
        let english = first(Some("en-GB"));
        assert_eq!((english.0.as_str(), english.1.as_str()), ("en-US", "en"));
        assert_eq!(english.2, ["auto", "en", "pt", "es"]);
        assert!(
            english
                .3
                .starts_with("- Answer in the language of the transcript.")
        );
        assert_eq!(first(Some("de-DE")), english);
        assert_eq!(first(Some("C")), english);
        assert_eq!(first(None), english);
        assert!(!missing.exists());
    }

    #[test]
    fn an_existing_config_keeps_its_language_whatever_the_system_says() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, "kinds = [\"meeting\"]\n").unwrap();
        let config = read(&path, Some("ja_JP.UTF-8")).unwrap().0;
        assert_eq!(config.ui_language, "auto");
        assert_eq!(config.stt.language, "pt");
        assert_eq!(config.rules, DEFAULT_RULES);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "kinds = [\"meeting\"]\n"
        );
    }

    fn models_of(names: &[(&str, &str)]) -> Value {
        let models: Vec<Value> = names
            .iter()
            .map(|(name, kind)| {
                json!({"name": name, "type": kind, "base_url": "http://x", "model": name})
            })
            .collect();
        json!({"models": models})
    }

    fn defaults_of(value: Value) -> (String, String) {
        let config = from_old(value).unwrap();
        (config.llm.model, config.stt.model)
    }

    #[test]
    fn an_empty_default_stays_empty_so_it_stays_automatic() {
        let both = models_of(&[("w", "transcription"), ("m", "chat"), ("n", "chat")]);
        assert_eq!(defaults_of(both), (String::new(), String::new()));
    }

    #[test]
    fn a_chosen_default_is_kept_and_a_lost_one_is_refused() {
        let mut value = models_of(&[("w", "transcription"), ("m", "chat"), ("n", "chat")]);
        value["llm"] = json!({"model": "n"});
        value["stt"] = json!({"model": "w"});
        assert_eq!(defaults_of(value.clone()), ("n".into(), "w".into()));
        value["translation"] = json!({"model": "gone"});
        assert!(error_of(value.clone()).contains("translation must name a registered chat model"));
        value["translation"] = json!({});
        value["models"].as_array_mut().unwrap().remove(2);
        assert!(error_of(value.clone()).contains("the assistant must name"));
        value["llm"] = json!({});
        value["models"].as_array_mut().unwrap().remove(0);
        assert!(error_of(value).contains("transcription must name"));
    }

    #[test]
    fn an_unreadable_or_malformed_file_names_itself() {
        let directory = tempfile::tempdir().unwrap();
        let shown = directory.path().display();
        let error = load(directory.path()).unwrap_err().0;
        assert!(error.starts_with(&format!("{shown}: ")), "{error}");

        let path = directory.path().join("config.toml");
        fs::write(&path, "stt = ").unwrap();
        let error = load(&path).unwrap_err().0;
        assert!(
            error.starts_with(&format!("{}: ", path.display())),
            "{error}"
        );

        fs::write(&path, "[stt]\nmodel = \"whisper\"\n").unwrap();
        let error = load(&path).unwrap_err().0;
        assert_eq!(
            error,
            "transcription must name a registered transcription model"
        );

        fs::write(&path, "[llm]\nmodel = \"whisper\"\n").unwrap();
        let error = load(&path).unwrap_err().0;
        assert_eq!(error, "the assistant must name a registered chat model");

        fs::write(&path, "[llm]\nmodle = \"m\"\n").unwrap();
        let error = load(&path).unwrap_err().0;
        assert!(error.starts_with("invalid config: "), "{error}");
        assert!(error.contains("unknown field `modle`"), "{error}");
    }

    #[test]
    fn the_global_context_joins_its_files() {
        let directory = tempfile::tempdir().unwrap();
        let (cv, notes) = (
            directory.path().join("cv.md"),
            directory.path().join("notes.md"),
        );
        fs::write(&cv, "Currículo").unwrap();
        fs::write(&notes, "Notas").unwrap();
        let files = json!([cv.to_string_lossy(), notes.to_string_lossy()]);
        let config = from_old(with("context", json!({"files": files}))).unwrap();
        assert_eq!(config.context().unwrap(), "Currículo\n\nNotas");
        fs::remove_file(&notes).unwrap();
        assert!(config.context().is_err());
    }

    #[test]
    fn kinds_translations_and_actions_must_not_repeat() {
        for kinds in [json!([]), json!(["meeting", "meeting"]), json!([" idea"])] {
            assert!(error_of(with("kinds", kinds)).contains("kinds must be unique"));
        }
        let mut value = raw();
        value["models"][1]["translates"] = json!(["meeting"]);
        value["models"][2]["translates"] = json!(["meeting"]);
        assert!(error_of(value).contains("at most one translation model"));
        let mut value = raw();
        let probe = value["actions"][0].clone();
        value["actions"].as_array_mut().unwrap().push(probe);
        assert!(error_of(value).contains("action names must be unique"));
    }

    #[test]
    fn languages_default_when_absent() {
        let config = from_old(with("stt", json!({"model": "whisper"}))).unwrap();
        assert_eq!(config.stt.languages, ["auto", "pt", "en", "es"]);
        assert_eq!(config.stt.language, "pt");
    }

    #[test]
    fn rejects_invalid_languages() {
        let cases = [
            (
                json!({"language": "fr", "languages": ["auto", "pt"]}),
                "not in languages",
            ),
            (
                json!({"language": "pt", "languages": ["pt", "pt"]}),
                "repeat",
            ),
            (
                json!({"language": "pt", "languages": ["pt", "português"]}),
                "language codes",
            ),
            (json!({"language": "pt", "languages": []}), "language codes"),
        ];
        for (mut stt, message) in cases {
            stt["model"] = json!("whisper");
            let error = error_of(with("stt", stt));
            assert!(error.contains(message), "{error:?} lacks {message:?}");
        }
    }

    #[test]
    fn colors_must_be_hex() {
        assert!(error_of(with("colors", json!({"@default-input": "red"}))).contains("#rrggbb"));
    }

    #[test]
    fn appearance_defaults_to_system_and_preserves_custom_colors_in_every_preset() {
        assert_eq!(
            from_old(raw()).unwrap().appearance,
            AppearanceConfig::default()
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        for preset in ["system", "dark", "light", "custom"] {
            let appearance =
                json!({"preset": preset, "colors": {"background": "#102030", "accent": "#AbCdEf"}});
            let config = from_old(with("ui", json!({"appearance": appearance}))).unwrap();
            assert_eq!(config.to_value()["ui"]["appearance"], appearance);
            save(&config, &path).unwrap();
            assert_eq!(load(&path).unwrap(), config);
        }
        for appearance in [
            json!({"preset": "sepia"}),
            json!({"colors": {"accent": "red"}}),
            json!({"colors": {"unknown": "#123456"}}),
        ] {
            assert!(error_of(with("ui", json!({"appearance": appearance}))).contains("appearance"));
        }
        assert!(
            error_of(with("ui", json!({"appearance": {"unknown": true}})))
                .contains("unknown field")
        );
    }

    #[test]
    fn ui_language_defaults_to_auto_and_round_trips() {
        assert_eq!(from_old(raw()).unwrap().ui_language, "auto");
        let config = from_old(with("ui", json!({"language": "ja-JP"}))).unwrap();
        assert_eq!(from_old(config.to_value()).unwrap().ui_language, "ja-JP");
        assert!(error_of(with("ui", json!({"language": "japanese"}))).contains("ui language"));
    }

    #[test]
    fn hide_from_share_defaults_to_off_and_round_trips() {
        assert!(!from_old(raw()).unwrap().hide_from_share);
        let config = from_old(with("ui", json!({"hide_from_share": true}))).unwrap();
        assert!(config.hide_from_share);
        assert_eq!(config.ui_language, "auto");
        assert!(from_old(config.to_value()).unwrap().hide_from_share);
        assert!(error_of(with("ui", json!({"hide_from_share": "yes"}))).contains("invalid config"));
    }

    #[test]
    fn the_tray_defaults_to_off_and_round_trips() {
        assert!(!from_old(raw()).unwrap().tray);
        let config = from_old(with("ui", json!({"tray": true}))).unwrap();
        assert!(config.tray);
        assert!(!config.hide_from_share);
        assert!(from_old(config.to_value()).unwrap().tray);
        assert!(error_of(with("ui", json!({"tray": "yes"}))).contains("invalid config"));
    }

    #[test]
    fn notifications_default_to_off_and_round_trip() {
        assert!(!from_old(raw()).unwrap().notifications);
        let config = from_old(with("ui", json!({"notifications": true}))).unwrap();
        assert!(config.notifications);
        assert!(!config.tray);
        assert!(from_old(config.to_value()).unwrap().notifications);
        assert!(error_of(with("ui", json!({"notifications": "yes"}))).contains("invalid config"));
    }

    #[test]
    fn the_interface_pack_is_the_chosen_one_or_the_system_s_on_auto() {
        let mut config = from_old(raw()).unwrap();
        assert_eq!(config.interface_pack(Some("pt_BR.UTF-8")), "pt-BR");
        assert_eq!(config.interface_pack(Some("ja-JP")), "ja-JP");
        assert_eq!(config.interface_pack(Some("de_DE")), "en-US");
        assert_eq!(config.interface_pack(None), "en-US");
        config.ui_language = "ja-JP".into();
        assert_eq!(config.interface_pack(Some("pt_BR.UTF-8")), "ja-JP");
    }

    #[test]
    fn the_update_check_defaults_to_on_and_round_trips() {
        assert!(from_old(raw()).unwrap().update.check);
        assert!(from_old(with("update", json!({}))).unwrap().update.check);
        let config = from_old(with("update", json!({"check": false}))).unwrap();
        assert!(!config.update.check);
        assert!(!from_old(config.to_value()).unwrap().update.check);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        save(&config, &path).unwrap();
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("[update]\ncheck = false")
        );
        assert!(!load(&path).unwrap().update.check);
        assert!(error_of(with("update", json!({"check": "yes"}))).contains("invalid config"));
    }

    #[test]
    fn shortcuts_keep_the_defaults_until_written_and_must_be_valid() {
        let config = from_old(raw()).unwrap();
        assert_eq!(config.shortcuts, None);
        assert!(config.to_value().get("shortcuts").is_none());
        let config = from_old(with("shortcuts", json!({"Ctrl+Alt+M": "sessions"}))).unwrap();
        assert_eq!(
            config.shortcuts,
            Some(BTreeMap::from([("Ctrl+Alt+M".into(), "sessions".into())]))
        );
        assert_eq!(from_old(config.to_value()).unwrap(), config);
        assert_eq!(
            error_of(with("shortcuts", json!({"M": "sessions"}))),
            "shortcut \"M\": needs Ctrl, Alt, Shift or Win with the key"
        );
    }

    /// A config in the new shape: one secret in each store, a keyless LAN
    /// provider, and three models.
    fn shaped() -> Value {
        json!({
            "secrets": [
                {"name": "openrouter", "omapass": "openrouter"},
                {"name": "deepgram", "env": "DEEPGRAM_API_KEY"},
            ],
            "providers": [
                {"name": "lan", "base_url": "http://lan:8081/v1"},
                {"name": "openrouter", "base_url": "https://openrouter.ai/api/v1", "secret": "openrouter"},
                {"name": "deepgram", "base_url": "wss://api.deepgram.com/v1/listen", "secret": "deepgram"},
            ],
            "models": [
                {"name": "whisper", "type": "transcription", "provider": "lan", "model": "whisper-1"},
                {"name": "dg", "type": "transcription", "provider": "deepgram", "model": "nova-3"},
                {"name": "fast", "type": "chat", "provider": "openrouter", "model": "x/fast"},
            ],
        })
    }

    #[test]
    fn secrets_providers_and_models_are_linked_by_name() {
        let config = from_old(shaped()).unwrap();
        let resolved = config.resolved();
        assert_eq!(resolved[0].base_url, "http://lan:8081/v1");
        assert_eq!(resolved[0].key(), Key::None);
        assert_eq!(resolved[1].key(), Key::Env("DEEPGRAM_API_KEY"));
        assert_eq!(resolved[2].base_url, "https://openrouter.ai/api/v1");
        assert_eq!(resolved[2].key(), Key::Omapass("openrouter"));
        let written = toml::to_string_pretty(&config.to_raw()).unwrap();
        for text in [
            "[[secrets]]",
            "[[providers]]",
            "provider = \"lan\"",
            "secret = \"openrouter\"",
        ] {
            assert!(written.contains(text), "{text} missing from\n{written}");
        }
        assert!(!written.contains("api_key"), "{written}");
        let again: Raw = toml::from_str(&written).unwrap();
        assert_eq!(Config::from_raw(again).unwrap(), config);
    }

    #[test]
    fn links_and_names_must_hold() {
        type Change = fn(&mut Value);
        let cases: [(&str, Change, &str); 8] = [
            (
                "two sources",
                |v| v["secrets"][1]["omapass"] = json!("dg"),
                "exactly one of env",
            ),
            (
                "no source",
                |v| v["secrets"][1] = json!({"name": "deepgram"}),
                "exactly one of env",
            ),
            (
                "same secret",
                |v| v["secrets"][1]["name"] = json!("openrouter"),
                "secret names",
            ),
            (
                "credential word",
                |v| v["secrets"][1] = json!({"name": "deepgram", "credential": "a b"}),
                "a word with no spaces",
            ),
            (
                "same provider",
                |v| v["providers"][1]["name"] = json!("lan"),
                "provider names",
            ),
            (
                "no url",
                |v| v["providers"][0]["base_url"] = json!(" "),
                "needs a base URL",
            ),
            (
                "lost secret",
                |v| v["providers"][1]["secret"] = json!("gone"),
                "must be a registered secret",
            ),
            (
                "lost provider",
                |v| v["models"][2]["provider"] = json!("gone"),
                "registered provider",
            ),
        ];
        for (what, change, message) in cases {
            let mut value = shaped();
            change(&mut value);
            let error = error_of(value);
            assert!(
                error.contains(message),
                "{what}: {error:?} lacks {message:?}"
            );
        }
        let mut value = shaped();
        value["models"][2]["model"] = json!("");
        assert!(error_of(value).contains("model id"));
        assert_eq!(Key::of(Some("X"), Some(" "), None), Key::Env("X"));
        assert_eq!(Key::of(None, None, Some(" ")), Key::None);
    }

    #[test]
    fn a_secret_may_be_a_windows_credential() {
        let mut value = shaped();
        value["secrets"][0] = json!({"name": "openrouter", "credential": "openrouter"});
        let config = from_old(value).unwrap();
        assert_eq!(config.resolved()[2].key(), Key::Credential("openrouter"));
        let written = toml::to_string(&config.to_raw()).unwrap();
        assert!(written.contains("credential = \"openrouter\""), "{written}");
    }

    /// A synthetic config shaped like a 0.13.0 one: every model carries its
    /// address and key source. Two models share an address with different
    /// keys, one is on the LAN with no key, one keeps its key in omapass and
    /// two share a provider and a key.
    const OLD: &str = r#"
kinds = ["meeting", "idea"]

[stt]
model = "lan"
language = "pt"

[llm]
model = "fast"

[[models]]
name = "lan"
type = "transcription"
base_url = "http://192.0.2.10:8000/v1"
model = "whisper-large-v3"
price_per_minute = 0.0

[[models]]
name = "dg"
type = "transcription"
base_url = "wss://api.deepgram.com/v1/listen"
model = "nova-3"
api_key_env = "DEEPGRAM_API_KEY"

[[models]]
name = "fast"
type = "chat"
base_url = "https://openrouter.ai/api/v1"
model = "google/gemini-3.5-flash-lite"
api_key_env = "OPENROUTER_API_KEY"
reasoning = "low"
kinds = ["idea"]

[[models]]
name = "deep"
type = "chat"
base_url = "https://openrouter.ai/api/v1"
model = "anthropic/claude"
api_key_omapass = "openrouter"
extra = { temperature = 0.2 }

[[models]]
name = "second"
type = "chat"
base_url = "https://openrouter.ai/api/v1"
model = "other/model"
api_key_env = "OPENROUTER_API_KEY"
translates = ["meeting"]
"#;

    #[test]
    fn an_old_config_becomes_providers_and_secrets() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, OLD).unwrap();
        let (config, old) = read(&path, Some("en_US.UTF-8")).unwrap();
        assert_eq!(old.as_deref(), Some(OLD));
        let names = |items: Vec<(&str, &str)>| -> Vec<String> {
            items.iter().map(|(a, b)| format!("{a}={b}")).collect()
        };
        assert_eq!(
            names(
                config
                    .secrets
                    .iter()
                    .map(|s| (
                        s.name.as_str(),
                        s.env.as_deref().or(s.omapass.as_deref()).unwrap()
                    ))
                    .collect()
            ),
            [
                "DEEPGRAM=DEEPGRAM_API_KEY",
                "OPENROUTER=OPENROUTER_API_KEY",
                "openrouter=openrouter",
                "ELEVENLABS=ELEVEN_LABS_API_KEY",
                "GROQ=GROQ_API_KEY"
            ]
        );
        let providers: Vec<_> = config
            .providers
            .iter()
            .map(|p| (p.name.as_str(), p.base_url.as_str(), p.secret.as_deref()))
            .collect();
        assert_eq!(
            providers,
            [
                ("192.0.2.10", "http://192.0.2.10:8000/v1", None),
                (
                    "DEEPGRAM",
                    "wss://api.deepgram.com/v1/listen",
                    Some("DEEPGRAM")
                ),
                (
                    "OPENROUTER",
                    "https://openrouter.ai/api/v1",
                    Some("OPENROUTER")
                ),
                (
                    "OPENROUTER-2",
                    "https://openrouter.ai/api/v1",
                    Some("openrouter")
                ),
                (
                    "ELEVENLABS",
                    "wss://api.elevenlabs.io/v1/speech-to-text/realtime",
                    Some("ELEVENLABS")
                ),
                ("GROQ", "https://api.groq.com/openai/v1", Some("GROQ")),
            ]
        );
        let used: Vec<&str> = config.models.iter().map(|m| m.provider.as_str()).collect();
        assert_eq!(
            used,
            [
                "192.0.2.10",
                "DEEPGRAM",
                "OPENROUTER",
                "OPENROUTER-2",
                "OPENROUTER",
                "OPENROUTER",
                "ELEVENLABS",
                "DEEPGRAM",
                "GROQ"
            ]
        );
        let deep = &config.models[3];
        assert_eq!(deep.extra["temperature"], 0.2);
        assert_eq!(config.models[2].reasoning.as_deref(), Some("low"));
        assert_eq!(config.models[2].kinds, ["idea"]);
        assert_eq!(config.models[4].translates, ["meeting"]);
        assert_eq!(config.models[0].price_per_minute, Some(0.0));
        let resolved = config.resolved();
        assert_eq!(resolved[0].key(), Key::None);
        assert_eq!(resolved[3].key(), Key::Omapass("openrouter"));
        assert_eq!(resolved[4].key(), Key::Env("OPENROUTER_API_KEY"));
    }

    #[test]
    fn a_converted_key_or_address_a_preset_has_takes_the_presets_name() {
        let value = json!({"models": [
            {"name": "a", "type": "chat", "base_url": "https://openrouter.ai/api/v1",
             "model": "m", "api_key_env": "OPENROUTER_API_KEY"},
            {"name": "b", "type": "chat", "base_url": "http://lan/v1", "model": "m",
             "api_key_env": "LAN_KEY"},
        ]});
        let config = from_old(value).unwrap();
        let secrets: Vec<&str> = config.secrets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(secrets, ["OPENROUTER", "LAN_KEY"]);
        let providers: Vec<&str> = config.providers.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(providers, ["OPENROUTER", "lan"]);
        assert_eq!(config.providers[0].secret.as_deref(), Some("OPENROUTER"));
    }

    #[test]
    fn a_conversion_follows_a_models_named_provider_and_drops_what_it_leaves() {
        assert!(!convert(&mut json!(3)));
        let mut value = json!({
            "secrets": [{"name": "gone", "env": "G"}, {"name": "shared", "env": "S"}],
            "providers": [
                {"name": "old", "base_url": "http://a/v1", "secret": "gone"},
                {"name": "old2", "base_url": "http://b/v1", "secret": "shared"},
                {"name": "other", "base_url": "http://c/v1", "secret": "shared"},
            ],
            "models": [
                3,
                {"name": "keep", "type": "chat", "provider": "other", "model": "m"},
                {"name": "c1", "type": "chat", "provider": "old", "model": "m",
                 "api_key_omapass": "acct"},
                {"name": "c2", "type": "chat", "provider": "old2", "model": "m",
                 "api_key_omapass": "acct"},
                {"name": "c3", "type": "chat", "provider": "old", "model": "m",
                 "api_key_credential": "cred"},
            ],
        });
        assert!(convert(&mut value));
        let names = |list: &str| -> Vec<String> {
            value[list]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|item| item["name"].as_str().map(String::from))
                .collect()
        };
        // A provider no model uses any more goes, and so does a secret no
        // provider uses; one another provider still uses stays.
        assert_eq!(names("secrets"), ["shared", "acct", "cred"]);
        assert_eq!(names("providers"), ["other", "a", "b", "a-2"]);
        assert_eq!(value["models"][4]["provider"], "a-2");
        assert_eq!(value["models"][2]["provider"], "a");
        assert_eq!(value["providers"][1]["secret"], "acct");
        assert!(value["models"][2].get("api_key_omapass").is_none());
    }

    #[test]
    fn a_key_names_its_source_and_is_read_back_from_it() {
        for key in [
            Key::None,
            Key::Env("VARIABLE"),
            Key::Omapass("account"),
            Key::Credential("name"),
        ] {
            let (env, omapass, credential) = key.fields();
            assert_eq!(Key::of(env, omapass, credential), key);
        }
    }

    #[test]
    fn a_key_is_read_from_the_process_environment_when_asked() {
        testing::read_process_env();
        assert!(!env_key("PATH").unwrap().is_empty());
        assert_eq!(
            env_key("ECO_TEST_NO_SUCH_VARIABLE").unwrap_err().0,
            "ECO_TEST_NO_SUCH_VARIABLE is not set"
        );
    }

    #[test]
    fn a_file_is_not_written_under_a_directory_that_cannot_be_made() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("file");
        fs::write(&file, "x").unwrap();
        assert!(write_atomically(&file.join("config.toml"), b"y").is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), "x");
    }

    #[test]
    fn several_key_sources_of_an_old_model_do_not_reject_the_file() {
        let old = OLD.replace(
            "api_key_omapass = \"openrouter\"",
            "api_key_omapass = \"openrouter\"\napi_key_env = \"OPENROUTER_API_KEY\"",
        );
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, old).unwrap();
        let config = read(&path, None).unwrap().0;
        assert_eq!(config.resolved()[3].key(), Key::Omapass("openrouter"));
    }

    #[test]
    fn reading_converts_in_memory_and_the_daemon_saves_with_a_backup() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, OLD).unwrap();
        let memory = load(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), OLD);
        assert!(!directory.path().join("config.toml.bak").exists());
        let (config, failure) = load_converting(&path).unwrap();
        assert!(failure.is_none());
        assert_eq!(config, memory);
        assert_eq!(
            fs::read_to_string(directory.path().join("config.toml.bak")).unwrap(),
            OLD
        );
        let saved = fs::read_to_string(&path).unwrap();
        assert!(
            saved.contains("[[providers]]") && !saved.contains("api_key"),
            "{saved}"
        );
        assert_eq!(load(&path).unwrap(), config);
        let (again, failure) = load_converting(&path).unwrap();
        assert!(failure.is_none());
        assert_eq!(again, config);
        assert_eq!(
            fs::read_to_string(directory.path().join("config.toml.bak")).unwrap(),
            OLD
        );
    }

    const PRESET_MODELS: [&str; 4] = ["gpt-6-luna", "scribe-v2", "nova-3", "whisper-turbo"];

    fn model_names(config: &Config) -> Vec<&str> {
        config.models.iter().map(|m| m.name.as_str()).collect()
    }

    #[test]
    fn a_first_run_has_the_presets_as_ordinary_entries() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let config = read(&path, None).unwrap().0;
        assert_eq!(model_names(&config), PRESET_MODELS);
        assert_eq!(config.providers.len(), 4);
        assert_eq!(config.secrets.len(), 4);
        assert!(!path.exists());
        save(&config, &path).unwrap();
        assert_eq!(read(&path, None).unwrap().0, config);
    }

    #[test]
    fn an_old_file_is_converted_and_given_the_presets_and_the_save_writes_them() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, OLD).unwrap();
        let (config, failure) = load_converting(&path).unwrap();
        assert!(failure.is_none());
        let names = model_names(&config);
        assert!(PRESET_MODELS.iter().all(|preset| names.contains(preset)));
        let saved = fs::read_to_string(&path).unwrap();
        for preset in PRESET_MODELS {
            assert!(saved.contains(preset), "{preset} in {saved}");
        }
        assert_eq!(load(&path).unwrap(), config);
    }

    #[test]
    fn a_file_in_the_new_shape_is_taken_as_it_is_so_a_deleted_preset_stays_deleted() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let mut config = read(&path, None).unwrap().0;
        config.models.retain(|m| m.name != "nova-3");
        save(&config, &path).unwrap();
        let (loaded, failure) = load_converting(&path).unwrap();
        assert!(failure.is_none());
        assert_eq!(
            model_names(&loaded),
            ["gpt-6-luna", "scribe-v2", "whisper-turbo"]
        );
        assert_eq!(loaded, config);
    }

    #[test]
    fn a_conversion_that_cannot_be_saved_is_told_and_kept_in_memory() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, OLD).unwrap();
        // A directory where the temporary file goes makes the save fail.
        fs::create_dir(directory.path().join("config.toml.tmp")).unwrap();
        let (config, failure) = load_converting(&path).unwrap();
        assert!(failure.unwrap().0.contains("config.toml"));
        assert_eq!(config.providers.len(), 6);
        assert_eq!(fs::read_to_string(&path).unwrap(), OLD);
        fs::remove_dir(directory.path().join("config.toml.tmp")).unwrap();
        // Without a backup the old file is left alone.
        fs::remove_file(directory.path().join("config.toml.bak")).unwrap();
        fs::create_dir(directory.path().join("config.toml.bak")).unwrap();
        let (_, failure) = load_converting(&path).unwrap();
        assert!(failure.unwrap().0.contains("config.toml.bak"));
        assert_eq!(fs::read_to_string(&path).unwrap(), OLD);
    }

    #[test]
    fn the_snapshot_carries_the_three_lists_and_no_per_model_address_or_key() {
        let config = Config::from_value(shaped()).unwrap();
        let snapshot = config.to_value();
        assert_eq!(snapshot["secrets"].as_array().unwrap().len(), 2);
        assert_eq!(snapshot["providers"].as_array().unwrap().len(), 3);
        for model in snapshot["models"].as_array().unwrap() {
            for field in OLD_FIELDS {
                assert!(model.get(field).is_none(), "{model}");
            }
        }
        assert_eq!(Config::from_value(snapshot).unwrap(), config);
    }

    #[test]
    fn a_draft_with_a_model_in_the_old_shape_is_refused() {
        let mut value = shaped();
        value["models"][2]["api_key_env"] = json!("OTHER_KEY");
        let refused = Config::from_value(value).unwrap_err();
        assert!(refused.0.contains("api_key_env"), "{}", refused.0);
    }

    #[test]
    fn an_unset_variable_is_an_error() {
        let failure = env_key("ECO_TEST_UNSET_VARIABLE").unwrap_err();
        assert!(failure.0.contains("is not set"));
    }
}
