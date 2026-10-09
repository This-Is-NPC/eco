//! `~/.config/eco/config.toml`: its schema, validation, and atomic save.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::{env, fs};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::domain::action::Action;
use crate::domain::prompts::{DEFAULT_REVIEW, DEFAULT_RULES};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ConfigError(pub String);

/// The XDG base directory `variable` names, or `fallback` under the home.
fn xdg_base(variable: &str, fallback: &str) -> PathBuf {
    env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(fallback))
}

fn xdg(variable: &str, fallback: &str) -> PathBuf {
    xdg_base(variable, fallback).join("eco")
}

pub fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

/// A file eco ships beside its binary, `<prefix>/share/eco/<path>` for
/// `<prefix>/bin/eco`, when it is there.
pub fn shipped(path: &str) -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    Some(exe.parent()?.parent()?.join("share/eco").join(path)).filter(|file| file.exists())
}

/// The Hyprland file that holds the user's key bindings.
pub fn hypr_bindings() -> PathBuf {
    xdg_base("XDG_CONFIG_HOME", ".config").join("hypr/bindings.lua")
}

pub fn config_file() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("config.toml")
}

pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share")
}

pub fn vad_model() -> PathBuf {
    data_dir().join("models").join("silero_vad.onnx")
}

pub fn speaker_model() -> PathBuf {
    data_dir().join("models").join("wespeaker_campplus.onnx")
}

pub fn people_dir() -> PathBuf {
    data_dir().join("people")
}

pub fn sessions_dir() -> PathBuf {
    data_dir().join("sessions")
}

pub fn socket_path() -> PathBuf {
    env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", users_uid())))
        .join("eco.sock")
}

fn users_uid() -> u32 {
    fs::metadata("/proc/self")
        .map(|m| std::os::unix::fs::MetadataExt::uid(&m))
        .unwrap_or(0)
}

/// The key held by the environment variable `name`.
pub fn env_key(name: &str) -> Result<String, ConfigError> {
    env::var(name)
        .ok()
        .filter(|key| !key.is_empty())
        .ok_or_else(|| ConfigError(format!("{name} is not set")))
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
    /// The name of a registered transcription model.
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
}

impl<'a> Key<'a> {
    /// The source `env` and `omapass` name; both set is the caller's to refuse.
    pub fn of(env: Option<&'a str>, omapass: Option<&'a str>) -> Self {
        let given = |name: Option<&'a str>| name.filter(|n| !n.trim().is_empty());
        match (given(env), given(omapass)) {
            (_, Some(account)) => Self::Omapass(account),
            (Some(variable), None) => Self::Env(variable),
            (None, None) => Self::None,
        }
    }
}

impl SttConfig {
    fn default_language() -> String {
        "pt".into()
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

/// A model the user registered once, named so roles, actions and session kinds pick it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ModelType,
    pub base_url: String,
    /// The provider's id for it.
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// The omapass account holding the key, instead of a variable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_omapass: Option<String>,
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

impl ModelConfig {
    pub fn key(&self) -> Key<'_> {
        Key::of(self.api_key_env.as_deref(), self.api_key_omapass.as_deref())
    }

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
    /// The name of a registered model.
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

/// The model that translates each line and answer, below it, in a session that
/// turned translation on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranslationConfig {
    /// The name of a registered chat model; the assistant's when empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
}

/// A second model every answer passes through before the user sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewerConfig {
    #[serde(default)]
    pub enabled: bool,
    /// The name of a registered model; may be empty while the reviewer is off.
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UiTable {
    #[serde(default = "UiTable::auto")]
    language: String,
    #[serde(default)]
    hide_from_share: bool,
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
        }
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
    stt: SttConfig,
    llm: LlmConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reviewer: Option<ReviewerConfig>,
    #[serde(default)]
    translation: TranslationConfig,
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
    /// translation default, or the assistant's.
    pub fn translator(&self, session_kind: &str) -> &ModelConfig {
        let own = self
            .models
            .iter()
            .find(|m| m.translates.iter().any(|k| k == session_kind));
        let default = Some(&self.translation.model).filter(|name| !name.is_empty());
        own.or_else(|| self.model(default.unwrap_or(&self.llm.model)))
            .expect("a valid config names registered chat models")
    }

