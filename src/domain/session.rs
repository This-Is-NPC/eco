//! One session: its state and a timeline of speech, the user's notes and kept
//! suggestions.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::domain::people::Person;
use crate::ports::{Record, RecordSink};

pub const RECORDING: &str = "recording";
pub const PAUSED: &str = "paused";
pub const ENDED: &str = "ended";
/// A session being transcribed from a file; it ends when the file does.
pub const IMPORTING: &str = "importing";
/// How a client reads a stored session left open that no daemon holds: a daemon
/// stopped mid-session. Never logged; the log says `paused`.
pub const INTERRUPTED: &str = "interrupted";

/// Where a session's speech came from: heard live, or decoded from a file.
pub const LIVE: &str = "live";
pub const IMPORT: &str = "import";

/// What a `spent` record is for when a transcription request cost it.
const TRANSCRIPTION: &str = "transcription";

/// Seconds since the epoch, as the Python daemon stamped its records.
pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// When an imported recording began: the date the user gave, else the one the
/// file was recorded with, else when the file last changed, else now.
pub fn recorded_at(given: Option<f64>, tagged: Option<f64>, modified: Option<f64>) -> f64 {
    given.or(tagged).or(modified).unwrap_or_else(now)
}

/// `text` lowercased without accents, to search for words however they are written.
pub fn folded(text: &str) -> String {
    text.nfkd()
        .filter(|c| !is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase()
}

/// A tag as the user means it: spaces trimmed and collapsed; `None` when nothing is left.
pub fn tag_name(text: &str) -> Option<String> {
    let name = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!name.is_empty()).then_some(name)
}

/// Whether two tags are one: the same name whatever its case.
pub fn same_tag(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// Every tag and how many sessions carry it, sorted by name. `tags` come oldest
/// session first, so a tag keeps the casing it was first given.
pub fn tag_counts<'a>(tags: impl IntoIterator<Item = &'a [String]>) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, (String, usize)> = BTreeMap::new();
    for tag in tags.into_iter().flatten() {
        counts
            .entry(tag.to_lowercase())
            .or_insert_with(|| (tag.clone(), 0))
            .1 += 1;
    }
    let mut counts: Vec<(String, usize)> = counts.into_values().collect();
    counts.sort_by_cached_key(|(name, _)| folded(name));
    counts
}

/// A random hexadecimal id of `length` characters.
pub fn short_id(length: usize) -> String {
    uuid::Uuid::new_v4().simple().to_string()[..length].to_string()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Speech {
    pub who: String,
    pub text: String,
    pub at: f64,
}

/// An answer to the user: an action or a free question.
///
/// `prompt` is what the user sees they asked (an action name or the question);
/// `request` is the exact text sent for it, kept so later requests repeat it byte
/// for byte and the provider can reuse its cache. `text` grows while it streams
/// and `done` marks it complete. `draft` is the unreviewed answer a reviewer
/// rewrote, kept only when the user asked to inspect it.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub id: String,
    pub action: String,
    pub model: String,
    pub at: f64,
    pub prompt: String,
    pub request: String,
    pub text: String,
    pub draft: String,
    pub done: bool,
}

/// What the user wrote into a session for later requests to know; no one said it.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub id: String,
    pub text: String,
    pub at: f64,
}

/// What a translation is of: a line, by when it was heard (which survives its
/// speaker being renamed), or an answer by id.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Line(f64),
    Answer(String),
}

impl Source {
    fn is(&self, other: &Source) -> bool {
        match (self, other) {
            (Source::Line(a), Source::Line(b)) => (a - b).abs() < 1e-6,
            (Source::Answer(a), Source::Answer(b)) => a == b,
            _ => false,
        }
    }

    /// How a record or an event names it.
    pub fn fields(&self) -> Value {
        match self {
            Source::Line(at) => json!({"at": at}),
            Source::Answer(id) => json!({"id": id}),
        }
    }
}

/// A line or an answer in another language.
#[derive(Debug, Clone, PartialEq)]
pub struct Translation {
    pub of: Source,
    pub language: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Speech(Speech),
    Note(Note),
    Suggestion(Suggestion),
}

impl Entry {
    pub fn at(&self) -> f64 {
        match self {
            Entry::Speech(speech) => speech.at,
            Entry::Note(note) => note.at,
            Entry::Suggestion(suggestion) => suggestion.at,
        }
    }

    /// The id a note or a suggestion is removed by; speech has none.
    pub fn id(&self) -> Option<&str> {
        match self {
            Entry::Speech(_) => None,
            Entry::Note(note) => Some(&note.id),
            Entry::Suggestion(suggestion) => Some(&suggestion.id),
        }
    }

    /// Speech, a note, or an answer that finished streaming: what later requests may carry.
    fn kept(&self) -> bool {
        match self {
            Entry::Speech(_) | Entry::Note(_) => true,
            Entry::Suggestion(suggestion) => suggestion.done,
        }
    }

    fn size(&self) -> usize {
        match self {
            Entry::Speech(speech) => speech.text.chars().count(),
            Entry::Note(note) => note.text.chars().count(),
            Entry::Suggestion(s) => s.text.chars().count() + s.request.chars().count(),
        }
    }
}

/// What transcribes a session while it runs.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum HeardBy {
    /// Not recorded: a log from before eco kept it.
    #[default]
    Unrecorded,
    /// Nothing: its lines came written, from a transcript file.
    Nothing,
    /// A transcription model, by name, fed this many inputs at once, at the
    /// price per minute the user gave it then.
    Model(String, u32, Option<f64>),
}

/// A transcription request a session listened to that no `billed` record
/// closed: its model, the seconds of the finished stretches, when the last of
/// them ended, and when the stretch running now began.
#[derive(Debug, Clone, PartialEq)]
struct Listened {
    model: String,
    seconds: f64,
    until: f64,
    since: Option<f64>,
}

impl Listened {
    fn stop(&mut self, at: f64) {
        if let Some(since) = self.since.take() {
            self.seconds += (at - since).max(0.0);
            self.until = at.max(since);
        }
    }
}

/// A transcription request a session listened to that was never billed.
#[derive(Debug, Clone, PartialEq)]
pub struct Unclosed {
    pub request: String,
    pub model: String,
    /// Seconds the session listened to it.
    pub seconds: f64,
    /// When it last did.
    pub last: f64,
}

/// How long a session has run: its recording and importing runs, pauses left out.
/// A run lasts until it stops, or until its last line when that comes later.
/// Each stretch of a run counts for the transcriber that heard it, once per input.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Runs {
    /// Seconds the finished runs lasted.
    pub counted: f64,
    /// When the current run began, or `None` while the session does not run.
    pub since: Option<f64>,
    until: f64,
    /// What hears the session now.
    transcriber: HeardBy,
    /// When the current stretch of the current run began.
    from: f64,
    /// Seconds of audio each transcription model heard in the closed stretches.
    transcribed: BTreeMap<String, f64>,
    /// What the audio each model heard at a price kept in the log came to, in USD.
    estimated: BTreeMap<String, f64>,
    /// Seconds of audio each model heard with no price kept in the log.
    unpriced: BTreeMap<String, f64>,
    /// Seconds of the closed stretches no transcriber was recorded for.
    unrecorded: f64,
    /// The requests it listened to that no record closed, by the provider's id.
    requests: BTreeMap<String, Listened>,
}

impl Runs {
    /// The session moved to `state` at `at`.
    fn state(&mut self, state: &str, at: f64) {
        let running = state == RECORDING || state == IMPORTING;
        match self.since {
            None if running => {
                self.since = Some(at);
                self.until = at;
                self.from = at;
            }
            Some(since) if !running => {
                let end = self.until.max(at);
                self.counted += end - since;
                self.close(end);
                self.since = None;
                for listened in self.requests.values_mut() {
                    listened.stop(end);
                }
            }
            _ => {}
        }
    }

    /// A line was heard at `at`.
    fn heard(&mut self, at: f64) {
        if self.since.is_some() {
            self.until = self.until.max(at);
        }
    }

    /// `transcriber` hears the session from `at`; one first recorded mid-run
    /// takes the whole run. The requests of another model stop counting.
    fn transcriber(&mut self, transcriber: HeardBy, at: f64) {
        if self.since.is_some() && self.transcriber != HeardBy::Unrecorded {
            self.close(at.max(self.from));
        }
        for listened in self.requests.values_mut() {
            if !matches!(&transcriber, HeardBy::Model(model, ..) if *model == listened.model) {
                listened.stop(at);
            }
        }
        self.transcriber = transcriber;
    }

    /// Count the stretch from `from` to `end` for the transcriber hearing it.
    fn close(&mut self, end: f64) {
        let seconds = end - self.from;
        match &self.transcriber {
            HeardBy::Unrecorded => self.unrecorded += seconds,
            HeardBy::Nothing => {}
            HeardBy::Model(name, inputs, per_minute) => {
                let heard = seconds * f64::from(*inputs);
                *self.transcribed.entry(name.clone()).or_default() += heard;
                match per_minute {
                    Some(price) => {
                        *self.estimated.entry(name.clone()).or_default() += heard / 60.0 * price;
                    }
                    None => *self.unpriced.entry(name.clone()).or_default() += heard,
                }
            }
        }
        self.from = end;
    }

    /// The session listens to the request `request` of `model` from `at`.
    fn listen(&mut self, request: &str, model: &str, at: f64) {
        let listened = self
            .requests
            .entry(request.into())
            .or_insert_with(|| Listened {
                model: model.into(),
                seconds: 0.0,
                until: at,
                since: None,
            });
        listened.since.get_or_insert(at);
    }

    /// The price per minute kept for `model`, when it hears the session now.
    fn price_of(&self, model: &str) -> Option<f64> {
        match &self.transcriber {
            HeardBy::Model(name, _, per_minute) if name == model => *per_minute,
            _ => None,
        }
    }

    /// Seconds run up to the last stop or line.
    pub fn total(&self) -> f64 {
        self.counted + self.since.map_or(0.0, |since| self.until - since)
    }

    /// The runs with the current stretch closed at the last line: what each
    /// transcription model heard, and the seconds run with no transcriber
    /// recorded, up to the last stop or line.
    fn closed(&self) -> Self {
        let mut closed = self.clone();
        if closed.since.is_some() {
            closed.close(closed.until.max(closed.from));
        }
        closed
    }
}

/// A completion the session paid for — an answer, its review or a translation —
/// as its provider reported; `answer` is the answer's id, for an answer or review.
#[derive(Debug, Clone, PartialEq)]
struct Spent {
    what: String,
    model: String,
    usd: Option<f64>,
    at: Option<f64>,
    answer: Option<String>,
}

/// Why a part of a session's cost is not known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unknown {
    /// The provider reported no cost.
    Unreported,
    /// The key may not read what its requests cost (Deepgram's `usage:read` scope).
    NoScope,
    /// The provider does not tell the request's cost yet.
    Pending,
    /// The model's provider tells no costs and the user gave it no price.
    NoPrice,
    /// The log does not say what transcribed it.
    Unrecorded,
    /// Answers from before eco kept costs.
    Untracked,
}

impl Unknown {
    pub fn code(self) -> &'static str {
        match self {
            Self::Unreported => "unreported",
            Self::NoScope => "no_scope",
            Self::Pending => "pending",
            Self::NoPrice => "no_price",
            Self::Unrecorded => "unrecorded",
            Self::Untracked => "untracked",
        }
    }

    fn of(code: &str) -> Self {
        [
            Self::NoScope,
            Self::Pending,
            Self::NoPrice,
            Self::Unrecorded,
            Self::Untracked,
        ]
        .into_iter()
        .find(|unknown| unknown.code() == code)
        .unwrap_or(Self::Unreported)
    }
}

/// What eco knows of a transcription model's price: what the user says a minute
/// costs, and whether its provider tells what each request cost.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rate {
    pub per_minute: Option<f64>,
    pub billed: bool,
}

/// A transcription request that heard the session: its model, how long the
/// session listened while it was open, its share of the cost, when it closed,
/// the price per minute its model had then, and its part of the cost once the
/// provider told it or why it could not.
#[derive(Debug, Clone, PartialEq)]
struct Billed {
    model: String,
    seconds: f64,
    share: f64,
    at: Option<f64>,
    per_minute: Option<f64>,
    usd: Option<Result<f64, Unknown>>,
}

