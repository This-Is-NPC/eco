//! Runs the session: its lifecycle, what it hears, and actions over its timeline.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use futures::StreamExt;
use serde_json::{Value, json};
use tokio::sync::{Semaphore, watch};
use tokio::task::AbortHandle;
use tokio::time::Instant;

use crate::domain::action::Action;
use crate::domain::billing::{Joined, Requests, priced, shares};
use crate::domain::channel::Utterance;
use crate::domain::diarization::{Diarization, normalized, speaker_of};
use crate::domain::echo;
use crate::domain::events::{Event, error};
use crate::domain::people::{GUESSING, Match, Person, SUGGESTING, ranked};
use crate::domain::prompts::{
    LANGUAGES, action_request, conversation, question_request, review, translation,
};
use crate::domain::session::{
    ENDED, Entry, HeardBy, INTERRUPTED, LIVE, LogError, Note, PAUSED, RECORDING, Rate, Session,
    Source, Speech, Suggestion, Unclosed, folded, now, same_tag, summarize, tag_counts, tag_name,
};
use crate::domain::transcribers::Listening;
use crate::ports::{
    Chunk, Hooks, LanguageModel, PeopleStore, Record, SessionLog, TranscriptionBilling, Usage,
};

pub type Emit = Arc<dyn Fn(Event) + Send + Sync>;

/// How many translations run at once.
const TRANSLATIONS_AT_ONCE: usize = 3;

/// How many stored sessions stay in memory once read or asked about; past it,
/// the least recently used idle one leaves, its log still holding all of it.
const LOADED_STORED: usize = 8;

/// A named slot of the user's context, read from its files.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextSlot {
    pub name: String,
    /// Sessions of these kinds have it on until the user chooses.
    pub kinds: Vec<String>,
    pub text: String,
}

impl Setup {
    /// The hook of `action`, and whether it runs on its own; `None` when it has none.
    fn hook_of(&self, action: &str) -> Option<(&str, bool)> {
        self.actions
            .iter()
            .find(|a| a.name == action && !a.hook.trim().is_empty())
            .map(|a| (a.hook.as_str(), a.hook_auto))
    }

    /// The model that answers `action` in a session of `kind`: the one the action
    /// names, then the kind's own, then the assistant's.
    fn model_for(&self, kind: &str, action: &str) -> &Model {
        self.actions
            .iter()
            .find(|a| a.name == action)
            .and_then(|a| a.model.as_ref())
            .and_then(|name| self.models.get(name))
            .or_else(|| self.kind_models.get(kind))
            .unwrap_or(&self.model)
    }

    /// What a session listens with: its kind's transcription model, in its language.
    fn listening_of(&self, session: &Session) -> Listening {
        let model = self.transcription.get(&session.kind);
        Listening {
            model: model.unwrap_or(&self.default_transcription).clone(),
            language: session.language.clone(),
        }
    }

    /// Whether `other` answers `action` in a session of `kind` as this setup does:
    /// the same model and the same reviewer.
    fn answers_alike(&self, other: &Setup, kind: &str, action: &str) -> bool {
        let reviewer = |setup: &Setup| {
            let reviewer = setup.reviewer.as_ref()?;
            let model = setup.kind_models.get(kind).unwrap_or(&reviewer.model);
            Some((
                model.settings.clone(),
                reviewer.prompt.clone(),
                reviewer.verbose,
            ))
        };
        self.model_for(kind, action).settings == other.model_for(kind, action).settings
            && reviewer(self) == reviewer(other)
    }

    /// The context slots a session has on: the ones the user chose, or those of its kind.
    pub fn slots_of<'a>(&'a self, session: &Session) -> Vec<&'a ContextSlot> {
        let on = |slot: &ContextSlot| match session.contexts() {
            Some(chosen) => chosen.contains(&slot.name),
            None => slot.kinds.contains(&session.kind),
        };
        self.slots.iter().filter(|slot| on(slot)).collect()
    }

    /// The user's context for a session: the global one, then each slot it has
    /// on under its name.
    fn context_of(&self, session: &Session) -> String {
        let slots = self.slots_of(session);
        let mut parts: Vec<String> = Vec::new();
        if !self.context.trim().is_empty() {
            parts.push(self.context.trim().to_string());
        }
        for slot in slots {
            parts.push(format!("## {}\n{}", slot.name, slot.text.trim()));
        }
        parts.join("\n\n")
    }
}

/// What translates a session's lines and answers, below each one.
pub struct Translating {
    pub model: Model,
    /// The translator of each session kind that has its own.
    pub kind_models: HashMap<String, Model>,
}

impl Setup {
    fn translator(&self, kind: &str) -> &Model {
        self.translation
            .kind_models
            .get(kind)
            .unwrap_or(&self.translation.model)
    }
}

/// A chat model ready to stream: its provider and the provider's id for it.
#[derive(Clone)]
pub struct Model {
    pub llm: Arc<dyn LanguageModel>,
    pub id: String,
    /// What it was built from, so a new setup tells whether it changed.
    pub settings: String,
}

/// A second model every answer passes through before the user sees it.
#[derive(Clone)]
pub struct Reviewer {
    pub model: Model,
    pub prompt: String,
    /// The draft it rewrote is shown and kept, to inspect.
    pub verbose: bool,
}

type Chunks = futures::stream::BoxStream<'static, Result<Chunk, crate::ports::CompletionError>>;

/// What a configuration gives the assistant; replaced whenever the user saves.
pub struct Setup {
    /// The assistant's model: it answers questions and every action that names none.
    pub model: Model,
    /// The models actions name, by name.
    pub models: HashMap<String, Model>,
    /// The chat model of each session kind that has one: in those sessions it
    /// answers questions, actions that name no model, and the reviewer's pass.
    pub kind_models: HashMap<String, Model>,
    /// What each kind uses, by model name, for clients: `{kind: {"transcription", "chat"}}`.
    pub routes: Value,
    pub translation: Translating,
    pub actions: Vec<Action>,
    /// The rules every answer follows unless its request says otherwise.
    pub rules: String,
    /// Rewrites every answer before it is shown, when on.
    pub reviewer: Option<Reviewer>,
    /// The user's global context, sent with every request.
    pub context: String,
    /// Named context slots, sent while a session has them on.
    pub slots: Vec<ContextSlot>,
    /// Characters of transcript and earlier answers a request may carry.
    pub max_context_chars: usize,
    /// (name, is the user)
    pub participants: Vec<(String, bool)>,
    /// What is being captured: {"id", "label", "participant", "color"} per audio input.
    pub inputs: Vec<Value>,
    /// Transcription language code, or "auto", and the codes the user can pick.
    pub language: String,
    pub languages: Vec<String>,
    /// The kinds a session can be given.
    pub kinds: Vec<String>,
    /// The user's lines that repeat the others' are dropped as leaked audio.
    pub drop_echoes: bool,
    /// Interface language pack, or "auto" to follow the system.
    pub ui_language: String,
    /// The transcription model of each session kind, by name, and of any other.
    pub transcription: HashMap<String, String>,
    pub default_transcription: String,
    /// USD per minute of audio, of each transcription model the user gave a
    /// price, by name: it stands in where a provider reports no cost.
    pub transcription_prices: HashMap<String, f64>,
    /// What tells the cost of each transcription model's requests, for the
    /// models whose provider reports it, by name.
    pub transcription_billing: HashMap<String, Arc<dyn TranscriptionBilling>>,
    /// The parts that could not be set up, as errors every client is told of.
    pub problems: Vec<Event>,
    /// How many answers stream at once, across sessions.
    pub limit: Arc<Semaphore>,
}

/// A line of speech: `who` is the label its speaker carries, `name` what the
/// speaker goes by in this session.
fn speech_event(session: &Session, speech: &Speech, latency: Duration) -> Event {
    json!({
        "type": "transcript", "session": session.id, "who": speech.who, "name": session.name_of(&speech.who),
        "text": speech.text, "at": speech.at, "latency_ms": millis(latency),
    })
}

/// The person the voice of the speaker `label` is closest to, close enough to
/// guess, unless the user cleared that guess.
fn guess(session: &Session, label: &str, ranking: &[Match]) -> Option<Match> {
    ranking
        .iter()
        .take_while(|m| m.score >= GUESSING)
        .find(|m| !session.dismissed(label, &m.person))
        .cloned()
}

fn rounded(score: f32) -> f64 {
    (f64::from(score) * 100.0).round() / 100.0
}

/// The number after the highest `Speaker N` among `labels`.
fn next_speaker<'a>(labels: impl Iterator<Item = &'a String>) -> usize {
    let numbers = labels.filter_map(|label| label.strip_prefix("Speaker ")?.parse::<usize>().ok());
    numbers.max().unwrap_or(0) + 1
}

fn millis(duration: Duration) -> u64 {
    (duration.as_secs_f64() * 1000.0).round() as u64
}

/// What clients hear when the entry `id` leaves a session: a note or an answer.
fn removed_event(entry: Option<&Entry>, id: &str) -> Event {
    let kind = match entry {
        Some(Entry::Note(_)) => "note_removed",
        _ => "suggestion_removed",
    };
    json!({"type": kind, "id": id})
}

fn note_event(note: &Note) -> Event {
    json!({"type": "note", "id": note.id, "text": note.text, "at": note.at})
}

fn suggestion_event(s: &Suggestion) -> Event {
    json!({
        "type": "suggestion", "id": s.id, "action": s.action, "model": s.model, "prompt": s.prompt,
        "text": s.text, "draft": s.draft, "done": s.done, "at": s.at,
    })
}

/// A session's timeline as events, each line and answer with its translation into
/// the session's translation language when there is one.
pub fn timeline_events(session: &Session) -> Vec<Event> {
    let language = session.translation();
    let translated = |of: Source| language.and_then(|l| session.translation_of(&of, l));
    session
        .timeline
        .iter()
        .map(|entry| {
            let (mut event, of) = match entry {
                Entry::Speech(speech) => (
                    speech_event(session, speech, Duration::ZERO),
                    Some(Source::Line(speech.at)),
                ),
                Entry::Note(note) => (note_event(note), None),
                Entry::Suggestion(suggestion) => (
                    suggestion_event(suggestion),
                    Some(Source::Answer(suggestion.id.clone())),
                ),
            };
            if let Some(text) = of.and_then(translated) {
                event["translation"] = json!(text);
            }
            event
        })
        .collect()
}

/// A line or an answer of a session, translated into `language` in `took`.
fn translated_event(
    session_id: &str,
    of: &Source,
    language: &str,
    text: &str,
    took: Duration,
    cost: Option<f64>,
) -> Event {
    let mut event = json!({"type": "translated", "session": session_id, "language": language,
                           "text": text, "ms": millis(took), "cost_usd": cost});
    for (field, value) in of.fields().as_object().expect("an object") {
        event[field] = value.clone();
    }
    event
}

/// A `translated` event for each line and answer a session already holds in `language`.
fn kept_translations(session: &Session, language: &str) -> Vec<Event> {
    let mut told = Vec::new();
    for entry in &session.timeline {
        let of = match entry {
            Entry::Speech(speech) => Source::Line(speech.at),
            Entry::Suggestion(answer) => Source::Answer(answer.id.clone()),
            Entry::Note(_) => continue,
        };
        if let Some(text) = session.translation_of(&of, language) {
            told.push(translated_event(
                &session.id,
                &of,
                language,
                text,
                Duration::ZERO,
                None,
            ));
        }
    }
    told
}

#[derive(Default)]
struct State {
    setup: Option<Arc<Setup>>,
    /// The sessions in memory by id: the live ones and stored ones read or asked about.
    loaded: HashMap<String, Session>,
    /// The live sessions, recording or paused, in `loaded`; the last is the one
    /// shown, which commands without a session address.
    live: Vec<String>,
    /// The loaded stored sessions, least recently used first; never a live one.
    used: VecDeque<String>,
    /// Each loaded session's answers, by session id.
    answering: HashMap<String, Queue>,
    /// The transcription requests open now, and the sessions each hears.
    requests: Requests,
}

/// A session's answers: one streams at a time, so each sees the ones before it.
#[derive(Default)]
struct Queue {
    /// The answer streaming now, and how to stop it.
    running: Option<(String, AbortHandle)>,
    /// The answer asked for while another streams; it starts when that one ends.
    waiting: Option<String>,
}

impl State {
    /// The live session shown.
    fn session(&self) -> Option<&Session> {
        self.live.last().map(|id| &self.loaded[id])
    }

    /// The live session `id`, or the one shown when it is none.
    fn live_one(&self, id: Option<&str>) -> Option<String> {
        match id {
            Some(id) => self.is_live(id).then(|| id.to_string()),
            None => self.live.last().cloned(),
        }
    }

    fn live(&self) -> impl Iterator<Item = &Session> {
        self.live.iter().map(|id| &self.loaded[id])
    }

    /// The sessions recording now.
    fn recording(&self) -> impl Iterator<Item = &Session> {
        self.live().filter(|session| session.state == RECORDING)
    }

    fn is_live(&self, id: &str) -> bool {
        self.live.iter().any(|live| live == id)
    }

    /// The loaded session `id`, made the most recently used.
    fn pick(&mut self, id: &str) -> &mut Session {
        if let Some(at) = self.used.iter().position(|used| used == id) {
            let id = self.used.remove(at).expect("found above");
            self.used.push_back(id);
        }
        self.loaded
            .get_mut(id)
            .expect("a target names a loaded session")
    }

    /// The loaded session holding a suggestion.
    fn holder(&mut self, id: &str) -> Option<&mut Session> {
        self.loaded
            .values_mut()
            .find(|session| session.timeline.iter().any(|e| e.id() == Some(id)))
    }

    /// Whether an answer of the session `id` streams or waits.
    fn busy(&self, id: &str) -> bool {
        self.answering.contains_key(id)
    }

    /// The session whose queue holds the answer `id`, streaming or waiting.
    fn queued(&self, id: &str) -> Option<String> {
        self.answering
            .iter()
            .find(|(_, queue)| {
                queue.waiting.as_deref() == Some(id)
                    || queue
                        .running
                        .as_ref()
                        .is_some_and(|(running, _)| running == id)
            })
            .map(|(session, _)| session.clone())
    }

    /// The answers of the session `id` in its queue, the waiting one first.
    fn answers_of(&self, id: &str) -> Vec<String> {
        let Some(queue) = self.answering.get(id) else {
            return Vec::new();
        };
        let running = queue.running.as_ref().map(|(id, _)| id.clone());
        queue.waiting.iter().cloned().chain(running).collect()
    }
}

/// Each session streams one suggestion at a time; a new action waits for it, and
/// a newer one replaces the one waiting. Sessions stream side by side, as many at
/// once as the setup's `limit` lets. Capture should
/// run while `listening` names what the recording sessions listen with.
#[derive(Clone)]
pub struct Assistant {
    inner: Arc<Inner>,
}

struct Inner {
    emit: Emit,
    log: Arc<dyn SessionLog>,
    people: Arc<dyn PeopleStore>,
    hooks: Arc<dyn Hooks>,
    state: Mutex<State>,
    /// Translations run a few at a time, apart from everything else.
    translating: Arc<Semaphore>,
    /// The translations asked for and not yet back, so none is asked twice.
    pending: Mutex<HashSet<String>>,
    /// What the recording sessions listen with; empty while none records.
    listening: watch::Sender<BTreeSet<Listening>>,
    /// The transcription requests whose cost is being asked for.
    pricing: Mutex<HashSet<String>>,
}

impl Assistant {
    pub fn new(
        emit: Emit,
        log: Arc<dyn SessionLog>,
        people: Arc<dyn PeopleStore>,
        hooks: Arc<dyn Hooks>,
    ) -> Self {
        let (listening, _) = watch::channel(BTreeSet::new());
        Self {
            inner: Arc::new(Inner {
                emit,
                log,
                people,
                hooks,
                state: Mutex::new(State::default()),
                translating: Arc::new(Semaphore::new(TRANSLATIONS_AT_ONCE)),
                pending: Mutex::default(),
                listening,
                pricing: Mutex::default(),
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.inner
            .state
            .lock()
            .expect("assistant state is never poisoned")
    }

    fn emit(&self, event: Event) {
        (self.inner.emit)(event);
    }

    /// What the recording sessions listen with; changes as sessions start, pause
    /// and end, and as the setup changes their transcription models.
    pub fn listening(&self) -> watch::Receiver<BTreeSet<Listening>> {
        self.inner.listening.subscribe()
    }

    /// Adopt a new setup; an answer streaming goes only when the new setup would
    /// answer it with another model or reviewer.
    pub fn configure(&self, setup: Arc<Setup>) {
        let mut state = self.state();
        let old = state.setup.replace(Arc::clone(&setup));
        self.listen(&mut state);
        self.price_unpaid_in(&mut state);
        let Some(old) = old else {
            return;
        };
        let changed: Vec<String> = state
            .answering
            .values()
            .filter_map(|queue| queue.running.as_ref().map(|(id, _)| id))
            .filter(|id| {
                let session = state
                    .loaded
                    .values()
                    .find_map(|session| suggestion_in(session, id).map(|answer| (session, answer)));
                session.is_none_or(|(session, answer)| {
                    !old.answers_alike(&setup, &session.kind, &answer.action)
                })
            })
            .cloned()
            .collect();
        for id in changed {
            self.remove_from(&mut state, &id);
        }
    }

    /// Forget `setup` when it is still the current one: its clients are closing.
    pub fn release(&self, setup: &Arc<Setup>) {
        let mut state = self.state();
        if state
            .setup
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, setup))
        {
            self.cancel_running(&mut state);
            state.setup = None;
            self.listen(&mut state);
        }
    }

    // Session lifecycle.

    /// Start a live session, recording beside any other, and show it; `contexts`
    /// are the slots to turn on, or `None` for those of its kind. Returns its id.
    pub fn start(
        &self,
        title: &str,
        kind: &str,
        language: &str,
        contexts: Option<&[String]>,
    ) -> String {
        let mut state = self.state();
        let mut session =
            Session::begin(title.trim(), kind, LIVE, language, self.inner.log.writer());
        if let Some(contexts) = contexts {
            session.set_contexts(contexts);
        }
        let id = session.id.clone();
        state.live.push(id.clone());
        state.loaded.insert(id.clone(), session);
        self.announce(&mut state);
        id
    }

    /// Turn on exactly the context slots `slots` in a session — open, read back or
    /// stored; clients are told which are on now.
    pub fn set_contexts(&self, session_id: &str, slots: &[String]) {
        let changed = {
            let mut state = self.state();
            self.edit_session(&mut state, session_id, |session| {
                session.set_contexts(slots)
            })
        };
        match changed {
            Some(()) => self
                .emit(json!({"type": "session_context", "session": session_id, "contexts": slots})),
            None => self.not_found(session_id),
        }
    }

    /// Transcribe a session in `language` — one the setup offers — from now on;
    /// while it records, its transcriber changes and no other session's does.
    pub fn set_language(&self, session_id: &str, language: &str) {
        let mut state = self.state();
        let offered = state
            .setup
            .as_ref()
            .is_some_and(|setup| setup.languages.iter().any(|code| code == language));
        if !offered {
            return self.emit(error(
                "session.invalid",
                format!("session: no language {language:?}"),
                json!({"detail": format!("no language {language:?}")}),
            ));
        }
        match self.edit_session(&mut state, session_id, |session| {
            session.set_language(language)
        }) {
            Some(()) if state.is_live(session_id) => self.announce(&mut state),
            Some(()) => {}
            None => self.not_found(session_id),
        }
    }

    /// Resume a paused or ended stored session from the log, recording beside any
    /// other, and show it.
    pub fn reopen(&self, session_id: &str) {
        let mut state = self.state();
        if state.is_live(session_id) {
            drop(state);
            return self.focus(session_id);
        }
        if let Err(event) = self.load(&mut state, session_id) {
            return self.emit(event);
        }
        let session = &state.loaded[session_id];
        if session.state != PAUSED && session.state != ENDED {
            self.emit(error(
                "session.not_resumable",
                "only a paused or ended session can be resumed",
                Value::Null,
            ));
            return;
        }
        state.used.retain(|id| id != session_id);
        state.live.push(session_id.to_string());
        state.pick(session_id).set_state(RECORDING);
        self.announce(&mut state);
    }

    /// The live sessions, the one shown last.
    pub fn live(&self) -> Vec<String> {
        self.state().live.clone()
    }

    /// Show the live session `session_id`: commands without a session address it
    /// from now on.
    pub fn focus(&self, session_id: &str) {
        let mut state = self.state();
        let Some(at) = state.live.iter().position(|id| id == session_id) else {
            return self.not_found(session_id);
        };
        let id = state.live.remove(at);
        state.live.push(id);
        self.announce(&mut state);
    }

    /// Announce the timeline of the live session `session_id`, for a window that
    /// shows it.
    pub fn timeline(&self, session_id: &str) {
        let state = self.state();
        if !state.is_live(session_id) {
            return self.not_found(session_id);
        }
        self.emit(json!({
            "type": "session_timeline", "session": session_id,
            "timeline": timeline_events(&state.loaded[session_id]),
        }));
    }

    /// A stored session's summary, read now: one left open that no daemon holds
    /// is interrupted; its transcription costs what the setup's prices say.
    fn summary_of(&self, records: &[Record]) -> Result<Value, LogError> {
        let state = self.state();
        let setup = state.setup.as_deref();
        let mut summary = summarize(records, |model| rate_in(setup, model))?;
        let open = matches!(summary["state"].as_str(), Some(RECORDING | PAUSED));
        if open && !summary["id"].as_str().is_some_and(|id| state.is_live(id)) {
            summary["state"] = json!(INTERRUPTED);
        }
        Ok(summary)
    }

    /// Announce every stored session, newest first; an unreadable log is skipped, not fatal.
    pub fn sessions(&self) {
        let (found, unreadable): (Vec<_>, Vec<_>) = self
            .inner
            .log
            .all()
            .iter()
            .map(|records| self.summary_of(records))
            .partition(Result::is_ok);
        let found: Vec<Value> = found.into_iter().map(Result::unwrap).collect();
        self.emit(json!({"type": "sessions", "sessions": found}));
        if !unreadable.is_empty() {
            let count = unreadable.len();
            let message = format!("{count} unreadable session log(s) skipped");
            self.emit(error(
                "session.unreadable",
                message,
                json!({"count": count}),
            ));
        }
    }

    /// Announce the ids of the stored sessions whose title, lines, notes, questions or
    /// answers hold `query`, ignoring case and accents; newest first.
    pub fn search(&self, query: &str) {
        let needle = folded(query.trim());
        let ids: Vec<String> = self
            .inner
            .log
            .all()
            .iter()
            .filter_map(|records| Session::restore(records, Box::new(|_| {})).ok())
            .filter(|session| session.mentions(&needle))
            .map(|session| session.id)
            .collect();
        self.emit(json!({"type": "sessions_found", "query": query, "ids": ids}));
    }

    /// Announce one stored session with its timeline, to read it back. What is
    /// still untranslated is translated now, when the session translates.
    pub fn show(&self, session_id: &str) {
        let setup = self.state().setup.clone();
        let summary = self.inner.log.read(session_id).and_then(|records| {
            let session = Session::restore(&records, Box::new(|_| {})).ok()?;
            let speakers = self.speakers_of(&session);
            let mut summary = self.summary_of(&records).ok()?;
            let storage = self.inner.log.storage(session_id)?;
            summary["path"] = json!(storage.path);
            summary["bytes"] = json!(storage.bytes);
            let timeline = timeline_events(&session);
            let missing = session.translation().map(|language| {
                let lines = session.untranslated(language);
                (session.kind.clone(), language.to_string(), lines)
            });
            Some((summary, timeline, speakers, missing))
        });
        match summary {
            Some((summary, timeline, speakers, missing)) => {
                self.emit(json!({
                    "type": "session_detail", "session": summary, "timeline": timeline, "speakers": speakers,
                }));
                if let (Some(setup), Some((kind, language, lines))) = (setup, missing) {
                    for (of, text) in lines {
                        self.translate(&setup, session_id, &kind, &language, of, text);
                    }
                }
            }
            None => self.emit(error(
                "session.not_found",
                format!("no session {session_id:?}"),
                json!({"id": session_id}),
            )),
        }
    }

    pub fn delete_session(&self, session_id: &str) {
        if session_id.len() != 12
            || !session_id
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        {
            return self.emit(error(
                "session.invalid",
                "invalid session id",
                json!({"id": session_id}),
            ));
        }
        if self.state().is_live(session_id) {
            return self.emit(error(
                "session.live",
                "end the session before deleting it",
                json!({"id": session_id}),
            ));
        }
        match self.inner.log.delete(session_id) {
            Ok(true) => {}
            Ok(false) => return self.not_found(session_id),
            Err(failure) => return self.store_failed(&failure),
        }
        {
            let mut state = self.state();
            if state.loaded.contains_key(session_id) {
                self.unload(&mut state, session_id);
            }
        }
        if let Err(failure) = self
            .inner
            .people
            .keep_voices(session_id, &Default::default())
        {
            self.store_failed(&failure);
        }
        for mut person in self.inner.people.people() {
            let count = person.voiceprints.len();
            person
                .voiceprints
                .retain(|voice| voice.session != session_id);
            if person.voiceprints.len() != count
                && let Err(failure) = self.inner.people.save(&person)
            {
                self.store_failed(&failure);
            }
        }
        self.emit(json!({"type": "session_deleted", "id": session_id}));
        self.sessions();
        self.people();
    }

    /// A session's speech — open, loaded or stored — as (seconds from its start,
    /// who, text), in order.
    pub fn transcript(&self, session_id: &str) -> Option<Vec<(f64, String, String)>> {
        let lines = |session: &Session| {
            let speech = session.timeline.iter().filter_map(|entry| match entry {
                Entry::Speech(s) => {
                    let name = session.name_of(&s.who).to_string();
                    Some((s.at - session.started_at, name, s.text.clone()))
                }
                Entry::Note(_) | Entry::Suggestion(_) => None,
            });
            speech.collect()
        };
        {
            let state = self.state();
            if let Some(session) = state.loaded.get(session_id) {
                return Some(lines(session));
            }
        }
        let records = self.inner.log.read(session_id)?;
        Session::restore(&records, Box::new(|_| {}))
            .ok()
            .map(|session| lines(&session))
    }

    /// Apply `change` to a session — the open one, the one read back, or a stored
    /// one, whose log it appends to; `None` when there is no such session.
    fn edit_session<R>(
        &self,
        state: &mut State,
        session_id: &str,
        change: impl FnOnce(&mut Session) -> R,
    ) -> Option<R> {
        if let Some(session) = state.loaded.get_mut(session_id) {
            return Some(change(session));
        }
        let log = &self.inner.log;
        let records = log.read(session_id)?;
        let mut session = Session::restore(&records, log.append_to(session_id)?).ok()?;
        Some(change(&mut session))
    }

    fn not_found(&self, session_id: &str) {
        self.emit(error(
            "session.not_found",
            format!("no session {session_id:?}"),
            json!({"id": session_id}),
        ));
    }

    /// Tell clients what the speaker `label` of a session goes by now.
    fn speaker_renamed(&self, session_id: &str, label: &str, name: &str) {
        self.emit(
            json!({"type": "speaker_renamed", "session": session_id, "label": label, "name": name}),
        );
    }

    // People.

    fn store_failed(&self, failure: &crate::ports::StoreError) {
        let detail = &failure.0;
        self.emit(error(
            "people.failed",
            format!("people: {detail}"),
            json!({"detail": detail}),
        ));
    }

    fn save_person(&self, person: &Person) {
        if let Err(failure) = self.inner.people.save(person) {
            self.store_failed(&failure);
        }
    }

    /// The speakers of a session in the order they first speak: label, name, the
    /// person they are, whether their voice is kept, the people a speaker with a
    /// voice and no person may be, and the one eco guesses they are.
    fn speakers_of(&self, session: &Session) -> Vec<Value> {
        let voices = self.inner.people.voices(&session.id);
        let everyone = self.inner.people.people();
        let name_of = |id: &str| everyone.iter().find(|p| p.id == id).map(|p| p.name.clone());
        let described = |m: &Match| json!({"person": m.person, "name": name_of(&m.person), "score": rounded(m.score)});
        session
            .labels()
            .iter()
            .map(|label| {
                let person = session.person_of(label);
                let voice = voices.get(label).filter(|_| person.is_none());
                let ranking = voice
                    .map(|voice| ranked(&everyone, voice))
                    .unwrap_or_default();
                let suggestions: Vec<Value> = ranking
                    .iter()
                    .filter(|m| m.score >= SUGGESTING)
                    .take(3)
                    .map(described)
                    .collect();
                let guess = guess(session, label, &ranking);
                json!({
                    "label": label, "name": session.name_of(label), "person": person,
                    "color": session.color_of(label), "voice": voices.contains_key(label),
                    "suggestions": suggestions, "guess": guess.as_ref().map(described),
                })
            })
            .collect()
    }

    /// The speaker `label` of a session is not the person their voice suggests:
    /// the guess goes, and is not made again for them.
    pub fn clear_guess(&self, session_id: &str, label: &str) {
        let voices = self.inner.people.voices(session_id);
        let everyone = self.inner.people.people();
        let cleared = {
            let mut state = self.state();
            self.edit_session(&mut state, session_id, |session| {
                let voice = voices
                    .get(label)
                    .filter(|_| session.person_of(label).is_none());
                let ranking = voice.map(|voice| ranked(&everyone, voice));
                let guessed = guess(session, label, &ranking.unwrap_or_default());
                if let Some(guessed) = &guessed {
                    session.dismiss_guess(label, &guessed.person);
                }
                guessed.is_some()
            })
        };
        match cleared {
            Some(true) => self.announce_speakers(session_id),
            Some(false) => self.emit(error(
                "speaker.invalid",
                "the speaker has no guess to clear",
                json!({}),
            )),
            None => self.not_found(session_id),
        }
    }

    /// What diarization found in a live input heard as `who`: each line, by its
    /// time and its span in the input, goes to the voice that said most of it. A
    /// voice close to one the session already keeps takes that speaker's label,
    /// a single voice keeps `who`, and any other becomes a new `Speaker N`. The
    /// voices are kept, so the session's speakers can be guessed for the user to
    /// confirm; a speaker already identified adds the voice to their person's.
    pub fn voices_heard(
        &self,
        session_id: &str,
        who: &str,
        lines: &[(f64, f64, f64)],
        diarization: &Diarization,
    ) {
        let said: Vec<(f64, usize)> = lines
            .iter()
            .filter_map(|&(at, start, end)| {
                speaker_of(&diarization.turns, start, end).map(|speaker| (at, speaker))
            })
            .collect();
        let mut found: Vec<usize> = Vec::new();
        for &(_, speaker) in &said {
            if !found.contains(&speaker) {
                found.push(speaker);
            }
        }
        if found.is_empty() {
            return;
        }
        let mut state = self.state();
        let mut voices = self.inner.people.voices(session_id);
        let heard = self.edit_session(&mut state, session_id, |session| {
            let mut next = next_speaker(session.labels().iter().chain(voices.keys()));
            let mut labels: Vec<String> = Vec::new();
            for &speaker in &found {
                let voice = &diarization.voices[speaker];
                let kept = voices
                    .iter()
                    .filter(|(label, _)| !labels.contains(label))
                    .map(|(label, kept)| {
                        (
                            label,
                            kept.iter().zip(voice).map(|(a, b)| a * b).sum::<f32>(),
                        )
                    })
                    .filter(|(_, score)| *score >= GUESSING)
                    .max_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(label, _)| label.clone());
                let label = kept.unwrap_or_else(|| {
                    if found.len() == 1 && !voices.contains_key(who) {
                        who.to_string()
                    } else {
                        next += 1;
                        format!("Speaker {}", next - 1)
                    }
                });
                labels.push(label);
            }
            let pairs: Vec<(f64, String)> = said
                .iter()
                .map(|(at, speaker)| {
                    let index = found.iter().position(|s| s == speaker).expect("found");
                    (*at, labels[index].clone())
                })
                .collect();
            let relabelled = session.split_speaker(who, &pairs);
            let names: Vec<String> = relabelled
                .iter()
                .map(|label| session.name_of(label).to_string())
                .collect();
            let people: Vec<Option<String>> = labels
                .iter()
                .map(|label| session.person_of(label).map(String::from))
                .collect();
            (labels, people, relabelled, names)
        });
        let Some((labels, people, relabelled, names)) = heard else {
            return;
        };
        for ((label, person), &speaker) in labels.iter().zip(&people).zip(&found) {
            let voice = &diarization.voices[speaker];
            let merged = match voices.get(label) {
                Some(kept) => normalized(kept.iter().zip(voice).map(|(a, b)| a + b).collect()),
                None => voice.clone(),
            };
            if let Some(mut person) = person.as_deref().and_then(|id| self.person(id)) {
                person.enroll(session_id, label, merged.clone());
                self.save_person(&person);
            }
            voices.insert(label.clone(), merged);
        }
        if let Err(failure) = self.inner.people.keep_voices(session_id, &voices) {
            self.store_failed(&failure);
        }
        drop(state);
        self.emit(
            json!({"type": "diarized", "session": session_id, "who": relabelled, "names": names}),
        );
    }

