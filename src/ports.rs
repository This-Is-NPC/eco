//! The ports the domain talks to; every external system is an adapter behind one.

use std::collections::BTreeMap;

use futures::future::BoxFuture;
use futures::stream::BoxStream;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::domain::people::Person;

pub const SAMPLE_RATE: u32 = 16_000;
/// One Silero VAD window at 16 kHz: 32 ms of mono s16le.
pub const FRAME_SAMPLES: usize = 512;
pub const FRAME_BYTES: usize = FRAME_SAMPLES * 2;

/// Exactly `FRAME_SAMPLES` samples of mono s16le audio.
pub type Frame = Vec<i16>;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct AudioError(pub String);

pub trait AudioSource: Send {
    /// Frames until the source ends.
    fn frames(&mut self) -> BoxStream<'_, Result<Frame, AudioError>>;
}

/// Something eco can listen to: a microphone ("input") or what an output plays ("output").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Device {
    pub id: String,
    pub label: String,
    pub kind: String,
}

impl Device {
    pub fn new(id: &str, label: &str, kind: &str) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind: kind.into(),
        }
    }
}

/// Echo cancellation running on one microphone; dropping it stops it.
pub type EchoCancelling = Box<dyn Send>;

/// The platform's audio: the devices eco can listen to, and capture from each.
pub trait AudioDevices: Send + Sync {
    /// Every device, the system defaults first.
    fn list(&self) -> BoxFuture<'static, Vec<Device>>;
    /// Mono s16le at `SAMPLE_RATE` from `device`.
    fn capture(&self, device: &Device) -> Box<dyn AudioSource>;
    /// The microphone `cancel_echo` runs on, with what the default output plays
    /// taken out; it hears only while that cancellation runs.
    fn capture_cancelled(&self) -> Box<dyn AudioSource>;
    /// Start cancelling, on `mic` (a device id), what the default output plays.
    fn cancel_echo(&self, mic: &str) -> BoxFuture<'static, Result<EchoCancelling, AudioError>>;
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct TranscriptionError(pub String);

/// Words said over `[start, end)`, in seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Phrase {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// What a provider heard in one clip.
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    /// What is said, phrase by phrase, timed from the clip's start.
    pub phrases: Vec<Phrase>,
    /// The provider's id for the request it billed the clip under, when it names one.
    pub request: Option<String>,
}

pub trait SpeechToText: Send + Sync {
    /// What is said in `pcm`.
    fn transcribe<'a>(
        &'a self,
        pcm: &'a [i16],
    ) -> BoxFuture<'a, Result<Transcript, TranscriptionError>>;
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct EmbeddingError(pub String);

pub trait SpeakerEmbedder: Send + Sync {
    /// A vector that lies close to other clips of the same voice.
    fn embed(&self, pcm: &[i16]) -> Result<Vec<f32>, EmbeddingError>;
}

/// What a streaming transcriber hears.
#[derive(Debug, Clone, PartialEq)]
pub enum Heard {
    /// The words of the phrase being spoken so far; they may still change.
    Partial(String),
    /// A finished phrase, timed from the start of the stream.
    Phrase(Phrase),
    /// The provider's id for the request the stream is billed under.
    Request(String),
}

/// A transcriber fed a live stream: the provider decides where speech ends,
/// says what it hears as it goes, and sends each finished phrase.
pub trait StreamingSpeechToText: Send + Sync {
    fn transcribe<'a>(
        &'a self,
        frames: BoxStream<'a, Frame>,
    ) -> BoxStream<'a, Result<Heard, TranscriptionError>>;
}

#[derive(Debug, thiserror::Error)]
pub enum BillingError {
    /// The key may not read what requests cost.
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    Failed(String),
}

/// What a transcription provider charged for a request it named.
pub trait TranscriptionBilling: Send + Sync {
    /// The request's cost in USD; `None` while the provider does not list it yet.
    fn cost(&self, request: &str) -> BoxFuture<'static, Result<Option<f64>, BillingError>>;
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct CompletionError(pub String);

/// What a completion cost: prompt tokens, how many came from the provider's cache.
#[derive(Debug, Clone, PartialEq)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub cached_tokens: u64,
    pub cost_usd: Option<f64>,
}