    /// The model that transcribes sessions of `session_kind`: its own, or the default.
    pub fn transcriber(&self, session_kind: &str) -> &ModelConfig {
        self.model_of_kind(ModelType::Transcription, session_kind)
            .or_else(|| self.model(&self.stt.model))
            .expect("a valid config names a registered transcription model")
    }

    /// Build a Config from its JSON shape (what the overlay sends); validate it.
    pub fn from_value(value: Value) -> Result<Self, ConfigError> {
        let raw: Raw = serde_json::from_value(value)
            .map_err(|error| ConfigError(format!("invalid config: {error}")))?;
        Self::from_raw(raw)
    }

    /// The JSON shape of this Config; `Config::from_value(c.to_value())` gives `c` back.
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self.to_raw()).expect("a config always serializes")
    }

    fn from_raw(raw: Raw) -> Result<Self, ConfigError> {
        let config = Self {
            kinds: raw.kinds,
            rules: raw.rules,
            stt: raw.stt,
            llm: raw.llm,
            reviewer: raw.reviewer,
            translation: raw.translation,
            models: raw.models,
            participants: raw.participants,
            actions: raw.actions,
            context_files: raw.context.files,
            contexts: raw.contexts,
            colors: raw.colors,
            audio: raw.audio,
            ui_language: raw.ui.language,
            hide_from_share: raw.ui.hide_from_share,
        };
        config.validate()?;
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
            },
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
        let both = |env: &Option<String>, omapass: &Option<String>| {
            let set = |value: &Option<String>| value.as_ref().is_some_and(|v| !v.trim().is_empty());
            set(env) && set(omapass)
        };
        if self
            .models
            .iter()
            .any(|m| both(&m.api_key_env, &m.api_key_omapass))
        {
            return fail("a provider's key comes from api_key_env or api_key_omapass, not both");
        }
        let models: Vec<&String> = self.models.iter().map(|m| &m.name).collect();
        if !distinct(&models) || models.iter().any(|n| n.trim() != *n || n.is_empty()) {
            return fail("model names must be unique, trimmed and not empty");
        }
        if self
            .models
            .iter()
            .any(|m| m.base_url.trim().is_empty() || m.model.trim().is_empty())
        {
            return fail("a model needs a base URL and the provider's model id");
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
        if self
            .model(&self.stt.model)
            .is_none_or(|model| model.kind != ModelType::Transcription)
        {
            return fail("transcription must name a registered transcription model");
        }
        if unknown(&self.llm.model) {
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
        if !self.colors.values().all(|color| valid_color(color)) {
            return fail("colors must be #rrggbb");
        }
        let actions: Vec<&String> = self.actions.iter().map(|a| &a.name).collect();
        if !distinct(&actions) {
            return fail("action names must be unique");
        }
        Ok(())
    }
}

/// The files, read and joined, `~` expanded.
fn read_all(files: &[String]) -> std::io::Result<String> {
    let texts = files
        .iter()
        .map(|file| fs::read_to_string(expand(file)))
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

fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(path),
    }
}

pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let text = fs::read_to_string(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => ConfigError(format!(
            "no config at {}; see docs/design.md §9",
            path.display()
        )),
        _ => ConfigError(format!("{}: {error}", path.display())),
    })?;
    let raw: Raw = toml::from_str(&text).map_err(|error| {
        let message = error.to_string();
        if message.contains("missing field") || message.contains("unknown field") {
            ConfigError(format!("invalid config: {}", message.trim()))
        } else {
            ConfigError(format!("{}: {}", path.display(), message.trim()))
        }
    })?;
    Config::from_raw(raw)
}

/// Options that create a file only its owner may read or write (0600).
pub fn private_file() -> fs::OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = fs::OpenOptions::new();
    options.mode(0o600);
    options
}

/// Create `path` and its missing parents, each only its owner may enter (0700).
pub fn private_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