    /// Tell clients who a session's speakers are now.
    pub fn announce_speakers(&self, session_id: &str) {
        let speakers = {
            let mut state = self.state();
            self.edit_session(&mut state, session_id, |session| self.speakers_of(session))
        };
        match speakers {
            Some(speakers) => self.emit(
                json!({"type": "session_speakers", "session": session_id, "speakers": speakers}),
            ),
            None => self.not_found(session_id),
        }
    }

    /// Tell clients what a session cost, with each charge.
    pub fn announce_cost(&self, session_id: &str) {
        let told = {
            let mut state = self.state();
            let setup = state.setup.clone();
            self.edit_session(&mut state, session_id, |session| {
                cost_event(setup.as_deref(), session)
            })
        };
        match told {
            Some(event) => self.emit(event),
            None => self.not_found(session_id),
        }
    }

    /// Every stored session linked to `person_id`, with any speaker labels.
    fn sessions_with(&self, person_id: &str) -> Vec<(String, Vec<String>)> {
        self.inner
            .log
            .all()
            .iter()
            .filter_map(|records| Session::restore(records, Box::new(|_| {})).ok())
            .filter(|session| session.person_ids().iter().any(|id| id == person_id))
            .map(|session| (session.id.clone(), session.labels_of(person_id)))
            .collect()
    }

    /// Announce everyone known: name, how many voices, the sessions they are in.
    pub fn people(&self) {
        let mut taking_part: Vec<(String, String)> = Vec::new();
        for records in self.inner.log.all() {
            if let Ok(session) = Session::restore(&records, Box::new(|_| {})) {
                for person in session.person_ids() {
                    taking_part.push((person, session.id.clone()));
                }
            }
        }
        let people: Vec<Value> = self
            .inner
            .people
            .people()
            .iter()
            .map(|p| {
                let sessions: Vec<&str> = taking_part
                    .iter()
                    .filter(|(person, _)| *person == p.id)
                    .map(|(_, session)| session.as_str())
                    .collect();
                json!({"id": p.id, "name": p.name, "color": p.color,
                    "voices": p.voiceprints.len(), "sessions": sessions})
            })
            .collect();
        self.emit(json!({"type": "people", "people": people}));
    }