/// One thing a session spent on, as the cost screen lists it: a completion, a
/// transcription request or the time a model heard; `usd` is `None` when
/// `unknown` says why, and `estimate` when the user's price gave it.
#[derive(Debug, Clone, PartialEq)]
struct Charge {
    what: String,
    at: Option<f64>,
    model: Option<String>,
    usd: Option<f64>,
    unknown: Option<Unknown>,
    estimate: bool,
    seconds: Option<f64>,
    /// What the answer or review was asked with, while it is in the timeline.
    prompt: Option<String>,
    /// Answers it stands for, when it is the ones that kept no cost.
    count: Option<usize>,
}

impl Charge {
    fn of(what: &str, at: Option<f64>, model: Option<&str>, usd: Result<f64, Unknown>) -> Self {
        Self {
            what: what.into(),
            at,
            model: model.map(String::from),
            usd: usd.ok(),
            unknown: usd.err(),
            estimate: false,
            seconds: None,
            prompt: None,
            count: None,
        }
    }

    fn json(&self) -> Value {
        json!({
            "for": self.what, "at": self.at, "model": self.model, "usd": self.usd,
            "unknown": self.unknown.map(Unknown::code), "estimate": self.estimate,
            "seconds": self.seconds, "prompt": self.prompt, "count": self.count,
        })
    }
}

/// The USD `charges` add up to, and whether one is left out as unknown.
fn sum<'a>(charges: impl Iterator<Item = &'a Charge>) -> (f64, bool) {
    charges.fold((0.0, false), |(usd, unknown), charge| match charge.usd {
        Some(spent) => (usd + spent, unknown),
        None => (usd, true),
    })
}

#[derive(Debug, thiserror::Error)]
#[error("unreadable session log: {0}")]
pub struct LogError(String);

fn field<'a>(record: &'a Record, key: &str) -> Result<&'a Value, LogError> {
    record
        .get(key)
        .ok_or_else(|| LogError(format!("missing {key:?}")))
}

fn text(record: &Record, key: &str) -> Result<String, LogError> {
    field(record, key)?
        .as_str()
        .map(String::from)
        .ok_or_else(|| LogError(format!("{key:?} is not text")))
}

fn texts(record: &Record, key: &str) -> Result<Vec<String>, LogError> {
    let not_texts = || LogError(format!("{key:?} is not a list of text"));
    let list = field(record, key)?.as_array().ok_or_else(not_texts)?;
    list.iter()
        .map(|item| item.as_str().map(String::from))
        .collect::<Option<_>>()
        .ok_or_else(not_texts)
}

fn number(record: &Record, key: &str) -> Result<f64, LogError> {
    field(record, key)?
        .as_f64()
        .ok_or_else(|| LogError(format!("{key:?} is not a number")))
}

fn as_record(value: Value) -> Record {
    match value {
        Value::Object(map) => map,
        _ => unreachable!("records are built as objects"),
    }
}

/// One session. Every change that must survive goes to `record`, one object per
/// change, so the session can be stored as an append-only log.
pub struct Session {
    pub id: String,
    pub title: String,
    /// The user's label for what this is: meeting, conversation, idea…
    pub kind: String,
    pub source: String,
    pub language: String,
    pub started_at: f64,
    /// Seconds its course — states, transcribers, speaker turns — runs ahead of
    /// the wall clock: an imported recording runs from when it was recorded.
    ahead: f64,
    pub state: String,
    runs: Runs,
    spent: Vec<Spent>,
    /// The transcription requests that heard it, by the provider's id.
    billed: BTreeMap<String, Billed>,
    pub timeline: Vec<Entry>,
    /// Entries before this moment are left out of requests; it only moves forward,
    /// in large steps, so the start of every request stays the same for long.
    context_since: f64,
    /// The names speakers were given in this session, by the label their lines carry.
    names: BTreeMap<String, String>,
    colors: BTreeMap<String, String>,
    /// The people speakers are, by label: ids in the people registry.
    people: BTreeMap<String, String>,
    line_labels: BTreeSet<String>,
    attendees: BTreeSet<String>,
    /// The voice guesses the user cleared, as (speaker label, person id).
    dismissed: BTreeSet<(String, String)>,
    /// The context slots the user turned on, or `None` until they choose: then
    /// the slots of the session's kind are on.
    contexts: Option<Vec<String>>,
    /// The language the user chose to translate into, "" until they turn it on
    /// or after they turn it off.
    translation: String,
    translations: Vec<Translation>,
    /// The user's tags, in the order given, one per name whatever its case.
    tags: Vec<String>,
    record: RecordSink,
}

impl Session {
    /// A new session — recording when live, importing from a file — whose first
    /// records say what it is.
    pub fn begin(
        title: &str,
        kind: &str,
        source: &str,
        language: &str,
        record: RecordSink,
    ) -> Self {
        Self::begin_at(title, kind, source, language, now(), record)
    }

    /// A new session that began at `started_at`, seconds since the epoch.
    pub fn begin_at(
        title: &str,
        kind: &str,
        source: &str,
        language: &str,
        started_at: f64,
        mut record: RecordSink,
    ) -> Self {
        let id = short_id(12);
        let ahead = started_at - now();
        let state = if source == IMPORT {
            IMPORTING
        } else {
            RECORDING
        };
        let mut runs = Runs::default();
        runs.state(state, started_at);
        record(as_record(json!({
            "type": "session", "id": id, "title": title, "kind": kind, "source": source,
            "language": language, "started_at": started_at,
        })));
        record(as_record(
            json!({"type": "state", "state": state, "at": started_at}),
        ));
        Self {
            id,
            title: title.into(),
            kind: kind.into(),
            source: source.into(),
            language: language.into(),
            started_at,
            ahead,
            state: state.into(),
            runs,
            spent: Vec::new(),
            billed: BTreeMap::new(),
            timeline: Vec::new(),
            context_since: 0.0,
            names: BTreeMap::new(),
            colors: BTreeMap::new(),
            people: BTreeMap::new(),
            line_labels: BTreeSet::new(),
            attendees: BTreeSet::new(),
            dismissed: BTreeSet::new(),
            contexts: None,
            translation: String::new(),
            translations: Vec::new(),
            tags: Vec::new(),
            record,
        }
    }

    /// Rebuild a session from its records; further changes go to `record`.
    pub fn restore(records: &[Record], record: RecordSink) -> Result<Self, LogError> {
        let head = records
            .first()
            .ok_or_else(|| LogError("empty log".into()))?;
        if text(head, "type")? != "session" {
            return Err(LogError("invalid session log".into()));
        }
        let mut session = Self {
            id: text(head, "id")?,
            title: text(head, "title")?,
            kind: text(head, "kind")?,
            source: text(head, "source")?,
            language: text(head, "language")?,
            started_at: number(head, "started_at")?,
            ahead: 0.0,
            state: RECORDING.into(),
            runs: Runs::default(),
            spent: Vec::new(),
            billed: BTreeMap::new(),
            timeline: Vec::new(),
            context_since: 0.0,
            names: BTreeMap::new(),
            colors: BTreeMap::new(),
            people: BTreeMap::new(),
            line_labels: BTreeSet::new(),
            attendees: BTreeSet::new(),
            dismissed: BTreeSet::new(),
            contexts: None,
            translation: String::new(),
            translations: Vec::new(),
            tags: Vec::new(),
            record,
        };
        for item in &records[1..] {
            match text(item, "type")?.as_str() {
                "state" => {
                    session.state = text(item, "state")?;
                    if let Some(at) = item.get("at").and_then(Value::as_f64) {
                        session.runs.state(&session.state, at);
                    }
                }
                "transcriber" => {
                    let transcriber = match field(item, "model")?.as_str() {
                        Some(model) => {
                            let inputs = number(item, "inputs")? as u32;
                            let per_minute = item.get("per_minute").and_then(Value::as_f64);
                            HeardBy::Model(model.into(), inputs, per_minute)
                        }
                        None => HeardBy::Nothing,
                    };
                    session.runs.transcriber(transcriber, number(item, "at")?);
                }
                "listening" => {
                    let (request, model) = (text(item, "request")?, text(item, "model")?);
                    session.runs.listen(&request, &model, number(item, "at")?);
                }
                "billed" => {
                    let request = text(item, "request")?;
                    let billed = Billed {
                        model: text(item, "model")?,
                        seconds: number(item, "seconds")?,
                        share: number(item, "share")?,
                        at: item.get("at").and_then(Value::as_f64),
                        per_minute: item.get("per_minute").and_then(Value::as_f64),
                        usd: None,
                    };
                    session.runs.requests.remove(&request);
                    session.billed.insert(request, billed);
                }
                "spent" => {
                    let (what, usd) = (text(item, "for")?, field(item, "usd")?.as_f64());
                    if what == TRANSCRIPTION {
                        if let Some(billed) = session.billed.get_mut(&text(item, "request")?) {
                            let unknown = item.get("unknown").and_then(Value::as_str);
                            billed.usd = Some(usd.ok_or(Unknown::of(unknown.unwrap_or(""))));
                        }
                    } else {
                        session.spent.push(Spent {
                            what,
                            model: text(item, "model")?,
                            usd,
                            at: item.get("at").and_then(Value::as_f64),
                            answer: item.get("answer").and_then(Value::as_str).map(String::from),
                        });
                    }
                }
                "person" => {
                    let label = text(item, "label")?;
                    match field(item, "person")?.as_str() {
                        Some(person) => session.people.insert(label, person.into()),
                        None => session.people.remove(&label),
                    };
                }
                "attendee" => {
                    let person = text(item, "person")?;
                    if field(item, "present")?.as_bool() == Some(true) {
                        session.attendees.insert(person);
                    } else {
                        session.attendees.remove(&person);
                    }
                }
                "speaker" => {
                    let (label, name) = (text(item, "label")?, text(item, "name")?);
                    session.name_speaker(label, name);
                }
                "context" => session.contexts = Some(texts(item, "slots")?),
                "translation" => session.translation = text(item, "language")?,
                "tags" => session.tags = texts(item, "tags")?,
                "language" => session.language = text(item, "language")?,
                "translated" => {
                    let of = match item.get("id").and_then(Value::as_str) {
                        Some(id) => Source::Answer(id.into()),
                        None => Source::Line(number(item, "at")?),
                    };
                    let (language, translated) = (text(item, "language")?, text(item, "text")?);
                    session.keep_translation(of, language, translated);
                }
                "guess_dismissed" => {
                    let (label, person) = (text(item, "label")?, text(item, "person")?);
                    session.dismissed.insert((label, person));
                }
                "speaker_color" => {
                    let (label, color) = (text(item, "label")?, text(item, "color")?);
                    if color.is_empty() {
                        session.colors.remove(&label);
                    } else {
                        session.colors.insert(label, color);
                    }
                }
                "meta" => {
                    session.title = text(item, "title")?;
                    session.kind = text(item, "kind")?;
                }
                "speech" => {
                    let at = number(item, "at")?;
                    session.runs.heard(at);
                    session.timeline.push(Entry::Speech(Speech {
                        who: text(item, "who")?,
                        text: text(item, "text")?,
                        at,
                    }));
                }
                "note" => session.timeline.push(Entry::Note(Note {
                    id: text(item, "id")?,
                    text: text(item, "text")?,
                    at: number(item, "at")?,
                })),
                "suggestion" => session.timeline.push(Entry::Suggestion(Suggestion {
                    id: text(item, "id")?,
                    action: text(item, "action")?,
                    model: text(item, "model")?,
                    at: number(item, "at")?,
                    prompt: text(item, "prompt")?,
                    request: text(item, "request")?,
                    text: text(item, "text")?,
                    draft: item
                        .get("draft")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .into(),
                    done: true,
                })),
                "diarized" => {
                    let not_labels = || LogError("\"who\" is not a list of text".into());
                    let who = field(item, "who")?.as_array().ok_or_else(not_labels)?;
                    let labels: Option<Vec<&str>> = who.iter().map(Value::as_str).collect();
                    session.relabel(&labels.ok_or_else(not_labels)?);
                }
                "edited" => {
                    let (who, at) = (text(item, "who")?, number(item, "at")?);
                    session.rewrite(&who, at, &text(item, "text")?);
                }
                "line_person" => {
                    let (who, at) = (text(item, "who")?, number(item, "at")?);
                    let label = text(item, "label")?;
                    if session.relabel_line(&who, at, &label).is_some() {
                        session.line_labels.insert(label.clone());
                        session.people.insert(label.clone(), text(item, "person")?);
                        session.name_speaker(label, text(item, "name")?);
                    }
                }
                "unheard" => {
                    let (who, at) = (text(item, "who")?, number(item, "at")?);
                    session.take_back(&who, at);
                }
                "removed" => {
                    let id = text(item, "id")?;
                    session
                        .timeline
                        .retain(|entry| entry.id() != Some(id.as_str()));
                }
                _ => {}
            }
        }
        Ok(session)
    }