/// One piece of a streamed completion.
#[derive(Debug, Clone, PartialEq)]
pub enum Chunk {
    Text(String),
    /// Reasoning a model shows before its answer; not part of the answer.
    Thinking(String),
    /// What the provider reports once the answer is complete.
    Usage(Usage),
}

/// A chat message. `cache` ends a stretch worth caching: the adapter marks it for
/// providers that need explicit marks and drops it otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: &'static str,
    pub content: String,
    pub cache: bool,
}

pub trait LanguageModel: Send + Sync {
    fn stream(
        &self,
        model: &str,
        messages: Vec<Message>,
    ) -> BoxStream<'static, Result<Chunk, CompletionError>>;
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct StoreError(pub String);

/// The people the user has named, and the voices of each session's speakers, kept
/// apart from the sessions.
pub trait PeopleStore: Send + Sync {
    /// Everyone, sorted by name.
    fn people(&self) -> Vec<Person>;
    fn save(&self, person: &Person) -> Result<(), StoreError>;
    /// Delete a person: their name and voiceprints.
    fn forget(&self, person_id: &str) -> Result<(), StoreError>;
    /// The voices of a session's speakers, by the label their lines carry.
    fn voices(&self, session_id: &str) -> BTreeMap<String, Vec<f32>>;
    fn keep_voices(
        &self,
        session_id: &str,
        voices: &BTreeMap<String, Vec<f32>>,
    ) -> Result<(), StoreError>;
}

/// One change in a session's append-only log.
pub type Record = Map<String, Value>;

/// Where a session's records go, in order.
pub type RecordSink = Box<dyn FnMut(Record) + Send>;

pub struct SessionStorage {
    pub path: String,
    pub bytes: u64,
}

pub trait SessionLog: Send + Sync {
    /// A sink for a new session's records; the first is the session itself.
    fn writer(&self) -> RecordSink;
    /// Every stored session's records, newest session first.
    fn all(&self) -> Vec<Vec<Record>>;
    /// One session's records, or `None` when there is no such session.
    fn read(&self, session_id: &str) -> Option<Vec<Record>>;
    /// A sink that continues an existing session's records.
    fn append_to(&self, session_id: &str) -> Option<RecordSink>;
    fn storage(&self, session_id: &str) -> Option<SessionStorage>;
    fn delete(&self, session_id: &str) -> Result<bool, StoreError>;
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct HookError(pub String);

/// Runs the commands the user ties to actions, telling each which session it was for.
pub trait Hooks: Send + Sync {
    /// Run `command` for the session `session_id`, titled `title`.
    fn run(
        &self,
        command: String,
        session_id: String,
        title: String,
    ) -> BoxFuture<'static, Result<(), HookError>>;
}

/// Runs the daemon as a background service of the user's session.
pub trait ServiceManager: Send + Sync {
    /// Start the service, with the graphical session it has to reach.
    fn start(&self) -> BoxFuture<'_, anyhow::Result<()>>;
}

/// Loads eco's window rules and shortcuts into the desktop's own config.
pub trait DesktopIntegration {
    /// What was done; `Err` is a warning: the desktop config could not be
    /// read or written, so the rules are not loaded.
    fn load_rules(&self) -> Result<String, String>;
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct WindowError(pub String);

/// The desktop's control of the windows of eco's window processes, each
/// process named by its pid.
pub trait WindowControl: Send + Sync {
    /// Give the keyboard to the window of process `pid`, once it shows.
    fn focus(&self, pid: u32) -> BoxFuture<'static, Result<(), WindowError>>;
    /// Leave the windows of the processes `pids` out of screen sharing, or
    /// show them in it again; while they are left out, so is every window
    /// those processes open later.
    fn hide_from_share(
        &self,
        pids: Vec<u32>,
        hidden: bool,
    ) -> BoxFuture<'static, Result<(), WindowError>>;
}