    pub fn adopt_live_speakers(&self) {
        let pending: Vec<(String, String, String)> = self
            .inner
            .log
            .all()
            .iter()
            .filter_map(|records| Session::restore(records, Box::new(|_| {})).ok())
            .filter(|session| session.source == LIVE)
            .flat_map(|session| {
                session
                    .labels()
                    .into_iter()
                    .filter(|label| {
                        session.person_of(label).is_none() && session.name_of(label) != label
                    })
                    .map(|label| {
                        (
                            session.id.clone(),
                            label.clone(),
                            session.name_of(&label).to_string(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut count = 0;
        for (session, label, name) in pending {
            self.assign_person(&session, &label, None, &name, None);
            if self
                .inner
                .log
                .read(&session)
                .and_then(|records| Session::restore(&records, Box::new(|_| {})).ok())
                .is_some_and(|loaded| loaded.person_of(&label).is_some())
            {
                count += 1;
            }
        }
        self.emit(json!({"type": "people_adopted", "count": count}));
    }

    fn person(&self, person_id: &str) -> Option<Person> {
        let found = self
            .inner
            .people
            .people()
            .into_iter()
            .find(|p| p.id == person_id);
        if found.is_none() {
            self.emit(error(
                "person.not_found",
                format!("no person {person_id:?}"),
                json!({"id": person_id}),
            ));
        }
        found
    }

    fn chosen_person(&self, person_id: Option<&str>, name: &str) -> Option<Person> {
        match person_id {
            Some(id) => self.person(id),
            None if !name.trim().is_empty() => Some(
                self.inner
                    .people
                    .people()
                    .into_iter()
                    .find(|person| person.name == name.trim())
                    .unwrap_or_else(|| Person::new(name.trim())),
            ),
            None => {
                self.emit(error(
                    "person.invalid",
                    "a person id or name is required",
                    json!({}),
                ));
                None
            }
        }
    }

    pub fn set_attendee(
        &self,
        session_id: &str,
        person_id: Option<&str>,
        name: &str,
        present: bool,
    ) {
        if !present && person_id.is_none() {
            return self.emit(error("person.invalid", "person id required", json!({})));
        }
        let Some(person) = self.chosen_person(person_id, name) else {
            return;
        };
        let changed = {
            let mut state = self.state();
            self.edit_session(&mut state, session_id, |session| {
                let changed = session.has_attendee(&person.id) != present;
                session.set_attendee(&person.id, present);
                changed
            })
        };
        let Some(changed) = changed else {
            return self.not_found(session_id);
        };
        if present {
            self.save_person(&person);
        }
        self.announce_attendees(session_id);
        if changed {
            self.sessions();
        }
        self.people();
    }

    fn announce_attendees(&self, session_id: &str) {
        let from_state = {
            let state = self.state();
            state
                .loaded
                .get(session_id)
                .map(|session| (session.person_ids(), session.attendee_ids()))
        };
        let ids = from_state.or_else(|| {
            self.inner
                .log
                .read(session_id)
                .and_then(|records| Session::restore(&records, Box::new(|_| {})).ok())
                .map(|session| (session.person_ids(), session.attendee_ids()))
        });
        if let Some((people, attendees)) = ids {
            self.emit(json!({"type": "attendees_changed", "session": session_id,
                "people": people, "attendees": attendees}));
        }
    }

    /// Say the speaker `label` of a session is the person `person_id`, or the one
    /// called `name` (a new person when no one is); the speaker's voice joins theirs.
    pub fn assign_person(
        &self,
        session_id: &str,
        label: &str,
        person_id: Option<&str>,
        name: &str,
        color: Option<&str>,
    ) {
        self.assign_labels(
            session_id,
            Some(&[label.to_string()]),
            person_id,
            name,
            color,
        );
    }

    fn assign_labels(
        &self,
        session_id: &str,
        selected: Option<&[String]>,
        person_id: Option<&str>,
        name: &str,
        color: Option<&str>,
    ) -> Option<usize> {
        if let Some(color) = color
            && !color.is_empty()
            && !crate::config::valid_color(color)
        {
            self.emit(error("color.invalid", "invalid speaker color", json!({})));
            return None;
        }
        let mut person = self.chosen_person(person_id, name)?;
        if let Some(color) = color {
            person.color = color.to_ascii_lowercase();
        }
        let previous = {
            let mut state = self.state();
            self.edit_session(&mut state, session_id, |session| {
                let known = session.labels();
                let labels = selected.map_or_else(
                    || known.clone(),
                    |selected| {
                        selected
                            .iter()
                            .filter(|label| known.contains(label))
                            .cloned()
                            .collect()
                    },
                );
                labels
                    .iter()
                    .map(|label| {
                        let before = session.person_of(label).map(String::from);
                        session.name_person(label, Some(&person));
                        (label.clone(), before)
                    })
                    .collect::<Vec<_>>()
            })
        };
        let Some(previous) = previous else {
            self.not_found(session_id);
            return None;
        };
        if previous.is_empty() {
            self.emit(error(
                "speaker.invalid",
                "session has no matching speaker",
                json!({}),
            ));
            return None;
        }
        let everyone = self.inner.people.people();
        let voices = self.inner.people.voices(session_id);
        let mut former: BTreeMap<String, Person> = BTreeMap::new();
        for (label, before) in &previous {
            if let Some(id) = before.as_ref().filter(|id| *id != &person.id) {
                if !former.contains_key(id)
                    && let Some(found) = everyone.iter().find(|entry| entry.id == *id)
                {
                    former.insert(id.clone(), found.clone());
                }
                if let Some(found) = former.get_mut(id) {
                    found.unenroll(session_id, label);
                }
            }
            if let Some(voice) = voices.get(label) {
                person.enroll(session_id, label, voice.clone());
            }
        }
        for old in former.values() {
            self.save_person(old);
        }
        self.save_person(&person);
        for (label, _) in &previous {
            self.speaker_renamed(session_id, label, &person.name);
        }
        self.announce_speakers(session_id);
        self.announce_attendees(session_id);
        self.sessions();
        self.people();
        Some(previous.len())
    }

    pub fn assign_line(
        &self,
        session_id: &str,
        who: &str,
        at: f64,
        person_id: Option<&str>,
        name: &str,
    ) {
        let Some(mut person) = self.chosen_person(person_id, name) else {
            return;
        };
        let changed = {
            let mut state = self.state();
            self.edit_session(&mut state, session_id, |session| {
                let previous = session.person_of(who).map(String::from);
                session
                    .assign_line(who, at, &person)
                    .map(|(label, retired)| (label, retired, previous))
            })
        };
        match changed {
            Some(Some((label, retired, previous))) => {
                if let Some(mut before) = previous
                    .filter(|id| *id != person.id)
                    .and_then(|id| self.person(&id))
                    && (retired || who == label)
                {
                    before.unenroll(session_id, who);
                    self.save_person(&before);
                }
                if retired {
                    let mut voices = self.inner.people.voices(session_id);
                    if let Some(voice) = voices.remove(who) {
                        person.enroll(session_id, &label, voice.clone());
                        voices.insert(label.clone(), voice);
                        if let Err(failure) = self.inner.people.keep_voices(session_id, &voices) {
                            self.store_failed(&failure);
                        }
                    }
                }
                self.save_person(&person);
                self.emit(json!({
                    "type": "transcript_reassigned", "session": session_id,
                    "who": who, "at": at, "label": label, "name": person.name,
                }));
                self.announce_speakers(session_id);
                self.announce_attendees(session_id);
                self.sessions();
                self.people();
            }
            Some(None) => self.line_not_found(session_id, who, at),
            None => self.not_found(session_id),
        }
    }

    pub fn assign_all(&self, session_id: &str, person_id: Option<&str>, name: &str) {
        if let Some(count) = self.assign_labels(session_id, None, person_id, name, None) {
            self.emit(
                json!({"type": "person_assigned_all", "session": session_id, "speakers": count}),
            );
        }
    }

    /// Undo an assignment: the speaker `label` of a session is no one again, and
    /// their voice leaves the person.
    pub fn unassign_person(&self, session_id: &str, label: &str) {
        let previous = {
            let mut state = self.state();
            self.edit_session(&mut state, session_id, |session| {
                let previous = session.person_of(label).map(String::from);
                session.name_person(label, None);
                session.rename_speaker(label, label);
                previous
            })
        };
        let Some(previous) = previous else {
            return self.not_found(session_id);
        };
        if let Some(mut before) = previous.and_then(|id| self.person(&id)) {
            before.unenroll(session_id, label);
            self.save_person(&before);
        }
        self.speaker_renamed(session_id, label, label);
        self.announce_speakers(session_id);
        self.announce_attendees(session_id);
        self.sessions();
        self.people();
    }

    /// Name every speaker of every session who is `person` after them again.
    fn rename_everywhere(&self, person_id: &str, person: Option<&Person>) {
        for (session_id, labels) in self.sessions_with(person_id) {
            let mut state = self.state();
            let names = self.edit_session(&mut state, &session_id, |session| {
                labels
                    .iter()
                    .map(|label| {
                        session.name_person(label, person);
                        (label.clone(), session.name_of(label).to_string())
                    })
                    .collect::<Vec<_>>()
            });
            drop(state);
            for (label, name) in names.unwrap_or_default() {
                self.speaker_renamed(&session_id, &label, &name);
            }
            if let Some(person) = person
                && person.id != person_id
            {
                let mut state = self.state();
                self.edit_session(&mut state, &session_id, |session| {
                    if session.has_attendee(person_id) {
                        session.set_attendee(person_id, false);
                        session.set_attendee(&person.id, true);
                    }
                });
            } else if person.is_none() {
                let mut state = self.state();
                self.edit_session(&mut state, &session_id, |session| {
                    session.set_attendee(person_id, false)
                });
            }
            self.announce_speakers(&session_id);
            self.announce_attendees(&session_id);
        }
        self.sessions();
    }

    /// Keep a new person by name, before any session names them. A name someone
    /// already has, in any case, is refused: that person is the one meant.
    pub fn add_person(&self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return self.emit(error("person.invalid", "a person needs a name", json!({})));
        }
        let lower = name.to_lowercase();
        if let Some(known) = self
            .inner
            .people
            .people()
            .into_iter()
            .find(|person| person.name.to_lowercase() == lower)
        {
            return self.emit(error(
                "person.exists",
                format!("{:?} is already someone", known.name),
                json!({"id": known.id, "name": known.name}),
            ));
        }
        self.save_person(&Person::new(name));
        self.people();
    }

    pub fn rename_person(&self, person_id: &str, name: &str) {
        let name = name.trim();
        let Some(mut person) = self.person(person_id) else {
            return;
        };
        if !name.is_empty() {
            person.name = name.into();
            self.save_person(&person);
            self.rename_everywhere(person_id, Some(&person));
        }
        self.people();
    }

    pub fn set_person_color(&self, person_id: &str, color: &str) {
        if !color.is_empty() && !crate::config::valid_color(color) {
            self.emit(error("color.invalid", "invalid speaker color", json!({})));
            return;
        }
        let Some(mut person) = self.person(person_id) else {
            return;
        };
        person.color = color.to_ascii_lowercase();
        self.save_person(&person);
        self.people();
    }

    pub fn set_speaker_color(&self, session_id: &str, label: &str, color: &str) {
        if !color.is_empty() && !crate::config::valid_color(color) {
            self.emit(error("color.invalid", "invalid speaker color", json!({})));
            return;
        }
        let mut state = self.state();
        let changed = self.edit_session(&mut state, session_id, |session| {
            if !session.labels().iter().any(|known| known == label) {
                return false;
            }
            session.set_speaker_color(label, &color.to_ascii_lowercase());
            true
        });
        drop(state);
        match changed {
            Some(true) => self.announce_speakers(session_id),
            Some(false) => self.emit(error("speaker.invalid", "speaker not found", json!({}))),
            None => self.not_found(session_id),
        }
    }

    /// `from` and `into` are one person: `into` takes `from`'s voices and sessions.
    pub fn merge_people(&self, into: &str, from: &str) {
        if into == from {
            return;
        }
        let (Some(mut kept), Some(gone)) = (self.person(into), self.person(from)) else {
            return;
        };
        kept.absorb(gone);
        self.save_person(&kept);
        if let Err(failure) = self.inner.people.forget(from) {
            self.store_failed(&failure);
        }
        self.rename_everywhere(from, Some(&kept));
        self.people();
    }

    /// Delete a person and the voices kept for them; sessions keep the name they
    /// were given. A person no longer stored still leaves the sessions naming them.
    pub fn forget_person(&self, person_id: &str) {
        let linked = self.sessions_with(person_id);
        if linked.is_empty() && self.person(person_id).is_none() {
            return;
        }
        for (session_id, labels) in linked {
            let mut voices = self.inner.people.voices(&session_id);
            voices.retain(|label, _| !labels.contains(label));
            if let Err(failure) = self.inner.people.keep_voices(&session_id, &voices) {
                self.store_failed(&failure);
            }
        }
        if let Err(failure) = self.inner.people.forget(person_id) {
            self.store_failed(&failure);
        }
        self.rename_everywhere(person_id, None);
        self.people();
    }

    fn line_not_found(&self, session_id: &str, who: &str, at: f64) {
        self.emit(error(
            "line.not_found",
            format!("session {session_id} has no line of {who:?} at {at}"),
            json!({"id": session_id, "who": who, "at": at}),
        ));
    }

    /// Correct the line `who` said at `at` in a session — open, loaded or stored:
    /// the new text is what the screen, the context and the export see.
    pub fn edit_line(&self, session_id: &str, who: &str, at: f64, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return self.remove_line(session_id, who, at);
        }
        let edited = {
            let mut state = self.state();
            self.edit_session(&mut state, session_id, |session| {
                session.edit_line(who, at, text)
            })
        };
        match edited {
            Some(true) => self.emit(json!({
                "type": "transcript_edited", "session": session_id, "who": who, "at": at, "text": text,
            })),
            Some(false) => self.line_not_found(session_id, who, at),
            None => self.not_found(session_id),
        }
    }

    /// Remove the line `who` said at `at` from a session — open, loaded or stored:
    /// it leaves the screen, the context and the export.
    pub fn remove_line(&self, session_id: &str, who: &str, at: f64) {
        let removed = {
            let mut state = self.state();
            self.edit_session(&mut state, session_id, |session| {
                let person = session.person_of(who).map(String::from);
                let removed = session.unhear(who, at);
                let retired = removed && !session.labels().iter().any(|label| label == who);
                (removed, retired, person)
            })
        };
        match removed {
            Some((true, retired, person)) => {
                if retired {
                    if let Some(mut person) = person.and_then(|id| self.person(&id)) {
                        person.unenroll(session_id, who);
                        self.save_person(&person);
                    }
                    let mut voices = self.inner.people.voices(session_id);
                    if voices.remove(who).is_some()
                        && let Err(failure) = self.inner.people.keep_voices(session_id, &voices)
                    {
                        self.store_failed(&failure);
                    }
                }
                self.emit(json!({"type": "transcript_removed", "session": session_id, "who": who, "at": at}));
                self.announce_speakers(session_id);
                self.announce_attendees(session_id);
                self.sessions();
                self.people();
            }
            Some((false, _, _)) => self.line_not_found(session_id, who, at),
            None => self.not_found(session_id),
        }
    }

    /// Name the speaker whose lines carry `label` in a session — open, loaded or
    /// stored; clients update the lines on screen.
    pub fn rename_speaker(&self, session_id: &str, label: &str, name: &str) {
        let name = name.trim();
        let mut state = self.state();
        let renamed = self.edit_session(&mut state, session_id, |session| {
            session.rename_speaker(label, name);
            session.name_of(label).to_string()
        });
        match renamed {
            Some(shown) => self.speaker_renamed(session_id, label, &shown),
            None => self.not_found(session_id),
        }
    }

    /// Give a session, the open one or a stored one, a new title and a kind: a
    /// configured one, or the one it has.
    pub fn rename(&self, session_id: &str, title: &str, kind: &str) {
        let title = title.trim();
        let mut state = self.state();
        let kinds = state
            .setup
            .as_ref()
            .map(|s| s.kinds.clone())
            .unwrap_or_default();
        let allowed = |session: &Session| kinds.iter().any(|k| k == kind) || session.kind == kind;
        let unknown = || {
            error(
                "session.invalid",
                format!("session: unknown kind {kind:?}"),
                json!({"detail": format!("unknown kind {kind:?}")}),
            )
        };
        let renamed = self.edit_session(&mut state, session_id, |session| {
            let ok = allowed(session);
            if ok {
                session.rename(title, kind);
            }
            ok
        });
        match renamed {
            None => return self.not_found(session_id),
            Some(false) => return self.emit(unknown()),
            Some(true) => {}
        }
        if state.is_live(session_id) {
            self.announce(&mut state);
        }
        self.emit(
            json!({"type": "session_renamed", "id": session_id, "title": title, "kind": kind}),
        );
    }

    // Tags.

    /// Every session's id and tags, stored or only in memory, oldest first.
    fn tagged(&self) -> Vec<(String, Vec<String>)> {
        let stored: Vec<Session> = self
            .inner
            .log
            .all()
            .iter()
            .filter_map(|records| Session::restore(records, Box::new(|_| {})).ok())
            .collect();
        let state = self.state();
        let mut sessions: Vec<(f64, String, Vec<String>)> = stored
            .iter()
            .filter(|session| !state.loaded.contains_key(&session.id))
            .chain(state.loaded.values())
            .map(|s| (s.started_at, s.id.clone(), s.tags().to_vec()))
            .collect();
        sessions.sort_by(|a, b| a.0.total_cmp(&b.0));
        sessions
            .into_iter()
            .map(|(_, id, tags)| (id, tags))
            .collect()
    }

    /// Announce every tag and how many sessions carry it.
    pub fn tags(&self) {
        let tagged = self.tagged();
        self.announce_tags(&tagged);
    }

    fn announce_tags(&self, tagged: &[(String, Vec<String>)]) {
        let counts = tag_counts(tagged.iter().map(|(_, tags)| tags.as_slice()));
        let tags: Vec<Value> = counts
            .iter()
            .map(|(tag, sessions)| json!({"tag": tag, "sessions": sessions}))
            .collect();
        self.emit(json!({"type": "tags", "tags": tags}));
    }

    /// Tag a session — open, read back or stored — `tag`, as another session
    /// already writes it when one does; or, unless `present`, take it off. Clients
    /// are told its tags, then every tag.
    pub fn tag(&self, session_id: &str, tag: &str, present: bool) {
        let Some(mut tag) = tag_name(tag) else {
            return self.emit(invalid_tag(json!({"session": session_id})));
        };
        if present {
            let tagged = self.tagged();
            let counts = tag_counts(tagged.iter().map(|(_, tags)| tags.as_slice()));
            if let Some((known, _)) = counts.into_iter().find(|(known, _)| same_tag(known, &tag)) {
                tag = known;
            }
        }
        let mut state = self.state();
        let tags = self.edit_session(&mut state, session_id, |session| {
            if present {
                session.tag(&tag);
            } else {
                session.untag(&tag);
            }
            session.tags().to_vec()
        });
        let Some(tags) = tags else {
            drop(state);
            return self.not_found(session_id);
        };
        if state.is_live(session_id) {
            self.announce(&mut state);
        }
        drop(state);
        self.emit(json!({"type": "session_tags", "session": session_id, "tags": tags}));
        self.tags();
    }

    /// Call the tag `from` `to` in every session; where `to` is already a tag the
    /// two become one, written `to`.
    pub fn rename_tag(&self, from: &str, to: &str) {
        let (Some(from), Some(to)) = (tag_name(from), tag_name(to)) else {
            return self.emit(invalid_tag(Value::Null));
        };
        let done = json!({"type": "tag_renamed", "from": from, "to": to});
        self.change_tag(&[&from, &to], done, |session| session.retag(&from, &to));
    }

    /// Take the tag `tag` off every session.
    pub fn delete_tag(&self, tag: &str) {
        let Some(tag) = tag_name(tag) else {
            return self.emit(invalid_tag(Value::Null));
        };
        let done = json!({"type": "tag_deleted", "tag": tag});
        self.change_tag(&[&tag], done, |session| session.untag(&tag));
    }

    /// Apply `change` to every session carrying one of the tags `names`, then
    /// announce `done` with the ids of those it changed, the tags and the sessions.
    /// No session carrying the first is `tag.not_found`.
    fn change_tag(&self, names: &[&str], mut done: Event, change: impl Fn(&mut Session)) {
        let carries = |tags: &[String], name: &str| tags.iter().any(|tag| same_tag(tag, name));
        let tagged = self.tagged();
        if !tagged.iter().any(|(_, tags)| carries(tags, names[0])) {
            return self.emit(error(
                "tag.not_found",
                format!("no session is tagged {:?}", names[0]),
                json!({"tag": names[0]}),
            ));
        }
        let mut changed: Vec<&str> = Vec::new();
        for (id, tags) in &tagged {
            if !names.iter().any(|name| carries(tags, name)) {
                continue;
            }
            let mut state = self.state();
            let edited = self.edit_session(&mut state, id, |session| {
                change(session);
                session.tags() != tags.as_slice()
            });
            if edited == Some(true) {
                if state.is_live(id) {
                    self.announce(&mut state);
                }
                changed.push(id);
            }
        }
        done["sessions"] = json!(changed);
        self.emit(done);
        self.tags();
        self.sessions();
    }

    /// Pause the live session `session_id`, or the one shown when it is none.
    pub fn pause(&self, session_id: Option<&str>) {
        self.transition(session_id, RECORDING, PAUSED);
    }

    /// Resume the live session `session_id`, or the one shown when it is none.
    pub fn resume(&self, session_id: Option<&str>) {
        self.transition(session_id, PAUSED, RECORDING);
    }

    /// Pause the session shown if it records, or resume it, for a single shortcut.
    pub fn toggle(&self) {
        let recording = self.state().session().map(|m| m.state == RECORDING);
        match recording {
            Some(true) => self.pause(None),
            Some(false) => self.resume(None),
            None => {}
        }
    }

    /// End the live session `session_id`, or the one shown when it is none; the
    /// one shown before it, if any, is shown again.
    pub fn end(&self, session_id: Option<&str>) {
        let mut state = self.state();
        let Some(id) = state.live_one(session_id) else {
            return;
        };
        state.pick(&id).set_state(ENDED);
        self.unload(&mut state, &id);
    }

    fn transition(&self, session_id: Option<&str>, source: &str, target: &str) {
        let mut state = self.state();
        let Some(id) = state.live_one(session_id) else {
            return;
        };
        let session = state.pick(&id);
        if session.state == source {
            session.set_state(target);
            self.announce(&mut state);
        }
    }

    fn announce(&self, state: &mut State) {
        self.listen(state);
        self.emit(json!({
            "type": "session", "session": session_state(state),
            "live": live_state(state), "transcribers": transcribers_state(state),
        }));
    }

    /// Tell capture what the recording sessions listen with, and each one's log
    /// which model hears it on how many inputs; only a real change wakes capture,
    /// so a running transcriber never restarts.
    fn listen(&self, state: &mut State) {
        let heard = heard(state);
        // Sessions are heard only under a setup.
        if let Some(setup) = state.setup.clone() {
            let inputs = u32::try_from(setup.inputs.len()).unwrap_or(u32::MAX);
            for (id, listening) in &heard {
                let session = state.loaded.get_mut(id).expect("a live session is loaded");
                let model = &listening.model;
                let per_minute = setup.transcription_prices.get(model).copied();
                session.transcribe_with(HeardBy::Model(model.clone(), inputs, per_minute));
            }
        }
        let at = now();
        let joined = state.requests.follow(&heard, at);
        self.listened(state, joined, at);
        let wanted: BTreeSet<Listening> =
            heard.into_iter().map(|(_, listening)| listening).collect();
        self.inner
            .listening
            .send_if_modified(|current| std::mem::replace(current, wanted.clone()) != wanted);
    }

    /// The requests the transcriber of `from` bills under, as it opens them.
    pub fn bills(&self, from: &Listening) -> Bills {
        Bills {
            assistant: self.clone(),
            from: from.clone(),
            open: Mutex::new(None),
        }
    }

    /// The transcriber of `from` bills under `request` from `at`, when the
    /// setup can tell what its model's requests cost; each session listening
    /// keeps that it does.
    fn open_request(&self, state: &mut State, from: &Listening, request: &str, at: f64) {
        let Some(setup) = state.setup.clone() else {
            return;
        };
        let Some(billing) = setup.transcription_billing.get(&from.model) else {
            return;
        };
        let heard = heard(state);
        let billing = (from, Arc::clone(billing));
        let joined = state.requests.open(request, billing, &heard, at);
        self.listened(state, joined, at);
    }

    /// Each session `joined` keeps that it listens to the request from `at`.
    fn listened(&self, state: &mut State, joined: Vec<Joined>, at: f64) {
        for joined in joined {
            self.edit_session(state, &joined.session, |session| {
                session.listen(&joined.request, &joined.model, at);
            });
        }
    }

    /// `request` is over at `at`: each session it heard keeps its share, and
    /// its cost is asked for.
    fn close_request(&self, state: &mut State, request: &str, at: f64) {
        let Some((bill, billing)) = state.requests.close(request, at) else {
            return;
        };
        for share in &bill.shares {
            self.edit_session(state, &share.session, |session| {
                session.bill(request, &bill.model, (share.seconds, share.share), at);
            });
        }
        let sessions = bill.shares.into_iter().map(|share| share.session).collect();
        self.price(billing, request.into(), sessions, at);
    }

    /// The transcriber of `from` billed `seconds` of speech under `request`,
    /// just answered: the sessions listening share it alike.
    pub fn billed_segment(&self, from: &Listening, request: &str, seconds: f64) {
        let mut state = self.state();
        let at = now();
        self.open_request(&mut state, from, request, at - seconds);
        self.close_request(&mut state, request, at);
    }

    /// Ask the provider what `request`, closed at `closed`, cost, off the
    /// caller, and keep it in `sessions`; one already asked for is not asked
    /// twice.
    fn price(
        &self,
        billing: Arc<dyn TranscriptionBilling>,
        request: String,
        sessions: Vec<String>,
        closed: f64,
    ) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let mut pricing = self.inner.pricing.lock().expect("not poisoned");
        if !pricing.insert(request.clone()) {
            return;
        }
        let assistant = self.clone();
        runtime.spawn(async move {
            let usd = priced(billing.as_ref(), &request, closed).await;
            let mut state = assistant.state();
            let setup = state.setup.clone();
            for id in &sessions {
                assistant.edit_session(&mut state, id, |session| session.pay(&request, usd));
                if let Some(session) = state.loaded.get(id) {
                    assistant.emit(cost_event(setup.as_deref(), session));
                }
            }
            let mut pricing = assistant.inner.pricing.lock().expect("not poisoned");
            pricing.remove(&request);
        });
    }

    /// Bill what the stored sessions listened to and were never billed for,
    /// then ask what each request they were billed for cost, when none is kept
    /// and the setup can tell: the daemon stopped while a request was open, or
    /// before its provider listed it, or a file was imported.
    pub fn price_unpaid(&self) {
        let mut state = self.state();
        self.price_unpaid_in(&mut state);
    }

    fn price_unpaid_in(&self, state: &mut State) {
        let Some(setup) = state.setup.clone() else {
            return;
        };
        if setup.transcription_billing.is_empty() {
            return;
        }
        let stored: Vec<Session> = self
            .inner
            .log
            .all()
            .iter()
            .filter_map(|records| Session::restore(records, Box::new(|_| {})).ok())
            .collect();
        let billed: HashSet<&str> = stored.iter().flat_map(Session::billed).collect();
        // Requests left open, by id, with each session that listened to them.
        let mut unclosed: BTreeMap<String, Vec<(String, Unclosed)>> = BTreeMap::new();
        for session in &stored {
            for left in session.unclosed() {
                if billed.contains(left.request.as_str()) || state.requests.is_open(&left.request) {
                    continue;
                }
                let listeners = unclosed.entry(left.request.clone()).or_default();
                listeners.push((session.id.clone(), left));
            }
        }
        let mut unpaid: BTreeMap<String, (String, Vec<String>, f64)> = BTreeMap::new();
        for session in &stored {
            for (request, model, at) in session.unpaid() {
                let (_, sessions, _) = unpaid
                    .entry(request.into())
                    .or_insert_with(|| (model.into(), Vec::new(), at.unwrap_or_else(now)));
                sessions.push(session.id.clone());
            }
        }
        for (request, listeners) in unclosed {
            let model = listeners[0].1.model.clone();
            let last = listeners
                .iter()
                .map(|(_, left)| left.last)
                .fold(0.0, f64::max);
            let seconds = listeners
                .iter()
                .map(|(id, left)| (id.clone(), left.seconds));
            let mut sessions = Vec::new();
            for share in shares(seconds) {
                let billed = self.edit_session(state, &share.session, |session| {
                    let open = session
                        .unclosed()
                        .iter()
                        .any(|left| left.request == request);
                    if open {
                        session.bill(&request, &model, (share.seconds, share.share), last);
                    }
                    open
                });
                if billed == Some(true) {
                    sessions.push(share.session);
                }
            }
            unpaid.insert(request, (model, sessions, last));
        }
        for (request, (model, sessions, closed)) in unpaid {
            if let Some(billing) = setup.transcription_billing.get(&model) {
                self.price(Arc::clone(billing), request, sessions, closed);
            }
        }
    }

    // What happens inside a session.

    /// The words `who` is saying, not yet a line, on every recording session
    /// listening `from`: shown, never kept. An empty text clears them.
    pub fn hear_partial(&self, from: &Listening, who: &str, words: &str) {
        let state = self.state();
        let Some(setup) = state.setup.clone() else {
            return;
        };
        for session in state.recording().filter(|s| setup.listening_of(s) == *from) {
            let name = session.name_of(who);
            self.emit(
                json!({"type": "transcript_partial", "session": session.id, "who": who,
                             "name": name, "text": words}),
            );
        }
    }

    /// Add what was heard `from` a transcriber to every recording session listening
    /// with it; returns each session and the time of the line it became there.
    pub fn hear(&self, from: &Listening, utterance: &Utterance) -> Vec<(String, f64)> {
        let mut state = self.state();
        let Some(setup) = state.setup.clone() else {
            return Vec::new();
        };
        let listeners: Vec<String> = state
            .recording()
            .filter(|session| setup.listening_of(session) == *from)
            .map(|session| session.id.clone())
            .collect();
        // One capture time for the line in every session that hears it.
        let at = now();
        listeners
            .iter()
            .filter_map(|id| self.hear_in(&setup, state.pick(id), utterance, at))
            .collect()
    }

    /// A line heard in one recording session. With `drop_echoes`, a line of the
    /// user's that repeats what the others just said is their audio leaking into
    /// the microphone: dropped when it comes after theirs, taken back when it
    /// came first.
    fn hear_in(
        &self,
        setup: &Setup,
        session: &mut Session,
        utterance: &Utterance,
        at: f64,
    ) -> Option<(String, f64)> {
        let user = Some(setup)
            .filter(|setup| setup.drop_echoes)
            .and_then(|setup| setup.participants.iter().find(|p| p.1))
            .map(|p| p.0.clone());
        let text = utterance.text();
        // The lines `pick` keeps from the last few seconds, as (at, text).
        let recent = |session: &Session,
                      since: f64,
                      pick: &dyn Fn(&str) -> bool|
         -> Vec<(f64, String)> {
            let lines = session.timeline.iter().filter_map(|entry| match entry {
                Entry::Speech(s) if s.at >= since && pick(&s.who) => Some((s.at, s.text.clone())),
                _ => None,
            });
            lines.collect()
        };
        if let Some(user) = &user
            && utterance.who == *user
        {
            let theirs = recent(session, at - echo::WINDOW_S, &|who| who != user);
            let heard: Vec<&str> = theirs.iter().map(|(_, text)| text.as_str()).collect();
            if echo::repeats(&text, &heard) {
                // What was shown of it while it was said goes too.
                self.emit(json!({"type": "transcript_partial", "session": session.id,
                                 "who": user, "name": user, "text": ""}));
                return None;
            }
        }
        let speech = session.hear_at(&utterance.who, &text, at);
        self.emit(speech_event(session, &speech, utterance.latency));
        if let Some(language) = session.translation().map(String::from) {
            let of = Source::Line(speech.at);
            self.translate(
                setup,
                &session.id,
                &session.kind,
                &language,
                of,
                speech.text.clone(),
            );
        }
        let line = Some((session.id.clone(), speech.at));
        let Some(user) = user.filter(|user| utterance.who != *user) else {
            return line;
        };
        let since = speech.at - echo::WINDOW_S;
        let theirs = recent(session, since, &|who| who != user);
        let heard: Vec<&str> = theirs.iter().map(|(_, text)| text.as_str()).collect();
        let leaked: Vec<f64> = recent(session, since, &|who| who == user)
            .into_iter()
            .filter(|(_, text)| echo::repeats(text, &heard))
            .map(|(at, _)| at)
            .collect();
        for at in leaked {
            session.unhear(&user, at);
            let removed =
                json!({"type": "transcript_removed", "session": session.id, "who": user, "at": at});
            self.emit(removed);
        }
        line
    }

    /// Start an action on the open session, or on a stored one by id, replacing the
    /// suggestion in progress.
    pub fn trigger(&self, session_id: Option<&str>, name: &str) {
        let mut state = self.state();
        let target = match self.target(&mut state, session_id) {
            Ok(target) => target,
            Err(event) => return self.emit(event),
        };
        let Some(setup) = state.setup.clone() else {
            return self.emit(error(
                "action.unknown",
                format!("unknown action {name:?}"),
                json!({"name": name}),
            ));
        };
        let Some(action) = setup.actions.iter().find(|a| a.name == name).cloned() else {
            return self.emit(error(
                "action.unknown",
                format!("unknown action {name:?}"),
                json!({"name": name}),
            ));
        };
        let kind = state.pick(&target).kind.clone();
        let model = setup.model_for(&kind, &action.name).id.clone();
        let request = action_request(&action);
        self.launch(
            &mut state,
            &target,
            &action.name,
            &model,
            &action.name,
            &request,
        );
    }

    /// Answer a free question about the open session, or a stored one by id, as a
    /// chat turn.
    pub fn ask(&self, session_id: Option<&str>, question: &str) {
        let mut state = self.state();
        let target = match self.target(&mut state, session_id) {
            Ok(target) => target,
            Err(event) => return self.emit(event),
        };
        let Some(setup) = state.setup.clone() else {
            return self.emit(error("session.none", "start a session first", Value::Null));
        };
        let question = question.trim();
        if !question.is_empty() {
            let kind = state.pick(&target).kind.clone();
            let model = setup.model_for(&kind, "chat").id.clone();
            self.launch(
                &mut state,
                &target,
                "chat",
                &model,
                question,
                &question_request(question),
            );
        }
    }

    /// Keep a note in the open session, or a stored one by id: context later
    /// answers use, not a question.
    pub fn note(&self, session_id: Option<&str>, text: &str) {
        let mut state = self.state();
        let target = match self.target(&mut state, session_id) {
            Ok(target) => target,
            Err(event) => return self.emit(event),
        };
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let session = state.pick(&target);
        let note = session.note(text);
        let mut event = note_event(&note);
        event["session"] = json!(session.id);
        self.emit(event);
    }

    /// The id of the session `session_id` names — the open one when it is none —
    /// loading a stored one to read and ask; the error to show when there is none.
    fn target(&self, state: &mut State, session_id: Option<&str>) -> Result<String, Event> {
        let id = match (session_id, state.live.last()) {
            (Some(id), _) => id.to_string(),
            (None, Some(shown)) => shown.clone(),
            (None, None) => {
                return Err(error("session.none", "start a session first", Value::Null));
            }
        };
        self.load(state, &id)?;
        Ok(id)
    }

    /// Have the session `id` loaded, reading a stored one from the log; past
    /// `LOADED_STORED`, the least recently used idle one other than `id` leaves
    /// memory, and none does while all are answering.
    fn load(&self, state: &mut State, id: &str) -> Result<(), Event> {
        if state.loaded.contains_key(id) {
            state.pick(id);
            return Ok(());
        }
        let log = &self.inner.log;
        let session = log
            .read(id)
            .zip(log.append_to(id))
            .and_then(|(records, sink)| Session::restore(&records, sink).ok())
            .ok_or_else(|| {
                error(
                    "session.not_found",
                    format!("no session {id:?}"),
                    json!({"id": id}),
                )
            })?;
        state.loaded.insert(id.to_string(), session);
        state.used.push_back(id.to_string());
        while state.used.len() > LOADED_STORED {
            let idle = state
                .used
                .iter()
                .find(|used| *used != id && !state.busy(used));
            let Some(idle) = idle.cloned() else {
                break;
            };
            self.unload(state, &idle);
        }
        Ok(())
    }

    /// Drop a loaded session from memory and stop its answers; its log keeps it.
    fn unload(&self, state: &mut State, id: &str) {
        // The waiting one first, so stopping the running one starts nothing there.
        for answer in state.answers_of(id) {
            self.remove_from(state, &answer);
        }
        state.loaded.remove(id);
        state.used.retain(|used| used != id);
        if let Some(at) = state.live.iter().position(|live| live == id) {
            state.live.remove(at);
            self.announce(state);
        }
    }

    /// Add the suggestion asked for and stream it, or let it wait for the one streaming.
    fn launch(
        &self,
        state: &mut State,
        target: &str,
        action: &str,
        model: &str,
        prompt: &str,
        request: &str,
    ) {
        let session = state.pick(target);
        let suggestion = session.suggest(action, model, prompt, request);
        self.emit(json!({
            "type": "suggestion_start", "id": suggestion.id, "session": session.id, "action": suggestion.action,
            "model": suggestion.model, "prompt": suggestion.prompt, "at": suggestion.at,
        }));
        let queue = state.answering.entry(target.to_string()).or_default();
        if queue.running.is_none() {
            return self.stream(state, &suggestion.id);
        }
        if let Some(older) = queue.waiting.replace(suggestion.id) {
            self.remove_from(state, &older);
        }
    }

    /// Start streaming the suggestion `id` asked for, with everything kept before it.
    fn stream(&self, state: &mut State, id: &str) {
        let Some(setup) = state.setup.clone() else {
            return;
        };
        let Some(session) = state.holder(id) else {
            return;
        };
        let suggestion = suggestion_in(session, id).expect("the holder has it");
        let user = setup
            .participants
            .iter()
            .find(|(_, user)| *user)
            .map(|(name, _)| name.as_str());
        let entries = session.context(setup.max_context_chars);
        let context = setup.context_of(session);
        let messages = conversation(
            &session.kind,
            &session.title,
            &setup.rules,
            &context,
            user,
            &entries,
            &suggestion.request,
        );
        let model = setup.model_for(&session.kind, &suggestion.action).clone();
        let reviewer = setup.reviewer.clone().map(|mut reviewer| {
            if let Some(model) = setup.kind_models.get(&session.kind) {
                reviewer.model = model.clone();
            }
            reviewer
        });
        let session_id = session.id.clone();
        let limit = Arc::clone(&setup.limit);
        let assistant = self.clone();
        let id = suggestion.id.clone();
        let task = tokio::spawn(async move {
            // Held until the answer ends: past the limit, an answer waits here.
            let _streaming = limit.acquire_owned().await;
            let started = Instant::now();
            let draft = model.llm.stream(&model.id, messages.clone());
            let (stream, by) = match reviewer {
                None => (draft, ("answer", model.id)),
                Some(reviewer) => match assistant
                    .draft(&id, draft, &model.id, reviewer.verbose)
                    .await
                {
                    Ok(text) => {
                        let Model { llm, id, .. } = &reviewer.model;
                        let review = llm.stream(id, review(messages, &text, &reviewer.prompt));
                        (review, ("review", id.clone()))
                    }
                    Err(detail) => return assistant.fail(&id, &detail),
                },
            };
            assistant.run(id, stream, started, by).await
        });
        state.answering.entry(session_id).or_default().running =
            Some((suggestion.id, task.abort_handle()));
    }

    /// Read the answer a reviewer will rewrite, from `model`; it reaches the user
    /// only when `verbose`, as a draft.
    async fn draft(
        &self,
        id: &str,
        mut stream: Chunks,
        model: &str,
        verbose: bool,
    ) -> Result<String, String> {
        let mut draft = String::new();
        let mut cost = None;
        while let Some(chunk) = stream.next().await {
            match chunk.map_err(|failure| failure.0)? {
                Chunk::Thinking(text) => {
                    self.emit(json!({"type": "suggestion_thinking", "id": id, "text": text}));
                }
                Chunk::Text(text) => {
                    if verbose {
                        if let Some(session) = self.state().holder(id) {
                            session.extend_draft(id, &text);
                        }
                        self.emit(json!({"type": "suggestion_draft", "id": id, "text": text}));
                    }
                    draft.push_str(&text);
                }
                Chunk::Usage(usage) => cost = usage.cost_usd,
            }
        }
        self.spend(&mut self.state(), id, "answer", model, cost);
        if draft.trim().is_empty() {
            return Err("the model sent no answer".into());
        }
        Ok(draft)
    }

    /// Stream the answer `id` to clients; `by` says what gives it — the answer or
    /// its review — and the model's id.
    async fn run(&self, id: String, mut stream: Chunks, started: Instant, by: (&str, String)) {
        let mut first: Option<Duration> = None;
        let mut usage: Option<Usage> = None;
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(Chunk::Usage(reported)) => usage = Some(reported),
                // Shown while the model reasons, so its wait reads as work; not kept.
                Ok(Chunk::Thinking(text)) => {
                    self.emit(json!({"type": "suggestion_thinking", "id": id, "text": text}));
                }
                Ok(Chunk::Text(text)) => {
                    first.get_or_insert_with(|| started.elapsed());
                    if let Some(session) = self.state().holder(&id) {
                        session.extend(&id, &text);
                    }
                    self.emit(json!({"type": "suggestion_delta", "id": id, "text": text}));
                }
                Err(failure) => return self.fail(&id, &failure.0),
            }
        }
        let (what, model) = by;
        let cost = usage.as_ref().and_then(|u| u.cost_usd);
        // A stream that ends without a word is no answer: say so rather than keep an empty card.
        if first.is_none() {
            self.spend(&mut self.state(), &id, what, &model, cost);
            return self.fail(&id, "the model sent no answer");
        }
        let mut state = self.state();
        let queue = state.queued(&id);
        if let Some(queue) = queue.as_ref().and_then(|s| state.answering.get_mut(s)) {
            queue.running = None;
        }
        let mut auto = None;
        if let Some(session) = state.holder(&id) {
            session.finish(&id);
            auto = Some((session.id.clone(), session.title.clone()));
        }
        let action = suggestion_of(&state, &id).map(|s| s.action);
        let hook = state
            .setup
            .as_ref()
            .zip(action.as_deref())
            .and_then(|(setup, action)| setup.hook_of(action))
            .filter(|(_, auto)| *auto)
            .map(|(command, _)| command.to_string());
        self.emit(json!({
            "type": "suggestion_end",
            "id": id,
            "ttft_ms": millis(first.unwrap_or_default()),
            "total_ms": millis(started.elapsed()),
            "prompt_tokens": usage.as_ref().map(|u| u.prompt_tokens),
            "cached_tokens": usage.as_ref().map(|u| u.cached_tokens),
            "cost_usd": cost,
        }));
        self.spend(&mut state, &id, what, &model, cost);
        if let Some(setup) = state.setup.clone()
            && let Some(session) = state.holder(&id)
            && let Some(language) = session.translation().map(String::from)
            && let Some(text) = session.text_of(&Source::Answer(id.clone()))
        {
            let (session_id, kind, text) =
                (session.id.clone(), session.kind.clone(), text.to_string());
            self.translate(
                &setup,
                &session_id,
                &kind,
                &language,
                Source::Answer(id.clone()),
                text,
            );
        }
        if let (Some((session_id, title)), Some(command)) = (auto, hook) {
            self.run_hook(session_id, title, id, command);
        }
        if let Some(session) = queue {
            self.next(&mut state, &session);
        }
    }

    /// Keep in its session what a completion for the answer `id` cost, and tell
    /// clients what the session has cost now.
    fn spend(&self, state: &mut State, id: &str, what: &str, model: &str, usd: Option<f64>) {
        let setup = state.setup.clone();
        if let Some(session) = state.holder(id) {
            session.spend(what, model, usd, Some(id));
            self.emit(cost_event(setup.as_deref(), session));
        }
    }

    // Hooks.

    /// Run the hook of the action that gave the finished answer `id` of a session —
    /// open, read back or stored.
    pub fn send(&self, session_id: &str, id: &str) {
        let state = self.state();
        let loaded = state
            .loaded
            .get(session_id)
            .map(|session| (session.title.clone(), suggestion_in(session, id)));
        let found = loaded.or_else(|| {
            let records = self.inner.log.read(session_id)?;
            let session = Session::restore(&records, Box::new(|_| {})).ok()?;
            Some((session.title.clone(), suggestion_in(&session, id)))
        });
        let Some((title, answer)) = found else {
            return self.not_found(session_id);
        };
        let Some(answer) = answer.filter(|answer| answer.done) else {
            return self.emit(error(
                "answer.not_found",
                format!("no finished answer {id:?}"),
                json!({"id": id}),
            ));
        };
        let Some((command, _)) = state
            .setup
            .as_ref()
            .and_then(|setup| setup.hook_of(&answer.action))
        else {
            return self.emit(error(
                "hook.none",
                format!("{} has no hook", answer.action),
                json!({"id": id, "action": answer.action}),
            ));
        };
        self.run_hook(session_id.into(), title, id.into(), command.into());
    }

    // Translation.

    /// Whether eco translates into `language` (`prompts::LANGUAGES`); clients are
    /// told when not.
    fn known_language(&self, language: &str) -> bool {
        let known = LANGUAGES.iter().any(|(code, _)| *code == language);
        if !known {
            self.emit(error(
                "translation.unknown",
                format!("eco does not translate into {language:?}"),
                json!({"language": language}),
            ));
        }
        known
    }

    /// Translate a session into `language` from now on, or stop for ""; what it
    /// already holds untranslated is translated now, and what it holds translated
    /// shows again at once. Its own language is refused.
    pub fn set_translation(&self, session_id: &str, language: &str) {
        if !language.is_empty() && !self.known_language(language) {
            return;
        }
        let mut state = self.state();
        let Some(setup) = state.setup.clone() else {
            return;
        };
        let changed = self.edit_session(&mut state, session_id, |session| {
            if !session.set_translation(language) {
                return None;
            }
            let target = session.translation().map(String::from);
            let missing = target
                .as_ref()
                .map(|l| session.untranslated(l))
                .unwrap_or_default();
            let kept = target
                .as_deref()
                .map(|l| kept_translations(session, l))
                .unwrap_or_default();
            Some((session.kind.clone(), target, missing, kept))
        });
        let (kind, target, missing, kept) = match changed {
            None => return self.not_found(session_id),
            Some(None) => {
                return self.emit(error(
                    "translation.own",
                    format!("the session is already in {language:?}"),
                    json!({"language": language}),
                ));
            }
            Some(Some(changed)) => changed,
        };
        if state.is_live(session_id) {
            self.announce(&mut state);
        }
        self.emit(
            json!({"type": "session_translation", "session": session_id, "translating": target}),
        );
        kept.into_iter().for_each(|event| self.emit(event));
        drop(state);
        if let Some(language) = target {
            for (of, text) in missing {
                self.translate(&setup, session_id, &kind, &language, of, text);
            }
        }
    }

    /// Translate one finished answer of a session — into its translation language,
    /// or `language` when the session does not translate.
    pub fn translate_answer(&self, session_id: &str, id: &str, language: &str) {
        if !self.known_language(language) {
            return;
        }
        let mut state = self.state();
        let Some(setup) = state.setup.clone() else {
            return;
        };
        let found = self.edit_session(&mut state, session_id, |session| {
            let language = session.translation().unwrap_or(language).to_string();
            let text = session
                .text_of(&Source::Answer(id.into()))
                .map(String::from);
            (session.kind.clone(), language, text)
        });
        drop(state);
        match found {
            Some((kind, language, Some(text))) => self.translate(
                &setup,
                session_id,
                &kind,
                &language,
                Source::Answer(id.into()),
                text,
            ),
            Some((_, _, None)) => self.emit(error(
                "answer.not_found",
                format!("no finished answer {id:?}"),
                json!({"id": id}),
            )),
            None => self.not_found(session_id),
        }
    }

    /// Translate a line or an answer apart from everything else; once back it is
    /// kept in its session and clients are told. The same one is never asked twice.
    fn translate(
        &self,
        setup: &Setup,
        session_id: &str,
        kind: &str,
        language: &str,
        of: Source,
        text: String,
    ) {
        let key = format!("{session_id}|{language}|{}", of.fields());
        if !self
            .inner
            .pending
            .lock()
            .expect("never poisoned")
            .insert(key.clone())
        {
            return;
        }
        let model = setup.translator(kind).clone();
        let (assistant, session_id, language) =
            (self.clone(), session_id.to_string(), language.to_string());
        let slots = Arc::clone(&self.inner.translating);
        tokio::spawn(async move {
            let _slot = slots.acquire_owned().await;
            let started = Instant::now();
            let mut stream = model.llm.stream(&model.id, translation(&language, &text));
            let mut out = String::new();
            let mut failure = None;
            let mut cost = None;
            let translator = model.id.clone();
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(Chunk::Text(piece)) => out.push_str(&piece),
                    Ok(Chunk::Usage(usage)) => cost = usage.cost_usd,
                    Ok(Chunk::Thinking(_)) => {}
                    Err(error) => {
                        failure = Some(error.0);
                        break;
                    }
                }
            }
            let result = match failure {
                Some(detail) => Err(detail),
                None if out.trim().is_empty() => Err("the model sent no translation".into()),
                None => Ok(out.trim().to_string()),
            };
            let took = started.elapsed();
            let spent = (translator.as_str(), cost);
            assistant.translated(&session_id, &key, of, &language, result, took, spent);
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn translated(
        &self,
        session_id: &str,
        key: &str,
        of: Source,
        language: &str,
        result: Result<String, String>,
        took: Duration,
        (model, cost): (&str, Option<f64>),
    ) {
        self.inner
            .pending
            .lock()
            .expect("never poisoned")
            .remove(key);
        let text = match result {
            Ok(text) => text,
            Err(detail) => {
                return self.emit(error(
                    "translation.failed",
                    format!("translation: {detail}"),
                    json!({"session": session_id, "detail": detail}),
                ));
            }
        };
        let mut state = self.state();
        let setup = state.setup.clone();
        let kept = self.edit_session(&mut state, session_id, |session| {
            session.spend("translation", model, cost, None);
            self.emit(cost_event(setup.as_deref(), session));
            session.translated(of.clone(), language, &text)
        });
        if kept == Some(true) {
            self.emit(translated_event(
                session_id, &of, language, &text, took, cost,
            ));
        }
    }

    /// Run a hook apart from the session; clients hear when it starts and how it ended.
    fn run_hook(&self, session_id: String, title: String, id: String, command: String) {
        self.emit(json!({"type": "hook_started", "session": session_id, "id": id}));
        let running = self.inner.hooks.run(command, session_id.clone(), title);
        let assistant = self.clone();
        tokio::spawn(async move {
            assistant.emit(match running.await {
                Ok(()) => json!({"type": "hook_sent", "session": session_id, "id": id}),
                Err(failure) => error(
                    "hook.failed",
                    format!("hook: {}", failure.0),
                    json!({"session": session_id, "id": id, "detail": failure.0}),
                ),
            });
        });
    }

    /// Start the suggestion waiting in a session, if any; a session with none
    /// leaves the queues.
    fn next(&self, state: &mut State, session: &str) {
        let Some(queue) = state.answering.get_mut(session) else {
            return;
        };
        match queue.waiting.take() {
            Some(id) => self.stream(state, &id),
            None if queue.running.is_none() => {
                state.answering.remove(session);
            }
            None => {}
        }
    }

    /// The answer `id` failed for `detail`: clients read why, then it is dropped.
    fn fail(&self, id: &str, detail: &str) {
        let mut state = self.state();
        let action = suggestion_of(&state, id)
            .map(|s| s.action)
            .unwrap_or_default();
        let queue = state.queued(id);
        if let Some(queue) = queue.as_ref().and_then(|s| state.answering.get_mut(s)) {
            queue.running = None;
        }
        // The reason first, so a client waiting on this answer reads it.
        self.emit(error(
            "completion.failed",
            format!("{action}: {detail}"),
            json!({"id": id, "action": action, "detail": detail}),
        ));
        self.remove_from(&mut state, id);
        if let Some(session) = queue {
            self.next(&mut state, &session);
        }
    }

    /// Drop a note or a suggestion of a session — streaming, in a loaded session, or
    /// kept in a stored one, whose log it appends to; it leaves the context of later
    /// requests. Clients are told it is gone even when it already was.
    pub fn remove(&self, session_id: &str, id: &str) {
        let mut state = self.state();
        if state.holder(id).is_some() {
            return self.remove_from(&mut state, id);
        }
        match self.edit_session(&mut state, session_id, |session| session.remove(id)) {
            Some(removed) => self.emit(removed_event(removed.as_ref(), id)),
            None => self.not_found(session_id),
        }
    }

    /// Drop a loaded note or suggestion; the one waiting starts when the streaming one goes.
    fn remove_from(&self, state: &mut State, suggestion_id: &str) {
        let session = state.queued(suggestion_id);
        let mut streaming = false;
        if let Some(queue) = session.as_ref().and_then(|s| state.answering.get_mut(s)) {
            if queue.waiting.as_deref() == Some(suggestion_id) {
                queue.waiting = None;
            }
            if let Some((_, task)) = queue
                .running
                .take_if(|(running, _)| running == suggestion_id)
            {
                task.abort();
                streaming = true;
            }
        }
        if let Some(removed) = state
            .holder(suggestion_id)
            .and_then(|session| session.remove(suggestion_id))
        {
            self.emit(removed_event(Some(&removed), suggestion_id));
        }
        if let Some(session) = session {
            if streaming {
                self.next(state, &session);
            } else if state
                .answering
                .get(&session)
                .is_some_and(|q| q.running.is_none())
            {
                state.answering.remove(&session);
            }
        }
    }

    /// Stop every suggestion in progress and drop the ones waiting; their text is dropped.
    fn cancel_running(&self, state: &mut State) {
        let sessions: Vec<String> = state.answering.keys().cloned().collect();
        for session in sessions {
            for id in state.answers_of(&session) {
                self.remove_from(state, &id);
            }
        }
    }

    /// Events that bring a client up to date: the session, then the session's timeline.
    pub fn snapshot(&self) -> Vec<Event> {
        let state = self.state();
        let setup = state.setup.as_deref();
        let session = json!({
            "type": "snapshot",
            "actions": setup.map(|s| s.actions.iter().map(|a| a.name.clone()).collect::<Vec<_>>()).unwrap_or_default(),
            "hooks": setup
                .map(|s| s.actions.iter().filter(|a| s.hook_of(&a.name).is_some()).map(|a| a.name.clone()).collect::<Vec<_>>())
                .unwrap_or_default(),
            "participants": setup
                .map(|s| s.participants.iter().map(|(name, user)| json!({"name": name, "user": user})).collect::<Vec<_>>())
                .unwrap_or_default(),
            "inputs": setup.map(|s| s.inputs.clone()).unwrap_or_default(),
            "language": setup.map_or("", |s| &s.language),
            "languages": setup.map(|s| s.languages.clone()).unwrap_or_default(),
            "language_codes": LANGUAGES.map(|(code, _)| code),
            "kinds": setup.map(|s| s.kinds.clone()).unwrap_or_default(),
            "kind_models": setup.map_or(Value::Null, |s| s.routes.clone()),
            "contexts": setup
                .map(|s| s.slots.iter().map(|slot| json!({"name": slot.name, "kinds": slot.kinds})).collect::<Vec<_>>())
                .unwrap_or_default(),
            "ui_language": setup.map_or("auto", |s| &s.ui_language),
            "session": session_state(&state),
            "live": live_state(&state),
            "transcribers": transcribers_state(&state),
        });
        let mut events = vec![session];
        if let Some(setup) = setup {
            events.extend(setup.problems.iter().cloned());
        }
        events
    }
}

fn invalid_tag(params: Value) -> Event {
    error("tag.invalid", "a tag needs a name", params)
}

/// The session shown, or null.
fn session_state(state: &State) -> Value {
    state.session().map_or(Value::Null, session_json)
}

fn session_json(m: &Session) -> Value {
    json!({
        "id": m.id, "title": m.title, "kind": m.kind, "source": m.source, "language": m.language,
        "state": m.state, "started_at": m.started_at,
        "active_s": m.runs().counted, "running_since": m.runs().since,
        "people": m.person_ids(),
        "attendees": m.attendee_ids(),
        "contexts": m.contexts(),
        "tags": m.tags(),
        "translating": m.translation(),
    })
}

/// Each recording session, with what it listens with; none without a setup.
fn heard(state: &State) -> Vec<(String, Listening)> {
    let Some(setup) = state.setup.as_ref() else {
        return Vec::new();
    };
    state
        .recording()
        .map(|session| (session.id.clone(), setup.listening_of(session)))
        .collect()
}

/// The requests one transcriber bills under, one at a time: each is over when
/// the next opens or the transcriber stops.
pub struct Bills {
    assistant: Assistant,
    from: Listening,
    open: Mutex<Option<String>>,
}

impl Bills {
    /// The transcriber bills under `request` from now.
    pub fn opened(&self, request: String) {
        let previous = self
            .open
            .lock()
            .expect("not poisoned")
            .replace(request.clone());
        let mut state = self.assistant.state();
        let at = now();
        if let Some(previous) = previous {
            self.assistant.close_request(&mut state, &previous, at);
        }
        self.assistant
            .open_request(&mut state, &self.from, &request, at);
    }
}

impl Drop for Bills {
    fn drop(&mut self) {
        let open = self.open.get_mut().expect("not poisoned").take();
        if let Some(request) = open {
            let mut state = self.assistant.state();
            self.assistant.close_request(&mut state, &request, now());
        }
    }
}

/// What `setup` knows of the transcription model `model`'s price.
fn rate_in(setup: Option<&Setup>, model: &str) -> Rate {
    setup.map_or_else(Rate::default, |setup| Rate {
        per_minute: setup.transcription_prices.get(model).copied(),
        billed: setup.transcription_billing.contains_key(model),
    })
}

/// Tells clients what a session has cost, with each charge: asked for, or now
/// that it spent more.
fn cost_event(setup: Option<&Setup>, session: &Session) -> Event {
    let cost = session.spending(|model| rate_in(setup, model));
    json!({"type": "session_cost", "session": session.id, "cost": cost})
}

/// Every live session, in the order they started, so a client lists them in place.
fn live_state(state: &State) -> Value {
    let mut live: Vec<&Session> = state.live().collect();
    live.sort_by(|a, b| a.started_at.total_cmp(&b.started_at));
    Value::Array(live.iter().map(|m| session_json(m)).collect())
}

/// The transcribers the recording sessions keep running, each with how many
/// sessions it feeds.
fn transcribers_state(state: &State) -> Value {
    let Some(setup) = state.setup.as_ref() else {
        return json!([]);
    };
    let mut feeding: BTreeMap<Listening, usize> = BTreeMap::new();
    for session in state.recording() {
        *feeding.entry(setup.listening_of(session)).or_default() += 1;
    }
    let transcribers = feeding.into_iter().map(|(listening, sessions)| {
        json!({"model": listening.model, "language": listening.language, "sessions": sessions})
    });
    Value::Array(transcribers.collect())
}

fn suggestion_of(state: &State, id: &str) -> Option<Suggestion> {
    state
        .loaded
        .values()
        .find_map(|session| suggestion_in(session, id))
}

fn suggestion_in(session: &Session, id: &str) -> Option<Suggestion> {
    session.timeline.iter().find_map(|entry| match entry {
        Entry::Suggestion(s) if s.id == id => Some(s.clone()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use futures::stream::{self, BoxStream};

    use super::*;
    use crate::adapters::people_files::PeopleFiles;
    use crate::adapters::session_files::{NoSessionFiles, SessionFiles};
    use crate::domain::session::IMPORT;
    use crate::ports::{CompletionError, Message, Phrase};

    type Calls = Arc<Mutex<Vec<(String, Vec<Message>)>>>;

    #[derive(Clone, Default)]
    struct FakeLlm {
        deltas: Vec<&'static str>,
        fail: bool,
        delay: Duration,
        usage: Option<Usage>,
        calls: Calls,
    }

    impl LanguageModel for FakeLlm {
        fn stream(
            &self,
            model: &str,
            messages: Vec<Message>,
        ) -> BoxStream<'static, Result<Chunk, CompletionError>> {
            self.calls.lock().unwrap().push((model.into(), messages));
            let delay = self.delay;
            let mut items: Vec<Result<Chunk, CompletionError>> = self
                .deltas
                .iter()
                .map(|d| Ok(Chunk::Text((*d).into())))
                .collect();
            if let Some(usage) = self.usage.clone() {
                items.push(Ok(Chunk::Usage(usage)));
            }
            if self.fail {
                items.push(Err(CompletionError("429: rate limited".into())));
            }
            stream::iter(items)
                .then(move |item| async move {
                    tokio::time::sleep(delay).await;
                    item
                })
                .boxed()
        }
    }

    type Ran = Arc<Mutex<Vec<(String, String, String)>>>;

    /// Keeps every hook it is asked to run: (command, session id, title).
    #[derive(Clone, Default)]
    struct FakeHooks {
        ran: Ran,
        fail: bool,
    }

    impl Hooks for FakeHooks {
        fn run(
            &self,
            command: String,
            session_id: String,
            title: String,
        ) -> futures::future::BoxFuture<'static, Result<(), crate::ports::HookError>> {
            self.ran.lock().unwrap().push((command, session_id, title));
            let fail = self.fail;
            Box::pin(async move {
                match fail {
                    true => Err(crate::ports::HookError("401 from the CRM".into())),
                    false => Ok(()),
                }
            })
        }
    }

    fn no_hooks() -> Arc<dyn Hooks> {
        Arc::new(FakeHooks::default())
    }

    fn llm(deltas: &[&'static str]) -> FakeLlm {
        FakeLlm {
            deltas: deltas.to_vec(),
            ..FakeLlm::default()
        }
    }

    fn probe() -> Action {
        Action {
            name: "probe".into(),
            prompt: "Faça perguntas.".into(),
            format: "Lista numerada.".into(),
            model: Some("cheap".into()),
            ..Action::default()
        }
    }

    fn ask_action() -> Action {
        Action {
            name: "ask".into(),
            prompt: "Sugira uma resposta.".into(),
            format: "Tópicos.".into(),
            model: None,
            ..Action::default()
        }
    }

    fn setup(llm: &FakeLlm) -> Arc<Setup> {
        setup_with(Arc::new(llm.clone()), Vec::new())
    }

    fn setup_with(llm: Arc<dyn LanguageModel>, problems: Vec<Event>) -> Arc<Setup> {
        setup_slots(llm, problems, Vec::new())
    }

    fn setup_slots(
        llm: Arc<dyn LanguageModel>,
        problems: Vec<Event>,
        slots: Vec<ContextSlot>,
    ) -> Arc<Setup> {
        let cheap = Model {
            llm: Arc::clone(&llm),
            id: "cheap/model".into(),
            settings: String::new(),
        };
        let translator = Model {
            llm: Arc::clone(&llm),
            id: "translate/model".into(),
            settings: String::new(),
        };
        Arc::new(Setup {
            model: Model {
                llm,
                id: "default/model".into(),
                settings: String::new(),
            },
            models: HashMap::from([("cheap".into(), cheap)]),
            kind_models: HashMap::new(),
            translation: Translating {
                model: translator,
                kind_models: HashMap::new(),
            },
            routes: json!({"meeting": {"transcription": "whisper", "chat": "default"}}),
            actions: vec![probe(), ask_action()],
            rules: crate::domain::prompts::DEFAULT_RULES.into(),
            reviewer: None,
            context: "Sou dev.".into(),
            max_context_chars: 60_000,
            participants: vec![("Eu".into(), true), ("Recrutador".into(), false)],
            inputs: vec![json!({"id": "@default-input", "label": "Mic", "participant": "Eu"})],
            language: "pt".into(),
            languages: vec!["auto".into(), "pt".into(), "ja".into()],
            kinds: vec!["meeting".into(), "idea".into()],
            drop_echoes: true,
            ui_language: "ja-JP".into(),
            slots,
            transcription: HashMap::new(),
            default_transcription: "whisper".into(),
            transcription_prices: HashMap::new(),
            transcription_billing: HashMap::new(),
            problems,
            limit: Arc::new(Semaphore::new(8)),
        })
    }

    fn reviewed(draft: &FakeLlm, reviewer: &FakeLlm, verbose: bool) -> Arc<Setup> {
        let mut setup = Arc::into_inner(setup(draft)).expect("only reference");
        setup.reviewer = Some(Reviewer {
            model: Model {
                llm: Arc::new(reviewer.clone()),
                id: "review/model".into(),
                settings: String::new(),
            },
            prompt: "Revise.".into(),
            verbose,
        });
        Arc::new(setup)
    }

    type Events = Arc<Mutex<Vec<Event>>>;

    fn with_log(llm: &FakeLlm, log: Arc<dyn SessionLog>, session: bool) -> (Assistant, Events) {
        let events: Events = Arc::default();
        let sink = Arc::clone(&events);
        let people = Arc::new(PeopleFiles::new(std::env::temp_dir().join("eco-no-people")));
        let emit: Emit = Arc::new(move |event| sink.lock().unwrap().push(event));
        let assistant = Assistant::new(emit, log, people, no_hooks());
        assistant.configure(setup(llm));
        if session {
            assistant.start("Entrevista", "meeting", "pt", None);
        }
        events.lock().unwrap().clear();
        (assistant, events)
    }

    fn make(llm: &FakeLlm, session: bool) -> (Assistant, Events) {
        with_log(llm, Arc::new(NoSessionFiles), session)
    }

    async fn settle() {
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// The timeline of the session shown, as a window showing it gets it.
    fn shown_timeline(assistant: &Assistant) -> Vec<Value> {
        let state = assistant.state();
        timeline_events(state.session().expect("a session is shown"))
    }

    fn kinds(events: &Events) -> Vec<String> {
        events
            .lock()
            .unwrap()
            .iter()
            .map(|e| e["type"].as_str().unwrap().to_string())
            .collect()
    }

    /// What the test sessions listen with: the default model, in Portuguese.
    fn heard() -> Listening {
        Listening {
            model: "whisper".into(),
            language: "pt".into(),
        }
    }

    fn utterance(who: &str, text: &str) -> Utterance {
        Utterance {
            who: who.into(),
            phrases: vec![Phrase {
                start: 0.0,
                end: 1.0,
                text: text.into(),
            }],
            latency: Duration::from_millis(500),
        }
    }

    /// An assistant whose `probe` action has a hook, on its own when `auto`.
    fn hooked(
        llm: &FakeLlm,
        log: Arc<dyn SessionLog>,
        hooks: &FakeHooks,
        auto: bool,
    ) -> (Assistant, Events) {
        let events: Events = Arc::default();
        let sink = Arc::clone(&events);
        let people = Arc::new(PeopleFiles::new(std::env::temp_dir().join("eco-no-people")));
        let emit: Emit = Arc::new(move |event| sink.lock().unwrap().push(event));
        let assistant = Assistant::new(emit, log, people, Arc::new(hooks.clone()));
        let mut setup = Arc::into_inner(setup(llm)).expect("only reference");
        setup.actions[0].hook = "crm-push --board leads".into();
        setup.actions[0].hook_auto = auto;
        assistant.configure(Arc::new(setup));
        (assistant, events)
    }

    #[tokio::test]
    async fn a_finished_answer_runs_its_action_hook_on_its_own() {
        let hooks = FakeHooks::default();
        let (assistant, events) = hooked(
            &llm(&["Pergunte do prazo."]),
            Arc::new(NoSessionFiles),
            &hooks,
            true,
        );
        assistant.start("Kickoff Acme", "meeting", "pt", None);
        let id = assistant.state().session().unwrap().id.clone();
        assistant.trigger(None, "ask");
        settle().await;
        assert!(hooks.ran.lock().unwrap().is_empty(), "ask has no hook");
        assistant.trigger(None, "probe");
        settle().await;
        assert_eq!(
            *hooks.ran.lock().unwrap(),
            [(
                "crm-push --board leads".to_string(),
                id.clone(),
                "Kickoff Acme".to_string()
            )]
        );
        let sent: Vec<_> = kinds(&events)
            .into_iter()
            .filter(|k| k.starts_with("hook"))
            .collect();
        assert_eq!(sent, ["hook_started", "hook_sent"]);
        let snapshot = assistant.snapshot();
        assert_eq!(snapshot[0]["hooks"], json!(["probe"]));
    }

    #[tokio::test]
    async fn a_hook_that_waits_runs_only_when_the_answer_is_sent() {
        let directory = tempfile::tempdir().unwrap();
        let log: Arc<dyn SessionLog> = Arc::new(SessionFiles::new(directory.path().into()));
        let hooks = FakeHooks {
            fail: true,
            ..FakeHooks::default()
        };
        let (assistant, events) = hooked(&llm(&["Pergunte do prazo."]), log, &hooks, false);
        assistant.start("Kickoff Acme", "meeting", "pt", None);
        let id = assistant.state().session().unwrap().id.clone();
        assistant.trigger(None, "probe");
        assistant.trigger(None, "ask");
        settle().await;
        assert!(hooks.ran.lock().unwrap().is_empty());
        let answers: Vec<String> = assistant
            .state()
            .session()
            .unwrap()
            .timeline
            .iter()
            .filter_map(|entry| match entry {
                Entry::Suggestion(s) => Some(s.id.clone()),
                _ => None,
            })
            .collect();
        assistant.end(None);
        let last = || events.lock().unwrap().last().unwrap().clone();

        assistant.send(&id, &answers[1]);
        assert_eq!(last()["code"], "hook.none");
        assistant.send(&id, "missing");
        assert_eq!(last()["code"], "answer.not_found");
        assistant.send(&id, &answers[0]);
        settle().await;
        assert_eq!(hooks.ran.lock().unwrap().len(), 1);
        assert_eq!(hooks.ran.lock().unwrap()[0].1, id);
        assert_eq!(
            (last()["code"].as_str(), last()["params"]["detail"].as_str()),
            (Some("hook.failed"), Some("401 from the CRM"))
        );
    }

    #[test]
    fn search_finds_what_was_said_however_it_is_written() {
        let directory = tempfile::tempdir().unwrap();
        let log: Arc<dyn SessionLog> = Arc::new(SessionFiles::new(directory.path().into()));
        let mut said = Session::begin("Kickoff", "meeting", LIVE, "pt", log.writer());
        said.hear_at("Eles", "A reunião de prazo fica para sexta.", now());
        let mut removed = Session::begin("Daily", "meeting", LIVE, "pt", log.writer());
        let line = removed.hear_at("Eles", "O prazo mudou.", now());
        removed.unhear("Eles", line.at);
        let mut noted = Session::begin("Ideias", "idea", LIVE, "pt", log.writer());
        noted.note("Revisar PRAZOS com o time");
        let (assistant, events) = with_log(&llm(&[]), log, false);
        let found = |query: &str| {
            assistant.search(query);
            let event = events.lock().unwrap().last().unwrap().clone();
            assert_eq!(
                (event["type"].as_str(), event["query"].as_str()),
                (Some("sessions_found"), Some(query))
            );
            let mut ids: Vec<String> = serde_json::from_value(event["ids"].clone()).unwrap();
            ids.sort();
            ids
        };
        let mut both = [noted.id.clone(), said.id.clone()];
        both.sort();
        assert_eq!(found("Prázo"), both);
        assert_eq!(found("reuniao"), [said.id.clone()]);
        assert_eq!(found("ideias"), [noted.id.clone()]);
        assert!(found("mudou").is_empty(), "a removed line is not there");
    }

    #[test]
    fn tags_are_added_renamed_and_deleted_across_stored_sessions() {
        let directory = tempfile::tempdir().unwrap();
        let log: Arc<dyn SessionLog> = Arc::new(SessionFiles::new(directory.path().into()));
        let first = Session::begin("Kickoff", "meeting", LIVE, "pt", log.writer()).id;
        let second = Session::begin("Daily", "meeting", LIVE, "pt", log.writer()).id;
        let third = Session::begin("Ideias", "idea", LIVE, "pt", log.writer()).id;
        let (assistant, events) = with_log(&llm(&[]), Arc::clone(&log), false);
        let last = || events.lock().unwrap().last().unwrap().clone();
        let tags_of = |id: &str| {
            let records = log.read(id).unwrap();
            summarize(&records, |_| Rate::default()).unwrap()["tags"].clone()
        };

        assistant.tag(&first, "  Client   X ", true);
        let told: Vec<Value> = events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .take(2)
            .cloned()
            .collect();
        assert_eq!(
            told[1],
            json!({"type": "session_tags", "session": first, "tags": ["Client X"]})
        );
        assert_eq!(told[0]["tags"], json!([{"tag": "Client X", "sessions": 1}]));
        // A tag another session has keeps the casing it was first written in.
        assistant.tag(&second, "client x", true);
        assistant.tag(&second, "Q3", true);
        assistant.tag(&third, "job", true);
        assert_eq!(tags_of(&second), json!(["Client X", "Q3"]));
        assistant.tags();
        assert_eq!(
            last()["tags"],
            json!([{"tag": "Client X", "sessions": 2}, {"tag": "job", "sessions": 1}, {"tag": "Q3", "sessions": 1}])
        );

        assistant.tag(&second, "q3", false);
        assert_eq!(tags_of(&second), json!(["Client X"]));
        assistant.tag(&first, " ", true);
        assert_eq!(last()["code"], "tag.invalid");
        assistant.tag("nope", "x", true);
        assert_eq!(last()["code"], "session.not_found");

        events.lock().unwrap().clear();
        assistant.rename_tag("CLIENT x", "Job");
        let renamed = events.lock().unwrap()[0].clone();
        assert_eq!(renamed["type"], "tag_renamed");
        let mut changed: Vec<String> = serde_json::from_value(renamed["sessions"].clone()).unwrap();
        changed.sort();
        let mut all = vec![first.clone(), second.clone(), third.clone()];
        all.sort();
        assert_eq!(changed, all, "the tag renamed into is recased too");
        assert_eq!(tags_of(&first), json!(["Job"]));
        assert_eq!(tags_of(&third), json!(["Job"]));
        let kinds: Vec<Value> = events
            .lock()
            .unwrap()
            .iter()
            .map(|e| e["type"].clone())
            .collect();
        assert_eq!(
            kinds,
            [json!("tag_renamed"), json!("tags"), json!("sessions")]
        );

        assistant.delete_tag("job");
        assert!(
            events
                .lock()
                .unwrap()
                .iter()
                .any(|e| e["type"] == "tag_deleted")
        );
        for id in [&first, &second, &third] {
            assert_eq!(tags_of(id), json!([]));
        }
        assistant.delete_tag("job");
        assert_eq!(last()["code"], "tag.not_found");
    }

    #[test]
    fn a_live_session_shows_its_tags() {
        let directory = tempfile::tempdir().unwrap();
        let log: Arc<dyn SessionLog> = Arc::new(SessionFiles::new(directory.path().into()));
        let (assistant, _) = with_log(&llm(&[]), log, false);
        let id = assistant.start("Kickoff", "meeting", "pt", None);
        assistant.tag(&id, "Acme", true);
        assert_eq!(assistant.snapshot()[0]["session"]["tags"], json!(["Acme"]));
        assistant.rename_tag("acme", "ACME");
        assert_eq!(assistant.snapshot()[0]["session"]["tags"], json!(["ACME"]));
    }

    #[tokio::test]
    async fn lines_and_answers_are_translated_below_them_and_kept() {
        let directory = tempfile::tempdir().unwrap();
        let log: Arc<dyn SessionLog> = Arc::new(SessionFiles::new(directory.path().into()));
        let answers = llm(&["Resposta."]);
        let english = llm(&["In English."]);
        let (assistant, events) = with_log(&answers, Arc::clone(&log), false);
        let mut on = Arc::into_inner(setup(&answers)).expect("only reference");
        on.translation.model = Model {
            llm: Arc::new(english.clone()),
            id: "translate/model".into(),
            settings: String::new(),
        };
        assistant.configure(Arc::new(on));
        let translated = |language: &str| {
            events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e["type"] == "translated" && e["language"] == language)
                .cloned()
                .collect::<Vec<Value>>()
        };
        assistant.start("Daily", "meeting", "pt", None);
        let id = assistant.state().session().unwrap().id.clone();
        assistant.hear(&heard(), &utterance("Recrutador", "Bom dia a todos."));
        settle().await;
        assert!(
            translated("en").is_empty() && english.calls.lock().unwrap().is_empty(),
            "a session translates nothing until the user turns it on"
        );
        assert!(assistant.snapshot()[0]["session"]["translating"].is_null());

        assistant.set_translation(&id, "en");
        settle().await;
        assert_eq!(
            translated("en").len(),
            1,
            "turning it on translates the line held"
        );
        assistant.trigger(None, "ask");
        settle().await;
        let english_ones = translated("en");
        assert_eq!(english_ones.len(), 2, "the line and the answer");
        assert!(english_ones.iter().all(|e| e["text"] == "In English."));
        assert!(
            english_ones.iter().any(|e| e["at"].is_f64())
                && english_ones.iter().any(|e| e["id"].is_string())
        );
        let calls = english.calls.lock().unwrap().clone();
        assert!(calls[0].1[0].content.contains("English"));
        assert!(calls.iter().any(|c| c.1[1].content == "Bom dia a todos."));

        events.lock().unwrap().clear();
        assistant.set_translation(&id, "");
        let told = events
            .lock()
            .unwrap()
            .iter()
            .find(|e| e["type"] == "session_translation")
            .cloned()
            .unwrap();
        assert!(told["translating"].is_null());
        assistant.hear(&heard(), &utterance("Recrutador", "Até logo."));
        settle().await;
        assert!(
            translated("en").is_empty(),
            "turning it off stops translating"
        );

        assistant.set_translation(&id, "en");
        assert_eq!(
            translated("en").len(),
            2,
            "a language translated before shows again at once"
        );
        settle().await;
        assert_eq!(translated("en").len(), 3, "and what it lacks is translated");

        assistant.end(None);
        assistant.show(&id);
        let detail = events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|e| e["type"] == "session_detail")
            .cloned()
            .unwrap();
        assert_eq!(detail["timeline"][0]["translation"], "In English.");
        assert_eq!(detail["session"]["translating"], "en");

        assistant.set_translation(&id, "ja");
        settle().await;
        assert_eq!(
            translated("ja").len(),
            3,
            "turning a language on translates what the session already holds"
        );
        let refused = |language: &str| {
            assistant.set_translation(&id, language);
            events.lock().unwrap().last().unwrap()["code"].clone()
        };
        assert_eq!(refused("japanese"), "translation.unknown");
        assert_eq!(
            refused("zz"),
            "translation.unknown",
            "a code no language has"
        );
        assert_eq!(
            refused("pt"),
            "translation.own",
            "the session's own language"
        );
        assistant.show(&id);
        let detail = events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|e| e["type"] == "session_detail")
            .cloned()
            .unwrap();
        assert_eq!(
            detail["session"]["translating"], "ja",
            "a refused language leaves the translation as it was"
        );
    }

    #[tokio::test]
    async fn session_lifecycle_drives_recording() {
        let (assistant, events) = make(&llm(&[]), false);
        let listening = assistant.listening();
        let recording = || !listening.borrow().is_empty();
        assert!(!recording());
        assistant.start("Entrevista", "meeting", "pt", None);
        assert!(recording());
        assistant.pause(None);
        assert!(!recording());
        assistant.resume(None);
        assert!(recording());
        assistant.toggle();
        assert!(!recording());
        assistant.toggle();
        assistant.end(None);
        assert!(!recording());
        let states: Vec<Value> = events
            .lock()
            .unwrap()
            .iter()
            .map(|e| e["session"]["state"].clone())
            .collect();
        let expected = ["recording", "paused", "recording", "paused", "recording"].map(Value::from);
        assert_eq!(&states[..5], expected.as_slice());
        assert_eq!(states[5], Value::Null);
        assert_eq!(events.lock().unwrap()[0]["session"]["title"], "Entrevista");
    }

    #[tokio::test]
    async fn speech_is_kept_only_while_recording() {
        let (assistant, events) = make(&llm(&[]), true);
        assistant.hear(&heard(), &utterance("Recrutador", "Primeira."));
        assistant.pause(None);
        assistant.hear(&heard(), &utterance("Recrutador", "Durante a pausa."));
        let texts: Vec<Value> = events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e["type"] == "transcript")
            .map(|e| e["text"].clone())
            .collect();
        assert_eq!(texts, [json!("Primeira.")]);
        assert_eq!(events.lock().unwrap()[0]["latency_ms"], 500);
    }

    #[tokio::test]
    async fn actions_need_a_session() {
        let fake = llm(&["ok"]);
        let (assistant, events) = make(&fake, false);
        assistant.trigger(None, "ask");
        assert_eq!(events.lock().unwrap()[0]["code"], "session.none");
        assert!(fake.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn action_streams_a_suggestion_into_the_timeline() {
        let fake = llm(&["1. Qual ", "a escala?"]);
        let (assistant, events) = make(&fake, true);
        assistant.hear(
            &heard(),
            &utterance("Recrutador", "Como você faria o cache?"),
        );
        assistant.trigger(None, "probe");
        settle().await;
        let calls = fake.calls.lock().unwrap();
        let [(model, messages)] = calls.as_slice() else {
            panic!("one call")
        };
        assert_eq!(model, "cheap/model");
        assert!(
            messages[0]
                .content
                .contains("As falas de \"Eu\" são do próprio usuário.")
        );
        assert!(messages[0].content.contains("Sou dev."));
        let prompt = &messages.last().unwrap().content;
        assert!(prompt.contains("Recrutador: Como você faria o cache?"));
        assert!(prompt.contains("Faça perguntas.") && prompt.contains("Lista numerada."));
        assert_eq!(
            kinds(&events),
            [
                "transcript",
                "suggestion_start",
                "suggestion_delta",
                "suggestion_delta",
                "suggestion_end",
                "session_cost"
            ]
        );
        let events = events.lock().unwrap();
        let start = &events[1];
        assert_eq!(
            (start["action"].as_str(), start["model"].as_str()),
            (Some("probe"), Some("cheap/model"))
        );
        let answer = events[1..].iter().filter(|e| e["type"] != "session_cost");
        assert!(answer.into_iter().all(|e| e["id"] == start["id"]));
        let cost = events.last().unwrap();
        assert_eq!(
            (cost["session"].clone(), cost["cost"]["llm_unknown"].clone()),
            (start["session"].clone(), json!(true))
        );
        let suggestion = shown_timeline(&assistant).pop().unwrap();
        assert_eq!(
            (suggestion["text"].as_str(), suggestion["done"].as_bool()),
            (Some("1. Qual a escala?"), Some(true))
        );
    }

    #[tokio::test]
    async fn kept_suggestions_go_to_the_next_action_and_removed_ones_do_not() {
        let fake = llm(&["Use Redis."]);
        let (assistant, events) = make(&fake, true);
        assistant.trigger(None, "ask");
        settle().await;
        let first = events.lock().unwrap()[0]["id"]
            .as_str()
            .unwrap()
            .to_string();
        assistant.trigger(None, "ask");
        settle().await;
        {
            let calls = fake.calls.lock().unwrap();
            let roles: Vec<&str> = calls[1].1.iter().map(|m| m.role).collect();
            assert_eq!(roles, ["system", "user", "assistant", "user"]);
            assert_eq!(calls[1].1[2].content, "Use Redis.");
        }
        let second = events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|e| e["type"] == "suggestion_end")
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let session = events.lock().unwrap()[0]["session"]
            .as_str()
            .unwrap()
            .to_string();
        assistant.remove(&session, &first);
        assistant.remove(&session, &second);
        assert_eq!(
            kinds(&events)[kinds(&events).len() - 2..],
            ["suggestion_removed", "suggestion_removed"]
        );
        assistant.trigger(None, "ask");
        settle().await;
        let calls = fake.calls.lock().unwrap();
        assert_eq!(
            calls[2].1.iter().map(|m| m.role).collect::<Vec<_>>(),
            ["system", "user"]
        );
    }

    #[tokio::test]
    async fn a_note_in_a_stored_session_reaches_later_answers_until_removed() {
        let directory = tempfile::tempdir().unwrap();
        let log = Arc::new(SessionFiles::new(directory.path().join("sessions")));
        let store: Arc<dyn PeopleStore> =
            Arc::new(PeopleFiles::new(directory.path().join("people")));
        let events: Events = Arc::default();
        let sink = Arc::clone(&events);
        let emit: Emit = Arc::new(move |event| sink.lock().unwrap().push(event));
        let mut stored = Session::begin("Entrevista", "meeting", LIVE, "pt", log.writer());
        stored.hear_at("Recrutador", "Como você migrou?", now());
        let fake = llm(&["ok"]);
        let assistant = Assistant::new(emit, log.clone(), store, no_hooks());
        assistant.configure(setup(&fake));
        assistant.note(Some(&stored.id), "Migrei um app WPF para MVVM.");
        let note = events.lock().unwrap().last().unwrap().clone();
        assert_eq!(
            (note["type"].as_str(), note["session"].as_str()),
            (Some("note"), Some(stored.id.as_str()))
        );
        assistant.ask(Some(&stored.id), "Como respondo?");
        settle().await;
        let heard = |call: usize| fake.calls.lock().unwrap()[call].1[1].content.clone();
        assert!(heard(0).contains("Nota do usuário: Migrei um app WPF para MVVM."));
        assistant.remove(&stored.id, note["id"].as_str().unwrap());
        assert_eq!(
            events.lock().unwrap().last().unwrap()["type"],
            "note_removed"
        );
        let records = log.read(&stored.id).unwrap();
        assert_eq!(records.last().unwrap()["type"], "removed");
        assistant.ask(Some(&stored.id), "E agora?");
        settle().await;
        assert!(!heard(1).contains("Nota do usuário"));
    }

    #[tokio::test]
    async fn stored_sessions_stay_loaded_until_the_least_recently_used_leaves() {
        let directory = tempfile::tempdir().unwrap();
        let log = Arc::new(SessionFiles::new(directory.path().join("sessions")));
        let fake = llm(&["ok"]);
        let (assistant, _) = with_log(&fake, log.clone(), false);
        let ids: Vec<String> = (0..=LOADED_STORED)
            .map(|n| {
                let mut stored =
                    Session::begin(&format!("Reunião {n}"), "meeting", LIVE, "pt", log.writer());
                stored.hear_at("Ana", &format!("Linha {n}."), now());
                stored.id.clone()
            })
            .collect();
        for id in &ids[..LOADED_STORED] {
            assistant.note(Some(id), &format!("Nota de {id}."));
            assistant.ask(Some(id), "Resuma.");
            settle().await;
        }
        let loaded = |id: &String| assistant.state().loaded.contains_key(id);
        assert!(ids[..LOADED_STORED].iter().all(loaded));
        // Asking the first again makes the second the least recently used.
        assistant.ask(Some(&ids[0]), "E o prazo?");
        settle().await;
        assistant.ask(Some(&ids[LOADED_STORED]), "Resuma.");
        settle().await;
        assert!(loaded(&ids[0]) && loaded(&ids[LOADED_STORED]));
        assert!(!loaded(&ids[1]));
        // Back from its log, it holds its line, its note and its answer.
        assistant.ask(Some(&ids[1]), "E então?");
        settle().await;
        let calls = fake.calls.lock().unwrap();
        let heard = &calls.last().unwrap().1[1].content;
        assert!(heard.contains("Linha 1."));
        assert!(heard.contains(&format!("Nota de {}.", ids[1])));
        let session = &assistant.state().loaded[&ids[1]];
        let answers = session
            .timeline
            .iter()
            .filter(|e| matches!(e, Entry::Suggestion(s) if s.done));
        assert_eq!(answers.count(), 2);
    }

    #[tokio::test]
    async fn sessions_all_answering_stay_loaded_past_the_limit() {
        let directory = tempfile::tempdir().unwrap();
        let log = Arc::new(SessionFiles::new(directory.path().join("sessions")));
        let fake = FakeLlm {
            delay: Duration::from_millis(20),
            ..llm(&["a", "b"])
        };
        let (assistant, events) = with_log(&fake, log.clone(), false);
        let ids: Vec<String> = (0..=LOADED_STORED)
            .map(|n| {
                let mut stored =
                    Session::begin(&format!("Reunião {n}"), "meeting", LIVE, "pt", log.writer());
                stored.hear_at("Ana", "Bom dia.", now());
                stored.id.clone()
            })
            .collect();
        for id in &ids {
            assistant.ask(Some(id), "Resuma.");
        }
        assert_eq!(assistant.state().loaded.len(), LOADED_STORED + 1);
        tokio::time::sleep(Duration::from_millis(300)).await;
        let ends = kinds(&events)
            .iter()
            .filter(|k| *k == "suggestion_end")
            .count();
        assert_eq!(ends, LOADED_STORED + 1);
    }

    #[test]
    fn an_answer_kept_in_a_stored_session_is_removed_without_loading_it() {
        let directory = tempfile::tempdir().unwrap();
        let log = Arc::new(SessionFiles::new(directory.path().join("sessions")));
        let store: Arc<dyn PeopleStore> =
            Arc::new(PeopleFiles::new(directory.path().join("people")));
        let events: Events = Arc::default();
        let sink = Arc::clone(&events);
        let emit: Emit = Arc::new(move |event| sink.lock().unwrap().push(event));
        let mut stored = Session::begin("Daily", "meeting", LIVE, "pt", log.writer());
        let answer = stored.suggest("probe", "m", "probe", "Pedido");
        stored.finish(&answer.id);
        // A daemon started after the answer was kept: nothing is loaded.
        let assistant = Assistant::new(emit, log.clone(), store, no_hooks());
        assistant.remove(&stored.id, &answer.id);
        assert_eq!(
            events.lock().unwrap().last().unwrap()["type"],
            "suggestion_removed"
        );
        let records = log.read(&stored.id).unwrap();
        assert_eq!(records.last().unwrap()["type"], "removed");
        // Removing it again still says it is gone.
        assistant.remove(&stored.id, &answer.id);
        assert_eq!(
            events.lock().unwrap().last().unwrap()["type"],
            "suggestion_removed"
        );
    }

    #[tokio::test]
    async fn removing_the_streaming_suggestion_stops_it() {
        let fake = FakeLlm {
            delay: Duration::from_millis(50),
            ..llm(&["a", "b", "c"])
        };
        let (assistant, events) = make(&fake, true);
        assistant.trigger(None, "ask");
        tokio::time::sleep(Duration::from_millis(70)).await;
        let start = events.lock().unwrap()[0].clone();
        assistant.remove(
            start["session"].as_str().unwrap(),
            start["id"].as_str().unwrap(),
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!kinds(&events).contains(&"suggestion_end".to_string()));
        assert_eq!(kinds(&events).last().unwrap(), "suggestion_removed");
        assert!(shown_timeline(&assistant).is_empty());
    }

    #[tokio::test]
    async fn a_reviewer_rewrites_the_answer_before_anyone_sees_it() {
        let draft = llm(&["Rascunho ", "inventado."]);
        let reviewer = llm(&["Resposta ", "revisada."]);
        let (assistant, events) = make(&draft, true);
        assistant.configure(reviewed(&draft, &reviewer, false));
        assistant.trigger(None, "ask");
        settle().await;
        let shown: String = events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e["type"] == "suggestion_delta")
            .map(|e| e["text"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(shown, "Resposta revisada.");
        assert!(!kinds(&events).contains(&"suggestion_draft".to_string()));
        let calls = reviewer.calls.lock().unwrap();
        let (model, messages) = &calls[0];
        assert_eq!(model, "review/model");
        let tail: Vec<(&str, &str)> = messages[messages.len() - 2..]
            .iter()
            .map(|m| (m.role, m.content.as_str()))
            .collect();
        assert_eq!(
            tail,
            [("assistant", "Rascunho inventado."), ("user", "Revise.")]
        );
        let kept = shown_timeline(&assistant).last().unwrap().clone();
        assert_eq!(
            (kept["text"].as_str(), kept["draft"].as_str()),
            (Some("Resposta revisada."), Some(""))
        );
    }

    #[tokio::test]
    async fn answers_reviews_translations_and_listening_are_kept_to_cost_the_session() {
        let directory = tempfile::tempdir().unwrap();
        let log = Arc::new(SessionFiles::new(directory.path().join("sessions")));
        let usage = Usage {
            prompt_tokens: 10,
            cached_tokens: 0,
            cost_usd: Some(0.001),
        };
        let draft = FakeLlm {
            usage: Some(usage),
            ..llm(&["Rascunho."])
        };
        let reviewer = llm(&["Revisada."]);
        let (assistant, events) = with_log(&draft, log.clone(), true);
        let mut reviewing = Arc::into_inner(reviewed(&draft, &reviewer, false)).unwrap();
        reviewing.inputs.push(json!({"id": "@default-output"}));
        reviewing.transcription_prices = HashMap::from([("whisper".into(), 0.006)]);
        assistant.configure(Arc::new(reviewing));
        assistant.trigger(None, "ask");
        settle().await;
        let id = assistant.snapshot()[0]["session"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let answer = events
            .lock()
            .unwrap()
            .iter()
            .find(|e| e["type"] == "suggestion_end")
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        assistant.remove(&id, &answer);
        assistant.translate_answer(&id, &answer, "en");
        settle().await;

        let records = log.read(&id).unwrap();
        let heard: Vec<(Value, Value)> = records
            .iter()
            .filter(|r| r["type"] == "transcriber")
            .map(|r| (r["model"].clone(), r["inputs"].clone()))
            .collect();
        assert_eq!(
            heard,
            [(json!("whisper"), json!(1)), (json!("whisper"), json!(2))]
        );
        let spent: Vec<(Value, Value)> = records
            .iter()
            .filter(|r| r["type"] == "spent")
            .map(|r| (r["for"].clone(), r["usd"].clone()))
            .collect();
        // The reviewer reported no cost; a removed answer was paid all the same.
        assert_eq!(
            spent,
            [
                (json!("answer"), json!(0.001)),
                (json!("review"), Value::Null)
            ]
        );
        let told = events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e["type"] == "session_cost")
            .count();
        assert_eq!(told, 2);

        // Asked for, the cost comes with each charge, newest first.
        assistant.announce_cost(&id);
        let cost = events.lock().unwrap().last().unwrap()["cost"].clone();
        assert_eq!(
            (cost["llm_usd"].clone(), cost["llm_unknown"].clone()),
            (json!(0.001), json!(true))
        );
        assert_eq!(cost["transcription_unknown"], false);
        let completions: Vec<(Value, Value, Value)> = cost["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["for"] != "transcription")
            .map(|item| {
                (
                    item["for"].clone(),
                    item["usd"].clone(),
                    item["unknown"].clone(),
                )
            })
            .collect();
        assert_eq!(
            completions,
            [
                (json!("review"), Value::Null, json!("unreported")),
                (json!("answer"), json!(0.001), Value::Null)
            ]
        );
        let heard = cost["items"].as_array().unwrap().iter();
        assert!(
            heard
                .filter(|item| item["for"] == "transcription")
                .all(|item| item["estimate"] == true && item["model"] == "whisper"),
            "listening is costed at the user's price"
        );
        assistant.announce_cost("000000000000");
        assert_eq!(
            events.lock().unwrap().last().unwrap()["code"],
            "session.not_found"
        );
    }

    #[tokio::test]
    async fn a_translation_is_paid_in_its_session() {
        let directory = tempfile::tempdir().unwrap();
        let log = Arc::new(SessionFiles::new(directory.path().join("sessions")));
        let usage = Usage {
            prompt_tokens: 10,
            cached_tokens: 0,
            cost_usd: Some(0.0002),
        };
        let fake = FakeLlm {
            usage: Some(usage),
            ..llm(&["Hi."])
        };
        let (assistant, events) = with_log(&fake, log.clone(), true);
        assistant.trigger(None, "ask");
        settle().await;
        let end = events
            .lock()
            .unwrap()
            .iter()
            .find(|e| e["type"] == "suggestion_end")
            .unwrap()
            .clone();
        let id = assistant.snapshot()[0]["session"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        assistant.translate_answer(&id, end["id"].as_str().unwrap(), "ja");
        settle().await;
        let records = log.read(&id).unwrap();
        let translation = records
            .iter()
            .find(|r| r.get("for") == Some(&json!("translation")))
            .unwrap();
        assert_eq!(
            (translation["model"].clone(), translation["usd"].clone()),
            (json!("translate/model"), json!(0.0002))
        );
        assert!(
            events
                .lock()
                .unwrap()
                .iter()
                .any(|e| e["type"] == "translated" && e["language"] == "ja"),
            "a session that does not translate translates the answer into the language asked"
        );
        let last = events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|e| e["type"] == "session_cost")
            .unwrap()
            .clone();
        assert!((last["cost"]["llm_usd"].as_f64().unwrap() - 0.0004).abs() < 1e-12);
    }

    #[tokio::test]
    async fn an_action_answers_through_the_model_it_names() {
        let assistant_model = llm(&["Padrão."]);
        let cheap = llm(&["Barato."]);
        let (assistant, _) = make(&assistant_model, true);
        let mut named = Arc::into_inner(setup(&assistant_model)).expect("only reference");
        named.models.insert(
            "cheap".into(),
            Model {
                llm: Arc::new(cheap.clone()),
                id: "cheap/model".into(),
                settings: String::new(),
            },
        );
        assistant.configure(Arc::new(named));
        assistant.trigger(None, "probe");
        settle().await;
        assistant.trigger(None, "ask");
        settle().await;
        assert_eq!(cheap.calls.lock().unwrap()[0].0, "cheap/model");
        assert_eq!(assistant_model.calls.lock().unwrap()[0].0, "default/model");
        let texts: Vec<Value> = shown_timeline(&assistant)
            .iter()
            .map(|e| e["text"].clone())
            .collect();
        assert_eq!(texts, [json!("Barato."), json!("Padrão.")]);
    }

    /// A provider that charges US$ 0.02 a request, asked for each.
    #[derive(Default)]
    struct Charges(Mutex<Vec<String>>);

    impl TranscriptionBilling for Charges {
        fn cost(
            &self,
            request: &str,
        ) -> futures::future::BoxFuture<'static, Result<Option<f64>, crate::ports::BillingError>>
        {
            self.0.lock().unwrap().push(request.into());
            Box::pin(async { Ok(Some(0.02)) })
        }
    }

    fn billed_by(llm: &FakeLlm, charges: &Arc<Charges>) -> Arc<Setup> {
        let mut billed = Arc::into_inner(setup(llm)).unwrap();
        let billing: Arc<dyn TranscriptionBilling> = charges.clone();
        billed.transcription_billing = HashMap::from([("whisper".into(), billing)]);
        Arc::new(billed)
    }

    #[tokio::test(start_paused = true)]
    async fn a_transcription_request_costs_the_sessions_it_heard_their_share() {
        let directory = tempfile::tempdir().unwrap();
        let log: Arc<dyn SessionLog> = Arc::new(SessionFiles::new(directory.path().into()));
        let llm = llm(&[]);
        let (assistant, events) = with_log(&llm, log.clone(), false);
        let charges = Arc::new(Charges::default());
        assistant.configure(billed_by(&llm, &charges));
        let first = assistant.start("Daily", "meeting", "pt", None);
        let second = assistant.start("Kickoff", "meeting", "pt", None);
        let bills = assistant.bills(&heard());
        bills.opened("r1".into());
        std::thread::sleep(Duration::from_millis(2));
        // The transcriber stops: the request is over and its cost asked for.
        drop(bills);
        settle().await;
        tokio::time::sleep(Duration::from_secs(6)).await;
        assert_eq!(*charges.0.lock().unwrap(), ["r1"]);
        for id in [&first, &second] {
            let records = log.read(id).unwrap();
            let kept = |kind: &str, key: &str| {
                let record = records.iter().find(|r| r["type"] == kind).unwrap();
                record[key].clone()
            };
            assert_eq!(kept("billed", "share"), json!(0.5));
            assert_eq!(kept("spent", "usd"), json!(0.01));
            assert_eq!(kept("spent", "request"), json!("r1"));
        }
        let costed =
            events.lock().unwrap().iter().any(|e| {
                e["type"] == "session_cost" && e["cost"]["transcription_usd"] == json!(0.01)
            });
        assert!(costed, "clients are told what it cost now");

        // A request billed before the daemon stopped is priced with the next setup.
        let mut stopped = Session::begin("Call", "meeting", LIVE, "pt", log.writer());
        stopped.bill("r0", "whisper", (30.0, 1.0), now());
        assistant.configure(billed_by(&llm, &charges));
        tokio::time::sleep(Duration::from_secs(6)).await;
        assert_eq!(*charges.0.lock().unwrap(), ["r1", "r0"]);
        let records = log.read(&stopped.id).unwrap();
        let summary = summarize(&records, |_| Rate::default()).unwrap();
        assert_eq!(summary["cost"]["transcription_usd"], json!(0.02));
        assert_eq!(summary["cost"]["transcription_unknown"], false);

        // An imported file, a request per segment, is priced once it is in.
        let mut imported = Session::begin("Aula", "other", IMPORT, "pt", log.writer());
        imported.transcribe_with(HeardBy::Model("whisper".into(), 1, None));
        imported.bill("s1", "whisper", (9.0, 1.0), now());
        imported.bill("s2", "whisper", (4.0, 1.0), now());
        assistant.price_unpaid();
        tokio::time::sleep(Duration::from_secs(6)).await;
        assert_eq!(*charges.0.lock().unwrap(), ["r1", "r0", "s1", "s2"]);
        let records = log.read(&imported.id).unwrap();
        let summary = summarize(&records, |_| Rate::default()).unwrap();
        assert_eq!(summary["cost"]["transcription_usd"], json!(0.04));
    }

    /// The records of `kind` in a session's log.
    fn records_of(log: &Arc<dyn SessionLog>, id: &str, kind: &str) -> Vec<Record> {
        let records = log.read(id).unwrap();
        records.into_iter().filter(|r| r["type"] == kind).collect()
    }

    #[tokio::test(start_paused = true)]
    async fn a_request_open_when_the_daemon_stopped_is_billed_at_the_next_start() {
        let directory = tempfile::tempdir().unwrap();
        let log: Arc<dyn SessionLog> = Arc::new(SessionFiles::new(directory.path().into()));
        let llm = llm(&[]);
        let (assistant, _) = with_log(&llm, log.clone(), false);
        let charges = Arc::new(Charges::default());
        assistant.configure(billed_by(&llm, &charges));
        let first = assistant.start("Daily", "meeting", "pt", None);
        let second = assistant.start("Kickoff", "meeting", "pt", None);
        let bills = assistant.bills(&heard());
        bills.opened("r1".into());
        // Each session listening keeps it as the request opens.
        for id in [&first, &second] {
            let listening = records_of(&log, id, "listening");
            assert_eq!(listening.len(), 1);
            assert_eq!(listening[0]["request"], "r1");
        }
        std::thread::sleep(Duration::from_millis(2));
        assistant.hear(&heard(), &utterance("Eu", "Oi."));
        // The daemon stops with the request open: nothing closes it.
        std::mem::forget(bills);
        assert!(records_of(&log, &first, "billed").is_empty());

        let (next, _) = with_log(&llm, log.clone(), false);
        next.configure(billed_by(&llm, &charges));
        tokio::time::sleep(Duration::from_secs(6)).await;
        assert_eq!(*charges.0.lock().unwrap(), ["r1"]);
        for id in [&first, &second] {
            assert_eq!(records_of(&log, id, "billed")[0]["share"], json!(0.5));
            assert_eq!(records_of(&log, id, "spent")[0]["usd"], json!(0.01));
        }
        // Billed once: the start after does not bill it again.
        let (again, _) = with_log(&llm, log.clone(), false);
        again.configure(billed_by(&llm, &charges));
        tokio::time::sleep(Duration::from_secs(6)).await;
        assert_eq!(*charges.0.lock().unwrap(), ["r1"]);
        assert_eq!(records_of(&log, &first, "billed").len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_request_open_now_is_not_billed_when_the_setup_changes() {
        let directory = tempfile::tempdir().unwrap();
        let log: Arc<dyn SessionLog> = Arc::new(SessionFiles::new(directory.path().into()));
        let llm = llm(&[]);
        let (assistant, _) = with_log(&llm, log.clone(), false);
        let charges = Arc::new(Charges::default());
        assistant.configure(billed_by(&llm, &charges));
        let id = assistant.start("Daily", "meeting", "pt", None);
        let bills = assistant.bills(&heard());
        bills.opened("r1".into());
        assistant.configure(billed_by(&llm, &charges));
        tokio::time::sleep(Duration::from_secs(6)).await;
        assert!(charges.0.lock().unwrap().is_empty());
        assert!(records_of(&log, &id, "billed").is_empty());
        drop(bills);
        tokio::time::sleep(Duration::from_secs(6)).await;
        assert_eq!(*charges.0.lock().unwrap(), ["r1"]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_request_for_one_segment_is_shared_alike_by_the_sessions_listening() {
        let directory = tempfile::tempdir().unwrap();
        let log: Arc<dyn SessionLog> = Arc::new(SessionFiles::new(directory.path().into()));
        let llm = llm(&[]);
        let (assistant, _) = with_log(&llm, log.clone(), false);
        let charges = Arc::new(Charges::default());
        assistant.configure(billed_by(&llm, &charges));
        let first = assistant.start("Daily", "meeting", "pt", None);
        let second = assistant.start("Kickoff", "meeting", "pt", None);
        assistant.billed_segment(&heard(), "s1", 4.0);
        tokio::time::sleep(Duration::from_secs(6)).await;
        assert_eq!(*charges.0.lock().unwrap(), ["s1"]);
        for id in [&first, &second] {
            let billed = &records_of(&log, id, "billed")[0];
            assert_eq!(
                (&billed["seconds"], &billed["share"]),
                (&json!(4.0), &json!(0.5))
            );
            assert_eq!(records_of(&log, id, "spent")[0]["usd"], json!(0.01));
        }
    }

    #[test]
    fn the_price_a_session_is_heard_at_is_kept_as_it_changes() {
        let directory = tempfile::tempdir().unwrap();
        let log: Arc<dyn SessionLog> = Arc::new(SessionFiles::new(directory.path().into()));
        let llm = llm(&[]);
        let (assistant, _) = with_log(&llm, log.clone(), false);
        let priced = |price: f64| {
            let mut priced = Arc::into_inner(setup(&llm)).unwrap();
            priced.transcription_prices = HashMap::from([("whisper".into(), price)]);
            Arc::new(priced)
        };
        assistant.configure(priced(0.006));
        let id = assistant.start("Daily", "meeting", "pt", None);
        assistant.configure(priced(0.012));
        let prices: Vec<Value> = records_of(&log, &id, "transcriber")
            .iter()
            .map(|r| r["per_minute"].clone())
            .collect();
        assert_eq!(prices, [json!(0.006), json!(0.012)]);
    }

    fn named(id: &str) -> Model {
        Model {
            llm: Arc::new(llm(&[])),
            id: id.into(),
            settings: String::new(),
        }
    }

    #[test]
    fn a_skill_naming_a_model_answers_with_it_in_a_kind_with_its_own() {
        let mut own = Arc::into_inner(setup(&llm(&[]))).expect("only reference");
        own.kind_models.insert("idea".into(), named("local/model"));
        assert_eq!(own.model_for("idea", "probe").id, "cheap/model");
    }

    #[test]
    fn a_kind_model_answers_what_names_no_model_instead_of_the_default() {
        let mut own = Arc::into_inner(setup(&llm(&[]))).expect("only reference");
        own.kind_models.insert("idea".into(), named("local/model"));
        assert_eq!(own.model_for("idea", "ask").id, "local/model");
        assert_eq!(own.model_for("idea", "chat").id, "local/model");
        assert_eq!(own.model_for("meeting", "ask").id, "default/model");
    }

    #[test]
    fn without_skill_or_kind_model_the_default_answers() {
        let own = setup(&llm(&[]));
        assert_eq!(own.model_for("idea", "ask").id, "default/model");
        assert_eq!(own.model_for("idea", "chat").id, "default/model");
    }

    #[tokio::test]
    async fn a_kind_with_its_own_model_answers_questions_and_reviews_with_it() {
        let paid = llm(&["Pago."]);
        let local = llm(&["Local."]);
        let (assistant, _) = make(&paid, false);
        let mut own = Arc::into_inner(reviewed(&paid, &paid, false)).expect("only reference");
        let model = Model {
            llm: Arc::new(local.clone()),
            id: "local/model".into(),
            settings: String::new(),
        };
        own.kind_models.insert("idea".into(), model);
        assistant.configure(Arc::new(own));
        assistant.start("Ideias", "idea", "pt", None);
        assistant.trigger(None, "probe");
        settle().await;
        assistant.ask(None, "E agora?");
        settle().await;
        let ids = |fake: &FakeLlm| -> Vec<String> {
            fake.calls
                .lock()
                .unwrap()
                .iter()
                .map(|c| c.0.clone())
                .collect()
        };
        assert_eq!(ids(&paid), ["cheap/model"], "only the skill's own model");
        assert_eq!(
            ids(&local),
            ["local/model"; 3],
            "two reviews and the question"
        );
        let models: Vec<Value> = shown_timeline(&assistant)
            .iter()
            .map(|e| e["model"].clone())
            .collect();
        assert_eq!(models, [json!("cheap/model"), json!("local/model")]);
    }

    #[tokio::test]
    async fn a_verbose_reviewer_shows_and_keeps_the_draft() {
        let draft = llm(&["Rascunho."]);
        let reviewer = llm(&["Revisada."]);
        let (assistant, events) = make(&draft, true);
        assistant.configure(reviewed(&draft, &reviewer, true));
        assistant.trigger(None, "ask");
        settle().await;
        assert!(kinds(&events).contains(&"suggestion_draft".to_string()));
        let kept = shown_timeline(&assistant).last().unwrap().clone();
        assert_eq!(
            (kept["text"].as_str(), kept["draft"].as_str()),
            (Some("Revisada."), Some("Rascunho."))
        );
    }

    #[tokio::test]
    async fn a_failed_draft_never_reaches_the_reviewer() {
        let draft = FakeLlm {
            fail: true,
            ..llm(&["Meio"])
        };
        let reviewer = llm(&["Revisada."]);
        let (assistant, events) = make(&draft, true);
        assistant.configure(reviewed(&draft, &reviewer, false));
        assistant.trigger(None, "ask");
        settle().await;
        assert!(reviewer.calls.lock().unwrap().is_empty());
        let failed = events
            .lock()
            .unwrap()
            .iter()
            .any(|e| e["code"] == "completion.failed");
        assert!(failed);
    }

    #[tokio::test]
    async fn new_action_waits_for_the_streaming_one() {
        let fake = FakeLlm {
            delay: Duration::from_millis(50),
            ..llm(&["a", "b", "c"])
        };
        let (assistant, events) = make(&fake, true);
        assistant.trigger(None, "ask");
        tokio::time::sleep(Duration::from_millis(70)).await;
        assistant.trigger(None, "probe");
        assistant.trigger(None, "ask");
        tokio::time::sleep(Duration::from_millis(500)).await;
        let events_now = events.lock().unwrap().clone();
        let starts: Vec<&str> = events_now
            .iter()
            .filter(|e| e["type"] == "suggestion_start")
            .map(|e| e["action"].as_str().unwrap())
            .collect();
        // The newer request replaces the one waiting; the streaming one finishes.
        assert_eq!(starts, ["ask", "probe", "ask"]);
        let count = |kind: &str| kinds(&events).iter().filter(|k| *k == kind).count();
        assert_eq!(count("suggestion_removed"), 1);
        assert_eq!(count("suggestion_end"), 2);
        // The request that waited is sent with the answer it waited for.
        let calls = fake.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert!(
            calls[1]
                .1
                .iter()
                .any(|m| m.role == "assistant" && m.content == "abc")
        );
    }

    /// Three stored sessions, each with a line, kept in `log`.
    fn stored(log: &SessionFiles) -> Vec<String> {
        (0..3)
            .map(|n| {
                let mut session =
                    Session::begin(&format!("Reunião {n}"), "meeting", LIVE, "pt", log.writer());
                session.hear_at("Ana", "Bom dia.", now());
                session.id.clone()
            })
            .collect()
    }

    #[tokio::test]
    async fn sessions_answer_side_by_side_up_to_the_limit() {
        let directory = tempfile::tempdir().unwrap();
        let log = Arc::new(SessionFiles::new(directory.path().join("sessions")));
        let fake = FakeLlm {
            delay: Duration::from_millis(40),
            ..llm(&["a", "b", "c"])
        };
        let (assistant, events) = with_log(&fake, log.clone(), false);
        let mut two = Arc::into_inner(setup(&fake)).expect("only reference");
        two.limit = Arc::new(Semaphore::new(2));
        assistant.configure(Arc::new(two));
        let ids = stored(&log);
        for id in &ids {
            assistant.ask(Some(id), "Resuma.");
        }
        tokio::time::sleep(Duration::from_millis(60)).await;
        // Two stream at once; the third waits for one of them to end.
        assert_eq!(fake.calls.lock().unwrap().len(), 2);
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(fake.calls.lock().unwrap().len(), 3);
        let ends = kinds(&events)
            .iter()
            .filter(|k| *k == "suggestion_end")
            .count();
        assert_eq!(ends, 3);
    }

    #[tokio::test]
    async fn saving_the_config_stops_only_answers_whose_model_changed() {
        let directory = tempfile::tempdir().unwrap();
        let log = Arc::new(SessionFiles::new(directory.path().join("sessions")));
        let fake = FakeLlm {
            delay: Duration::from_millis(40),
            ..llm(&["a", "b", "c"])
        };
        let (assistant, events) = with_log(&fake, log.clone(), false);
        let ids = stored(&log);
        assistant.ask(Some(&ids[0]), "Resuma.");
        assistant.trigger(Some(&ids[1]), "probe");
        tokio::time::sleep(Duration::from_millis(20)).await;
        let mut saved = Arc::into_inner(setup(&fake)).expect("only reference");
        saved.models.get_mut("cheap").unwrap().settings = "another provider".into();
        assistant.configure(Arc::new(saved));
        tokio::time::sleep(Duration::from_millis(300)).await;
        let events = events.lock().unwrap();
        let session_of = |kind: &str| -> Vec<String> {
            let id = |e: &Event| e["id"].as_str().unwrap().to_string();
            let starts: HashMap<String, String> = events
                .iter()
                .filter(|e| e["type"] == "suggestion_start")
                .map(|e| (id(e), e["session"].as_str().unwrap().to_string()))
                .collect();
            let of = events.iter().filter(|e| e["type"] == kind);
            of.map(|e| starts[&id(e)].clone()).collect()
        };
        assert_eq!(session_of("suggestion_end"), [ids[0].clone()]);
        assert_eq!(session_of("suggestion_removed"), [ids[1].clone()]);
    }

    #[tokio::test]
    async fn context_slots_follow_the_kind_until_the_user_chooses() {
        let fake = llm(&["ok"]);
        let (assistant, events) = make(&llm(&[]), false);
        let slot = |name: &str, kinds: &[&str], text: &str| ContextSlot {
            name: name.into(),
            kinds: kinds.iter().map(|k| k.to_string()).collect(),
            text: text.into(),
        };
        let slots = vec![
            slot("entrevista", &["meeting"], "Currículo: C#, .NET."),
            slot("projetos", &[], "Projeto Matome."),
        ];
        assistant.configure(setup_slots(Arc::new(fake.clone()), Vec::new(), slots));
        let system = |fake: &FakeLlm| {
            fake.calls.lock().unwrap().last().unwrap().1[0]
                .content
                .clone()
        };
        // A meeting starts with the slot of its kind on.
        assistant.start("Entrevista", "meeting", "pt", None);
        assistant.trigger(None, "ask");
        settle().await;
        let sent = system(&fake);
        assert!(sent.contains("Sou dev.\n\n## entrevista\nCurrículo: C#, .NET."));
        assert!(!sent.contains("Projeto Matome."));
        // The user's choice replaces it, is recorded, and clients hear of it.
        let id = assistant.snapshot()[0]["session"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        assistant.set_contexts(&id, &["projetos".to_string()]);
        assert_eq!(
            events.lock().unwrap().last().unwrap()["contexts"],
            json!(["projetos"])
        );
        assert_eq!(
            assistant.snapshot()[0]["session"]["contexts"],
            json!(["projetos"])
        );
        assistant.trigger(None, "ask");
        settle().await;
        let sent = system(&fake);
        assert!(sent.contains("## projetos\nProjeto Matome.") && !sent.contains("Currículo"));
        assert_eq!(
            assistant.snapshot()[0]["contexts"][0],
            json!({"name": "entrevista", "kinds": ["meeting"]})
        );
    }

    #[tokio::test]
    async fn an_unavailable_model_keeps_the_setup_and_says_why() {
        let (assistant, events) = make(&llm(&[]), true);
        let problem = error(
            "model.unavailable",
            "assistant: OPENROUTER_API_KEY is not set",
            json!({}),
        );
        let unavailable =
            crate::adapters::llm_openai::Unavailable("OPENROUTER_API_KEY is not set".into());
        assistant.configure(setup_with(Arc::new(unavailable), vec![problem]));
        // Actions and participants still reach clients, and so does why the model is missing.
        let snapshot = assistant.snapshot();
        assert_eq!(snapshot[0]["actions"], json!(["probe", "ask"]));
        assert_eq!(
            snapshot[0]["participants"][0],
            json!({"name": "Eu", "user": true})
        );
        assert_eq!(snapshot[1]["code"], "model.unavailable");
        // An action fails with that reason.
        assistant.trigger(None, "ask");
        settle().await;
        let failure = events.lock().unwrap().iter().rev().nth(1).unwrap().clone();
        assert_eq!(failure["code"], "completion.failed");
        assert_eq!(failure["params"]["detail"], "OPENROUTER_API_KEY is not set");
    }

    #[tokio::test]
    async fn an_empty_answer_is_a_failure() {
        let (assistant, events) = make(&llm(&[]), true);
        assistant.trigger(None, "ask");
        settle().await;
        assert_eq!(
            kinds(&events)[kinds(&events).len() - 2..],
            ["error", "suggestion_removed"]
        );
        let failure = events.lock().unwrap().iter().rev().nth(1).unwrap().clone();
        assert_eq!(failure["code"], "completion.failed");
        assert_eq!(failure["params"]["detail"], "the model sent no answer");
    }

    #[tokio::test]
    async fn completion_failure_removes_the_suggestion() {
        let fake = FakeLlm {
            fail: true,
            ..llm(&["meio"])
        };
        let (assistant, events) = make(&fake, true);
        assistant.trigger(None, "ask");
        settle().await;
        // The reason comes before the removal, so a waiting client reads it.
        assert_eq!(
            kinds(&events)[kinds(&events).len() - 2..],
            ["error", "suggestion_removed"]
        );
        let failure = events.lock().unwrap().iter().rev().nth(1).unwrap().clone();
        let removed = events.lock().unwrap().last().unwrap().clone();
        assert_eq!(failure["code"], "completion.failed");
        // It names the answer that failed, so a client can show why on that answer.
        assert_eq!(
            failure["params"],
            json!({"id": removed["id"], "action": "ask", "detail": "429: rate limited"})
        );
        assert!(shown_timeline(&assistant).is_empty());
    }

    #[tokio::test]
    async fn unknown_action_reports_error() {
        let fake = llm(&[]);
        let (assistant, events) = make(&fake, true);
        assistant.trigger(None, "nope");
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            (events[0]["code"].as_str(), &events[0]["params"]),
            (Some("action.unknown"), &json!({"name": "nope"}))
        );
        assert!(fake.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn snapshot_carries_the_session_and_a_window_its_timeline() {
        let (assistant, _) = make(&llm(&[]), true);
        assistant.hear(&heard(), &utterance("Eu", "Eu usaria Redis."));
        let [session] = assistant.snapshot().try_into().unwrap();
        let [speech] = shown_timeline(&assistant).try_into().unwrap();
        assert_eq!(session["actions"], json!(["probe", "ask"]));
        assert_eq!(
            session["participants"],
            json!([{"name": "Eu", "user": true}, {"name": "Recrutador", "user": false}])
        );
        assert_eq!(
            session["inputs"],
            json!([{"id": "@default-input", "label": "Mic", "participant": "Eu"}])
        );
        assert_eq!(
            (session["language"].clone(), session["languages"].clone()),
            (json!("pt"), json!(["auto", "pt", "ja"]))
        );
        assert_eq!(session["ui_language"], "ja-JP");
        assert_eq!(
            (
                session["session"]["title"].clone(),
                session["session"]["state"].clone()
            ),
            (json!("Entrevista"), json!("recording"))
        );
        assert_eq!(
            (speech["who"].clone(), speech["text"].clone()),
            (json!("Eu"), json!("Eu usaria Redis."))
        );
    }

    #[tokio::test]
    async fn one_line_reaches_every_session_listening_alike() {
        let (assistant, events) = make(&llm(&[]), true);
        assistant.start("Outra", "meeting", "pt", None);
        assistant.start("Em inglês", "meeting", "en", None);
        let wanted: Vec<String> = assistant
            .listening()
            .borrow()
            .iter()
            .map(|l| format!("{} {}", l.model, l.language))
            .collect();
        assert_eq!(wanted, ["whisper en", "whisper pt"]);
        events.lock().unwrap().clear();
        let heard_by = assistant.hear(&heard(), &utterance("Recrutador", "Bom dia."));
        // The two Portuguese sessions get it, at the same time; the English one does not.
        assert_eq!(heard_by.len(), 2);
        assert_eq!(heard_by[0].1, heard_by[1].1);
        let lines: Vec<(Value, Value)> = events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e["type"] == "transcript")
            .map(|e| (e["session"].clone(), e["at"].clone()))
            .collect();
        assert_eq!(lines.len(), 2);
        assert_ne!(lines[0].0, lines[1].0);
        let shown = assistant.snapshot()[0]["transcribers"].clone();
        assert_eq!(
            shown,
            json!([{"model": "whisper", "language": "en", "sessions": 1},
                   {"model": "whisper", "language": "pt", "sessions": 2}])
        );
    }

    #[tokio::test]
    async fn pausing_one_session_keeps_the_other_recording() {
        let (assistant, events) = make(&llm(&[]), true);
        let first = assistant.snapshot()[0]["session"]["id"].clone();
        assistant.start("Outra", "meeting", "pt", None);
        let second = assistant.snapshot()[0]["session"]["id"].clone();
        // The one shown is paused: the newest, until the first is shown again.
        assistant.pause(None);
        events.lock().unwrap().clear();
        assistant.hear(&heard(), &utterance("Recrutador", "Só na primeira."));
        let heard_in: Vec<Value> = events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e["type"] == "transcript")
            .map(|e| e["session"].clone())
            .collect();
        assert_eq!(heard_in, std::slice::from_ref(&first));
        assert_eq!(assistant.listening().borrow().len(), 1);
        let live: Vec<(Value, Value)> = assistant.snapshot()[0]["live"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (s["id"].clone(), s["state"].clone()))
            .collect();
        assert_eq!(
            live,
            [
                (first.clone(), json!("recording")),
                (second, json!("paused"))
            ]
        );
        // Ending the one shown shows the other again.
        events.lock().unwrap().clear();
        assistant.end(None);
        assert_eq!(assistant.snapshot()[0]["session"]["id"], first);
        assert_eq!(kinds(&events), ["session"]);
    }

    #[tokio::test]
    async fn a_window_addresses_its_own_session() {
        let (assistant, events) = make(&llm(&[]), true);
        let first = assistant.snapshot()[0]["session"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        assistant.hear(&heard(), &utterance("Recrutador", "Bom dia."));
        let second = assistant.start("Outra", "meeting", "pt", None);
        // The first is paused by name, though the second is the one shown.
        assistant.pause(Some(&first));
        let states: Vec<(Value, Value)> = assistant.snapshot()[0]["live"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (s["id"].clone(), s["state"].clone()))
            .collect();
        assert_eq!(
            states,
            [
                (json!(first), json!("paused")),
                (json!(second), json!("recording"))
            ]
        );
        // A window that shows the first asks for its timeline.
        events.lock().unwrap().clear();
        assistant.timeline(&first);
        let timeline = events.lock().unwrap()[0].clone();
        assert_eq!(timeline["type"], "session_timeline");
        assert_eq!(timeline["session"], json!(first));
        assert_eq!(timeline["timeline"][0]["text"], "Bom dia.");
        // Ending the first leaves the second shown and recording.
        assistant.end(Some(&first));
        assert_eq!(assistant.live(), std::slice::from_ref(&second));
        assert_eq!(assistant.snapshot()[0]["session"]["id"], json!(second));
    }

    #[tokio::test]
    async fn release_only_drops_the_current_setup() {
        let (assistant, events) = make(&llm(&[]), true);
        let old = assistant.state().setup.clone().unwrap();
        let new = setup(&llm(&["ok"]));
        assistant.configure(Arc::clone(&new));
        assistant.release(&old);
        assistant.trigger(None, "ask");
        settle().await;
        assert!(kinds(&events).contains(&"suggestion_end".to_string()));
        assistant.release(&new);
        assistant.trigger(None, "ask");
        assert_eq!(
            events.lock().unwrap().last().unwrap()["code"],
            "action.unknown"
        );
    }

    #[tokio::test]
    async fn free_questions_are_chat_turns_with_usage() {
        let usage = Usage {
            prompt_tokens: 1200,
            cached_tokens: 1024,
            cost_usd: Some(0.0004),
        };
        let fake = FakeLlm {
            usage: Some(usage),
            ..llm(&["Sim."])
        };
        let (assistant, events) = make(&fake, true);
        assistant.ask(None, "  Ele citou Kafka?  ");
        assistant.ask(None, "   ");
        settle().await;
        let events = events.lock().unwrap();
        let [start, _, end, cost] = events.as_slice() else {
            panic!("{events:?}")
        };
        assert_eq!(cost["cost"]["llm_usd"], 0.0004);
        assert_eq!(
            (start["action"].as_str(), start["prompt"].as_str()),
            (Some("chat"), Some("Ele citou Kafka?"))
        );
        assert!(
            fake.calls.lock().unwrap()[0]
                .1
                .last()
                .unwrap()
                .content
                .contains("Pergunta: Ele citou Kafka?")
        );
        assert_eq!(
            (
                end["prompt_tokens"].clone(),
                end["cached_tokens"].clone(),
                end["cost_usd"].clone()
            ),
            (json!(1200), json!(1024), json!(0.0004))
        );
        assert_eq!(
            shown_timeline(&assistant).last().unwrap()["prompt"],
            "Ele citou Kafka?"
        );
    }

    #[tokio::test]
    async fn open_and_stored_sessions_can_be_renamed() {
        let directory = tempfile::tempdir().unwrap();
        let fake = llm(&[]);
        let (assistant, events) = with_log(
            &fake,
            Arc::new(SessionFiles::new(directory.path().into())),
            false,
        );
        assistant.start("Rascunho", "idea", "pt", None);
        let id = assistant.snapshot()[0]["session"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        assistant.rename(&id, " Plano ", "meeting");
        let open = assistant.snapshot()[0]["session"].clone();
        assert_eq!(
            (open["title"].clone(), open["kind"].clone()),
            (json!("Plano"), json!("meeting"))
        );
        assistant.end(None);

        assistant.rename(&id, "Plano final", "idea");
        assert_eq!(
            events.lock().unwrap().last().unwrap().clone(),
            json!({"type": "session_renamed", "id": id, "title": "Plano final", "kind": "idea"})
        );
        assistant.sessions();
        let summary = events.lock().unwrap().last().unwrap()["sessions"][0].clone();
        assert_eq!(
            (summary["title"].clone(), summary["kind"].clone()),
            (json!("Plano final"), json!("idea"))
        );
        assistant.rename("nope", "x", "idea");
        assert_eq!(
            events.lock().unwrap().last().unwrap()["code"],
            "session.not_found"
        );
    }

    #[tokio::test]
    async fn a_closed_session_answers_and_keeps_its_answers() {
        let directory = tempfile::tempdir().unwrap();
        let fake = llm(&["Decidimos Redis."]);
        let (assistant, events) = with_log(
            &fake,
            Arc::new(SessionFiles::new(directory.path().into())),
            false,
        );
        assistant.start("Arquitetura", "meeting", "pt", None);
        assistant.hear(&heard(), &utterance("Eles", "Vamos de Redis."));
        let closed = assistant.snapshot()[0]["session"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        assistant.end(None);
        assistant.start("Outra", "idea", "pt", None);
        assistant.hear(&heard(), &utterance("Eu", "Ideia nova."));

        events.lock().unwrap().clear();
        assistant.trigger(Some(&closed), "ask");
        settle().await;
        let start = events.lock().unwrap()[0].clone();
        assert_eq!(
            (start["type"].as_str(), start["session"].as_str()),
            (Some("suggestion_start"), Some(closed.as_str()))
        );
        assert!(kinds(&events).contains(&"suggestion_end".to_string()));
        // The request carried the closed session, not the open one.
        let (_, messages) = fake.calls.lock().unwrap().last().unwrap().clone();
        assert!(messages[0].content.contains("Título: Arquitetura."));
        assert!(
            messages
                .last()
                .unwrap()
                .content
                .contains("Eles: Vamos de Redis.")
        );
        // The open session is untouched.
        assert_eq!(shown_timeline(&assistant).len(), 1);

        assistant.ask(Some(&closed), "O que decidimos?");
        settle().await;
        let answers: Vec<String> = events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e["type"] == "suggestion_start")
            .map(|e| e["id"].as_str().unwrap().to_string())
            .collect();
        assistant.remove(&closed, &answers[0]);
        assistant.show(&closed);
        let detail = events.lock().unwrap().last().unwrap().clone();
        let kept: Vec<&str> = detail["timeline"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["type"].as_str().unwrap())
            .collect();
        assert_eq!(kept, ["transcript", "suggestion"]);
        assert_eq!(detail["timeline"][1]["prompt"], "O que decidimos?");

        assistant.ask(Some("nope"), "x");
        assert_eq!(
            events.lock().unwrap().last().unwrap()["code"],
            "session.not_found"
        );
    }

    #[tokio::test]
    async fn speakers_are_renamed_in_open_and_stored_sessions() {
        let directory = tempfile::tempdir().unwrap();
        let (assistant, events) = with_log(
            &llm(&[]),
            Arc::new(SessionFiles::new(directory.path().into())),
            false,
        );
        assistant.start("Entrevista", "meeting", "pt", None);
        assistant.hear(&heard(), &utterance("Eles", "Oi."));
        let id = assistant.snapshot()[0]["session"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        assistant.rename_speaker(&id, "Eles", " Recrutador ");
        assistant.hear(&heard(), &utterance("Eles", "Tudo bem?"));
        let heard = events.lock().unwrap().last().unwrap().clone();
        assert_eq!(
            (heard["who"].clone(), heard["name"].clone()),
            (json!("Eles"), json!("Recrutador"))
        );
        assistant.end(None);

        assistant.rename_speaker(&id, "Eles", "Ana");
        assert_eq!(
            events.lock().unwrap().last().unwrap().clone(),
            json!({"type": "speaker_renamed", "session": id, "label": "Eles", "name": "Ana"})
        );
        let lines = assistant.transcript(&id).unwrap();
        assert!(lines.iter().all(|(_, name, _)| name == "Ana"));
        assistant.rename_speaker(&id, "Eles", "");
        assert_eq!(assistant.transcript(&id).unwrap()[0].1, "Eles");
        assistant.rename_speaker("nope", "Eles", "x");
        assert_eq!(
            events.lock().unwrap().last().unwrap()["code"],
            "session.not_found"
        );
    }

    #[tokio::test]
    async fn deleting_a_session_removes_its_file_and_voice_links() {
        let directory = tempfile::tempdir().unwrap();
        let log = Arc::new(SessionFiles::new(directory.path().join("sessions")));
        let people = Arc::new(PeopleFiles::new(directory.path().join("people")));
        let events: Events = Arc::default();
        let sink = Arc::clone(&events);
        let assistant = Assistant::new(
            Arc::new(move |event| sink.lock().unwrap().push(event)),
            log.clone(),
            people.clone(),
            no_hooks(),
        );
        assistant.configure(setup(&llm(&[])));
        assistant.start("Call", "meeting", "pt", None);
        assistant.hear(&heard(), &utterance("Eles", "Hello"));
        let id = assistant.snapshot()[0]["session"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        assistant.delete_session(&id);
        assert_eq!(
            events.lock().unwrap().last().unwrap()["code"],
            "session.live"
        );
        assistant.pause(None);
        assistant.delete_session(&id);
        assert_eq!(
            events.lock().unwrap().last().unwrap()["code"],
            "session.live"
        );
        assert!(log.read(&id).is_some());

        people
            .keep_voices(
                &id,
                &std::collections::BTreeMap::from([("Eles".into(), vec![1.0, 0.0])]),
            )
            .unwrap();
        assistant.assign_person(&id, "Eles", None, "Ij", None);
        assert_eq!(people.people()[0].voiceprints.len(), 1);
        assistant.end(None);
        assistant.show(&id);
        let detail = events.lock().unwrap().last().unwrap().clone();
        assert_eq!(detail["type"], "session_detail");
        assert!(detail["session"]["bytes"].as_u64().unwrap() > 0);
        assert!(
            detail["session"]["path"]
                .as_str()
                .unwrap()
                .contains("/sessions/")
        );

        assistant.delete_session(&id);
        assert!(log.read(&id).is_none());
        assert!(people.voices(&id).is_empty());
        assert!(people.people()[0].voiceprints.is_empty());
        assert!(
            events
                .lock()
                .unwrap()
                .iter()
                .any(|event| event["type"] == "session_deleted" && event["id"] == id)
        );
    }

    #[tokio::test]
    async fn a_stored_session_left_open_that_no_daemon_holds_is_interrupted() {
        let directory = tempfile::tempdir().unwrap();
        let log = Arc::new(SessionFiles::new(directory.path().into()));
        let mut left = Session::begin("Left", "meeting", LIVE, "pt", log.writer());
        left.set_state(PAUSED);
        let fake = llm(&[]);
        let (assistant, events) = with_log(&fake, log.clone(), false);
        assistant.start("Held", "meeting", "pt", None);
        assistant.pause(None);
        let held = assistant.snapshot()[0]["session"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let state_of = |id: &str| {
            assistant.sessions();
            let events = events.lock().unwrap();
            let listed = events.iter().rev().find(|e| e["type"] == "sessions");
            let sessions = listed.unwrap()["sessions"].as_array().unwrap();
            sessions.iter().find(|s| s["id"] == id).unwrap()["state"].clone()
        };
        assert_eq!(state_of(&held), "paused");
        assert_eq!(state_of(&left.id), "interrupted");
        assistant.show(&left.id);
        let detail = events.lock().unwrap().last().unwrap().clone();
        assert_eq!(detail["session"]["state"], "interrupted");

        assistant.reopen(&left.id);
        assert_eq!(state_of(&left.id), "recording");
        assistant.end(Some(&left.id));
        assert_eq!(state_of(&left.id), "ended");
    }

    #[tokio::test]
    async fn assigning_a_whole_session_updates_every_speaker_once() {
        let directory = tempfile::tempdir().unwrap();
        let log = Arc::new(SessionFiles::new(directory.path().join("sessions")));
        let people = Arc::new(PeopleFiles::new(directory.path().join("people")));
        let events: Events = Arc::default();
        let sink = Arc::clone(&events);
        let assistant = Assistant::new(
            Arc::new(move |event| sink.lock().unwrap().push(event)),
            log,
            people.clone(),
            no_hooks(),
        );
        assistant.configure(setup(&llm(&[])));
        assistant.start("Call", "meeting", "pt", None);
        assistant.hear(&heard(), &utterance("Eu", "Hello"));
        assistant.hear(&heard(), &utterance("Eles", "Hi"));
        let id = assistant.snapshot()[0]["session"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        events.lock().unwrap().clear();
        assistant.assign_all(&id, None, "Ana");
        assert_eq!(
            assistant
                .transcript(&id)
                .unwrap()
                .iter()
                .map(|(_, name, _)| name)
                .collect::<Vec<_>>(),
            ["Ana", "Ana"]
        );
        assert_eq!(people.people().len(), 1);
        assert_eq!(
            kinds(&events)
                .iter()
                .filter(|kind| *kind == "sessions")
                .count(),
            1
        );
        assert_eq!(events.lock().unwrap().last().unwrap()["speakers"], 2);
    }

    #[tokio::test]
    async fn speakers_become_people_across_sessions() {
        let directory = tempfile::tempdir().unwrap();
        let log = Arc::new(SessionFiles::new(directory.path().join("sessions")));
        let people = Arc::new(PeopleFiles::new(directory.path().join("people")));
        let events: Events = Arc::default();
        let sink = Arc::clone(&events);
        let emit: Emit = Arc::new(move |event| sink.lock().unwrap().push(event));
        let store: Arc<dyn PeopleStore> = people.clone();
        let assistant = Assistant::new(emit, log.clone(), store.clone(), no_hooks());
        let last = |kind: &str| {
            let events = events.lock().unwrap();
            events
                .iter()
                .rev()
                .find(|e| e["type"] == kind)
                .cloned()
                .unwrap()
        };
        // Two imported sessions, each with two diarized speakers and their voices.
        let mut ids = Vec::new();
        let sessions = [
            ("Daily", [[1.0, 0.0], [0.0, 1.0]]),
            ("Retro", [[0.95, 0.05], [0.0, 1.0]]),
        ];
        for (title, voices) in sessions {
            let mut session = Session::begin(title, "meeting", IMPORT, "pt", log.writer());
            session.hear_at("Eu", "Bom dia.", now());
            session.hear_at("Eu", "Oi.", now());
            session.assign_speakers(&["Speaker 1".into(), "Speaker 2".into()]);
            let kept = [("Speaker 1", voices[0]), ("Speaker 2", voices[1])]
                .map(|(label, voice)| (label.to_string(), voice.to_vec()));
            store
                .keep_voices(&session.id, &kept.into_iter().collect())
                .unwrap();
            ids.push(session.id.clone());
        }

        assistant.assign_person(&ids[0], "Speaker 1", None, " Ana ", None);
        let ana = last("people")["people"][0].clone();
        assert_eq!(
            (ana["name"].clone(), ana["voices"].clone()),
            (json!("Ana"), json!(1))
        );
        assert_eq!(ana["sessions"], json!([ids[0]]));
        // The other session's first speaker sounds like Ana: she is suggested.
        assistant.show(&ids[1]);
        let speakers = last("session_detail")["speakers"].clone();
        assert_eq!(speakers[0]["suggestions"][0]["name"], "Ana");
        assert_eq!(speakers[1]["suggestions"], json!([]));
        let ana_id = ana["id"].as_str().unwrap().to_string();
        assistant.set_person_color(&ana_id, "#ffb000");
        assert_eq!(last("people")["people"][0]["color"], "#ffb000");
        assert_eq!(
            store
                .people()
                .iter()
                .find(|p| p.id == ana_id)
                .unwrap()
                .color,
            "#ffb000"
        );
        // A name already known is that person.
        assistant.assign_person(&ids[1], "Speaker 1", None, "Ana", None);
        assistant.assign_person(&ids[1], "Speaker 2", None, "Bruno", None);
        assert_eq!(last("people")["people"][0]["voices"], 2);

        assistant.rename_person(&ana_id, "Ana Paula");
        assistant.show(&ids[0]);
        assert_eq!(
            last("session_detail")["session"]["speakers"],
            json!(["Ana Paula", "Speaker 2"])
        );
        let bruno = store
            .people()
            .into_iter()
            .find(|p| p.name == "Bruno")
            .unwrap();
        assistant.merge_people(&ana_id, &bruno.id);
        assistant.show(&ids[1]);
        assert_eq!(
            last("session_detail")["session"]["speakers"],
            json!(["Ana Paula"])
        );
        assert_eq!(store.people().len(), 1);

        assistant.unassign_person(&ids[1], "Speaker 2");
        assert_eq!(last("speaker_renamed")["name"], "Speaker 2");
        assert_eq!(store.people()[0].voiceprints.len(), 2);

        assistant.forget_person(&ana_id);
        assert!(store.people().is_empty());
        assert!(
            !directory
                .path()
                .join(format!("people/{ana_id}.json"))
                .exists()
        );
        assert!(!store.voices(&ids[0]).contains_key("Speaker 1"));
        assistant.show(&ids[0]);
        assert_eq!(last("session_detail")["speakers"][0]["person"], Value::Null);
        assert_eq!(last("session_detail")["speakers"][0]["name"], "Ana Paula");
        assistant.forget_person("nobody");
        assert_eq!(last("error")["code"], "person.not_found");
    }

    #[test]
    fn coloring_no_one_is_refused_once() {
        let directory = tempfile::tempdir().unwrap();
        let store = PeopleFiles::new(directory.path().join("people"));
        let events: Events = Arc::default();
        let sink = Arc::clone(&events);
        let emit: Emit = Arc::new(move |event| sink.lock().unwrap().push(event));
        let assistant = Assistant::new(emit, Arc::new(NoSessionFiles), Arc::new(store), no_hooks());
        assistant.set_person_color("nobody", "#ffb000");
        assert_eq!(
            *events.lock().unwrap(),
            [error(
                "person.not_found",
                "no person \"nobody\"",
                json!({"id": "nobody"})
            )]
        );
    }

    #[test]
    fn a_person_is_added_by_a_name_no_one_has() {
        let directory = tempfile::tempdir().unwrap();
        let log = Arc::new(SessionFiles::new(directory.path().join("sessions")));
        let store: Arc<dyn PeopleStore> =
            Arc::new(PeopleFiles::new(directory.path().join("people")));
        let events: Events = Arc::default();
        let sink = Arc::clone(&events);
        let emit: Emit = Arc::new(move |event| sink.lock().unwrap().push(event));
        let assistant = Assistant::new(emit, log, store.clone(), no_hooks());
        let last = || events.lock().unwrap().last().cloned().unwrap();

        assistant.add_person("  Sadao Maia ");
        assert_eq!(last()["type"], "people");
        assert_eq!(last()["people"][0]["name"], "Sadao Maia");
        assert_eq!(last()["people"][0]["voices"], 0);
        assert_eq!(last()["people"][0]["sessions"], json!([]));

        assistant.add_person("sadao maia");
        assert_eq!(last()["code"], "person.exists");
        assert_eq!(last()["params"]["name"], "Sadao Maia");
        assistant.add_person("   ");
        assert_eq!(last()["code"], "person.invalid");
        assert_eq!(store.people().len(), 1);
    }

    #[test]
    fn a_person_no_longer_stored_leaves_the_sessions_naming_them() {
        let directory = tempfile::tempdir().unwrap();
        let log = Arc::new(SessionFiles::new(directory.path().join("sessions")));
        let store: Arc<dyn PeopleStore> =
            Arc::new(PeopleFiles::new(directory.path().join("people")));
        let events: Events = Arc::default();
        let sink = Arc::clone(&events);
        let emit: Emit = Arc::new(move |event| sink.lock().unwrap().push(event));
        let assistant = Assistant::new(emit, log.clone(), store.clone(), no_hooks());
        let mut session = Session::begin("Aula", "meeting", IMPORT, "en", log.writer());
        session.hear_at("Eu", "Hello.", now());
        session.assign_speakers(&["Speaker 1".into()]);
        let id = session.id.clone();
        drop(session);
        assistant.assign_person(&id, "Speaker 1", None, "Paulo", None);
        assistant.set_attendee(&id, None, "Ana", true);
        let gone: Vec<String> = store.people().into_iter().map(|p| p.id).collect();
        for person in &gone {
            store.forget(person).unwrap();
        }

        for person in &gone {
            assistant.forget_person(person);
        }
        assistant.show(&id);
        let detail = events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|e| e["type"] == "session_detail")
            .cloned()
            .unwrap();
        assert_eq!(detail["session"]["people"], json!([]));
        assert_eq!(detail["speakers"][0]["person"], Value::Null);
        assert_eq!(detail["speakers"][0]["name"], "Paulo");
        assistant.forget_person(&gone[0]);
        let last = events.lock().unwrap().last().cloned().unwrap();
        assert_eq!(last["code"], "person.not_found");
    }

    #[test]
    fn live_voices_are_guessed_until_the_user_decides() {
        use crate::domain::diarization::Turn;

        let directory = tempfile::tempdir().unwrap();
        let log = Arc::new(SessionFiles::new(directory.path().join("sessions")));
        let store: Arc<dyn PeopleStore> =
            Arc::new(PeopleFiles::new(directory.path().join("people")));
        let events: Events = Arc::default();
        let sink = Arc::clone(&events);
        let emit: Emit = Arc::new(move |event| sink.lock().unwrap().push(event));
        let assistant = Assistant::new(emit, log.clone(), store.clone(), no_hooks());
        let last = |kind: &str| {
            let events = events.lock().unwrap();
            let found = events.iter().rev().find(|e| e["type"] == kind);
            found.cloned().unwrap()
        };
        let mut ana = Person::new("Ana");
        ana.enroll("earlier", "Speaker 1", vec![1.0, 0.0, 0.0]);
        store.save(&ana).unwrap();
        let turn = |start, end, speaker| Turn {
            start,
            end,
            speaker,
        };

        // Two voices among the others: the lines split, nobody is named.
        let mut call = Session::begin("Call", "meeting", LIVE, "pt", log.writer());
        call.hear_at("Eles", "Oi.", 10.0);
        call.hear_at("Eu", "Olá.", 11.0);
        call.hear_at("Eles", "Tudo bem?", 12.0);
        call.hear_at("Eles", "Vamos.", 14.0);
        let diarization = Diarization {
            turns: vec![turn(0.0, 3.0, 0), turn(3.0, 6.0, 1)],
            voices: vec![normalized(vec![0.99, 0.1, 0.0]), vec![0.0, 0.0, 1.0]],
        };
        let lines = [(10.0, 0.5, 2.5), (12.0, 3.5, 5.0), (14.0, 1.0, 2.0)];
        assistant.voices_heard(&call.id, "Eles", &lines, &diarization);
        assert_eq!(
            last("diarized")["who"],
            json!(["Speaker 1", "Eu", "Speaker 2", "Speaker 1"])
        );
        assistant.show(&call.id);
        let speakers = last("session_detail")["speakers"].clone();
        assert_eq!(speakers[0]["person"], Value::Null);
        assert_eq!(speakers[0]["guess"]["name"], "Ana");
        assert_eq!(speakers[2]["guess"], Value::Null);

        // Cleared, the guess stays gone; Ana is still offered by hand.
        assistant.clear_guess(&call.id, "Speaker 1");
        assistant.show(&call.id);
        let speakers = last("session_detail")["speakers"].clone();
        assert_eq!(speakers[0]["guess"], Value::Null);
        assert_eq!(speakers[0]["suggestions"][0]["name"], "Ana");
        assistant.clear_guess(&call.id, "Speaker 1");
        assert_eq!(last("error")["code"], "speaker.invalid");

        // Confirmed, the speaker is Ana and her voice grows.
        assistant.assign_person(&call.id, "Speaker 1", Some(&ana.id), "", None);
        assistant.show(&call.id);
        assert_eq!(
            last("session_detail")["speakers"][0]["person"],
            json!(ana.id)
        );
        assert_eq!(store.people()[0].voiceprints.len(), 2);

        // One voice keeps the participant's label, and is guessed too.
        let mut chat = Session::begin("Chat", "meeting", LIVE, "pt", log.writer());
        chat.hear_at("Eles", "Bom dia.", 30.0);
        let alone = Diarization {
            turns: vec![turn(0.0, 2.0, 0)],
            voices: vec![vec![1.0, 0.0, 0.0]],
        };
        assistant.voices_heard(&chat.id, "Eles", &[(30.0, 0.0, 2.0)], &alone);
        assistant.show(&chat.id);
        let speakers = last("session_detail")["speakers"].clone();
        assert_eq!(speakers[0]["label"], "Eles");
        assert_eq!(speakers[0]["guess"]["name"], "Ana");
    }

    #[test]
    fn adopts_only_named_live_speakers_once() {
        let directory = tempfile::tempdir().unwrap();
        let log = Arc::new(SessionFiles::new(directory.path().join("sessions")));
        let people = Arc::new(PeopleFiles::new(directory.path().join("people")));
        let events: Events = Arc::default();
        let sink = Arc::clone(&events);
        let emit: Emit = Arc::new(move |event| sink.lock().unwrap().push(event));
        let assistant = Assistant::new(emit, log.clone(), people.clone(), no_hooks());
        for source in [LIVE, IMPORT] {
            let mut session = Session::begin(source, "meeting", source, "en", log.writer());
            session.hear_at("Others", "Hello.", now());
            session.rename_speaker("Others", "Ana");
        }
        assistant.adopt_live_speakers();
        let adopted = events.lock().unwrap().last().unwrap().clone();
        assert_eq!(adopted["count"], 1);
        assert_eq!(people.people().len(), 1);
        let sessions = log.all();
        let links: Vec<_> = sessions
            .iter()
            .map(|records| {
                let session = Session::restore(records, Box::new(|_| {})).unwrap();
                (session.source.clone(), session.person_ids())
            })
            .collect();
        assert_eq!(
            links
                .iter()
                .find(|(source, _)| source == LIVE)
                .unwrap()
                .1
                .len(),
            1
        );
        assert!(
            links
                .iter()
                .find(|(source, _)| source == IMPORT)
                .unwrap()
                .1
                .is_empty()
        );
        assistant.adopt_live_speakers();
        assert_eq!(events.lock().unwrap().last().unwrap()["count"], 0);
    }

    #[tokio::test]
    async fn the_others_heard_by_the_microphone_are_not_the_user() {
        let (assistant, events) = make(&llm(&[]), true);
        let theirs = "O Windhawk tá chegando a nossa versão 2.0 e tá ficando realmente incrível.";
        let leaked = "O WinkYou tá chegando na versão 2.0 e tá ficando realmente incrível.";
        // After theirs: dropped.
        assistant.hear(&heard(), &utterance("Recrutador", theirs));
        assistant.hear(&heard(), &utterance("Eu", leaked));
        // Before theirs: taken back when theirs arrives.
        assistant.hear(
            &heard(),
            &utterance(
                "Eu",
                "Esse gerenciador de mods de código aberto pro Windows é ótimo.",
            ),
        );
        assistant.hear(
            &heard(),
            &utterance(
                "Recrutador",
                "Esse aqui é um gerenciador de mods de código aberto pro Windows.",
            ),
        );
        // The user's own words stay.
        assistant.hear(
            &heard(),
            &utterance("Eu", "Eu uso faz tempo, gosto bastante dele."),
        );
        let events = events.lock().unwrap().clone();
        let heard: Vec<String> = events
            .iter()
            .filter(|e| e["type"] == "transcript")
            .map(|e| {
                format!(
                    "{}: {}",
                    e["who"].as_str().unwrap(),
                    &e["text"].as_str().unwrap()[..12]
                )
            })
            .collect();
        assert_eq!(
            heard,
            [
                "Recrutador: O Windhawk t",
                "Eu: Esse gerenci",
                "Recrutador: Esse aqui é",
                "Eu: Eu uso faz t"
            ]
        );
        let removed: Vec<&Value> = events
            .iter()
            .filter(|e| e["type"] == "transcript_removed")
            .collect();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0]["who"], "Eu");
        let lines = shown_timeline(&assistant);
        let timeline = lines.iter().filter(|e| e["type"] == "transcript").count();
        assert_eq!(timeline, 3);
    }

    #[tokio::test]
    async fn a_live_speaker_can_be_someone_known() {
        let directory = tempfile::tempdir().unwrap();
        let store: Arc<dyn PeopleStore> = Arc::new(PeopleFiles::new(directory.path().into()));
        let mut ana = Person::new("Ana");
        ana.enroll("old", "Speaker 1", vec![1.0, 0.0]);
        store.save(&ana).unwrap();
        let events: Events = Arc::default();
        let sink = Arc::clone(&events);
        let assistant = Assistant::new(
            Arc::new(move |event| sink.lock().unwrap().push(event)),
            Arc::new(NoSessionFiles),
            store.clone(),
            no_hooks(),
        );
        assistant.configure(setup(&llm(&[])));
        assistant.start("Call", "meeting", "pt", None);
        assistant.hear(&heard(), &utterance("Recrutador", "Bom dia."));
        let id = assistant.snapshot()[0]["session"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        assistant.assign_person(&id, "Recrutador", Some(&ana.id), "", None);
        let renamed = events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|e| e["type"] == "speaker_renamed")
            .cloned();
        assert_eq!(renamed.unwrap()["name"], "Ana");
        // No voice was kept for a live speaker: Ana's voices are unchanged.
        assert_eq!(store.people()[0].voiceprints.len(), 1);
        assistant.announce_speakers(&id);
        let speakers = events.lock().unwrap().last().unwrap()["speakers"].clone();
        assert_eq!(speakers[0]["person"], json!(ana.id));
        assert_eq!(speakers[0]["voice"], false);
    }

    #[tokio::test]
    async fn a_line_can_be_corrected() {
        let (assistant, events) = make(&llm(&[]), true);
        assistant.hear(&heard(), &utterance("Recrutador", "O WinkYou chegou."));
        let id = assistant.snapshot()[0]["session"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let at = events.lock().unwrap()[0]["at"].as_f64().unwrap();
        assistant.edit_line(&id, "Recrutador", at, " O Windhawk chegou. ");
        let last = events.lock().unwrap().last().unwrap().clone();
        assert_eq!(last["type"], "transcript_edited");
        assert_eq!(last["text"], "O Windhawk chegou.");
        let lines: Vec<Value> = shown_timeline(&assistant)
            .into_iter()
            .filter(|e| e["type"] == "transcript")
            .collect();
        assert_eq!(lines[0]["text"], "O Windhawk chegou.");
        // Emptied, a line is removed.
        assistant.edit_line(&id, "Recrutador", at, "  ");
        assert!(kinds(&events).contains(&"transcript_removed".to_string()));
    }

    #[tokio::test]
    async fn any_line_can_be_removed() {
        let (assistant, events) = make(&llm(&[]), true);
        assistant.hear(&heard(), &utterance("Recrutador", "Bom dia."));
        assistant.hear(&heard(), &utterance("Eu", "Bom dia, tudo bem?"));
        let id = assistant.snapshot()[0]["session"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let at = events.lock().unwrap()[0]["at"].as_f64().unwrap();
        assistant.remove_line(&id, "Recrutador", at);
        let last = events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|event| event["type"] == "transcript_removed")
            .unwrap()
            .clone();
        assert_eq!(
            last,
            json!({"type": "transcript_removed", "session": id, "who": "Recrutador", "at": at})
        );
        let left: Vec<Value> = shown_timeline(&assistant)
            .into_iter()
            .filter(|e| e["type"] == "transcript")
            .collect();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0]["who"], "Eu");
        assistant.remove_line(&id, "Recrutador", at);
        assert_eq!(
            events.lock().unwrap().last().unwrap()["code"],
            "line.not_found"
        );
    }

    #[tokio::test]
    async fn sessions_can_be_listed_shown_and_resumed() {
        let directory = tempfile::tempdir().unwrap();
        let fake = llm(&["Use Redis."]);
        let (assistant, events) = with_log(
            &fake,
            Arc::new(SessionFiles::new(directory.path().into())),
            false,
        );
        assistant.start("Entrevista", "meeting", "ja", None);
        let japanese = Listening {
            language: "ja".into(),
            ..heard()
        };
        assistant.hear(&japanese, &utterance("Recrutador", "Oi."));
        assistant.trigger(None, "ask");
        settle().await;
        let id = assistant.snapshot()[0]["session"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        assistant.pause(None);
        assistant.reopen(&id); // still open: it stays paused
        assert_eq!(assistant.snapshot()[0]["session"]["state"], "paused");
        assistant.end(None);

        events.lock().unwrap().clear();
        assistant.sessions();
        let summary = events.lock().unwrap().last().unwrap()["sessions"][0].clone();
        assert_eq!(
            (
                summary["title"].clone(),
                summary["state"].clone(),
                summary["suggestions"].clone()
            ),
            (json!("Entrevista"), json!("ended"), json!(1))
        );
        assistant.show(&id);
        let detail = events.lock().unwrap().last().unwrap().clone();
        let timeline: Vec<&str> = detail["timeline"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["type"].as_str().unwrap())
            .collect();
        assert_eq!(timeline, ["transcript", "suggestion"]);
        assistant.reopen(&id); // ended, kept going
        let resumed = &assistant.snapshot()[0]["session"];
        assert_eq!(
            (&resumed["state"], &resumed["language"]),
            (&json!("recording"), &json!("ja"))
        );
        assert!(!assistant.listening().borrow().is_empty());
        assistant.end(None);

        assistant.start("Outra", "meeting", "pt", None);
        let other = assistant.snapshot()[0]["session"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        assistant.hear(&heard(), &utterance("Recrutador", "Antes da pausa."));
        assistant.pause(None);
        // The daemon stopped with it paused.
        let mut state = assistant.state();
        state.loaded.clear();
        state.live.clear();
        drop(state);
        events.lock().unwrap().clear();
        assistant.reopen(&other);
        assert_eq!(assistant.snapshot()[0]["session"]["language"], "pt");
        assert!(!assistant.listening().borrow().is_empty());
        assert_eq!(kinds(&events), ["session"]);
        assistant.hear(&heard(), &utterance("Recrutador", "Depois."));
        let texts: Vec<Value> = shown_timeline(&assistant)
            .iter()
            .map(|e| e["text"].clone())
            .collect();
        assert_eq!(texts, [json!("Antes da pausa."), json!("Depois.")]);
    }

    #[tokio::test]
    async fn an_unreadable_session_log_does_not_hide_the_others() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("2026-01-01-000000-quebrada.jsonl"),
            "{\"type\": \"session\"}\n",
        )
        .unwrap();
        let (assistant, events) = with_log(
            &llm(&[]),
            Arc::new(SessionFiles::new(directory.path().into())),
            false,
        );
        assistant.start("Boa", "meeting", "pt", None);
        assistant.end(None);
        events.lock().unwrap().clear();
        assistant.sessions();
        let events = events.lock().unwrap();
        let titles: Vec<&str> = events[0]["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["title"].as_str().unwrap())
            .collect();
        assert_eq!(titles, ["Boa"]);
        assert_eq!(
            (events[1]["code"].as_str(), &events[1]["params"]),
            (Some("session.unreadable"), &json!({"count": 1}))
        );
    }
}