    /// How long it has run.
    pub fn runs(&self) -> &Runs {
        &self.runs
    }

    /// Now on the session's own clock.
    fn now(&self) -> f64 {
        now() + self.ahead
    }

    pub fn set_state(&mut self, state: &str) {
        if state != self.state {
            let at = self.now();
            self.state = state.into();
            self.runs.state(state, at);
            (self.record)(as_record(
                json!({"type": "state", "state": state, "at": at}),
            ));
        }
    }

    /// Who transcribes the session from now on; only a change is recorded, and
    /// never `Unrecorded`.
    pub fn transcribe_with(&mut self, transcriber: HeardBy) {
        if transcriber == self.runs.transcriber {
            return;
        }
        let at = self.now();
        let record = match &transcriber {
            HeardBy::Model(model, inputs, per_minute) => {
                let mut record =
                    json!({"type": "transcriber", "model": model, "inputs": inputs, "at": at});
                if let Some(price) = per_minute {
                    record["per_minute"] = json!(price);
                }
                record
            }
            HeardBy::Nothing => json!({"type": "transcriber", "model": null, "at": at}),
            HeardBy::Unrecorded => return,
        };
        self.runs.transcriber(transcriber, at);
        (self.record)(as_record(record));
    }

    /// Keep what a completion of `model` cost — an answer, its review or a
    /// translation — as its provider reported; `None` when it reported none.
    /// `answer` is the answer's id, for an answer or its review.
    pub fn spend(&mut self, what: &str, model: &str, usd: Option<f64>, answer: Option<&str>) {
        let at = now();
        self.spent.push(Spent {
            what: what.into(),
            model: model.into(),
            usd,
            at: Some(at),
            answer: answer.map(String::from),
        });
        let mut record =
            json!({"type": "spent", "for": what, "model": model, "usd": usd, "at": at});
        if let Some(answer) = answer {
            record["answer"] = json!(answer);
        }
        (self.record)(as_record(record));
    }

    /// The session listens to the transcription request `request` of `model`
    /// from `at`: kept as it opens, so a request the daemon never closes is
    /// still billed by how long the session listened.
    pub fn listen(&mut self, request: &str, model: &str, at: f64) {
        self.runs.listen(request, model, at);
        (self.record)(as_record(json!({
            "type": "listening", "request": request, "model": model, "at": at
        })));
    }

    /// The transcription request `request` of `model`, closed at `at`, heard
    /// the session for `seconds`; `share` of its cost is the session's. The
    /// price per minute the model has now is kept with it.
    pub fn bill(&mut self, request: &str, model: &str, (seconds, share): (f64, f64), at: f64) {
        let per_minute = self.runs.price_of(model);
        let billed = Billed {
            model: model.into(),
            seconds,
            share,
            at: Some(at),
            per_minute,
            usd: None,
        };
        self.runs.requests.remove(request);
        self.billed.insert(request.into(), billed);
        let mut record = json!({
            "type": "billed", "request": request, "model": model, "seconds": seconds,
            "share": share, "at": at
        });
        if let Some(price) = per_minute {
            record["per_minute"] = json!(price);
        }
        (self.record)(as_record(record));
    }

    /// Keep what the transcription request `request` cost, as its provider
    /// reported, the session's share of it; or why the provider could not tell.
    pub fn pay(&mut self, request: &str, usd: Result<f64, Unknown>) {
        let Some(billed) = self.billed.get_mut(request) else {
            return;
        };
        let usd = usd.map(|usd| usd * billed.share);
        billed.usd = Some(usd);
        let mut record = json!({
            "type": "spent", "for": TRANSCRIPTION, "model": billed.model, "request": request,
            "usd": usd.ok(), "at": now()
        });
        if let Err(unknown) = usd {
            record["unknown"] = json!(unknown.code());
        }
        (self.record)(as_record(record));
    }

    /// The transcription requests that heard it whose cost is not kept yet, as
    /// (request, model, when it closed).
    pub fn unpaid(&self) -> impl Iterator<Item = (&str, &str, Option<f64>)> {
        self.billed
            .iter()
            .filter(|(_, billed)| billed.usd.is_none())
            .map(|(request, billed)| (request.as_str(), billed.model.as_str(), billed.at))
    }

    /// The transcription requests it was billed for, by the provider's id.
    pub fn billed(&self) -> impl Iterator<Item = &str> {
        self.billed.keys().map(String::as_str)
    }

    /// The requests it listened to that were never billed — the daemon stopped
    /// while they were open: a stretch still running counts up to its last line.
    pub fn unclosed(&self) -> Vec<Unclosed> {
        let mut runs = self.runs.clone();
        let end = runs.until;
        runs.requests
            .iter_mut()
            .map(|(request, listened)| {
                if let Some(since) = listened.since {
                    listened.stop(end.max(since));
                }
                Unclosed {
                    request: request.clone(),
                    model: listened.model.clone(),
                    seconds: listened.seconds,
                    last: listened.until,
                }
            })
            .collect()
    }

    /// The answers in the timeline that finished.
    fn answers(&self) -> usize {
        self.timeline
            .iter()
            .filter(|entry| matches!(entry, Entry::Suggestion(s) if s.done))
            .count()
    }

    /// Everything the session spent on, newest first, those of no one moment
    /// first: each completion as its provider reported; the answers that kept
    /// no cost (a log from before costs were); each transcription request as
    /// its provider reported or, not reported, at the model's price; and the
    /// audio a model heard with no request billed, at its price. A model whose
    /// requests were billed is costed by them alone. `rate` tells each
    /// transcription model's price and whether its provider tells costs. With
    /// them, the seconds of audio each transcription model heard.
    fn charges(&self, rate: impl Fn(&str) -> Rate) -> (Vec<Charge>, BTreeMap<String, f64>) {
        let prompt = |answer: &Option<String>| {
            self.timeline.iter().find_map(|entry| match entry {
                Entry::Suggestion(s) if Some(&s.id) == answer.as_ref() => Some(s.prompt.clone()),
                _ => None,
            })
        };
        let mut charges: Vec<Charge> = self
            .spent
            .iter()
            .map(|spent| Charge {
                prompt: prompt(&spent.answer),
                ..Charge::of(
                    &spent.what,
                    spent.at,
                    Some(&spent.model),
                    spent.usd.ok_or(Unknown::Unreported),
                )
            })
            .collect();
        let kept = self.spent.iter().filter(|s| s.what == "answer").count();
        let untracked = self.answers().saturating_sub(kept);
        if untracked > 0 {
            charges.push(Charge {
                count: Some(untracked),
                ..Charge::of("answer", None, None, Err(Unknown::Untracked))
            });
        }
        // What audio `model` heard comes to as an estimate: `kept` USD of it at
        // the prices the log kept, and `unpriced` seconds at the model's price
        // now, when it has one.
        let estimated = |model: &str, (kept, unpriced): (f64, f64)| {
            let rest = if unpriced > 0.0 {
                rate(model).per_minute.map(|price| unpriced / 60.0 * price)
            } else {
                Some(0.0)
            };
            rest.map(|rest| kept + rest)
        };
        let at_price = |model: &str, seconds: f64, at, (usd, why): (Option<f64>, Unknown)| Charge {
            estimate: usd.is_some(),
            seconds: Some(seconds),
            ..Charge::of(TRANSCRIPTION, at, Some(model), usd.ok_or(why))
        };
        for billed in self.billed.values() {
            let (model, seconds) = (billed.model.as_str(), billed.seconds);
            let kept = match billed.per_minute {
                Some(price) => (seconds / 60.0 * price, 0.0),
                None => (0.0, seconds),
            };
            let estimate = |why| (estimated(model, kept), why);
            charges.push(match billed.usd {
                Some(Ok(usd)) => Charge {
                    seconds: Some(seconds),
                    ..Charge::of(TRANSCRIPTION, billed.at, Some(model), Ok(usd))
                },
                Some(Err(unknown)) => at_price(model, seconds, billed.at, estimate(unknown)),
                None => at_price(model, seconds, billed.at, estimate(Unknown::Pending)),
            });
        }
        let runs = self.runs.closed();
        for (model, &seconds) in &runs.transcribed {
            if seconds > 0.0 && !self.billed.values().any(|billed| billed.model == *model) {
                let why = if rate(model).billed {
                    Unknown::Pending
                } else {
                    Unknown::NoPrice
                };
                let kept = (
                    runs.estimated.get(model).copied().unwrap_or_default(),
                    runs.unpriced.get(model).copied().unwrap_or_default(),
                );
                let usd = estimated(model, kept);
                charges.push(at_price(model, seconds, None, (usd, why)));
            }
        }
        if runs.unrecorded > 0.0 {
            charges.push(Charge {
                seconds: Some(runs.unrecorded),
                ..Charge::of(TRANSCRIPTION, None, None, Err(Unknown::Unrecorded))
            });
        }
        // Of two at one moment, the one kept later comes first.
        charges.reverse();
        charges.sort_by(|a, b| match (a.at, b.at) {
            (Some(a), Some(b)) => b.total_cmp(&a),
            (a, b) => a.is_some().cmp(&b.is_some()),
        });
        (charges, runs.transcribed)
    }

    /// What the session cost in USD, from its `charges`: the completions'
    /// part, the transcription's, and whether each leaves an unknown charge
    /// out; with the seconds of audio each transcription model heard.
    fn totals((charges, transcribed): &(Vec<Charge>, BTreeMap<String, f64>)) -> Value {
        let llm = sum(charges.iter().filter(|c| c.what != TRANSCRIPTION));
        let transcription = sum(charges.iter().filter(|c| c.what == TRANSCRIPTION));
        json!({
            "llm_usd": llm.0,
            "llm_unknown": llm.1,
            "transcription_usd": transcription.0,
            "transcription_unknown": transcription.1,
            "total_usd": llm.0 + transcription.0,
            "transcribed_s": transcribed,
        })
    }

    /// What the session cost in USD, as a session list gives it.
    pub fn cost(&self, rate: impl Fn(&str) -> Rate) -> Value {
        Self::totals(&self.charges(rate))
    }

    /// What the session cost with what each kind of charge adds up to — of
    /// those it has, answers, reviews, translations, transcription — and every
    /// charge, as the cost screen lists them.
    pub fn spending(&self, rate: impl Fn(&str) -> Rate) -> Value {
        let charges = self.charges(rate);
        let mut cost = Self::totals(&charges);
        let parts: Vec<Value> = ["answer", "review", "translation", TRANSCRIPTION]
            .into_iter()
            .filter(|what| charges.0.iter().any(|c| c.what == *what))
            .map(|what| {
                let (usd, unknown) = sum(charges.0.iter().filter(|c| c.what == what));
                json!({"for": what, "usd": usd, "unknown": unknown})
            })
            .collect();
        cost["parts"] = json!(parts);
        cost["items"] = charges.0.iter().map(Charge::json).collect();
        cost
    }

    /// Give the session a new title and kind; only a change is recorded.
    pub fn rename(&mut self, title: &str, kind: &str) {
        if (title, kind) != (self.title.as_str(), self.kind.as_str()) {
            self.title = title.into();
            self.kind = kind.into();
            (self.record)(as_record(
                json!({"type": "meta", "title": title, "kind": kind, "at": now()}),
            ));
        }
    }

    /// Transcribe in `language` from now on; only a change is recorded.
    pub fn set_language(&mut self, language: &str) {
        if self.language != language {
            self.language = language.into();
            (self.record)(as_record(
                json!({"type": "language", "language": language, "at": now()}),
            ));
        }
    }

    /// Speech heard at `at`: an imported file's speech is placed at its time in
    /// the recording, counted from when the session began.
    pub fn hear_at(&mut self, who: &str, text: &str, at: f64) -> Speech {
        let speech = Speech {
            who: who.into(),
            text: text.into(),
            at,
        };
        self.timeline.push(Entry::Speech(speech.clone()));
        self.runs.heard(at);
        (self.record)(as_record(
            json!({"type": "speech", "who": who, "text": text, "at": speech.at}),
        ));
        speech
    }

    /// Open a suggestion that will stream in; it is recorded only once finished.
    pub fn suggest(
        &mut self,
        action: &str,
        model: &str,
        prompt: &str,
        request: &str,
    ) -> Suggestion {
        let suggestion = Suggestion {
            id: short_id(8),
            action: action.into(),
            model: model.into(),
            at: now(),
            prompt: prompt.into(),
            request: request.into(),
            text: String::new(),
            draft: String::new(),
            done: false,
        };
        self.timeline.push(Entry::Suggestion(suggestion.clone()));
        suggestion
    }