/// Replace `path` with `bytes` through a new private temporary file, so a crash
/// never leaves half a file; its directory is created private when missing.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        private_dir(parent)?;
    }
    let temporary = path.with_extension("tmp");
    // A temporary left by a crash would keep its mode, so it goes first.
    match fs::remove_file(&temporary) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    private_file()
        .write(true)
        .create_new(true)
        .open(&temporary)?
        .write_all(bytes)?;
    fs::rename(&temporary, path)
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
        let config = Config::from_value(value.clone()).unwrap();
        let texts = config.slot_texts();
        assert_eq!(texts[0].0.kinds, ["meeting"]);
        assert_eq!(texts[0].1.as_ref().unwrap(), "Currículo");
        assert!(texts[1].1.is_err());
        assert_eq!(Config::from_value(config.to_value()).unwrap(), config);
        value["contexts"][1]["name"] = json!("entrevista");
        assert!(Config::from_value(value).is_err());
    }

    #[test]
    fn a_reviewer_is_optional_and_names_a_model_when_on() {
        assert_eq!(Config::from_value(raw()).unwrap().reviewer, None);
        let reviewer = json!({"enabled": true, "model": "grok"});
        let config = Config::from_value(with("reviewer", reviewer.clone())).unwrap();
        let on = config.reviewer.as_ref().unwrap();
        assert_eq!((on.prompt.as_str(), on.verbose), (DEFAULT_REVIEW, false));
        assert_eq!(Config::from_value(config.to_value()).unwrap(), config);
        let mut unnamed = reviewer;
        unnamed["model"] = json!("");
        assert!(error_of(with("reviewer", unnamed.clone())).contains("reviewer"));
        unnamed["enabled"] = json!(false);
        assert!(Config::from_value(with("reviewer", unnamed)).is_ok());
    }

    #[test]
    fn roles_and_actions_name_registered_models() {
        let config = Config::from_value(raw()).unwrap();
        assert_eq!(config.model("grok").unwrap().model, "x-ai/grok-4.20");
        assert!(error_of(with("llm", json!({"model": "nenhum"}))).contains("assistant"));
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
        assert!(error_of(with("stt", json!({"model": "gemini"}))).contains("transcription"));
        assert!(error_of(with("llm", json!({"model": "whisper"}))).contains("chat"));
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
        let config = Config::from_value(value.clone()).unwrap();
        assert_eq!(
            config.model("whisper").unwrap().price_per_minute,
            Some(0.0077)
        );
        assert_eq!(Config::from_value(config.to_value()).unwrap(), config);
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
        let config = Config::from_value(value.clone()).unwrap();
        assert_eq!(config.transcriber("idea").name, "local");
        assert_eq!(config.transcriber("meeting").name, "whisper");
        assert_eq!(
            config.model_of_kind(ModelType::Chat, "idea").unwrap().name,
            "grok"
        );
        assert!(config.model_of_kind(ModelType::Chat, "meeting").is_none());
        assert_eq!(Config::from_value(config.to_value()).unwrap(), config);
        let mut twice = value.clone();
        twice["models"][1]["kinds"] = json!(["idea"]);
        assert!(error_of(twice).contains("at most one"));
        let mut unknown = value;
        unknown["models"][1]["kinds"] = json!(["party"]);
        assert!(error_of(unknown).contains("configured session kinds"));
    }

    #[test]
    fn translation_names_a_chat_model_only() {
        let config = Config::from_value(raw()).unwrap();
        assert_eq!(config.translator("meeting").name, "gemini");
        let mut value = with("translation", json!({"model": "grok"}));
        let config = Config::from_value(value.clone()).unwrap();
        assert_eq!(config.translator("meeting").name, "grok");
        assert_eq!(Config::from_value(config.to_value()).unwrap(), config);
        value["models"][1]["translates"] = json!(["idea"]);
        assert_eq!(
            Config::from_value(value.clone())
                .unwrap()
                .translator("idea")
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
        assert_eq!(Config::from_value(raw()).unwrap().llm.concurrency, 8);
        let mut value = raw();
        value["llm"]["concurrency"] = json!(0);
        assert!(error_of(value).contains("concurrency"));
    }

    #[test]
    fn reasoning_is_sent_in_openrouter_form() {
        let mut value = raw();
        value["models"][1]["reasoning"] = json!("off");
        value["models"][2]["reasoning"] = json!("low");
        let config = Config::from_value(value.clone()).unwrap();
        assert_eq!(
            config.models[1].request_fields()["reasoning"],
            json!({"enabled": false})
        );
        assert_eq!(
            config.models[2].request_fields()["reasoning"],
            json!({"effort": "low"})
        );
        assert_eq!(Config::from_value(config.to_value()).unwrap(), config);
        value["models"][2]["reasoning"] = json!("max");
        assert!(error_of(value.clone()).contains("reasoning"));
        value["models"][2]["reasoning"] = json!("low");
        value["models"][0]["reasoning"] = json!("low");
        assert!(error_of(value).contains("only chat"));
    }

    #[test]
    fn rules_start_as_the_defaults_and_keep_what_the_user_writes() {
        let config = Config::from_value(raw()).unwrap();
        assert_eq!(config.rules, DEFAULT_RULES);
        let config = Config::from_value(with("rules", json!("- Fale como eu."))).unwrap();
        assert_eq!(config.rules, "- Fale como eu.");
        assert_eq!(Config::from_value(config.to_value()).unwrap(), config);
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
        Config::from_value(value).expect_err("must be rejected").0
    }

    #[test]
    fn round_trips_through_toml() {
        let config = Config::from_value(raw()).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("eco").join("config.toml");
        save(&config, &path).unwrap();
        assert_eq!(load(&path).unwrap(), config);
        assert_eq!(Config::from_value(config.to_value()).unwrap(), config);
        let written: toml::Table = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            written["participants"][1]["devices"][1].as_str(),
            Some("alsa_output.usb-G522")
        );
        assert!(!path.with_extension("tmp").exists());
    }

    #[test]
    fn participants_keep_their_devices() {
        let config = Config::from_value(raw()).unwrap();
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
    fn missing_file_is_a_config_error() {
        let directory = tempfile::tempdir().unwrap();
        let error = load(&directory.path().join("nope.toml")).unwrap_err().0;
        assert!(error.contains("no config"));
    }

    #[test]
    fn languages_default_when_absent() {
        let config = Config::from_value(with("stt", json!({"model": "whisper"}))).unwrap();
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
    fn ui_language_defaults_to_auto_and_round_trips() {
        assert_eq!(Config::from_value(raw()).unwrap().ui_language, "auto");
        let config = Config::from_value(with("ui", json!({"language": "ja-JP"}))).unwrap();
        assert_eq!(
            Config::from_value(config.to_value()).unwrap().ui_language,
            "ja-JP"
        );
        assert!(error_of(with("ui", json!({"language": "japanese"}))).contains("ui language"));
    }

    #[test]
    fn hide_from_share_defaults_to_off_and_round_trips() {
        assert!(!Config::from_value(raw()).unwrap().hide_from_share);
        let config = Config::from_value(with("ui", json!({"hide_from_share": true}))).unwrap();
        assert!(config.hide_from_share);
        assert_eq!(config.ui_language, "auto");
        assert!(
            Config::from_value(config.to_value())
                .unwrap()
                .hide_from_share
        );
        assert!(error_of(with("ui", json!({"hide_from_share": "yes"}))).contains("invalid config"));
    }

    #[test]
    fn a_key_comes_from_one_place() {
        let mut value = raw();
        let config = Config::from_value(value.clone()).unwrap();
        assert_eq!(config.models[2].key(), Key::Omapass("openrouter"));
        assert_eq!(config.models[0].key(), Key::None);
        value["models"][2]["api_key_env"] = json!("OPENROUTER_API_KEY");
        assert!(Config::from_value(value).is_err());
        assert_eq!(Key::of(Some("X"), Some(" ")), Key::Env("X"));
    }

    #[test]
    fn an_unset_variable_is_an_error() {
        let failure = env_key("ECO_TEST_UNSET_VARIABLE").unwrap_err();
        assert!(failure.0.contains("is not set"));
    }
}