    fn suggestion_mut(&mut self, id: &str) -> Option<&mut Suggestion> {
        self.timeline.iter_mut().find_map(|entry| match entry {
            Entry::Suggestion(suggestion) if suggestion.id == id => Some(suggestion),
            _ => None,
        })
    }

    /// Add streamed text to a suggestion still in the timeline.
    pub fn extend(&mut self, id: &str, text: &str) {
        if let Some(suggestion) = self.suggestion_mut(id) {
            suggestion.text.push_str(text);
        }
    }

    /// Grow the unreviewed answer kept for inspection.
    pub fn extend_draft(&mut self, id: &str, text: &str) {
        if let Some(suggestion) = self.suggestion_mut(id) {
            suggestion.draft.push_str(text);
        }
    }

    /// Mark a streamed suggestion complete and record it; removed ones are left out.
    pub fn finish(&mut self, id: &str) {
        let Some(suggestion) = self.suggestion_mut(id) else {
            return;
        };
        suggestion.done = true;
        let mut item = json!({
            "type": "suggestion", "id": suggestion.id, "action": suggestion.action, "model": suggestion.model,
            "prompt": suggestion.prompt, "request": suggestion.request, "text": suggestion.text, "at": suggestion.at,
        });
        if !suggestion.draft.is_empty() {
            item["draft"] = json!(suggestion.draft);
        }
        (self.record)(as_record(item));
    }

    /// Keep a note the user wrote, for later requests to carry.
    pub fn note(&mut self, text: &str) -> Note {
        let note = Note {
            id: short_id(8),
            text: text.into(),
            at: now(),
        };
        (self.record)(as_record(
            json!({"type": "note", "id": note.id, "text": note.text, "at": note.at}),
        ));
        self.timeline.push(Entry::Note(note.clone()));
        note
    }

    /// Drop a note or a suggestion; it leaves the context of later requests.
    pub fn remove(&mut self, id: &str) -> Option<Entry> {
        let index = self
            .timeline
            .iter()
            .position(|entry| entry.id() == Some(id))?;
        let removed = self.timeline.remove(index);
        self.translations
            .retain(|t| !t.of.is(&Source::Answer(id.into())));
        if removed.kept() {
            (self.record)(as_record(json!({"type": "removed", "id": id})));
        }
        Some(removed)
    }

    fn take_back(&mut self, who: &str, at: f64) -> bool {
        let line = self.timeline.iter().position(
            |entry| matches!(entry, Entry::Speech(s) if s.who == who && (s.at - at).abs() < 1e-6),
        );
        let Some(index) = line else {
            return false;
        };
        self.timeline.remove(index);
        self.translations.retain(|t| !t.of.is(&Source::Line(at)));
        if !self
            .timeline
            .iter()
            .any(|entry| matches!(entry, Entry::Speech(speech) if speech.who == who))
        {
            self.people.remove(who);
            self.names.remove(who);
            self.line_labels.remove(who);
        }
        true
    }

    fn rewrite(&mut self, who: &str, at: f64, text: &str) -> bool {
        let line = self.timeline.iter_mut().find_map(|entry| match entry {
            Entry::Speech(s) if s.who == who && (s.at - at).abs() < 1e-6 => Some(s),
            _ => None,
        });
        let edited = line.map(|speech| speech.text = text.into()).is_some();
        // A corrected line's translation no longer says the same.
        if edited {
            self.translations.retain(|t| !t.of.is(&Source::Line(at)));
        }
        edited
    }

    /// Correct the line `who` said at `at`; `false` when there is no such line.
    pub fn edit_line(&mut self, who: &str, at: f64, text: &str) -> bool {
        let edited = self.rewrite(who, at, text);
        if edited {
            let record = json!({"type": "edited", "who": who, "at": at, "text": text});
            (self.record)(as_record(record));
        }
        edited
    }

    /// Take back the line `who` was heard saying at `at` — it was not theirs,
    /// or the user removed it; `false` when there is no such line.
    pub fn unhear(&mut self, who: &str, at: f64) -> bool {
        let taken = self.take_back(who, at);
        if taken {
            (self.record)(as_record(json!({"type": "unheard", "who": who, "at": at})));
        }
        taken
    }

    /// Give the lines heard so far, in order, the speakers `who` names.
    fn relabel(&mut self, who: &[&str]) {
        let lines = self.timeline.iter_mut().filter_map(|entry| match entry {
            Entry::Speech(speech) => Some(speech),
            Entry::Note(_) | Entry::Suggestion(_) => None,
        });
        for (speech, label) in lines.zip(who) {
            speech.who = (*label).into();
        }
    }

    /// Say who spoke each line heard so far, in order, as diarization told them apart.
    pub fn assign_speakers(&mut self, who: &[String]) {
        let labels: Vec<&str> = who.iter().map(String::as_str).collect();
        self.relabel(&labels);
        let at = self.now();
        (self.record)(as_record(json!({"type": "diarized", "who": who, "at": at})));
    }

    /// Give the lines `who` was heard saying at the given times the labels paired
    /// with them, as diarization told their voices apart; other lines keep theirs.
    pub fn split_speaker(&mut self, who: &str, labels: &[(f64, String)]) -> Vec<String> {
        let relabelled: Vec<String> = self
            .timeline
            .iter()
            .filter_map(|entry| match entry {
                Entry::Speech(speech) => Some(speech),
                Entry::Note(_) | Entry::Suggestion(_) => None,
            })
            .map(|speech| {
                let found = labels
                    .iter()
                    .find(|(at, _)| speech.who == who && (speech.at - at).abs() < 1e-6);
                found.map_or_else(|| speech.who.clone(), |(_, label)| label.clone())
            })
            .collect();
        let current = self.timeline.iter().filter_map(|entry| match entry {
            Entry::Speech(speech) => Some(&speech.who),
            Entry::Note(_) | Entry::Suggestion(_) => None,
        });
        if current.ne(relabelled.iter()) {
            self.assign_speakers(&relabelled);
        }
        relabelled
    }

    /// The context slots the user turned on, or `None` until they choose.
    pub fn contexts(&self) -> Option<&[String]> {
        self.contexts.as_deref()
    }

    /// Turn on exactly these context slots; only a change is recorded.
    pub fn set_contexts(&mut self, slots: &[String]) {
        if self.contexts.as_deref() != Some(slots) {
            self.contexts = Some(slots.to_vec());
            (self.record)(as_record(
                json!({"type": "context", "slots": slots, "at": now()}),
            ));
        }
    }

    /// The user's tags, in the order given.
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|kept| same_tag(kept, tag))
    }

    /// Tag the session `tag`, a `tag_name`; only a change is recorded.
    pub fn tag(&mut self, tag: &str) {
        if !self.has_tag(tag) {
            let mut tags = self.tags.clone();
            tags.push(tag.into());
            self.set_tags(tags);
        }
    }

    /// Take the tag `tag` off, whatever its case; only a change is recorded.
    pub fn untag(&mut self, tag: &str) {
        if self.has_tag(tag) {
            let tags = self.tags.iter().filter(|kept| !same_tag(kept, tag));
            self.set_tags(tags.cloned().collect());
        }
    }

    /// Call the tag `from`, and any tag already called `to` in another case, `to`,
    /// in the place of the first; only a change is recorded.
    pub fn retag(&mut self, from: &str, to: &str) {
        let mut tags: Vec<String> = Vec::new();
        for tag in &self.tags {
            let tag = if same_tag(tag, from) || same_tag(tag, to) {
                to
            } else {
                tag
            };
            if !tags.iter().any(|kept| same_tag(kept, tag)) {
                tags.push(tag.into());
            }
        }
        if tags != self.tags {
            self.set_tags(tags);
        }
    }

    fn set_tags(&mut self, tags: Vec<String>) {
        (self.record)(as_record(
            json!({"type": "tags", "tags": tags, "at": now()}),
        ));
        self.tags = tags;
    }

    /// The language the session's lines and answers are translated into, or
    /// `None` while the user has not turned translation on.
    pub fn translation(&self) -> Option<&str> {
        Some(self.translation.as_str()).filter(|language| !language.is_empty())
    }

    /// Translate into `language` from now on, or stop for ""; only a change is
    /// recorded. The session's own language is refused: false.
    pub fn set_translation(&mut self, language: &str) -> bool {
        if !language.is_empty() && self.language.split('-').next() == Some(language) {
            return false;
        }
        if self.translation != language {
            self.translation = language.into();
            (self.record)(as_record(
                json!({"type": "translation", "language": language, "at": now()}),
            ));
        }
        true
    }

    fn keep_translation(&mut self, of: Source, language: String, text: String) {
        self.translations
            .retain(|t| !(t.of.is(&of) && t.language == language));
        self.translations.push(Translation { of, language, text });
    }

    /// Keep the translation of a line or an answer that is still in the session.
    pub fn translated(&mut self, of: Source, language: &str, text: &str) -> bool {
        if self.text_of(&of).is_none() {
            return false;
        }
        let mut record = json!({"type": "translated", "language": language, "text": text});
        for (key, value) in of.fields().as_object().expect("an object") {
            record[key] = value.clone();
        }
        (self.record)(as_record(record));
        self.keep_translation(of, language.into(), text.into());
        true
    }

    pub fn translation_of(&self, of: &Source, language: &str) -> Option<&str> {
        self.translations
            .iter()
            .find(|t| t.of.is(of) && t.language == language)
            .map(|t| t.text.as_str())
    }

    /// The text of a line or a finished answer.
    pub fn text_of(&self, of: &Source) -> Option<&str> {
        self.timeline.iter().find_map(|entry| match (entry, of) {
            (Entry::Speech(s), Source::Line(at)) if (s.at - at).abs() < 1e-6 => {
                Some(s.text.as_str())
            }
            (Entry::Suggestion(a), Source::Answer(id)) if a.id == *id && a.done => {
                Some(a.text.as_str())
            }
            _ => None,
        })
    }

    /// The lines and finished answers with no translation into `language` yet, in order.
    pub fn untranslated(&self, language: &str) -> Vec<(Source, String)> {
        self.timeline
            .iter()
            .filter_map(|entry| match entry {
                Entry::Speech(s) => Some((Source::Line(s.at), s.text.clone())),
                Entry::Suggestion(a) if a.done => {
                    Some((Source::Answer(a.id.clone()), a.text.clone()))
                }
                _ => None,
            })
            .filter(|(of, _)| self.translation_of(of, language).is_none())
            .collect()
    }

    /// Whether the user cleared the guess that the speaker `label` is `person_id`.
    pub fn dismissed(&self, label: &str, person_id: &str) -> bool {
        self.dismissed
            .contains(&(label.to_string(), person_id.to_string()))
    }

    /// The speaker `label` is not `person_id`, whatever their voice suggests.
    pub fn dismiss_guess(&mut self, label: &str, person_id: &str) {
        if self
            .dismissed
            .insert((label.to_string(), person_id.to_string()))
        {
            (self.record)(as_record(json!({
                "type": "guess_dismissed", "label": label, "person": person_id, "at": now()
            })));
        }
    }

    fn name_speaker(&mut self, label: String, name: String) {
        if name.is_empty() || name == label {
            self.names.remove(&label);
        } else {
            self.names.insert(label, name);
        }
    }

    /// The name a line's speaker goes by in this session: the one given, or its label.
    pub fn name_of<'a>(&'a self, label: &'a str) -> &'a str {
        self.names.get(label).map_or(label, String::as_str)
    }

    pub fn color_of(&self, label: &str) -> &str {
        self.colors.get(label).map_or("", String::as_str)
    }

    pub fn set_speaker_color(&mut self, label: &str, color: &str) {
        if self.color_of(label) == color {
            return;
        }
        if color.is_empty() {
            self.colors.remove(label);
        } else {
            self.colors.insert(label.into(), color.into());
        }
        (self.record)(as_record(json!({
            "type": "speaker_color", "label": label, "color": color, "at": now()
        })));
    }

    /// Give the speaker whose lines carry `label` a name in this session; naming two
    /// the same merges them, and their own label (or none) restores it.
    pub fn rename_speaker(&mut self, label: &str, name: &str) {
        if self.name_of(label) != name {
            self.name_speaker(label.into(), name.into());
            (self.record)(as_record(
                json!({"type": "speaker", "label": label, "name": name, "at": now()}),
            ));
        }
    }

    /// The person the speaker `label` is, by id.
    pub fn person_of(&self, label: &str) -> Option<&str> {
        self.people.get(label).map(String::as_str)
    }

    /// The labels of the speakers who are `person_id`.
    pub fn labels_of(&self, person_id: &str) -> Vec<String> {
        let people = self.people.iter();
        people
            .filter(|(_, p)| *p == person_id)
            .map(|(l, _)| l.clone())
            .collect()
    }

    pub fn has_attendee(&self, person_id: &str) -> bool {
        self.attendees.contains(person_id)
    }

    pub fn attendee_ids(&self) -> Vec<String> {
        self.attendees.iter().cloned().collect()
    }

    pub fn set_attendee(&mut self, person_id: &str, present: bool) {
        if self.attendees.contains(person_id) == present {
            return;
        }
        if present {
            self.attendees.insert(person_id.into());
        } else {
            self.attendees.remove(person_id);
        }
        (self.record)(as_record(json!({
            "type": "attendee", "person": person_id, "present": present, "at": now()
        })));
    }

    pub fn person_ids(&self) -> Vec<String> {
        self.attendees
            .iter()
            .chain(self.people.values())
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Say the speaker `label` is `person`, named after them; `None` drops the
    /// link and keeps the name.
    pub fn name_person(&mut self, label: &str, person: Option<&Person>) {
        let id = person.map(|p| p.id.as_str());
        if self.person_of(label) != id {
            match id {
                Some(id) => self.people.insert(label.into(), id.into()),
                None => self.people.remove(label),
            };
            (self.record)(as_record(
                json!({"type": "person", "label": label, "person": id, "at": now()}),
            ));
        }
        if let Some(person) = person {
            self.rename_speaker(label, &person.name);
        }
    }

    fn relabel_line(&mut self, who: &str, at: f64, label: &str) -> Option<bool> {
        let speech = self.timeline.iter_mut().find_map(|entry| match entry {
            Entry::Speech(speech) if speech.who == who && (speech.at - at).abs() < 1e-6 => {
                Some(speech)
            }
            _ => None,
        })?;
        speech.who = label.into();
        let retired = who != label
            && !self
                .timeline
                .iter()
                .any(|entry| matches!(entry, Entry::Speech(speech) if speech.who == who));
        if retired {
            self.people.remove(who);
            self.names.remove(who);
            self.colors.remove(who);
            self.line_labels.remove(who);
        }
        Some(retired)
    }

    /// Identify one line without changing the other lines of its speaker.
    pub fn assign_line(&mut self, who: &str, at: f64, person: &Person) -> Option<(String, bool)> {
        let label = if self.line_labels.contains(who) {
            who.to_string()
        } else {
            format!("{who}#{}", short_id(8))
        };
        let retired = self.relabel_line(who, at, &label)?;
        self.people.insert(label.clone(), person.id.clone());
        self.line_labels.insert(label.clone());
        self.name_speaker(label.clone(), person.name.clone());
        (self.record)(as_record(json!({
            "type": "line_person", "who": who, "at": at, "label": label,
            "person": person.id, "name": person.name
        })));
        Some((label, retired))
    }

    /// The labels of the lines heard, in the order they first speak.
    pub fn labels(&self) -> Vec<String> {
        let mut labels: Vec<String> = Vec::new();
        for entry in &self.timeline {
            if let Entry::Speech(speech) = entry
                && !labels.contains(&speech.who)
            {
                labels.push(speech.who.clone());
            }
        }
        labels
    }

    /// The speakers' names in the order they first spoke.
    pub fn speakers(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for label in self.labels() {
            let name = self.name_of(&label);
            if !names.iter().any(|n| n == name) {
                names.push(name.into());
            }
        }
        names
    }

    /// Whether its title, tags, lines, notes, questions or answers hold `needle`,
    /// which is already `folded`.
    pub fn mentions(&self, needle: &str) -> bool {
        let texts = self.timeline.iter().flat_map(|entry| match entry {
            Entry::Speech(speech) => [speech.text.as_str(), ""],
            Entry::Note(note) => [note.text.as_str(), ""],
            Entry::Suggestion(answer) => [answer.prompt.as_str(), answer.text.as_str()],
        });
        std::iter::once(self.title.as_str())
            .chain(self.tags.iter().map(String::as_str))
            .chain(texts)
            .any(|text| folded(text).contains(needle))
    }

    /// Speech and completed suggestions to send, oldest first, within `max_chars`.
    ///
    /// When they outgrow the budget the start jumps forward until they fill only
    /// 60% of it, so it moves once per overflow rather than on every request.
    pub fn context(&mut self, max_chars: usize) -> Vec<Entry> {
        let since = self.context_since;
        let mut entries: Vec<Entry> = self
            .timeline
            .iter()
            .filter(|e| e.at() >= since && e.kept())
            .cloned()
            .collect();
        if total(&entries) > max_chars {
            let budget = max_chars as f64 * 0.6;
            while !entries.is_empty() && total(&entries) as f64 > budget {
                entries.remove(0);
            }
            self.context_since = entries.first().map_or_else(now, Entry::at);
        }
        // Speakers go by the names given in this session.
        for entry in &mut entries {
            if let Entry::Speech(speech) = entry {
                speech.who = self.name_of(&speech.who).to_string();
            }
        }
        entries
    }
}

fn total(entries: &[Entry]) -> usize {
    entries.iter().map(Entry::size).sum()
}

/// What a session list shows: what, when, how long, in which state, how much,
/// and what it cost, its transcription models priced by `rate`.
pub fn summarize(records: &[Record], rate: impl Fn(&str) -> Rate) -> Result<Value, LogError> {
    let session = Session::restore(records, Box::new(|_| {}))?;
    let speech = session
        .timeline
        .iter()
        .filter(|e| matches!(e, Entry::Speech(_)))
        .count();
    let suggestions = session.timeline.len() - speech;
    Ok(json!({
        "id": session.id,
        "title": session.title,
        "kind": session.kind,
        "source": session.source,
        "language": session.language,
        "state": session.state,
        "started_at": session.started_at,
        // The time it ran: pauses, answers asked and renames made later do not count.
        "duration_s": session.runs().total().round_ties_even() as i64,
        "speech": speech,
        "suggestions": suggestions,
        "speakers": session.speakers(),
        "people": session.person_ids(),
        "attendees": session.attendee_ids(),
        "contexts": session.contexts(),
        "translating": session.translation(),
        "tags": session.tags(),
        "cost": session.cost(rate),
    }))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    fn collecting() -> (RecordSink, Arc<Mutex<Vec<Record>>>) {
        let records = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&records);
        (
            Box::new(move |record| sink.lock().unwrap().push(record)),
            records,
        )
    }

    fn no_prices(_: &str) -> Rate {
        Rate::default()
    }

    /// A transcription model the user priced at `price` a minute, or not.
    fn per_minute(price: Option<f64>) -> Rate {
        Rate {
            per_minute: price,
            billed: false,
        }
    }

    fn types(records: &[Record]) -> Vec<&str> {
        records
            .iter()
            .map(|r| r["type"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn tags_are_trimmed_and_one_whatever_their_case() {
        assert_eq!(tag_name("  Client   X \n").as_deref(), Some("Client X"));
        assert_eq!(tag_name(" \t "), None);
        assert!(same_tag("Cliente Á", "cliente á"));
        assert!(!same_tag("Q3", "Q4"));
    }

    #[test]
    fn a_session_is_not_translated_into_its_own_language() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Talk", "meeting", LIVE, "pt-BR", sink);
        assert!(!session.set_translation("pt"));
        assert_eq!(session.translation(), None);
        assert!(session.set_translation("en"));
        assert!(session.set_translation(""));
        assert_eq!(session.translation(), None);
        let records = records.lock().unwrap().clone();
        let languages: Vec<&Value> = records
            .iter()
            .filter(|r| r["type"] == "translation")
            .map(|r| &r["language"])
            .collect();
        assert_eq!(
            languages,
            [&json!("en"), &json!("")],
            "a refusal records nothing"
        );
    }

    #[test]
    fn tags_are_kept_as_records_and_survive_restore() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Talk", "meeting", LIVE, "en", sink);
        session.tag("Client X");
        session.tag("client x");
        session.tag("Q3");
        session.untag("q3");
        session.untag("Q3");
        session.tag("Hiring");
        let records = records.lock().unwrap().clone();
        let tags: Vec<&Value> = records
            .iter()
            .filter(|r| r["type"] == "tags")
            .map(|r| &r["tags"])
            .collect();
        assert_eq!(
            tags,
            [
                &json!(["Client X"]),
                &json!(["Client X", "Q3"]),
                &json!(["Client X"]),
                &json!(["Client X", "Hiring"])
            ]
        );
        let restored = Session::restore(&records, Box::new(|_| {})).unwrap();
        assert_eq!(restored.tags(), ["Client X", "Hiring"]);
        assert_eq!(
            summarize(&records, no_prices).unwrap()["tags"],
            json!(["Client X", "Hiring"])
        );
    }

    #[test]
    fn an_import_is_dated_by_the_users_date_then_the_recordings_then_the_files() {
        let (given, tagged, modified) = (3000.0, 2000.0, 1000.0);
        assert_eq!(
            recorded_at(Some(given), Some(tagged), Some(modified)),
            given
        );
        assert_eq!(recorded_at(None, Some(tagged), Some(modified)), tagged);
        assert_eq!(recorded_at(None, None, Some(modified)), modified);
        let before = now();
        assert!(recorded_at(None, None, None) >= before);
    }

    #[test]
    fn an_import_runs_on_its_recordings_clock() {
        let (sink, records) = collecting();
        let recorded = 1_700_000_000.0;
        let mut session = Session::begin_at("Talk", "meeting", IMPORT, "en", recorded, sink);
        session.transcribe_with(HeardBy::Model("whisper".into(), 1, None));
        session.hear_at("Eles", "Bom dia.", recorded + 600.0);
        session.assign_speakers(&["Speaker 1".into()]);
        session.set_state(ENDED);
        let records = records.lock().unwrap().clone();
        assert_eq!(records[0]["started_at"], json!(recorded));
        for record in &records[1..] {
            let at = record["at"].as_f64().unwrap();
            assert!((recorded..recorded + 601.0).contains(&at), "{record:?}");
        }
        let summary = summarize(&records, no_prices).unwrap();
        assert_eq!(summary["started_at"], json!(recorded));
        assert_eq!(summary["duration_s"], json!(600));
    }

    #[test]
    fn a_log_from_before_tags_has_none() {
        let (sink, records) = collecting();
        Session::begin("Old", "meeting", LIVE, "en", sink);
        let records = records.lock().unwrap().clone();
        assert!(
            Session::restore(&records, Box::new(|_| {}))
                .unwrap()
                .tags()
                .is_empty()
        );
        assert_eq!(summarize(&records, no_prices).unwrap()["tags"], json!([]));
    }

    #[test]
    fn retagging_renames_in_place_and_joins_a_tag_of_that_name() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Talk", "meeting", LIVE, "en", sink);
        session.tag("work");
        session.tag("Q3");
        session.tag("Job");
        session.retag("WORK", "job");
        assert_eq!(session.tags(), ["job", "Q3"]);
        session.retag("missing", "Other");
        session.retag("q3", "Q3");
        let kept = records.lock().unwrap().clone();
        assert_eq!(
            kept.iter().filter(|r| r["type"] == "tags").count(),
            4,
            "a rename that changes nothing is not recorded"
        );
    }

    #[test]
    fn search_finds_a_tag_and_counts_keep_the_first_casing() {
        let mut first = Session::begin("A", "meeting", LIVE, "en", Box::new(|_| {}));
        first.tag("Cliente Ágil");
        let mut second = Session::begin("B", "idea", LIVE, "en", Box::new(|_| {}));
        second.tag("cliente ágil");
        second.tag("Beta");
        assert!(second.mentions(&folded("AGIL")));
        let counts = tag_counts([first.tags(), second.tags()]);
        assert_eq!(
            counts,
            [("Beta".to_string(), 1), ("Cliente Ágil".to_string(), 2)]
        );
    }

    #[test]
    fn speaker_color_survives_restore() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Talk", "meeting", LIVE, "en", sink);
        session.hear_at("Speaker 1", "Hello.", now());
        session.set_speaker_color("Speaker 1", "#ffb000");
        session.set_speaker_color("Speaker 1", "#ffb000");
        let records = records.lock().unwrap().clone();
        assert_eq!(
            types(&records)
                .iter()
                .filter(|kind| **kind == "speaker_color")
                .count(),
            1
        );
        let restored = Session::restore(&records, Box::new(|_| {})).unwrap();
        assert_eq!(restored.color_of("Speaker 1"), "#ffb000");
    }

    #[test]
    fn attendees_restore_without_changing_transcript() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Talk", "meeting", LIVE, "en", sink);
        session.hear_at("Others", "Hello.", now());
        let person = Person::new("Ana");
        session.name_person("Others", Some(&person));
        session.set_attendee("guest", true);
        session.set_attendee("guest", true);
        session.set_attendee("guest", false);
        let records = records.lock().unwrap().clone();
        assert_eq!(
            types(&records)
                .iter()
                .filter(|kind| **kind == "attendee")
                .count(),
            2
        );
        let restored = Session::restore(&records, Box::new(|_| {})).unwrap();
        assert_eq!(restored.person_ids(), vec![person.id.clone()]);
        assert!(restored.attendee_ids().is_empty());
        assert_eq!(restored.name_of("Others"), "Ana");
        assert_eq!(restored.timeline.len(), 1);
        assert_eq!(
            summarize(&records, no_prices).unwrap()["people"],
            json!([person.id])
        );
    }

    #[test]
    fn records_every_lasting_change_and_leaves_out_removed_drafts() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Entrevista", "meeting", LIVE, "pt", sink);
        session.hear_at("Recrutador", "Oi.", now());
        let kept = session.suggest("ask", "m", "ask", "Pedido: x");
        session.extend(&kept.id, "Olá!");
        session.finish(&kept.id);
        let draft = session.suggest("probe", "m", "probe", "Pedido: y");
        session.remove(&draft.id);
        session.finish(&draft.id);
        session.remove(&kept.id);
        session.set_state(PAUSED);

        let records = records.lock().unwrap();
        assert_eq!(
            types(&records),
            [
                "session",
                "state",
                "speech",
                "suggestion",
                "removed",
                "state"
            ]
        );
        assert_eq!(records[0]["title"], "Entrevista");
        assert_eq!(records[0]["language"], "pt");
        assert_eq!(records[3]["text"], "Olá!");
        assert_eq!(records[5]["state"], "paused");
    }

    #[test]
    fn context_skips_unfinished_suggestions() {
        let mut session = Session::begin("", "meeting", LIVE, "pt", Box::new(|_| {}));
        session.hear_at("Eu", "Oi.", now());
        session.suggest("ask", "m", "ask", "Pedido: x");
        let context = session.context(1000);
        assert!(matches!(context.as_slice(), [Entry::Speech(_)]));
    }

    #[test]
    fn context_jumps_forward_once_per_overflow() {
        let mut session = Session::begin("", "meeting", LIVE, "pt", Box::new(|_| {}));
        let line = |at: f64, text: String| {
            Entry::Speech(Speech {
                who: "Eles".into(),
                text,
                at,
            })
        };
        for i in 0..10 {
            session
                .timeline
                .push(line(i as f64, format!("fala {i:02} {}", "x".repeat(91))));
        }
        assert_eq!(session.context(1000).len(), 10);
        session.timeline.push(line(10.0, "y".repeat(100)));
        let trimmed = session.context(1000);
        assert_eq!(trimmed.len(), 6); // 60% of the budget
        session.timeline.push(line(11.0, "z".repeat(100)));
        assert_eq!(&session.context(1000)[..6], trimmed.as_slice()); // same start until the next overflow
    }

    #[test]
    fn notes_are_kept_and_removed_like_answers() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Entrevista", "meeting", LIVE, "pt", sink);
        session.hear_at("Recrutador", "Como você migrou?", now());
        let kept = session.note("Migrei um app WPF para MVVM.");
        let dropped = session.note("Rascunho.");
        assert!(matches!(session.remove(&dropped.id), Some(Entry::Note(_))));
        let records = records.lock().unwrap().clone();
        let restored = Session::restore(&records, Box::new(|_| {})).unwrap();
        assert!(matches!(
            restored.timeline.as_slice(),
            [Entry::Speech(_), Entry::Note(note)] if note == &kept
        ));
    }

    #[test]
    fn restore_rebuilds_what_was_kept() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Entrevista", "meeting", LIVE, "pt", sink);
        session.hear_at("Recrutador", "Oi.", now());
        for text in ["Primeira.", "Segunda."] {
            let suggestion = session.suggest("ask", "m", "ask", "Pedido: x");
            session.extend(&suggestion.id, text);
            session.finish(&suggestion.id);
        }
        let first = session.timeline[1].id().unwrap().to_string();
        session.remove(&first);
        session.set_state(PAUSED);

        let records = records.lock().unwrap().clone();
        let (later_sink, later) = collecting();
        let mut restored = Session::restore(&records, later_sink).unwrap();
        assert_eq!(
            (restored.id.as_str(), restored.title.as_str()),
            (session.id.as_str(), "Entrevista")
        );
        assert_eq!(restored.state, PAUSED);
        assert!(matches!(
            restored.timeline.as_slice(),
            [Entry::Speech(speech), Entry::Suggestion(kept)]
                if (speech.who.as_str(), speech.text.as_str()) == ("Recrutador", "Oi.")
                    && (kept.text.as_str(), kept.done) == ("Segunda.", true)
                    && (kept.prompt.as_str(), kept.request.as_str()) == ("ask", "Pedido: x")
        ));
        restored.hear_at("Eu", "Voltei.", now());
        let later = later.lock().unwrap();
        assert_eq!(
            (later[0]["type"].as_str(), later[0]["text"].as_str()),
            (Some("speech"), Some("Voltei."))
        );

        let summary = summarize(&records, no_prices).unwrap();
        assert_eq!(
            (summary["title"].as_str(), summary["state"].as_str()),
            (Some("Entrevista"), Some("paused"))
        );
        assert_eq!(
            (summary["speech"].as_u64(), summary["suggestions"].as_u64()),
            (Some(1), Some(1))
        );
        assert!(summary["duration_s"].as_i64().unwrap() >= 0);
        // An answer asked an hour later does not lengthen the recording.
        let mut later = records.clone();
        later.push(as_record(json!({
            "type": "suggestion", "id": "late", "action": "ask", "model": "m", "prompt": "ask",
            "request": "Pedido: x", "text": "Depois.", "at": restored.started_at + 3600.0,
        })));
        assert_eq!(
            summarize(&later, no_prices).unwrap()["duration_s"],
            summary["duration_s"]
        );
    }

    #[test]
    fn a_session_runs_while_recording_or_importing_and_pauses_do_not_count() {
        let log = |source: &str, steps: &[(&str, &str, f64)]| -> Vec<Record> {
            let head = json!({"type": "session", "id": "s", "title": "", "kind": "meeting",
                "source": source, "language": "pt", "started_at": 100.0});
            std::iter::once(head)
                .chain(steps.iter().map(|&(kind, value, at)| match kind {
                    "state" => json!({"type": "state", "state": value, "at": at}),
                    _ => json!({"type": "speech", "who": "Eu", "text": value, "at": at}),
                }))
                .map(as_record)
                .collect()
        };
        let paused_once = [
            ("state", RECORDING, 100.0),
            ("speech", "Oi.", 130.0),
            ("state", PAUSED, 160.0),
            ("state", RECORDING, 400.0),
            ("speech", "Voltei.", 420.0),
        ];
        let open = Session::restore(&log(LIVE, &paused_once), Box::new(|_| {})).unwrap();
        assert_eq!(
            (open.runs().counted, open.runs().since),
            (60.0, Some(400.0))
        );
        assert_eq!(open.runs().total(), 80.0);

        let ended = log(
            LIVE,
            &[&paused_once[..], &[("state", ENDED, 430.0)]].concat(),
        );
        let closed = Session::restore(&ended, Box::new(|_| {})).unwrap();
        assert_eq!((closed.runs().counted, closed.runs().since), (90.0, None));
        assert_eq!(summarize(&ended, no_prices).unwrap()["duration_s"], 90);

        // An import runs as long as its file, even when transcribed faster.
        let imported = log(
            IMPORT,
            &[
                ("state", IMPORTING, 100.0),
                ("speech", "Bom dia.", 700.0),
                ("state", ENDED, 150.0),
            ],
        );
        assert_eq!(summarize(&imported, no_prices).unwrap()["duration_s"], 600);
    }

    #[test]
    fn what_answers_reviews_and_translations_cost_is_kept_even_once_removed() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Call", "meeting", LIVE, "pt", sink);
        let answer = session.suggest("ask", "m", "ask", "Pedido: x");
        session.extend(&answer.id, "Oi.");
        session.finish(&answer.id);
        session.spend("answer", "m", Some(0.001), Some(&answer.id));
        session.spend("review", "r", Some(0.0025), Some(&answer.id));
        session.remove(&answer.id);
        let records = records.lock().unwrap().clone();
        let spent: Vec<&Record> = records.iter().filter(|r| r["type"] == "spent").collect();
        assert_eq!(
            (spent[0]["for"].as_str(), spent[1]["model"].as_str()),
            (Some("answer"), Some("r"))
        );
        let cost = summarize(&records, no_prices).unwrap()["cost"].clone();
        assert!((cost["llm_usd"].as_f64().unwrap() - 0.0035).abs() < 1e-12);
        assert_eq!(cost["llm_unknown"], false);
        assert_eq!(cost["total_usd"], cost["llm_usd"]);

        // A provider that reports no cost leaves that part unknown, the rest summed.
        let mut restored = Session::restore(&records, Box::new(|_| {})).unwrap();
        restored.spend("translation", "t", None, None);
        let cost = restored.cost(no_prices);
        assert!((cost["llm_usd"].as_f64().unwrap() - 0.0035).abs() < 1e-12);
        assert_eq!(cost["llm_unknown"], true);
    }

    #[test]
    fn transcription_costs_each_models_minutes_per_input_at_its_price() {
        let head = json!({"type": "session", "id": "s", "title": "", "kind": "meeting",
            "source": LIVE, "language": "pt", "started_at": 100.0});
        let records: Vec<Record> = [
            head,
            json!({"type": "state", "state": RECORDING, "at": 100.0}),
            // Recorded a moment after the run began, it takes the whole run.
            json!({"type": "transcriber", "model": "nova", "inputs": 2, "at": 100.5}),
            json!({"type": "speech", "who": "Eu", "text": "Oi.", "at": 130.0}),
            json!({"type": "state", "state": PAUSED, "at": 160.0}),
            json!({"type": "state", "state": RECORDING, "at": 200.0}),
            json!({"type": "transcriber", "model": "whisper", "inputs": 1, "at": 230.0}),
            json!({"type": "state", "state": ENDED, "at": 260.0}),
        ]
        .into_iter()
        .map(as_record)
        .collect();
        let price = |model: &str| per_minute((model == "nova").then_some(0.01));
        let cost = summarize(&records, price).unwrap()["cost"].clone();
        assert_eq!(
            cost["transcribed_s"],
            json!({"nova": 180.0, "whisper": 30.0})
        );
        assert!((cost["transcription_usd"].as_f64().unwrap() - 0.03).abs() < 1e-12);
        assert_eq!(cost["transcription_unknown"], true); // whisper has no price
        let both = |model: &str| per_minute(Some(if model == "nova" { 0.01 } else { 0.006 }));
        let cost = summarize(&records, both).unwrap()["cost"].clone();
        assert_eq!(cost["transcription_unknown"], false);
        assert!((cost["total_usd"].as_f64().unwrap() - 0.033).abs() < 1e-12);

        // A session still recording counts up to its last line.
        let open = Session::restore(&records[..4], Box::new(|_| {})).unwrap();
        assert_eq!(open.cost(price)["transcribed_s"], json!({"nova": 60.0}));
    }

    fn logged(records: &[Value]) -> Vec<Record> {
        let head = json!({"type": "session", "id": "s", "title": "", "kind": "meeting",
            "source": LIVE, "language": "pt", "started_at": 100.0});
        std::iter::once(head)
            .chain(records.iter().cloned())
            .map(as_record)
            .collect()
    }

    #[test]
    fn a_price_kept_in_the_log_costs_the_audio_whatever_the_price_is_now() {
        let records = logged(&[
            json!({"type": "state", "state": RECORDING, "at": 100.0}),
            json!({"type": "transcriber", "model": "nova", "inputs": 1, "per_minute": 0.006,
                "at": 100.0}),
            json!({"type": "speech", "who": "Eu", "text": "Oi.", "at": 160.0}),
            // The price changed while it recorded: the rest is heard at the new one.
            json!({"type": "transcriber", "model": "nova", "inputs": 1, "per_minute": 0.012,
                "at": 160.0}),
            json!({"type": "state", "state": ENDED, "at": 220.0}),
        ]);
        let now_priced = |price: f64| move |_: &str| per_minute(Some(price));
        for price in [0.006, 0.5] {
            let cost = summarize(&records, now_priced(price)).unwrap()["cost"].clone();
            assert!((cost["transcription_usd"].as_f64().unwrap() - 0.018).abs() < 1e-12);
            assert_eq!(cost["transcription_unknown"], false);
        }
        // A log from before prices were kept takes the price there is now.
        let old = logged(&[
            json!({"type": "state", "state": RECORDING, "at": 100.0}),
            json!({"type": "transcriber", "model": "nova", "inputs": 1, "at": 100.0}),
            json!({"type": "state", "state": ENDED, "at": 160.0}),
        ]);
        let cost = summarize(&old, now_priced(0.5)).unwrap()["cost"].clone();
        assert!((cost["transcription_usd"].as_f64().unwrap() - 0.5).abs() < 1e-12);

        // A request keeps the price its model had when it closed.
        let (sink, records) = collecting();
        let mut session = Session::begin("Call", "meeting", LIVE, "pt", sink);
        session.transcribe_with(HeardBy::Model("nova".into(), 1, Some(0.006)));
        session.bill("r1", "nova", (60.0, 1.0), now());
        session.pay("r1", Err(Unknown::NoScope));
        let records = records.lock().unwrap().clone();
        let billed = records.iter().find(|r| r["type"] == "billed").unwrap();
        assert_eq!(billed["per_minute"], json!(0.006));
        let cost = summarize(&records, now_priced(0.5)).unwrap()["cost"].clone();
        assert!((cost["transcription_usd"].as_f64().unwrap() - 0.006).abs() < 1e-12);
        let item = &Session::restore(&records, Box::new(|_| {}))
            .unwrap()
            .spending(no_prices)["items"][0];
        assert_eq!(
            (&item["usd"], &item["estimate"]),
            (&json!(0.006), &json!(true))
        );
    }

    #[test]
    fn a_request_the_daemon_never_closed_counts_the_time_the_session_listened() {
        let records = logged(&[
            json!({"type": "state", "state": RECORDING, "at": 100.0}),
            json!({"type": "transcriber", "model": "nova", "inputs": 2, "at": 100.0}),
            json!({"type": "listening", "request": "r1", "model": "nova", "at": 100.0}),
            json!({"type": "billed", "request": "r1", "model": "nova", "seconds": 10.0,
                "share": 1.0, "at": 110.0}),
            json!({"type": "listening", "request": "r2", "model": "nova", "at": 110.0}),
            json!({"type": "speech", "who": "Eu", "text": "Oi.", "at": 150.0}),
            json!({"type": "state", "state": PAUSED, "at": 160.0}),
            json!({"type": "state", "state": RECORDING, "at": 200.0}),
            json!({"type": "listening", "request": "r2", "model": "nova", "at": 200.0}),
            json!({"type": "speech", "who": "Eu", "text": "Tchau.", "at": 230.0}),
        ]);
        let session = Session::restore(&records, Box::new(|_| {})).unwrap();
        // r1 closed; r2 was listened to from 110 to the pause, then up to the last line.
        assert_eq!(
            session.unclosed(),
            [Unclosed {
                request: "r2".into(),
                model: "nova".into(),
                seconds: 80.0,
                last: 230.0
            }]
        );
        assert_eq!(session.billed().collect::<Vec<_>>(), ["r1"]);

        // Another model hearing it ends the request's time.
        let switched = logged(&[
            json!({"type": "state", "state": RECORDING, "at": 100.0}),
            json!({"type": "transcriber", "model": "nova", "inputs": 1, "at": 100.0}),
            json!({"type": "listening", "request": "r1", "model": "nova", "at": 100.0}),
            json!({"type": "transcriber", "model": "whisper", "inputs": 1, "at": 140.0}),
            json!({"type": "speech", "who": "Eu", "text": "Oi.", "at": 190.0}),
        ]);
        let session = Session::restore(&switched, Box::new(|_| {})).unwrap();
        assert_eq!(session.unclosed()[0].seconds, 40.0);

        // Billed later, it is closed.
        let (sink, kept) = collecting();
        let mut session = Session::restore(&records, sink).unwrap();
        session.bill("r2", "nova", (80.0, 1.0), 230.0);
        assert!(session.unclosed().is_empty());
        assert_eq!(kept.lock().unwrap()[0]["type"], "billed");
    }

    #[test]
    fn billed_requests_cost_what_their_provider_reported_and_a_price_only_fills_in() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Call", "meeting", LIVE, "pt", sink);
        session.transcribe_with(HeardBy::Model("nova".into(), 2, None));
        session.bill("r1", "nova", (60.0, 0.5), now());
        session.bill("r2", "nova", (60.0, 1.0), now());
        session.pay("r1", Ok(0.02));
        session.pay("unknown request", Ok(1.0));
        let records = records.lock().unwrap().clone();
        let spent = records.iter().find(|r| r["type"] == "spent").unwrap();
        assert_eq!(
            (&spent["for"], &spent["request"], &spent["usd"]),
            (&json!("transcription"), &json!("r1"), &json!(0.01))
        );
        // r2 is not priced yet: unknown, or at the user's price for its minute.
        let restored = Session::restore(&records, Box::new(|_| {})).unwrap();
        let unpaid: Vec<_> = restored
            .unpaid()
            .map(|(id, model, _)| (id, model))
            .collect();
        assert_eq!(unpaid, [("r2", "nova")]);
        let cost = restored.cost(no_prices);
        assert_eq!(
            (
                cost["transcription_usd"].as_f64(),
                &cost["transcription_unknown"]
            ),
            (Some(0.01), &json!(true))
        );
        assert_eq!(cost["llm_unknown"], false, "transcription is not an LLM's");
        let cost = restored.cost(|_| per_minute(Some(0.006)));
        assert!((cost["transcription_usd"].as_f64().unwrap() - 0.016).abs() < 1e-12);
        assert_eq!(cost["transcription_unknown"], false);

        // Priced, the provider's cost stands and no price is used.
        let mut restored = Session::restore(&records, Box::new(|_| {})).unwrap();
        restored.pay("r2", Ok(0.03));
        let cost = restored.cost(|_| per_minute(Some(100.0)));
        assert!((cost["transcription_usd"].as_f64().unwrap() - 0.04).abs() < 1e-12);
        assert_eq!(cost["transcription_unknown"], false);
        // A provider that cannot tell leaves it to the price, or unknown.
        restored.pay("r1", Err(Unknown::NoScope));
        assert_eq!(restored.cost(no_prices)["transcription_unknown"], true);
    }

    #[test]
    fn each_charge_says_what_it_paid_for_and_why_its_cost_is_unknown() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Call", "meeting", LIVE, "pt", sink);
        session.transcribe_with(HeardBy::Model("nova".into(), 1, None));
        let answer = session.suggest("ask", "m", "Qual o prazo?", "Pedido: x");
        session.finish(&answer.id);
        session.spend("answer", "m", Some(0.001), Some(&answer.id));
        session.spend("translation", "t", None, None);
        session.bill("r1", "nova", (60.0, 1.0), now());
        session.pay("r1", Err(Unknown::NoScope));
        let restored = Session::restore(&records.lock().unwrap(), Box::new(|_| {})).unwrap();
        let items = |rate: Rate| restored.spending(|_| rate)["items"].clone();
        let what = |item: &Value| {
            (
                item["for"].as_str().unwrap().to_string(),
                item["usd"].as_f64(),
                item["unknown"].as_str().map(String::from),
                item["estimate"] == true,
            )
        };
        let listed: Vec<_> = items(no_prices(""))
            .as_array()
            .unwrap()
            .iter()
            .map(what)
            .collect();
        // Newest first: the request, the translation, then the answer.
        assert_eq!(
            listed,
            [
                ("transcription".into(), None, Some("no_scope".into()), false),
                ("translation".into(), None, Some("unreported".into()), false),
                ("answer".into(), Some(0.001), None, false),
            ]
        );
        assert_eq!(items(no_prices(""))[2]["prompt"], "Qual o prazo?");
        assert_eq!(
            restored.spending(no_prices)["parts"],
            json!([
                {"for": "answer", "usd": 0.001, "unknown": false},
                {"for": "translation", "usd": 0.0, "unknown": true},
                {"for": "transcription", "usd": 0.0, "unknown": true},
            ])
        );
        // The user's price stands in for what the provider could not tell, as an estimate.
        let priced = items(per_minute(Some(0.006)));
        assert_eq!(
            what(&priced[0]),
            ("transcription".into(), Some(0.006), None, true)
        );

        // Audio no request billed yet waits for its provider, or for a price.
        let (sink, _) = collecting();
        let mut live = Session::begin("Call", "meeting", LIVE, "pt", sink);
        live.transcribe_with(HeardBy::Model("nova".into(), 1, None));
        live.hear_at("Eu", "Oi.", live.started_at + 30.0);
        let billed = Rate {
            per_minute: None,
            billed: true,
        };
        let pending = &live.spending(|_| billed)["items"][0];
        assert_eq!(
            (&pending["unknown"], &pending["seconds"], &pending["at"]),
            (&json!("pending"), &json!(30.0), &Value::Null)
        );
        assert_eq!(live.spending(no_prices)["items"][0]["unknown"], "no_price");
    }

    #[test]
    fn a_new_transcriber_is_recorded_once_and_a_written_import_costs_nothing() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Call", "meeting", LIVE, "pt", sink);
        session.transcribe_with(HeardBy::Model("nova".into(), 2, None));
        session.transcribe_with(HeardBy::Model("nova".into(), 2, None));
        let records = records.lock().unwrap().clone();
        assert_eq!(types(&records), ["session", "state", "transcriber"]);
        let restored = Session::restore(&records, Box::new(|_| {})).unwrap();
        assert_eq!(
            restored.runs.transcriber,
            HeardBy::Model("nova".into(), 2, None)
        );

        let (sink, records) = collecting();
        let mut written = Session::begin("Aula", "other", IMPORT, "pt", sink);
        written.transcribe_with(HeardBy::Nothing);
        written.hear_at("Eles", "Bom dia.", written.started_at + 600.0);
        written.set_state(ENDED);
        let cost = summarize(&records.lock().unwrap(), no_prices).unwrap()["cost"].clone();
        assert_eq!(cost["transcribed_s"], json!({}));
        assert_eq!(
            (
                cost["transcription_unknown"].as_bool(),
                cost["total_usd"].as_f64()
            ),
            (Some(false), Some(0.0))
        );
    }

    #[test]
    fn an_old_log_without_costs_reads_with_its_costs_unknown() {
        let records: Vec<Record> = [
            json!({"type": "session", "id": "s", "title": "", "kind": "idea",
                "source": LIVE, "language": "pt", "started_at": 100.0}),
            json!({"type": "state", "state": RECORDING, "at": 100.0}),
            json!({"type": "suggestion", "id": "a", "action": "ask", "model": "m", "prompt": "ask",
                "request": "x", "text": "y", "at": 110.0}),
            json!({"type": "state", "state": ENDED, "at": 160.0}),
        ]
        .into_iter()
        .map(as_record)
        .collect();
        let cost = summarize(&records, |_| per_minute(Some(0.01))).unwrap()["cost"].clone();
        assert_eq!(
            cost,
            json!({"llm_usd": 0.0, "llm_unknown": true, "transcription_usd": 0.0,
                   "transcription_unknown": true, "total_usd": 0.0, "transcribed_s": {}})
        );
        let session = Session::restore(&records, Box::new(|_| {})).unwrap();
        let reasons: Vec<(Value, Value)> = session.spending(no_prices)["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| (item["unknown"].clone(), item["count"].clone()))
            .collect();
        assert_eq!(
            reasons,
            [
                (json!("unrecorded"), Value::Null),
                (json!("untracked"), json!(1))
            ]
        );
    }

    #[test]
    fn a_rename_is_recorded_once_and_restored() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Rascunho", "idea", LIVE, "pt", sink);
        session.rename("Plano", "other");
        session.rename("Plano", "other");
        let mut records = records.lock().unwrap().clone();
        assert_eq!(types(&records), ["session", "state", "meta"]);
        records[2].insert("at".into(), json!(session.started_at + 3600.0));
        let restored = Session::restore(&records, Box::new(|_| {})).unwrap();
        assert_eq!(
            (restored.title.as_str(), restored.kind.as_str()),
            ("Plano", "other")
        );
        let summary = summarize(&records, no_prices).unwrap();
        assert_eq!(
            (summary["title"].as_str(), summary["kind"].as_str()),
            (Some("Plano"), Some("other"))
        );
    }

    #[test]
    fn speakers_are_renamed_and_merged_by_record() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Daily", "meeting", IMPORT, "pt", sink);
        session.hear_at("Sérgio Figorelle", "Bom dia.", now());
        session.hear_at("Eles", "Oi.", now());
        session.hear_at("Ana", "Olá.", now());
        session.rename_speaker("Sérgio Figorelle", "Sérgio");
        session.rename_speaker("Sérgio Figorelle", "Sérgio"); // no change, no record
        session.rename_speaker("Eles", "Ana"); // merged with Ana
        assert_eq!(session.speakers(), ["Sérgio", "Ana"]);
        assert!(matches!(
            session.context(10_000).as_slice(),
            [Entry::Speech(a), Entry::Speech(b), Entry::Speech(c)]
                if [a.who.as_str(), b.who.as_str(), c.who.as_str()] == ["Sérgio", "Ana", "Ana"]
        ));
        assert_eq!(session.name_of("Eles"), "Ana");

        let records = records.lock().unwrap().clone();
        assert_eq!(
            types(&records).iter().filter(|t| **t == "speaker").count(),
            2
        );
        let mut restored = Session::restore(&records, Box::new(|_| {})).unwrap();
        assert_eq!(restored.speakers(), ["Sérgio", "Ana"]);
        restored.rename_speaker("Eles", "Eles"); // back to its own label
        assert_eq!(restored.speakers(), ["Sérgio", "Eles", "Ana"]);
        assert_eq!(
            summarize(&records, no_prices).unwrap()["speakers"],
            json!(["Sérgio", "Ana"])
        );
    }

    #[test]
    fn diarization_relabels_the_lines_and_survives_a_restore() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Aula", "other", IMPORT, "pt", sink);
        session.hear_at("Eles", "Bom dia.", now());
        session.hear_at("Eles", "Bom dia, professor.", now());
        session.hear_at("Eles", "Vamos lá.", now());
        let who = ["Speaker 1", "Speaker 2", "Speaker 1"].map(String::from);
        session.assign_speakers(&who);
        session.rename_speaker("Speaker 1", "Professor");
        assert_eq!(session.speakers(), ["Professor", "Speaker 2"]);
        let records = records.lock().unwrap().clone();
        let restored = Session::restore(&records, Box::new(|_| {})).unwrap();
        assert_eq!(restored.speakers(), ["Professor", "Speaker 2"]);
        let mut bad = records.clone();
        bad.push(as_record(json!({"type": "diarized", "who": [1]})));
        assert!(Session::restore(&bad, Box::new(|_| {})).is_err());
    }

    #[test]
    fn chosen_context_slots_are_recorded_once_and_restored() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Call", "meeting", LIVE, "pt", sink);
        assert_eq!(session.contexts(), None);
        session.set_contexts(&["entrevista".to_string()]);
        session.set_contexts(&["entrevista".to_string()]);
        let records = records.lock().unwrap().clone();
        assert_eq!(records.iter().filter(|r| r["type"] == "context").count(), 1);
        let restored = Session::restore(&records, Box::new(|_| {})).unwrap();
        assert_eq!(restored.contexts(), Some(&["entrevista".to_string()][..]));
    }

    #[test]
    fn a_live_speaker_is_split_by_voice_and_guesses_stay_cleared() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Call", "meeting", LIVE, "pt", sink);
        let first = session.hear_at("Eles", "Oi.", 10.0);
        session.hear_at("Eu", "Olá.", 11.0);
        let third = session.hear_at("Eles", "Tudo bem?", 12.0);
        let labels = [
            (first.at, "Speaker 1".to_string()),
            (third.at, "Speaker 2".to_string()),
        ];
        let who = session.split_speaker("Eles", &labels);
        assert_eq!(who, ["Speaker 1", "Eu", "Speaker 2"]);
        let logged = records.lock().unwrap().len();
        // The same labels again change nothing and record nothing.
        session.split_speaker("Speaker 1", &[(first.at, "Speaker 1".to_string())]);
        assert_eq!(records.lock().unwrap().len(), logged);
        session.dismiss_guess("Speaker 2", "ana");
        session.dismiss_guess("Speaker 2", "ana");
        let records = records.lock().unwrap().clone();
        let dismissals = records.iter().filter(|r| r["type"] == "guess_dismissed");
        assert_eq!(dismissals.count(), 1);
        let restored = Session::restore(&records, Box::new(|_| {})).unwrap();
        assert_eq!(restored.labels(), ["Speaker 1", "Eu", "Speaker 2"]);
        assert!(restored.dismissed("Speaker 2", "ana"));
        assert!(!restored.dismissed("Speaker 1", "ana"));
    }

    #[test]
    fn speakers_are_people_by_record() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Daily", "meeting", IMPORT, "pt", sink);
        session.hear_at("Speaker 1", "Bom dia.", now());
        session.hear_at("Speaker 2", "Oi.", now());
        let ana = Person::new("Ana");
        session.name_person("Speaker 2", Some(&ana));
        session.name_person("Speaker 2", Some(&ana)); // no change, no record
        assert_eq!(session.speakers(), ["Speaker 1", "Ana"]);
        assert_eq!(session.person_of("Speaker 2"), Some(ana.id.as_str()));
        assert_eq!(session.labels_of(&ana.id), ["Speaker 2"]);
        let records = records.lock().unwrap().clone();
        assert_eq!(
            types(&records).iter().filter(|t| **t == "person").count(),
            1
        );
        let mut restored = Session::restore(&records, Box::new(|_| {})).unwrap();
        assert_eq!(restored.person_of("Speaker 2"), Some(ana.id.as_str()));
        restored.name_person("Speaker 2", None);
        assert_eq!(restored.person_of("Speaker 2"), None);
        assert_eq!(restored.speakers(), ["Speaker 1", "Ana"]);
        assert_eq!(restored.labels(), ["Speaker 1", "Speaker 2"]);
    }

    #[test]
    fn an_edited_line_keeps_its_correction() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Call", "meeting", LIVE, "pt", sink);
        session.hear_at("Eles", "O WinkYou chegou.", 10.0);
        assert!(session.edit_line("Eles", 10.0, "O Windhawk chegou."));
        assert!(!session.edit_line("Eles", 99.0, "nada"));
        let records = records.lock().unwrap().clone();
        let restored = Session::restore(&records, Box::new(|_| {})).unwrap();
        assert!(matches!(
            &restored.timeline[0],
            Entry::Speech(line) if line.text == "O Windhawk chegou."
        ));
    }

    #[test]
    fn a_line_taken_back_stays_out() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Call", "meeting", LIVE, "pt", sink);
        session.hear_at("Eles", "Bom dia a todos.", 10.0);
        session.hear_at("Eu", "Bom dia a todos.", 11.0);
        session.unhear("Eu", 11.0);
        session.unhear("Eu", 11.0); // already gone: no record
        let records = records.lock().unwrap().clone();
        assert_eq!(
            types(&records).iter().filter(|t| **t == "unheard").count(),
            1
        );
        let restored = Session::restore(&records, Box::new(|_| {})).unwrap();
        assert_eq!(restored.speakers(), ["Eles"]);
    }

    #[test]
    fn an_unreadable_log_is_an_error_not_a_panic() {
        let head = as_record(json!({"type": "session"}));
        assert!(Session::restore(&[head], Box::new(|_| {})).is_err());
        let old = as_record(
            json!({"type": "note", "id": "n1", "title": "old", "kind": "idea", "source": "live", "language": "pt", "started_at": 1.0}),
        );
        assert!(Session::restore(&[old], Box::new(|_| {})).is_err());
        assert!(summarize(&[], no_prices).is_err());
    }

    #[test]
    fn assigning_one_line_keeps_other_lines_and_survives_restore() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Call", "meeting", LIVE, "pt", sink);
        session.hear_at("Eles", "Primeira fala", 10.0);
        session.hear_at("Eles", "Segunda fala", 11.0);
        let ij = Person::new("Ij");
        let (label, retired) = session.assign_line("Eles", 10.0, &ij).unwrap();
        assert!(!retired);
        assert!(label.starts_with("Eles#"));
        assert_eq!(session.labels(), [label.as_str(), "Eles"]);
        assert_eq!(session.person_of(&label), Some(ij.id.as_str()));
        assert_eq!(session.person_of("Eles"), None);
        assert!(session.assign_line("Eles", 99.0, &ij).is_none());
        let saved = records.lock().unwrap().clone();
        assert_eq!(
            types(&saved)
                .iter()
                .filter(|type_| **type_ == "line_person")
                .count(),
            1
        );
        let mut restored = Session::restore(&saved, Box::new(|_| {})).unwrap();
        assert_eq!(restored.labels(), [label.as_str(), "Eles"]);
        assert_eq!(restored.speakers(), ["Ij", "Eles"]);
        let context = restored.context(1000);
        assert!(matches!(&context[0], Entry::Speech(speech) if speech.who == "Ij"));
        assert!(matches!(&context[1], Entry::Speech(speech) if speech.who == "Eles"));
    }

    #[test]
    fn assigning_a_speakers_only_line_retires_its_label() {
        let (sink, _) = collecting();
        let mut session = Session::begin("Call", "meeting", LIVE, "pt", sink);
        session.hear_at("Eles", "Única fala", 10.0);
        session.set_speaker_color("Eles", "#ffb000");
        let ij = Person::new("Ij");
        let (label, retired) = session.assign_line("Eles", 10.0, &ij).unwrap();
        assert!(retired);
        assert_eq!(session.labels(), [label.as_str()]);
        assert_eq!(session.color_of("Eles"), "");
        let ana = Person::new("Ana");
        let (again, retired) = session.assign_line(&label, 10.0, &ana).unwrap();
        assert_eq!((again.as_str(), retired), (label.as_str(), false));
        assert_eq!(session.person_of(&label), Some(ana.id.as_str()));
    }

    #[test]
    fn a_language_change_and_a_cleared_color_survive_restore() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Talk", "meeting", LIVE, "pt", sink);
        session.hear_at("Speaker 1", "Hello.", now());
        session.set_language("en");
        session.set_language("en");
        session.set_speaker_color("Speaker 1", "#ffb000");
        session.set_speaker_color("Speaker 1", "");
        let records = records.lock().unwrap().clone();
        assert_eq!(
            types(&records).iter().filter(|t| **t == "language").count(),
            1
        );
        let restored = Session::restore(&records, Box::new(|_| {})).unwrap();
        assert_eq!(restored.language, "en");
        assert_eq!(restored.color_of("Speaker 1"), "");
    }

    #[test]
    fn notes_are_passed_over_by_speaker_and_translation_work() {
        let (sink, _) = collecting();
        let mut session = Session::begin("Call", "meeting", LIVE, "pt", sink);
        session.hear_at("Eles", "Oi.", 10.0);
        session.note("Lembrar.");
        session.hear_at("Eles", "Tchau.", 11.0);
        session.assign_speakers(&["A".into(), "B".into()]);
        assert_eq!(session.labels(), ["A", "B"]);
        assert_eq!(
            session.split_speaker("A", &[(10.0, "C".into())]),
            ["C", "B"]
        );
        let untranslated: Vec<String> = session
            .untranslated("en")
            .into_iter()
            .map(|(_, text)| text)
            .collect();
        assert_eq!(untranslated, ["Oi.", "Tchau."]);
        assert!(!session.translated(Source::Line(99.0), "en", "Hi."));
    }

    #[test]
    fn an_answer_is_found_by_its_question() {
        let (sink, _) = collecting();
        let mut session = Session::begin("Call", "meeting", LIVE, "pt", sink);
        let answer = session.suggest("ask", "m", "Qual é o prazo?", "Pedido");
        session.finish(&answer.id);
        assert!(session.mentions(&folded("prazo")));
        assert!(!session.mentions(&folded("orçamento")));
    }

    #[test]
    fn a_log_names_what_it_cannot_apply_and_skips_what_it_does_not_know() {
        let records = logged(&[
            json!({"type": "speech", "who": "Eles", "text": "Oi.", "at": 101.0}),
            json!({"type": "line_person", "who": "Eles", "at": 999.0, "label": "Eles#1",
                "person": "p", "name": "Ana"}),
            json!({"type": "from_a_later_eco", "at": 102.0}),
        ]);
        let restored = Session::restore(&records, Box::new(|_| {})).unwrap();
        assert_eq!(restored.labels(), ["Eles"]);
        assert_eq!(restored.person_of("Eles#1"), None);
    }

    #[test]
    fn an_unrecorded_transcriber_is_never_written() {
        let (sink, records) = collecting();
        let mut session = Session::begin("Call", "meeting", LIVE, "pt", sink);
        session.transcribe_with(HeardBy::Nothing);
        session.transcribe_with(HeardBy::Unrecorded);
        let records = records.lock().unwrap().clone();
        assert_eq!(
            types(&records)
                .iter()
                .filter(|t| **t == "transcriber")
                .count(),
            1
        );
    }
}
