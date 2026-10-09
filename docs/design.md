# How it is built

This page is for contributors. Setting eco up belongs in the
[README](../README.md), each task in [the how-to pages](README.md), every
command and flag in [the command line](cli.md), the latency and audio
measurements in [benchmarks](benchmarks.md), and the language packs in
[translations](i18n.md); this one is the model, the
architecture, the protocol and the state on disk that those pages rest on.

The section numbers below are load-bearing. Code comments, error messages and
the other pages cite them as `docs/design.md §N`, so a section keeps its number:
a new one is added after the last, never in between.

---

## 1. The map

eco keeps voice sessions beside a conversation on the laptop. During a meeting
(Meet, Zoom, Teams, Discord, etc.), a conversation, a class or a spoken idea, it:

1. Captures the audio the laptop plays and, optionally, the user's microphone.
2. Transcribes it in real time, each line labelled with who said it.
3. Answers with an LLM, from the transcript and context the user gives, when the
   user asks: a skill, a free question, a summary afterwards.
4. Shows everything in a discreet window that never steals focus or gets in the
   way of the call.

Live or afterwards is the same session: a stored one keeps answering.

### Target environment

| Item | Value |
|---|---|
| OS | Arch Linux (Omarchy), Hyprland/Wayland — Hyprland config in **Lua** (`~/.config/hypr/*.lua`) |
| Audio | PipeWire 1.6 (`pw-record` available); `ffmpeg`/`ffprobe` to import files |
| Daemon | Rust (edition 2024, tokio), toolchain pinned by `mise`, built with `cargo` |
| Interface | QML on **Quickshell** (already installed; the omarchy-shell is built on it) |
| Terminal | Ghostty |
| Hardware | 12 cores, 46 GB RAM, **no dedicated GPU** — Radeon 860M iGPU (Vulkan) + NPU (`/dev/accel0`, unused) |
| LAN model server (development) | Windows machine with a Radeon RX 9060 XT serving models over HTTP — see [how to use a model on the LAN](how-to-use-a-model-on-the-lan.md) |

---

## 2. The model

Six nouns, and the rest of the page is what happens to them.

**Session** — everything eco hears: a title, a **kind** (one of `kinds` in the
config — `meeting`, `conversation`, `other`, `idea` by default), a **source**
(`live`, or `import` for a file) and a language. Nothing is transcribed or stored
outside a session. §7.

**Audio source** — a name in the conversation (`Eu`, `Recrutador`…;
`[[participants]]` in the config) holding one or more PipeWire devices. One may
be marked as the user. Each device is its own stream, so a line already knows
whose it is. §4.

**Model** — registered once in `[[models]]`: a name, a type (`chat` or
`transcription`), the provider, its key and the provider's id. Everything else
names a model rather than a provider. §6.

**Action** — a user-defined request run against the transcript: a name, a
prompt, an output format and, optionally, a model. The interface calls them
**skills**; the config (`[[actions]]`), the socket and the CLI say action. §6.

**Person** — someone the user names, kept apart from any session, with up to 16
voiceprints. A session's speaker label becomes a person only when the user says
so. §7.4.

**Tag** — the user's grouping of sessions, several per session, apart from kinds
and people. §7.5.

---

## 3. Architecture

Two processes:

```
┌────────────────────────────── eco (Rust daemon) ─────────────────────────────┐
│                                                                              │
│  AudioSource ──PCM──▶ STT ──text──▶ ┌───────────── core ─────────────┐       │
│  (pw-record /         (Deepgram /   │ session: buffer, window        │       │
│   WAV file)            whisper.cpp /│ actions                        │──▶ LLM (OpenAI-compatible)
│                        Groq…)       │ prompts                        │       │
│                                     └───────────────┬────────────────┘       │
│                                                     │                        │
│                                  TranscriptStore ◀──┴──▶ EventSink           │
└──────────────────────────────────────────────────────────┬───────────────────┘
                                                           │ JSON lines
                                                           ▼ $XDG_RUNTIME_DIR/eco.sock
                                              ┌────────────────────────────┐
                                              │ eco-overlay (Quickshell)   │
                                              │ floating Hyprland window   │
                                              └────────────────────────────┘
```

The daemon does capture, VAD, transcription, prompts and the LLM call. The
overlay only renders events: it never calls a provider, never holds a secret and
never writes the config — it sends a draft and the daemon validates and saves it.

**Rust (daemon) + QML/Quickshell (overlay).**

- The first version was written in Python for the fastest iteration on the
  prompt, the segmenter and the interface. Once it was stable the daemon was
  rewritten in Rust for a small, always-on process (memory, start-up, no
  interpreter); the socket protocol did not change, so the overlay was
  untouched.
- The daemon's work is almost all I/O (network, STT, LLM); latency is dominated
  by external services, not by the language.
- Two processes (daemon ↔ overlay over a socket) let either side be rewritten
  without touching the other.
- The global shortcut does not start the daemon: it talks to the socket through
  `socat`.

### 3.1 Ports and adapters (light hexagonal)

The core (`src/domain/`) knows no external service; it only talks to ports
defined as traits in `src/ports.rs`. No dependency-injection framework —
composition happens in `src/session.rs` from `config.toml`. The crate forbids
`unsafe`.

| Port | Adapters | Why |
|---|---|---|
| `AudioSource` | `pw-record` (any input, or what any sink plays), **WAV file** | The file adapter replays recorded meetings (`--replay <file.wav>`) to tune prompt and trigger and for automated tests. |
| `STT` | Deepgram (streaming), ElevenLabs Scribe (streaming), OpenAI-compatible transcription (`/v1/audio/transcriptions`: LAN whisper.cpp server, Groq, OpenAI) | There is no common real-time STT standard: each protocol needs its own adapter. |
| `LLM` | a single OpenAI-compatible adapter | Covers OpenRouter, OpenAI, Groq, Ollama, llama.cpp — switching = `base_url` + key + model. |
| `EventSink` | Unix socket (overlay), text on stdout | stdout is written only when it is a terminal (`mise run start`); under the user service it is the journal, which never gets transcript, note or answer text. |
| `TranscriptStore` | file in `~/.local/share/eco/`, null (`--no-save`) | |

Rule: only add a port when two real implementations exist or a test clearly
benefits.

### 3.2 Project layout

```
eco/
├── AGENTS.md
├── Cargo.toml
├── mise.toml
├── README.md
├── docs/
├── scripts/                 # one job per script, consumed by mise tasks
├── src/
│   ├── main.rs              # the command line and its dispatch
│   ├── cli.rs               # session commands over the socket (§11)
│   ├── lifecycle.rs         # start, stop and status of the user service
│   ├── setup.rs, skill.rs   # `eco setup`: models and the agent skill
│   ├── import.rs            # a file into a session (§7.7)
│   ├── config.rs            # config.toml: schema, validation, atomic save (§9)
│   ├── session.rs           # adapter composition + orchestration
│   ├── ports.rs             # traits
│   ├── domain/              # segmenter, sessions, assistant, prompts, people, billing
│   ├── adapters/            # audio, vad, stt, llm, socket, terminal, overlay, files
│   └── bench/               # the measurements behind docs/benchmarks.md
├── overlay/                 # Quickshell QML and language packs
├── packaging/               # Hyprland rules, user service, launcher, icon
├── skills/eco/SKILL.md      # the agent skill, embedded in the binary
├── benchmark/               # the answer benchmark, a self-contained tool
└── tests/                   # i18n checks and the Python-parity fixtures
```

---

## 4. Audio

- **Others:** monitor of the default sink (`@DEFAULT_MONITOR@`) via `pw-record`.
  Optional: `target` to capture only the meeting app's node (avoids
  notifications/music).
- **Me:** default microphone source (`@DEFAULT_SOURCE@`).
- **Audio sources:** the user groups devices into audio sources (`Eu`,
  `Recrutador`…; `[[participants]]` in the config); each device is its own
  stream, labelled with its audio source, so no diarization is needed. One
  audio source may be marked as the user (THIS IS ME).
  Devices missing from `pw-dump` are reported and skipped, since `pw-record`
  silently falls back to the default device for an unknown target.
- Format: mono, 16 kHz, s16le, in ~100 ms blocks.
- **VAD:** Silero (the If-less ONNX export) on `tract`, pure Rust — no ONNX
  Runtime, no PyTorch. Used to cut silence, detect end of speech, and **only
  send speech segments to the STT** (lowers cost).
- **Outside a session** the inputs are only measured: each 32 ms frame's
  loudness and whether it is speech reach the clients as `signal`, for the input
  traces (§12.4). Nothing is transcribed or stored.
- **Live voices:** while a session records, each input that is not the
  user's sends its speech to a child `eco diarize` (when `eco setup` installed
  the speaker model), which keeps only embeddings in memory; nothing reaches
  disk. When the capture stops (pause or end), each of that input's lines goes
  to the voice that said most of it: a voice close (≥ 0.70) to one the session
  already keeps takes that speaker's label, a single voice keeps the
  audio source's, and others become `Speaker N` — a `diarized` record. The
  voices are kept like an import's, so their speakers get guesses.
- **Echoes:** without headphones, or with headphones loud, the user's
  microphone hears the call and duplicates "their" speech as "me". Headphones
  are recommended; two answers are built in. With `[audio] drop_echoes` (on by default; CAPTURE › YOUR
  MICROPHONE › DROP ECHOES), a line of the user's whose words mostly follow, in
  order, what the others said in the last 20 s (at least 60% of them, and 4
  words or more) is their audio: dropped when it comes after theirs, taken back
  when it came first (an `unheard` record; clients get `transcript_removed`).
  Live sessions only; imports have one stream. With `[audio] echo_cancel`
  (CAPTURE › YOUR MICROPHONE › ECHO CANCELLATION, off by default), while a
  session records eco runs PipeWire's `libpipewire-module-echo-cancel` (WebRTC,
  `monitor.mode`) inside a `pw-cli` it owns: the user's first microphone is
  captured through the module's source, which takes out what the **default
  output** plays (the reference cannot be another output). Pausing, ending or
  stopping kills the `pw-cli` and every `eco.aec.*` node goes with it; those
  nodes are never listed as devices. If PipeWire refuses, `echo_cancel.failed`
  is reported and the raw microphone is used. It costs ~3% of one core while a
  session records.
- **Errors show:** a failed transcription or a capture that stops mid-session
  reaches the overlay's status line, and the terminal when the daemon runs in
  one.

---

## 5. Transcription

| Backend | Pros | Cons |
|---|---|---|
| **Deepgram nova-3 (streaming)** | Phrases ~1.6 s after speech ends (AMI); files over HTTPS at ~17× | Paid, audio leaves the machine, own WebSocket protocol |
| **ElevenLabs Scribe v2 Realtime (streaming)** | Phrases ~1.1 s after speech ends (AMI); files streamed | Paid, audio leaves the machine, own WebSocket protocol |
| **whisper.cpp + Vulkan on the LAN server** — development | Free, private, fast on the RX 9060 XT | Per segment (no real streaming); only at home |
| **Groq/OpenAI (`/v1/audio/transcriptions`)** | Cheap, fast, same format as the LAN server | Per segment (no streaming) |
| **Gemini (audio straight into the LLM)** | Removes the STT | Audio input via OpenRouter unverified — not built until validated |

- `faster-whisper` dropped: CTranslate2 only accelerates on CUDA (NVIDIA).
- **Choice:** the transcription model's `base_url` picks the adapter. An
  `http(s)://` URL is the
  OpenAI-compatible endpoint, segment by segment (up to four segments in
  flight, lines kept in order). `wss://api.deepgram.com/v1/listen` and
  `wss://api.elevenlabs.io/v1/speech-to-text/realtime` (or a regional host)
  are streaming providers: while a session records, every frame goes to them as
  16 kHz PCM and they decide where a phrase ends (Deepgram: 300 ms of silence,
  or 1 s by `UtteranceEnd`; Scribe: its VAD at 0.6 s); outside a session nothing
  is sent — the local VAD only measures the inputs. The local VAD still cuts
  segments, for imports' diarization. A dropped connection that had worked is
  opened again after a second, its phrases timed after the audio sent
  before; one that never took audio is reported and not retried. An import
  streams to Scribe with waiting frames joined into chunks of up to a second
  (frame by frame it refuses them as too frequent); Deepgram streams only as
  fast as the audio plays, so an import posts each segment to the same path
  over HTTPS with `utterances=true`. The config window's presets fill both;
  their model lists come from Deepgram's `/v1/models` (streaming ones) and
  Scribe's one realtime model.
- **Words as they are said:** a streaming provider's interim results
  (Deepgram `is_final: false`, with the finals held so far; Scribe
  `partial_transcript`) reach the clients as `transcript_partial {who, name,
  text}` and are never stored. The open session shows, dimmed below its lines,
  what each speaker is saying; it gives way to the line when the phrase ends,
  and goes when the session pauses or ends. Segment-by-segment STTs show a line
  only when it is finished.
- **Phrases:** the OpenAI-compatible adapter asks for `verbose_json`, whose
  timed segments become phrases (whisper.cpp, Groq and `whisper-1` give them);
  a server that refuses it (400, e.g. `gpt-4o-transcribe`) is asked for plain
  `json` from then on, the whole segment one phrase. Imports keep a line per
  phrase; a live session joins a segment's phrases into one line.
- **Language:** `stt.languages` lists the codes the user works in (`pt`, `en`,
  `ja`…, plus `auto`, which leaves detection to the server per segment) and
  `stt.language` is the one new sessions start in. Each session is transcribed
  in its own language: the overlay's dropdown offers exactly that list, each
  code with its native name from Qt, and switches the open session mid-meeting
  (`session.language <json>` with `{"id","language"}` on the socket; a
  `language` record), changing only that session's transcriber; the config
  window edits the list and the default. The LAN server runs with `-l auto` so
  each request decides.
- **Transcribers:** each input is captured once, into a hub; a transcriber
  reads it for one model in one language — the model of a session's kind, in
  the session's language — and every recording session listening with the same
  pair shares it. One starts with the first session that needs it and stops
  with the last; another pair runs beside it on the same audio. Starting,
  pausing or ending a session never restarts the capture while another
  records.

---

## 6. Answers

An **action** is a user-defined request run against the transcript. "Suggest a
reply" is just the `ask` action; a recruiter's technical question can call a
`probe` action that suggests grounded questions to ask back, each with why it
matters.

- **Provider:** any OpenAI-compatible API (OpenRouter, the LAN `llama-server`,
  Groq, Ollama), SSE streaming. Models are registered once in `[[models]]`
  — a name, a type (`chat` or `transcription`), the provider, its key and the
  provider's id — and the assistant, the reviewer and actions name the chat
  model they use, `[stt]` the transcription model sessions use by default. A
  model may serve session kinds (`kinds`): each kind has at most one model of
  each type, and a session of that kind transcribes with it and answers with
  it instead of the assistant's model — questions, actions that name no model,
  and the reviewer's pass; an action that names a model always answers with
  it — so an `idea` session can run on local models. If such
  a model is down, its sessions say so; nothing falls back to another model.
  Each model's provider-specific fields live in its `extra` and are sent as-is.
  Reasoning a model streams before its answer
  (`reasoning_content` from llama.cpp and LM Studio, `reasoning` from
  OpenRouter) is shown while it lasts and never kept. A request fails only when
  the model sends nothing for 120 s (10 s to connect), so a long answer that
  keeps arriving is never cut off. A model that cannot be set up — its key
  missing, say — leaves the rest of eco working: audio sources, actions and
  capture are configured as usual, every client is told why
  (`model.unavailable`, with the model's name, and likewise `stt.unavailable`
  for transcription and `context.unavailable` for an unreadable context file),
  and an action fails with that reason. `completion.failed` carries the
  answer's `id` and why. A reply that is not a stream is read whole: the answer
  when it is one, otherwise the server's own error (with a hint when the base
  URL has no path, as servers usually serve under `/v1`); an answer that ends
  without a word fails too, rather than leaving an empty card.
- **Rules:** `rules` holds the rules every answer follows (language,
  brevity, never putting facts in the user's mouth); the config window edits
  them. An action's prompt and format override them, so a new action states
  its own behaviour instead of changing the shared rules.
- **Reviewer:** with `[reviewer] enabled`, every answer — an action or a free
  question — is first drafted unseen by its model, then the reviewer's `model`
  reads the same conversation, the draft and the
  review `prompt`, and only its rewrite streams into the card and is kept. It is
  there to cut what the transcript, notes and context do not support. A failed
  draft fails the answer before the reviewer is called. `verbose` also streams
  the draft (`suggestion_draft`) and keeps it on the answer, shown dimmed above
  it, to inspect what the reviewer changed. A reviewer whose model cannot be set
  up fails answers with that reason until it is fixed or turned off.
- **Default model:** `[llm] model` names the assistant's; an action's `model`
  names another (e.g. a slower, better model for a deep action). Only the models
  something uses are set up. Latency measurements: [benchmarks.md](benchmarks.md).
- **Prompt, built for the provider's cache:** a request is a conversation that
  only grows at its end. The system message (framing, `rules`, user context)
  comes first, then every earlier turn rebuilt byte for byte — the speech heard
  since the turn before, the exact request text (kept on each suggestion), and
  the kept answer — and finally the new speech and the new request. OpenAI,
  Gemini, DeepSeek and llama.cpp reuse such a prefix on their own; for Anthropic
  models the adapter adds `cache_control` at the end of the system message and
  of the last kept answer. Removing a suggestion changes the prefix only from
  that turn on. When the context outgrows `max_context_chars`, its start jumps
  forward until it fills 60% of the budget, so it moves once per overflow.
  Each answer reports prompt tokens, cached tokens and cost when the provider
  sends them (`stream_options.include_usage`).
- **Context of a request:** the session's kind and title, its speech and kept
  answers within `max_context_chars` (see above), plus the action's prompt and
  format or the user's question.
- **Context slots:** a slot is a name, files and the kinds that start with it
  on. A session nobody chose for has the slots of its kind on; the user turns
  them on and off per session (a `context` record, `{slots}`;
  `session.context <json>` with `{"id","contexts"}`, and `session_context` to
  clients; `eco context <id> --add/--remove` from the CLI). A request's system
  message carries the global context, then each slot on under `## <name>`.
  Files are read when the setup is built: an edit counts once the settings are
  saved or the daemon restarts; an unreadable one is reported
  (`context.unavailable`).
- **Free questions:** the composer sends `ask <text>`; the question and its
  answer form one removable card (action `chat`), labelled ASK where a skill's
  card carries the skill's name.
- **Notes:** the composer's note button, or Shift+Enter, sends `note <text>`
  instead: a fact the user writes during the conversation, kept in the timeline
  as a removable note and carried, among the lines heard, by every later request
  as `Nota do usuário: …`. It asks nothing; the next action or question uses it.
- **Translation:** each line, once saved, and each answer, once finished, can
  be translated below it, dimmer, by the translation model — a kind's own
  (`translates` on a chat model), `[translation] model`, or the assistant's.
  A session translates nothing until the user turns it on in that session:
  its TRANSLATION line (`TRANSLATION: OFF`) picks a language other than its
  own, or OFF (`session.translation <json>` with `{"id","language"}`, ""
  for off; a `translation` record; `session_translation {session,
  translating}` to clients). The languages are the daemon's list
  (`LANGUAGES` in `src/domain/prompts.rs`, sent as `language_codes` in
  `snapshot`); a code not in it is refused (`translation.unknown`), and so is
  the session's own language (`translation.own`). Turning it on, or opening a stored
  session that translates, translates what it holds and lacks; a language
  translated before shows again at once. Each translation is one request with
  only the text — no transcript — so it stays fast and cheap; up to three run
  at once, apart from transcription and answers, and none is asked twice. Kept
  as a `translated` record (`{at}` for a line, `{id}` for an answer, with
  `language` and `text`) and sent as `translated {session, at|id, language,
  text, ms}`; a line corrected or removed loses it. An answer's card also
  translates it on demand (`entry.translate <json>` with
  `{"session","id","language"}`): into the session's translation language, else
  `language`, the interface's.
  Translations never reach the context of later requests. `eco translate <id>
  --lang <code>` or `--off` from the CLI; `eco show` carries them.
- **One at a time per session:** each session streams one answer at a time: an
  action triggered while another of the same session streams waits for it and
  is then sent with that answer in its context; a newer one replaces the one
  waiting. Sessions answer side by side — the live one while a stored one is
  summarized — up to `[llm] concurrency` answers at once (8 by default); past
  it, an answer waits for one to end before its request is sent. Saving the
  config stops only the answers whose model or reviewer changed; the rest
  finish.
- **Hooks:** an action may have a **hook**: a
  shell command run (`sh -c`, at most 60 s) with only the session it answered
  for, in `ECO_SESSION` (its id) and `ECO_TITLE`; the other system reads the
  rest with `eco show "$ECO_SESSION"`. With `hook_auto` it runs once each answer
  is complete; either way the answer's card has a send button that runs it
  (again), and says SENDING, SENT or NOT SENT (`hook.send <json>` with
  `{"session","id"}`; `hook_started`, `hook_sent`, or a `hook.failed` error with
  the last line the command wrote to stderr).

### 6.1 Triggers

The trigger is manual: a global Hyprland shortcut per action sends the command
straight to the socket, and the overlay shows one button per action.

```lua
o.bind("SUPER + ALT + 1", "eco ask", "echo 'action ask' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/eco.sock")
o.bind("SUPER + ALT + 2", "eco probe", "echo 'action probe' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/eco.sock")
```

An automatic trigger and a running recap are not built; see Open questions.

---

## 7. Sessions

A session moves through `recording ⇄ paused → ended`, and a stored interrupted
or ended session can be resumed. A session's kind is translated by the overlay;
any other label shows as written.

- **Start:** `session.start` takes an optional title, the kind, the session's
  language (from `stt.languages`) and optional tags. The session is transcribed
  in that language by its kind's model. Without a title it is titled by its kind
  and the time the start dialog opened ("Meeting 14:05", in the interface
  language). §12.3 has the dialog.
- **Several at once:** a session can start, or a stored one resume, while
  others record — a call and a class, an interview in two languages. Each is
  live until it ends. Each overlay window shows its own session, and its
  composer and buttons address only that one; the session a window showed last
  is the one the shortcuts and commands without a session address. All of them
  hear the same inputs: sessions alike share a transcriber, and each other model
  or language runs its own (§5, Transcribers).
- **Pause/resume:** pausing stops the session's transcription and its clock;
  the session stays open, the others keep recording, and actions still work on
  what was said. A session's duration is the time it recorded (or imported, up
  to its file's last line): pauses, and a daemon that was down, do not count. A
  live session carries `active_s`, the seconds of its earlier runs, and
  `running_since`, when the current run began (null while paused); the capsule
  and the live rows of SESSIONS add the time since.
- **End:** closes the session; the session the window showed before it shows
  again, or the window returns to its start screen.
- **Timeline:** speech and suggestions in order. Each suggestion has an id and
  can be removed; a removed suggestion disappears and leaves the context.
- **Stored sessions keep answering:** questions and actions on a stored
  session stream into its timeline and are appended to its log, so minutes or a
  summary are just actions. Stored sessions asked about stay in memory beside
  the open one, eight at most: past that, the least recently used one with no
  answer under way leaves, its log still holding all of it. An interrupted one
  can be resumed, and an ended one reopened, to reuse its context — either
  records again in its own language, appending to the same log.
- **Edits:** its title and kind can be edited, open or stored (a `meta` record,
  which does not count toward the duration). The user can remove any line
  (`unheard`, as a dropped echo) or correct it in place: an `edited` record, so
  the context, the export and the CLI read the correction.
- **Delete:** an interrupted or ended session can be deleted; deletion removes
  its log and associated voice links. A live session, recording or paused,
  cannot be deleted: the daemon refuses with `session.live`.
- **Resilience:** an unreadable log is skipped in the list with a warning, and a
  command that fails is reported to the clients without dropping the socket.

### 7.1 Search

`sessions.search <text>` answers `sessions_found` with the ids, newest first: the
sessions whose title, tags, lines, notes, questions or answers hold the text, in
any case or accents, as they read now (removed and corrected lines count as
they are).

### 7.2 Speakers

A line keeps the label it was heard as (the audio source, or the name a
transcript gave); naming a speaker in a session is a `speaker` record. Two
labels given the same name merge on screen, and the label itself, or no name,
restores it. Names reach every reading: the timeline, the prompt, the export and
the speakers of the session's summary.

### 7.3 Voices

A diarized speaker's voice (the mean of its embeddings) is kept in
`~/.local/share/eco/people/voices/<session>.json`, apart from the session.
eco never names a speaker on its own: a speaker whose voice scores ≥ 0.70
against someone (cosine to the mean of their voiceprints; on AMI the closest
wrong person scored 0.52) carries a **guess**. The user confirms it (the speaker
becomes that person, as naming them by hand) or clears it (a `guess_dismissed`
record `{label, person}`: that guess is not made again for that speaker). People
from 0.40 are suggested when a speaker is named by hand.

### 7.4 People

The people the user names live in `people/<id>.json`: a name, an optional color,
and up to 16 voiceprints, each tied to the session speaker it came from. Naming
a speaker as a person (new or known) adds a `person` record (`{label, person}`)
and a `speaker` record with their name, and their voice joins the person's.

- Renaming or merging people appends records to every session that names them;
  undoing an assignment takes the voice back; forgetting a person deletes their
  file and the voices kept for their speakers, and sessions keep the name; a
  person id sessions name but no file holds is forgotten the same way.
- A speaker without a kept voice (a live session, a transcript) can be someone
  known too — linked, without teaching eco a voice. Naming a speaker creates or
  selects a person for live and imported sessions.
- An assignment can take one line alone (`person.assign_line`), or every line of
  a session at once (`person.assign_all`, from the CLI only).
- A person's color changes across sessions; a local speaker color is saved in
  the session log.
- A session can also name people who never spoke: an `attendee` record links or
  unlinks their person id independently of speaker labels. SESSIONS and the CLI
  filter on the union of the people added and identified speakers.

### 7.5 Tags

A tag is the text given with its spaces trimmed and collapsed; an empty one is
`tag.invalid`. Tags are one whatever their case, and a tag keeps the casing it
was first written in: tagging a session `acme` when another is tagged `Acme`
gives it `Acme`. Each change appends a `tags` record with the session's whole
list (`{"tags": [...]}`), so a log from before tags has none. Renaming a tag or
deleting it applies to every session carrying it — a `tags` record in each of
their logs — renaming into a tag that exists joins the two under the new
spelling, and a tag no session carries is `tag.not_found`. Listed sessions, the
session shown, every live one and a session read back carry `tags`; search
matches them; `eco sessions --tag` filters by one, combined with `--kind`,
`--person` and `--search`.

### 7.6 Export

A session's transcript as WebVTT — one cue per line, the speaker in a
`<v Name>` span, times from the session's start. Lines keep only their start,
so a cue ends at the next line or after its words (0.35 s each, 1–10 s);
importing the export gives the same lines, speakers and starts back. From the
CLI (`eco export <id> > session.vtt`) or the session's detail (COPY VTT in its ⋯
menu, to the clipboard).

### 7.7 Import

A WebVTT transcript (`.vtt`, recognised by its `WEBVTT` header) becomes a
session as it is — one line per cue, speakers from Teams' `<v Name>` spans or
Zoom's `Name: text`, times from the cues, numeric character references decoded,
no STT. Any file ffmpeg decodes (mp4, mkv, webm, m4a, mp3, ogg, flac, wav…)
becomes a session — from the overlay (§12.3), or
`quickshell ipc --path overlay call eco importFile <path>`.

Both get the path as `file:<path>` with `-protocol_whitelist file`: a name that
begins with `-` is never an option, and nothing in the file makes them open a
URL. ffmpeg decodes it to 16 kHz mono through a pipe (never to disk), ffprobe gives
its length, and the VAD and the configured STT transcribe it as fast as they
go, one line per phrase the STT times, each at its time in the recording.
The lines start as the audio source chosen at import; meanwhile a child
`eco diarize` (WeSpeaker CAM++ from `eco setup`, loaded only for the import)
embeds the same speech, and when the file ends each line goes to the speaker
who talks most over it — `Speaker 1`, `Speaker 2`… in the order they first
speak, a `diarized` record, renamed like any speaker. One voice found leaves
the audio source. Measurements in [benchmarks.md](benchmarks.md). The session
is `importing` until the file ends, then `ended`; it can be stopped
(`import.cancel`), and what was transcribed stays. One import at a time, beside
live capture; an import a daemon left behind ends at the next start.

The session starts when the recording did, not when it was imported: at the
date the user gives (`--date`, or the import dialog), else the `creation_time`
tag ffprobe reads in the same call as the length (the container's, else a
stream's), else the file's modification time — the only date a WebVTT
transcript has. Its log is named by that date, SESSIONS files it under that
day, and its states, transcribers and speaker turns run on the recording's
clock; what the import cost is kept at the time it was paid.

### 7.8 Cost

Each answer, reviewer pass and translation appends a `spent` record with what
its provider reported (`usd`, null when it reported none); a removed answer was
still paid. Whenever a session starts transcribing with another model, on
another number of inputs, or at another price, a `transcriber` record says so
(`model` null for a WebVTT import, which nothing transcribes; `per_minute`, the
model's `price_per_minute` then, when it has one).

A provider that bills by request names it: Deepgram's `dg-request-id` header on
the WebSocket upgrade, and `metadata.request_id` in the reply to each segment of
an imported file. A streamed request is shared by the sessions its transcriber
served, each by how long it listened while the request was open. As the request
opens, and each time a session starts listening to it again, that session gets a
`listening` record (`request`, `model`, `at`). When the request closes (the next
opens, or the transcriber stops), each of them gets a `billed` record
(`request`, `model`, `seconds`, `share`, and `per_minute` when the model has a
price). A segment's request is closed as soon as it is answered: an import's is
the import session's alone, for the segment's seconds; a live one is shared
alike by the sessions listening.

A request the daemon never closed — it stopped or crashed while the request was
open — is closed at the next setup: each session's seconds are read from its
`listening` records, each stretch lasting until the session paused, ended or
changed models, or else until its last line, and the sessions share it by them
(alike, if none has any). A request that any session has a `billed` record for,
or that a transcriber has open now, is not closed again.

The provider is then asked what each billed request cost — Deepgram's
`GET /v1/projects/{project}/requests/{request}` → `response.details.usd`, the
project being the key's first — off everything else: 5 s after it closes, then
after waits about as long as the request has been closed, from 5 s up to an
hour, for a day. A failed ask is asked again like an unlisted one. The answer is
a `spent` record (`for` `transcription`, `request`, `usd` × the share; null when
the provider cannot tell, with `unknown` saying why: `no_scope` for a key
without the `usage:read` scope — Deepgram's 403 — and `unreported` for a
request not listed a day after it closed). The queue is the logs: a `billed`
record with no `spent` record is asked for again at every setup — the next
start, a saved config, an import's end — the waits going on from the request's
age. An answer's or review's `spent` record names the answer (`answer`).

A session's `cost` is read from its log: `llm_usd` sums the completions'
reported costs, `transcribed_s` gives the seconds of audio each model heard —
its run time once per input, as every input streams on its own — and
`transcription_usd` sums the requests' reported costs. `price_per_minute` is
optional and empty by default: when the user gives one, it prices only what no
reported cost covers — a billed request not priced (yet), or the time of a model
whose provider names no requests (ElevenLabs, OpenAI-compatible endpoints). It
prices them at the `per_minute` the log kept with that request or that stretch
of audio, so changing a price later does not change past sessions; only what
the log kept no price for — a log from before eco kept prices, or audio heard
while the model had none — takes the price the model has now. A model with
billed requests is costed by them alone. `llm_unknown` and
`transcription_unknown` say a part is left out of `total_usd`: a provider
reported no cost (or not yet) and no price stands in, an answer has no `spent`
record (a log from before eco recorded costs), or a log from before eco recorded
its transcriber.

Clients get `cost` with each listed session and read back; `session.cost <id>`
answers a `session_cost` event, also sent whenever a completion or a request
adds to a session's cost. It adds `parts` — what answers, reviews, translations
and transcription each add up to, of those the session has — and `items`, every
charge, newest first and those of no one moment (the time a model heard with no
request billed, the audio of an unrecorded transcriber, the answers that kept no
cost) first: `for`, `at`, `model`, `usd`, `estimate` (priced at
`price_per_minute`), `seconds` (transcription), `prompt` (an answer or review
still in the timeline), `count` (answers that kept no cost) and, when `usd` is
null, `unknown`: `unreported`, `no_scope`, `pending` (a billing provider has not
told it yet), `no_price`, `unrecorded` or `untracked`. `eco sessions` lists each
session's cost; the overlay shows one only when asked (§12.3).

---

## 8. State on disk

Never audio. Everything below is text the user can read.

| Path | Holds | Written by |
|---|---|---|
| `~/.config/eco/config.toml` | the configuration (§9) | the daemon, from the config window's draft; hand edits work too, but comments are not kept |
| `~/.local/share/eco/sessions/<start>-<title or kind>.jsonl` | one session, append-only JSON Lines | the daemon |
| `~/.local/share/eco/people/<id>.json` | a person: name, color, voiceprints (§7.4) | the daemon |
| `~/.local/share/eco/people/voices/<session>.json` | the voices of a session's diarized speakers (§7.3) | the daemon |
| `~/.local/share/eco/models/` | the Silero VAD and WeSpeaker CAM++ models | `eco setup` |
| `$XDG_RUNTIME_DIR/eco.sock` | the socket (§10) | the daemon |

The config, session logs, people and voices are the user's alone: eco creates
their files `0600` and the directories it makes for them `0700`. A file eco
rewrites (the config, a person, a session's voices) comes back `0600` on its next
save; a session log made before keeps the mode it had.

A session log holds `session`, `state`, `speech`, `suggestion`, `removed`,
`meta`, `speaker`, `diarized`, `person`, `attendee`, `unheard`, `edited`,
`line_person`, `transcriber`, `listening`, `billed`, `spent`, `tags` records, and also
`language`, `translation`, `translated`, `context` and `guess_dismissed` (§5–§7).
`--no-save` keeps it in memory. A removed suggestion is a `removed` record, and
it leaves the context of later actions as well as the screen.

A daemon that stops mid-session leaves it open; the next start marks it
`paused` and opens with no session. A stored session left open that no daemon
holds is listed and shown as `interrupted`, never reopened by itself.

---

## 9. Configuration

`~/.config/eco/config.toml` (written by the config window; hand edits work too,
but comments are not kept):

```toml
kinds = ["meeting", "conversation", "other", "idea"]  # what a session can be
rules = """
- Responda no idioma da transcrição.
- Seja curto: o usuário pode estar lendo enquanto a conversa continua.
"""                                         # every answer's rules; actions override them

[stt]
model = "deepgram"                          # the transcription model sessions use by default
language = "pt"                             # one of `languages`
languages = ["auto", "pt", "en", "es", "ja"] # offered by the overlay's dropdown

[llm]
model = "gemini"                            # the assistant: a registered model's name
max_context_chars = 60000                   # transcript + answers per request (~15k tokens)
concurrency = 8                             # answers running at the same time, all sessions

[translation]                               # optional: the model of sessions that translate
model = "gemini"                            # a chat model; the assistant's when absent

[reviewer]                                  # optional: rewrites every answer before it shows
enabled = true
model = "grok"
verbose = false                             # true: show and keep the draft it rewrote
# prompt = "…"                              # what it is asked; a default cuts unsupported facts

# A model, registered once: [stt] names a transcription one; [llm], [reviewer]
# and actions name chat ones; `kinds` gives it the sessions of those kinds.
[[models]]
name = "deepgram"
type = "transcription"
base_url = "wss://api.deepgram.com/v1/listen" # streaming; or an OpenAI-compatible URL
model = "nova-3"
api_key_env = "DEEPGRAM_API_KEY"
# price_per_minute = 0.0077                 # optional: USD per minute, only where the provider reports no cost

[[models]]
name = "whisper-lan"
type = "transcription"
base_url = "http://192.168.0.200:8081/v1"   # LAN whisper.cpp server
model = "whisper-1"
kinds = ["idea"]                            # idea sessions transcribe here

[[models]]
name = "qwen-lan"
type = "chat"
base_url = "http://192.168.0.200:8080/v1"   # LAN llama-server
model = "qwen3-14b"
kinds = ["idea"]                            # and answer here unless the action names a model

[[models]]
name = "gemini"
type = "chat"
base_url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"          # or api_key_omapass = "<account>", not both
model = "google/gemini-3.5-flash-lite"      # the provider's id
extra = { reasoning = { effort = "minimal" } }

[[models]]
name = "grok"
type = "chat"
base_url = "https://openrouter.ai/api/v1"
api_key_omapass = "openrouter"
model = "x-ai/grok-4.20"
extra = { reasoning = { enabled = false } }

[audio]
drop_echoes = true                          # the user's lines that repeat the others' are dropped
echo_cancel = false                         # PipeWire echo cancellation on the user's microphone

[context]
files = ["~/.config/eco/context.md"]       # global: sent with every request

# A context slot: files sent only while a session has it on. Sessions of its
# kinds start with it on; the user turns slots on and off per session.
[[contexts]]
name = "entrevista"
files = ["~/cv.md", "~/projetos.md"]
kinds = ["meeting"]

[ui]
language = "auto"                           # interface language pack, or auto
hide_from_share = false                     # eco's windows black in screen shares (and screenshots)

[colors]                                    # trace colour per device (optional)
"@default-input" = "#ffb000"

# An audio source is a name in the conversation; it may hold several PipeWire
# devices (node names, or @default-input / @default-output). A device has one owner.
[[participants]]
name = "Eu"
user = true
devices = ["@default-input"]

[[participants]]
name = "Recrutador"
devices = ["@default-output"]

[[actions]]
name = "probe"
prompt = "Sugira perguntas para devolver, cada uma com o porquê."
format = "2 a 4 itens numerados."
model = "grok"                              # a registered model; the assistant's when absent
hook = "~/bin/crm-push --board leads"       # run with ECO_SESSION and ECO_TITLE (optional)
hook_auto = true                            # after every answer; else from its send button
```

The config window saves a draft whole; the daemon validates it, writes
`config.toml` and restarts capture. A restart first stops the running pipeline
and its capture or monitor task completely, then starts the new one, so a save
never leaves a second `pw-record` duplicating frames or transcribing twice.

A hook runs a shell command, a context file is sent to a model, and a model's
key is sent to its `base_url`, so only the user adds them. The daemon starts
every window with one random token per run in `ECO_TOKEN`, and the window's
`config.set` carries it. A `config.set` from any other socket client that adds
or changes an action's `hook`, adds a file to `[context]` or a `[[contexts]]`
slot, or adds a model or changes a model's `base_url`, `api_key_env` or
`api_key_omapass`, is not saved: the daemon holds it, announces
`config_pending` with each hook command, file path, and model's address and key
source (a variable's or an omapass account's name, never a key), and answers
the error `config.pending`. The window shows them in a dialog; APPROVE sends
`config.approve <token>` and saves it, REJECT sends `config.reject <token>`,
which drops it with the error `config.rejected`. Approving or rejecting without
the token answers `config.not_window`. One change is held at a time: a newer
one replaces it, and any saved config drops it. Every other change from any
client is saved at once, as before. The token keeps another client from
approving its own change; a process of the same user that reads the window's
environment or edits `config.toml` itself is not stopped by it.

### 9.1 Secrets

Secrets come from environment variables: `OPENROUTER_API_KEY`,
`DEEPGRAM_API_KEY`, `ELEVEN_LABS_API_KEY`, `GROQ_API_KEY` — or from omapass, the
user's password plugin over gnome-keyring: `api_key_omapass = "<account>"`
instead of `api_key_env` makes the daemon run `omapass get -- <account>` (piped,
never logged) each time it builds that provider. omapass is found on `PATH` or
in `~/.config/omarchy/plugins/io.github.this-is-npc.omapass/bin/`, and is
optional: the `config` event says whether it is there and where its install page
is, and without it the config window's OMAPASS is off, with an info button that
opens that page. The window's KEY FROM switches between ENV and OMAPASS and
lists its passwords (not its Nostr keys), never their secrets. Since the daemon may be launched from a
Hyprland shortcut (which does not read `.bashrc`), set them in
`~/.config/uwsm/env`. Only the daemon reads the keys; the shortcuts do not need
them. During development, mise loads them from `.env` at the project root
(git-ignored).

The `models` command (§10.2) lists a provider's models with a key, so it reads a
key only where a saved provider would: the key source of a saved model, sent to
that model's `base_url` (a trailing `/` aside), or one of the window's presets —
`DEEPGRAM_API_KEY`, `ELEVEN_LABS_API_KEY`, `GROQ_API_KEY`, `OPENAI_API_KEY`,
`OPENROUTER_API_KEY` — sent to that preset's own URL. The presets are `PRESETS`
in `src/session.rs`, and the window gets them from the `config` event (§10.1),
so the window and this rule read one table. Any other request
is refused before a key is read, so whatever can write to the socket, an agent
included, cannot send a secret to a server of its choosing. A draft model with a
custom URL and a key, or an omapass key, lists once it is saved.

---

## 10. The socket

The daemon listens on `$XDG_RUNTIME_DIR/eco.sock`. Every client receives all
events as JSON lines and may send commands, one per line. The first event on
every connection is `daemon` with `pid`, `version`, and `overlay` fields. A
daemon error carries a `code`, which the overlay translates as `error.<code>`.

Two limits keep one client from growing the daemon's memory. A command line
longer than 1 MiB (`MAX_LINE` in `src/adapters/control_socket.rs`; a
`config.set` with a full config is a few kilobytes) closes that connection
without running it. Each client has a queue of at most 4096 lines
(`MAX_QUEUED`) still to write; a client that falls that far behind, or stops
reading, is dropped and its connection closed, while the daemon and the other
clients go on.

### 10.1 Events

A new client first receives `snapshot`; a window asks for the timeline of the
session it shows:

```json
{"type":"snapshot","session":{"id":"…","title":"Entrevista","kind":"meeting","source":"live","language":"pt","state":"recording","started_at":1791083833.3,"active_s":312.4,"running_since":1791084400.0},"kinds":["meeting","conversation","other","idea"],"kind_models":{"meeting":{"transcription":"deepgram","chat":"gemini"},"idea":{"transcription":"whisper-lan","chat":"qwen-lan"}},"actions":["ask","probe"],"hooks":["probe"],"participants":[{"name":"Eu","user":true},{"name":"Eles","user":false}],"inputs":[{"id":"@default-input","label":"Microfone padrão","participant":"Eu","color":"#ffb000"}]}
{"type":"signal","input":"@default-input","level":0.42,"speech":true}
{"type":"session","session":{…} or null,"live":[{…} for each live session],"transcribers":[{"model":"deepgram","language":"pt","sessions":2}]}
{"type":"session_opened","id":"…","window":2}
{"type":"session_timeline","session":"…","timeline":[{"type":"transcript",…},{"type":"note",…},{"type":"suggestion",…}]}
{"type":"transcript","session":"…","who":"Eles","name":"Ana","text":"...","at":1791083840.1,"latency_ms":990}
{"type":"transcript_partial","session":"…","who":"Eles","name":"Ana","text":"so the next…"}
{"type":"suggestion_start","id":"54401f25","session":"…","action":"chat","model":"google/gemini-3.5-flash-lite","prompt":"Ele citou Kafka?","at":1791083860.0}
{"type":"suggestion_thinking","id":"54401f25","text":"..."}
{"type":"suggestion_draft","id":"54401f25","text":"..."}
{"type":"suggestion_delta","id":"54401f25","text":"..."}
{"type":"suggestion_end","id":"54401f25","ttft_ms":350,"total_ms":3400,"prompt_tokens":1919,"cached_tokens":1791,"cost_usd":null}
{"type":"suggestion_removed","id":"2f45de79"}
{"type":"session_cost","session":"…","cost":{"llm_usd":0.0031,"llm_unknown":false,"transcription_usd":0.0924,"transcription_unknown":false,"total_usd":0.0955,"transcribed_s":{"deepgram":720.0},"parts":[{"for":"answer","usd":0.0031,"unknown":false},{"for":"transcription","usd":0.0924,"unknown":false}],"items":[{"for":"transcription","at":1791084120.2,"model":"deepgram","usd":0.0924,"unknown":null,"estimate":false,"seconds":720.0,"prompt":null,"count":null},{"for":"answer","at":1791083863.4,"model":"google/gemini-3.5-flash-lite","usd":0.0031,"unknown":null,"estimate":false,"seconds":null,"prompt":"Ele citou Kafka?","count":null}]}}
{"type":"hook_started","session":"…","id":"54401f25"}
{"type":"hook_sent","session":"…","id":"54401f25"}
{"type":"session_translation","session":"…","translating":"en"}
{"type":"translated","session":"…","at":1791083840.1,"language":"en","text":"…","ms":640}
{"type":"note","id":"9c1d2e3f","session":"…","text":"Migrei um app WPF para MVVM.","at":1791083870.4}
{"type":"note_removed","id":"9c1d2e3f"}
{"type":"sessions","sessions":[{"id":"…","title":"…","kind":"idea","source":"live","language":"pt","state":"paused","started_at":…,"duration_s":312,"speech":40,"suggestions":3,"speakers":["Eu","Ana"],"cost":{…as in session_cost, without parts and items…}}]}
{"type":"sessions_found","query":"prazo","ids":["…"]}
{"type":"session_detail","session":{…summary…},"timeline":[…transcript, note and suggestion events…],"speakers":[{"label":"Speaker 1","name":"Ana","person":"…","voice":true,"suggestions":[],"guess":null},{"label":"Speaker 2","name":"Speaker 2","person":null,"voice":true,"suggestions":[{"person":"…","name":"Bruno","score":0.82}],"guess":{"person":"…","name":"Bruno","score":0.82}}]}
{"type":"session_deleted","id":"…"}
{"type":"transcript_reassigned","session":"…","who":"Speaker 1","at":1791083841.2,"label":"Speaker 1#abc12345","name":"Ana"}
{"type":"person_assigned_all","session":"…","speakers":2}
{"type":"people","people":[{"id":"…","name":"Ana","voices":3,"sessions":["…"]}]}
{"type":"session_renamed","id":"…","title":"…","kind":"idea"}
{"type":"speaker_renamed","session":"…","label":"Eles","name":"Ana"}
{"type":"transcript_removed","session":"…","who":"Eu","at":1791083841.2}
{"type":"diarized","session":"…","who":["Speaker 1","Speaker 2",…],"names":["Ana","Speaker 2",…]}
{"type":"import_date","path":"/home/me/retro.mp4","at":1790759700.0}
{"type":"import_started","session":{…},"total_s":1834.2}
{"type":"import_progress","id":"…","done_s":612.5,"total_s":1834.2}
{"type":"import_done","id":"…","complete":true}
{"type":"config","config":{...},"devices":[{"id":"@default-input","label":"...","kind":"input"}],"omapass":{"installed":false,"page":"https://plugins.omarchy.org/..."},"presets":{"transcription":[{"name":"DEEPGRAM","values":{"base_url":"wss://api.deepgram.com/v1/listen","model":"nova-3","api_key_env":"DEEPGRAM_API_KEY"}},…],"chat":[{"name":"OPENROUTER","values":{"base_url":"https://openrouter.ai/api/v1","model":"…","api_key_env":"OPENROUTER_API_KEY","extra":{}}},…]}}
{"type":"config_saved"}
{"type":"config_pending","hooks":[{"action":"minutes","command":"~/bin/crm-push"}],"files":["~/notes/cv.md"],"models":[{"name":"fast","base_url":"http://192.0.2.10:8000/v1","api_key_env":null,"api_key_omapass":"openrouter"}]}
{"type":"models","target":"llm","models":["..."],"error":"..."}
{"type":"error","code":"session.none","params":{},"message":"start a session first"}
```

### 10.2 Commands

`action <name>`, `ask <question>`,
`config` (current config and devices), `config.set <json>` (the config whole;
a window adds `"token"`; one from another client that adds a hook, a context
file or a model, or moves a model's key, is held, §9), `config.approve <token>`
and `config.reject <token>` (the held change; once nothing is held, `config_pending` with empty lists; a client
that connects while a change is held is greeted with its `config_pending`),
`session.language <json>`, `models <json>`
(`{"target","base_url","api_key_env","api_key_omapass"}`; the key is read only
with no key source, a saved model's own source at that model's `base_url`, or a
preset's variable at the preset's URL, as in §9.1; anything else answers `models`
with `"code":"models.key_refused"` and `params` `{"base_url"}`), `omapass` (its
passwords: `{"type":"omapass","installed","accounts":[{"account","folder"}],"error"?}`;
not installed is `"installed":false`, not an error),
`session.start <json>` (`{"title","kind","language","tags"}`; the kind defaults
to the first, the tags are added as by `session.tag`;
it records beside the live ones and is shown; with `"window"`, `session_opened`
tells that window to show it), `session.timeline <id>` (`session_timeline` of a
live session),
`window.show <json>` (`{"window","session"}`: the session a window shows now, or
`""`),
`session.pause [<id>]`, `session.resume [<id>]`, `session.end [<id>]` (the live
session named, or the one shown; ending it shows the one shown before),
`session.toggle` (the one shown),
`sessions` (list), `sessions.search <text>`, `session.show <id>` (read back),
`session.delete <id>`, `session.reopen <id>` (resume
an interrupted or ended one beside the live ones, or show a live one),
`session.rename <json>` (`{"id","title","kind"}`; a configured kind or the one
it has),
`session.speaker <json>` (`{"id","label","name"}`; an empty name restores the
label),
`session.speakers <id>` (`session_speakers` for that session),
`session.cost <id>` (`session_cost` for that session, with its parts and
charges), `session.translation <json>`, `session.context <json>`,
`entry.translate <json>`, `hook.send <json>`,
`tags` (`{"type":"tags","tags":[{"tag","sessions"}]}`, sorted by name),
`session.tag <json>` and `session.untag <json>` (`{"id","tag"}`: a `tags` record
and `session_tags` with `{"session","tags"}`, then `tags`), `tag.rename <json>`
(`{"from","to"}`:
`tag_renamed` with `{"from","to","sessions"}`, the ids changed, then `tags` and
`sessions`) and `tag.delete <json>` (`{"tag"}`: `tag_deleted` with
`{"tag","sessions"}`, then `tags` and `sessions`), `session.line.edit <json>`
(`{"id","who","at","text"}`: an `edited` record and `transcript_edited`; an
empty text removes the line), `session.line.remove <json>`
(`{"id","who","at"}`: removes a line of speech, an
`unheard` record and `transcript_removed`; `line.not_found` when it is gone),
`people`, `person.assign <json>` (`{"session","label","person","name"}`: a known
person's id, or `""` and a name — the person called that, or a new one),
`person.assign_line <json>` (`{"id","who","at","person","name"}`),
`person.assign_all <json>` (`{"session","person","name"}`),
`person.unassign <json>` (`{"session","label"}`), `person.guess.clear <json>`
(`{"session","label"}`: a `guess_dismissed` record and `session_speakers`;
`speaker.invalid` when there is no guess), `person.add <json>` (`{"name"}`: a
new person; `person.exists` with their `id` and `name` when someone has that
name in any case), `person.rename <json>` (`{"id","name"}`),
`person.attend <json>` (`{"session","person","name"}`), `person.leave <json>`
(`{"session","person"}`),
`person.merge <json>` (`{"into","from"}`), `person.forget <id>`,
`session.ask <json>` (`{"id","question"}`), `session.action <json>`
(`{"id","name"}`)
and `session.note <json>` (`{"id","text"}`)
(ask or note any session, open or stored; `ask`, `action` and `note` go to the
open one),
`session.export <id>` (the transcript as WebVTT:
`{"type":"session_export","id","format":"vtt","text"}`),
`overlay.open` (open another window and return `overlay_status`),
`session.import <json>` (`{"path"}`, optionally `"title"`, `"kind"`,
`"language"`, `"participant"`, `"started_at"` in seconds since the epoch),
`import.date <path>` (when the file was recorded, as an import without
`"started_at"` dates it: `import_date`, `at` null for a file that cannot be
imported), `import.cancel`, `entry.remove <json>`
(`{"session","id"}`: an answer or a note of the open session, the one read back,
or a stored one, whose log gets a `removed` record; clients get `note_removed`
for a note and `suggestion_removed` otherwise, even when it was already gone)
and `stop`.

---

## 11. The command line

Every command and flag is in [cli.md](cli.md), generated from the binary's help.
How the commands are built:

- `setup` downloads the VAD and speaker models and publishes the agent skill;
  `bench` measures model latency and diarization. Neither needs a daemon.
- `start`, `stop`, `restart` and `status` manage the daemon. The user service
  runs `eco daemon --headless`; `eco start` starts it if needed and waits for
  `overlay_status` after opening the window (`--headless` skips it).
- The session commands talk to the running daemon over its socket (start one
  with `eco start --headless` when the overlay is not wanted) and print one
  JSON object, as `okt` does: `{"ok":true,"data":…}`, or
  `{"ok":false,"code":"…","message":"…"}` with a non-zero exit —
  `argument.invalid` (an argument holds a control character, such as a line
  break, that could end the command line early; nothing is sent),
  `daemon.offline`, `daemon.access_denied`, `daemon.unavailable`,
  `session.not_found`, `action.unknown`, `completion.failed`,
  `suggestion.removed` (replaced by a newer request), `import.busy`,
  `import.failed`, `person.not_found`, `person.invalid`, `person.exists`,
  `people.failed`, `line.not_found`, `tag.invalid`, `tag.not_found`. They read
  the broadcast for their own reply, so they work beside the overlay. `export`
  prints the document itself, to pipe or redirect.
- `show` carries the session's speakers and storage path/byte count; `speaker`
  names one speaker, while `assign` can identify one line or every line.

Agents get no MCP server: the `eco` skill (`skills/eco/SKILL.md`, embedded in
the binary) teaches them the CLI. `eco setup --harnesses agents,claude-code`
publishes it to `~/.agents/skills/eco/` and `~/.claude/skills/eco/`; every
`eco setup` refreshes the copies eco owns (its front matter says
`owner: eco`) and never overwrites a foreign file at that path. A change to the
CLI updates the skill in the same commit.

---

## 12. The overlay

### 12.1 Visual language and the kit

Clean cyberpunk HUD — monospace text, hairlines, outlined
surfaces, small tracked uppercase labels with two-digit
indices, one accent from the active Omarchy theme
(`$XDG_STATE_HOME/omarchy/current/theme/colors.toml`, with `~/.local/state` as
the default). The overlay reloads the palette after file edits and theme
directory replacements, including light and dark mode changes. The
kit lives in `overlay/` (`Panel`, `SurfaceFrame`, `TraceButton`, `Chip`, `Tab`,
`TextBox`, `Field`, `NumberField`, `Kicker`, `Masthead`, `StatusMessage`,
`Icon`, `EcgTrace`); views compose it and hold no logic
beyond binding `Eco`, the singleton that owns the socket. The control lab
(`mise run preview:controls`) shows every kit component.

- **Icons:** one set (`Icon.qml`), thin line drawings on a 16-unit grid with
  one stroke weight and round ends — settings (sliders), play, pause, stop,
  list, close, chevrons, enter, warning, spark (AI) — instead of Unicode
  glyphs that each fallback font draws differently. No connection badge while
  things work; `DESCONECTADO` shows only when the daemon is unreachable.
- **Selection always shows:** the chosen option of a dropdown carries a dot and
  an accent tint, distinct from hover; the current row of a list is tinted
  with its title in the accent. No side bars.
- **Languages:** the interface is translated through JSON language packs in
  `overlay/i18n/` (English, Brazilian Portuguese, Japanese), found and
  reloaded on their own, chosen in Settings › Interface (`[ui] language`,
  `auto` follows the system). Dates follow the chosen locale; daemon errors
  carry codes the overlay translates. See [i18n.md](i18n.md).

### 12.2 The window

A Quickshell `FloatingWindow` titled `eco` (720×680), opened by
`eco start` (`--headless` skips it). It is a normal Hyprland window, so the user
moves and resizes it freely; `packaging/hypr/eco.lua` floats it, pins it to
every workspace and keeps its size. Each window is its own quickshell process,
numbered by the daemon (`ECO_WINDOW`). Opening the app opens another window,
showing a live session no window shows, if there is one (`ECO_SHOW`). It
reconnects to the socket on its own.

- **Without a session:** the live trace of every input in the room above the
  buttons (dimmed behind them when the window is too short), in
  the input's colour and nothing else — flat in silence, swinging with audio,
  dashed when the input sends nothing — so a glance says capture works; then
  **NEW SESSION**, the primary button, which opens the start dialog;
  **SESSIONS**, the sessions list, the one place live sessions show (its LIVE
  filter); **IMPORT**; and **PEOPLE** — their keys (`N`, `H`, `I`, `P`) listed
  only in the shortcuts dialog, as no control shows a key. The masthead says
  READY, or OFFLINE without the daemon.
- **During a session**, as a chat, with the conversation taking every pixel the
  header does not need: the session's title in the masthead (a session without
  one is named by its kind, there and everywhere else), with
  the details button (what it is, people, speakers, tags — added with the
  existing ones offered, taken off by their chip —, context, translation and
  cost, in a panel that opens over the top of the conversation, takes its clicks
  and scrolls past its height) and, top right, the close button, back
  to the sessions — a live one keeps recording; then the
  **session capsule**, one line — the running time (stopped while paused) and
  state (a dot with a ring that keeps leaving it while recording), the live trace
  of every input running through it, a warning only for an input that sends no
  audio while recording (its name on hover, the settings on click), and the
  pause/resume and end segments; clicking the state pauses or resumes, and end
  fills red asking once more —; the timeline — others' speech in a
  block tinted with their colour, the user's in a faint block on the right, every
  line at full brightness (only words still being said are dim), no side rules;
  a speaker's name and time head each turn and repeat after two minutes of
  silence or anything else between their lines; answers as framed cards whose
  Markdown is drawn as it streams (marks still open are closed; images, which
  Qt would fetch, become their alt text, and any `![` left, code included,
  gets a zero-width space so no image can open, `overlay/markdown.js`) that light up
  while they stream, show the question asked and can be removed; a complete
  answer can be copied (its Markdown to the clipboard, `wl-copy`); it follows the
  newest entry, but an answer streaming taller than the view keeps its top in
  view until the next entry comes, and once the user scrolls up a "↓ N new" pill
  above the composer counts the lines, answers and finished answers that
  arrived below — a click or End goes back to the end and follows again —; and
  at the bottom the composer: the skills as chips numbered for Alt+1…9, which
  run them whether the chips show or not (a window under 560 px tall hides
  them), and a line to ask anything or, with the note button or Shift+Enter, to
  keep what was typed as a note. Typing `/` lists the skills above the line,
  filtered as the word grows: ↑/↓ choose, Tab completes, Enter runs, Esc closes
  the list and leaves the keyboard in the line. `/skill` runs a skill; a lone
  `/word` that names no skill is never asked — an error under the line says so
  and the text stays to be fixed; `/` followed by more words, or a path such as
  `/etc/hosts`, is asked. The interface says skill and note throughout; the
  config, the socket and the CLI keep `action` and `note`. Each finished
  answer's latency and cache share show on hovering its card's heading, in the
  window showing its session only; errors about a session show only there too.
- **Lines:** every line shows its speaker and time; clicking a speaker's name
  names it in that session. × beside a line on hover removes it, or
  `eco line … --remove`; ✎ beside it corrects it in place (Enter saves, Esc
  cancels).
- **Conversation:** a chat — the user's lines on the right, everyone else on
  the left, the speaker's name, time, and color on every line.

### 12.3 Screens and dialogs

- **Start dialog:** asks for an optional title, the kind, the session's
  language (from `stt.languages`) and optional tags (offering the existing
  ones as they are typed), and lists who will be heard. The title's placeholder
  is the default title (§7). The dialog starts from the kind and language the
  window's last session used (kept while the window lives, not across
  restarts), offers the context slots with those of the chosen kind already on,
  says which models a kind runs on once it is picked (`kind_models` in the
  snapshot: `{kind: {"transcription", "chat"}}`), scrolls in a short window with
  its buttons kept below, and without an input it cannot start: it says so and
  offers the Audio settings.
- **Several windows:** opening the app again always opens another window, to
  start the next session in. The session view shows no other session: leaving
  it (it keeps recording) goes to SESSIONS as it was searched and filtered,
  whose LIVE filter lists the live ones, any of which shows in that window when
  opened.
- **End:** asks for confirmation, then closes the session the window shows.
- **SESSIONS:** the start screen links to a table of stored sessions — the live
  ones first under LIVE, with a dot before their state, then the others newest
  first under their day (TODAY, YESTERDAY, or the date): title, kind, tags, when
  (`Today 14:30`, `08 Oct 14:30`), duration, people, and state; no cost —,
  filtered to the live ones (LIVE, with TRANSCRIBING and each transcriber
  running — its model, language and `×n` when it feeds several sessions — so a
  second paid model never runs unseen), by kind when there is more than one, by
  tag, or by person — the person's chip (`PERSON: Sadao ×`), LIVE, the kinds
  and the tags share one line of chips that scrolls sideways. Until the first
  list arrives the table says it loads, a search not yet answered shows a
  loading trace in its field, and an empty table says why (no live session,
  nothing matching the search or the filters) with CLEAR FILTERS. A list that
  changes decodes only its new rows. A tag's chip renames or deletes it in
  every session (right click, the menu key, F2 or Delete; deleting asks first,
  saying how many sessions lose it). A field over the table searches it (§7.1;
  `/` goes to the field, and `↑` on the first row back to it). The table stacks
  those fields in narrow windows, a line with nothing in it left out. Selecting
  a row puts the session on screen: the same view as a live one — the
  conversation, the details panel and the composer — with, in place of the
  capsule, a line saying when it was, how long and how much was said, and the
  way to resume it, which turns that line into the capsule. The row's delete
  control (a trash can, as the details' DELETE) asks for confirmation; a live
  row offers no delete. An ended session is reopened from its ⋯ menu.
  Interrupted sessions are untinted: only the live sessions (the LIVE count) and
  an import under way are toned.
- **Keys:** `n` new session, `h` SESSIONS and `p` people on the start screen,
  `↑`/`↓`/`Enter` in the list, `Esc` back, `e` edit, `r` resume; on the People
  screen `F2` renames and `m` merges the selected person. Back returns to the
  screen the user came from: SESSIONS opened from People goes back to People,
  and People opened from SESSIONS back to SESSIONS.
- **Details:** the details panel opens with what the session is, on one line
  — its state, start ("Today 14:30", as the sessions list), running duration,
  and how many lines, answers and notes it holds — beside the edit (title and
  kind) button, the title itself being in the masthead; a stored session adds
  its absolute storage path, with COPY PATH beside it, and copies its
  transcript. An interrupted or ended session can be deleted after a
  title-specific confirmation; a live session's delete is off. Its CONTEXT line's
  chips turn slots on and off (§6), and its TRANSLATION line picks the session's
  translation language (§6).
- **Voice guesses:** a guess shows on the speaker's lines as `ANA?` and is
  counted on one line of the session (or, on a short window, on its details
  button), whose REVIEW opens the details at their SPEAKERS list.
- **Who is who** has one place in the overlay: the SPEAKERS list of the
  session's details, each speaker label with the person it is, eco's guess
  (CONFIRM or CLEAR), or the name it goes by, CLEAR undoing a person and CHANGE
  asking who they are. Clicking a speaker in the timeline asks the same:
  the people their voice suggests (with their score, when the session kept it),
  then everyone known, or a name. Opened on a line, the assignment can instead
  take THIS LINE alone. The dialog says how many lines will change, as a warning
  when more than one will; assigning every line of a session at once is left to
  the CLI. Its color picker changes the known person's color across sessions, or
  saves a local speaker color in the session log. The overlay never shows a
  person id: it names them as known, else by the session speaker linked to them,
  else as an unknown person the session's people dialog removes. Renaming a
  person to a name someone else has warns and offers to merge the two.
- **People dialog:** shows those FROM SPEAKERS apart, off with the reason on
  hover, and under ADD SOMEONE the ones added (a click takes them back) and
  everyone else known; each chip wears the person's colour, two with one name
  tell apart by their session count, and the dialog's top edge stays put as it
  grows. NOT THEM undoes it.
- **People screen** (PEOPLE on the start screen or in SESSIONS; `p` on the
  start screen) lists everyone with their voices and sessions, and also
  changes person colors; selecting a person opens
  their sessions. The same screen adds someone by a name no one has (ADD, also
  offered by the empty list), renames, merges or forgets them. MERGE shows a
  notice with CANCEL — the one way out besides Esc, as the way back hides —
  until the other person is picked — picking the same one says so — then asks
  which name stays and what moves, SWAP turning the direction around. FORGET
  says which voices are deleted and that the sessions keep the name. Below
  420 px a row's colour, rename, merge and forget fold into one ⋯ menu.
- **Import:** from the start screen or SESSIONS (`i`; the dialog's BROWSE opens
  a file dialog, and importing without a file says one is needed), or by
  dropping a file on the window. RECORDED shows when the file was recorded
  (`YYYY-MM-DD HH:MM`, read back as SESSIONS writes dates) and takes another;
  one it cannot read is outlined and stops the import. A strip shows the
  progress and stops it.
- **Cost:** shown only when asked, inside the session: the COST button of its
  details panel opens the session's cost screen (masthead `COST · <title>`,
  SESSION or Esc back to the session) — the total, the LLM part with answers,
  reviews and translations, the transcription's with its minutes, each unknown
  reason once in full and a note when a part is estimated, then the charges in
  the DataTable (what — the answer's question or action —, time, model, cost, an
  unknown one as "?" with its reason); it follows a live session while open.
  Neither the sessions list nor an answer card shows a cost.

### 12.4 Keyboard, motion and traces

- **Keyboard first, mouse too:** everything the pointer does, the keyboard
  does. Tab and Shift+Tab move through every control, drawn with a focus ring
  apart from its own state; Enter or Space presses it; a control's hint shows on
  focus as on hover. Menus and dropdowns open on the keyboard, start on the
  chosen entry, move with ↑/↓/Home/End and pick with Enter; Esc closes the
  innermost thing open — a menu, then a dialog, then the details panel, then
  the session view, live or stored, as its close button does — and focus
  returns to what opened it, even when the window was inactive as it opened. A
  menu closes when its page or view changes or the window goes inactive.
  Dialogs keep Tab inside and start on their first field, or on the way back
  when they confirm something that cannot be undone.
  Every dialog ends in one footer: what else it offers on the left, then the
  way back (Esc) and the primary action (Enter in its fields) on the right;
  none carries an index, and its top edge stays where it opened as it grows.
  Tables are one Tab stop: ↑/↓ choose, Enter opens, Delete asks to remove, and
  only the selected row's tools are Tab stops. The timeline is one Tab stop too,
  entering on the newest entry: ↑/↓/PgUp/PgDn/Home/End move, the entry shows its
  tools to Tab into, and N names a line's speaker. A view that appears puts the
  keyboard where it is used (the composer in a session), and scrolling views
  follow the focus. `?` or F1 lists every key.
- **Motion only where it means something** — a state, new content, or the
  user's own action; nothing animates on a timer for its own sake. Following
  omakiten (`OutCubic`, 90–300 ms): the recording dot sends out a ring (it is
  recording); an answer still waiting for its first word draws the same
  loading trace as a busy button — it nears the end but reaches it only when
  content comes, reasoning or reply — counts the seconds and shows the
  model's reasoning as it arrives (it is working), then lights its frame while
  the reply grows (it is still arriving) — text being read never scrambles; a
  failed answer stays, in red, saying why; speech slides in from its speaker's
  side, answers rise in and the new-content pill rises above the composer
  (something new); traces move only with real audio (the inputs work);
  views slide when the user navigates (see Navigation motion) and visible
  labels decode softly; action buttons grow an accent rail on hover and
  carry an audio trace while waiting for the next state.
- **Navigation motion:** going forward, the view being left slides out to the
  left while the new one comes in from the right. Navigation buttons show a
  short audio trace before the view changes; the entering view reveals its
  title and visible content with the Soft kana decode.
- **Input traces (ECG style):** one trace per captured input, in that input's
  colour. The daemon sends each 32 ms frame's loudness (log-scaled RMS, so
  quiet speech still shows) and whether it is speech; the trace keeps about
  five seconds, swings up and down with the loudness as a smooth curve, glows
  faintly, fades out to the left and is written by a bright head on the right.
  Brighter while it hears speech; a faint dashed line when the input sends
  nothing for a second. Smoothness: samples get evenly spaced times (32 ms
  apart, resynced if they drift) instead of arrival times, the trace runs 70 ms
  behind the newest sample to ride out socket bursts, its geometry (a QtQuick
  `Shape`, Catmull-Rom smoothed) is rebuilt only when a sample arrives, and in
  between it slides once per display frame (`FrameAnimation`). A `Canvas`
  repainted per frame looked as smooth but cost ~77% of a core; this costs
  ~12%. The same trace runs in the start screen's background,
  inside the session capsule, and next to each captured device in the config
  window.
- **Input colours:** the user picks a colour per device (a palette of ten vivid
  colours, or automatic by position) so a trace says which input hears audio
  by colour alone; stored in `[colors]` (device id → `#rrggbb`). The palette is
  the theme's colours made readable, then their hues turned when the theme has
  fewer; no two are alike and none is near the accent, so none reads as a
  selection. A person without a chosen colour gets the first palette colour
  from where their id points that no other listed person wears.

### 12.5 Responsive

Both windows go down to 240×320 px, because Hyprland may tile
a window below its minimum and Qt would then draw it at the minimum, clipped.
No layout row may need more than the window offers: one wide row widens the
whole layout, so buttons wrap, text elides and lists change columns instead.
Under 560 px the overlay's capsule keeps icons only (labels move to hints), its
trace shrinks, the language button shows the code, and the composer's send
button keeps its icon. Under 560 px tall the overlay tightens its margins and
gaps, the voice guesses become a count on the details button (which opens
the details at the speakers), and, from 360 px wide, a live session's capsule
joins the masthead line — state dot with its ring, running time, the
silent-input warning, pause and end, without the trace; on a window also under
560 px wide the name `ECO ╱` drops, the title elides first and the language
picker moves into the details. The config window scrolls long panels. Long
titles elide; the section name in the masthead keeps its room and the status
message condenses to a marked tooltip when space is tight. Nested
layouts are pinned to their natural height so the timeline gets the space.

### 12.6 The config window

A Quickshell `FloatingWindow` ("eco · configuração"), opened
from the overlay or with `quickshell ipc --path overlay call eco toggleConfig`.
It is created the first time it opens and then only shown and hidden: Qt writes
its pipeline cache to disk when a window is destroyed, and the interface would
wait for that, often for seconds. Each opening asks the daemon for the config.
It asks for 1040×760, or its screen less a 96 px margin when that is smaller;
the rule in `packaging/hypr/eco.lua` gives it no size, so it keeps that one, and
centres it on the screen (without `center`, Hyprland would centre it on the
overlay, past the screen's edge).

It edits a draft and saves it whole (SAVE, or `Ctrl+S`, named in its tooltip);
the daemon validates it and saves it (§9). Unsaved changes are marked;
discarding a changed draft asks first, and closing one offers SAVE AND CLOSE,
KEEP EDITING or DISCARD CHANGES. What keeps a draft from saving shows on its own
field — an outline in the error colour and the reason under it — with a dot on
the tab that holds it; a repeated name or a space in a skill's name shows as it
is typed, a missing value once a save is tried, which opens the first one's tab
and card and gives its field the keyboard. Each clears as soon as its field is
fixed; the line above the buttons only counts them. A daemon's refusal stays
there until the draft changes again. Every field is named by a caption right
above its control, pickers (models, languages) as much as text inputs: no label
sits in a column of its own. Prompt areas (rules, a skill's prompt and format,
the reviewer's prompt) grow to ten lines, then scroll inside. Number fields take
digits only and keep the last valid number. A device refresh sends `devices`
and leaves the draft intact. Lists (languages, context files) are compact rows:
the position, the item, a step up and ×. File lists add a path typed in the
field or picked, several at once, from the file dialog behind the folder button.

Tabs, `Ctrl+1`…`Ctrl+8`, Audio first and Models second, since the tabs after
it pick from it, each owning its own fields: no tab edits another's, and what
another tab decides is only read, never picked, there.

- **Audio:** one card per audio source — its name, whether it is the user,
  and its PipeWire devices (colour, kind, live trace, missing ones marked), with
  ADD DEVICE listing every device and who has it now; removing one asks first,
  naming the devices no longer heard, whether nobody else is heard (it held the
  only output device) and whether nobody is the user any more; then the
  microphone echo controls, each with what it does.
- **Models:** the registry only — every model as a card, chat and
  transcription alike; closed, its type, the provider's id and what uses it;
  open, what uses it in full and read-only (the defaults, the reviewer, the
  skills that name it and each kind's transcription, assistant and translation
  that answer with it, as the overlay's one resolver, `answering` in
  `overlay/draft.js`, mirrors the daemon's precedence), its type while nothing
  uses it (changing it keeps the name and model id; the provider and key change
  only when its base URL does not fit the new type), its name, for chat
  REASONING (the provider's — or IN EXTRA FIELDS when `extra` sets one —, off,
  low, medium or high: `reasoning`, sent as OpenRouter's `reasoning` field and
  winning over one in `extra`), the provider (a preset of its type or CUSTOM,
  which shows the base URL), model (typed, with the preset's model as an
  example, or picked from LIST, which closes the list again, as Esc does), key
  from an environment variable or an omapass password and, for chat, its extra
  JSON under ADVANCED. Renaming one renames it everywhere; the assistant's and
  the default transcription cannot be removed, and removing another asks first,
  listing what falls back to which model (as `answering` picks it) and whether
  the reviewer turns off. A new model starts from the base URL and key source
  of the last model of its type, with its name field focused.
- **Transcription:** the default transcription model, which sessions whose
  kind picks none use; the offered spoken languages picked by name, and
  the default spoken language new sessions start in.
- **Answers:** the default assistant model, which answers questions and the
  skills and kinds that pick none; the rules for every answer; the global
  context files and the context slots (a slot: its name, files and the kinds
  that start with it on); the reviewer (on, verbose, its model and prompt); and
  the limits: context size and answers running at the same time, across all
  sessions.
- **Skills:** the actions as cards that open to edit, each picking its model —
  or the default, named with the model it inherits (`default · <model>`) — and
  its hook, sent on its own (AUTOMATIC, once a hook is set) or from the answer.
  An action that names a model answers with it in every session kind. Cards
  move up and down (their chevrons, or `Alt+↑`/`Alt+↓` inside one), which orders
  the composer's skills. A name takes no spaces and no other skill's name;
  renaming one warns that Hyprland shortcuts call it by name (`action <name>`),
  and removing one asks first, saying those shortcuts stop working.
- **Translation:** the default translation model — or the default, the
  assistant's, named. Whether a session translates, and into which language,
  is chosen in that session only.
- **Sessions:** the kinds as cards — closed, the kind, DEFAULT on the first
  (suggested by the start and import dialogs) and the models its sessions run
  on; open, its name, MAKE DEFAULT, and its transcription, assistant and
  translation models, each its own or the default named with the model it
  inherits, noting that its assistant
  model answers questions and the actions that name no model. A kind is renamed
  once its name is left or Enter is pressed, never to an empty or another kind's
  name. Renaming or removing a kind follows it into the
  models and context slots; removing one asks first, saying that its sessions
  keep it, which kind becomes the default and that its own models and slots stop
  applying.
- **Interface:** interface language, and HIDE FROM SCREEN SHARING (off by
  default), whose help says eco then shows black in the user's screenshots and
  recordings too (§13).

---

## 13. Packaging

**The release package.** `packaging/arch/PKGBUILD` builds eco for pacman from
the committed tree (`git archive HEAD`, never uncommitted changes) and installs
`/usr/bin/eco`, the overlay under `/usr/share/eco/overlay`, the Hyprland rules
under `/usr/share/eco/hypr/eco.lua` pointing at it, the launcher, the icon, the
licence and the user service in `/usr/lib/systemd/user`. `mise run package`
(`scripts/package`) runs `makepkg` into `target/arch/pkg` and writes the
`SHA256SUMS` beside the package; `install.sh` downloads both from a GitHub
release, checks one against the other and runs `pacman -U`.

**Releases.** Commits follow Conventional Commits, and release-please reads
them (`release-please-config.json`, `.release-please-manifest.json`). Every
pull request merged into `master` runs `.github/workflows/release.yml`, which
opens or updates a release pull request: it moves the version in `Cargo.toml`,
`Cargo.lock` and the `PKGBUILD`, and writes `CHANGELOG.md`. Merging that pull
request tags `v<version>`, and the same workflow builds the package from the
tag with `scripts/package` in an Arch container and attaches it, with its
`SHA256SUMS`, to the release. The container is `archlinux:base-devel` pinned
by its multi-arch index digest, so the base image cannot change under a tag;
the job's `pacman -Syu` still installs the Rust toolchain and system packages
current on the day of the build, so the toolchain itself is not pinned. To
move the image, read the `docker-content-digest` header of the registry's
`library/archlinux/manifests/base-devel` and replace the digest and the date
in the workflow. The workflow runs no test; the gate
is local (see [Tests and the gate](#tests-and-the-gate)).

**From a checkout.** `mise run install` (`scripts/install`) builds the release binary and installs it
under a prefix (`~/.local` unless `PREFIX`): the binary, its own copy of the
overlay, the Hyprland rules pointing at it, the launcher, icon and user service,
with the models and the agent skill prepared first (`eco setup --harnesses
agents,claude-code`). It is idempotent: each install replaces the last, and a
daemon that was running is restarted on the new binary. Sessions, config and
models stay. `mise run uninstall` removes what install put under the prefix and
leaves sessions, config, models and the agent skill.

- **User service:** `packaging/eco.service` runs `eco daemon --headless`
  (`Restart=on-failure`). `eco start` starts it through `systemctl --user` when
  nothing answers on the socket.
- **Hyprland:** `packaging/hypr/eco.lua`, loaded from
  `~/.config/hypr/bindings.lua` by one line `eco setup` adds (`src/setup.rs`):
  the line ends in `-- eco setup`, names the `share/eco/hypr/eco.lua` beside the
  running binary, and runs `dofile` only when `io.open` finds that file, so a
  removed package leaves no error. Setup rewrites only its own line, adds none
  when another line already names an `eco.lua`, and creates no `bindings.lua`.
  The file binds `SUPER+ALT+<n>` to the actions,
  `SUPER+ALT+N` to a new session, `SUPER+ALT+P` to pause/resume, `SUPER+ALT+H`
  to SESSIONS, `SUPER+ALT+C` to the config window and `SUPER+ALT+E` to bring eco
  to the front; those that open a view also give eco the keyboard. The shortcuts
  reach the daemon through `socat`, never by starting a second daemon. It holds
  the window rules: the overlay (`float`, `pin`, `persistent_size`, opaque) and
  the config window (`float`, `pin`, centered, opaque). The config window is
  also a child of the overlay, so it always opens above it. Without the rules,
  Hyprland tiles both.
- **Screen sharing:** off by default, eco's windows show in captures.
  `[ui] hide_from_share` (Settings › Interface) makes each overlay process set
  Hyprland's `no_screen_share` on its own windows (the overlay and its config
  window, found by pid) through the Lua dispatcher, when the setting changes
  and when a Quickshell window opens. Hyprland cannot tell a screen share from
  a screenshot, so they show black in the user's screenshots and recordings too.

---

## 14. What this is, and what it is not

- It is not a bot that joins the meeting, and it does not record raw audio:
  audio is never written to disk, imports are decoded through a pipe, and voices
  are kept only as embeddings.
- It is Linux only (Omarchy, Hyprland, PipeWire); nothing special-cases macOS or
  Windows. The Windows machine on the LAN only serves models over HTTP and is
  reached through the same adapters as any provider.
- Transcripts stay local in `~/.local/share/eco/`; `--no-save` keeps a session
  in memory only. Secrets are never logged, nor transcript text at info level.
  No telemetry.

The targets the design is held to:

| Requirement | Target |
|---|---|
| End of speech → first suggestion token | **< 1.5 s** (fast model) |
| Idle CPU usage | < 10% |
| Cost per 1 h meeting (Deepgram + Gemini Flash, manual mode) | < US$0.50 |
| Privacy | No audio on disk; transcripts local in `~/.local/share/eco/` |

Cost: two continuous Deepgram streams cost 2 × 60 min × US$0.005 = US$0.60/h,
above target. The target depends on sending only speech segments (VAD) and/or
transcribing the "me" channel locally.

---

## Measured, and what it cost

The measurements live in [benchmarks.md](benchmarks.md): LLM latency, the LAN
models, the prompt cache, the Rust daemon against the Python one, diarization,
import, naming people by voice, streaming transcription, concurrent sessions
and the overlay's cost. Answer quality has its own tool, `mise run bench`
([benchmark/README.md](../benchmark/README.md)). Where a number above changed
the design it is stated beside the decision: the Deepgram and Scribe phrase
latencies (§5), the 0.70 guess threshold against the 0.52 closest wrong person
(§7.3), echo cancellation's ~3% of a core (§4), and the input trace's `Shape`
at ~12% of a core against a `Canvas` at ~77% (§12.4).

---

## Working on eco

From a clone of the repository. mise pins the Rust toolchain and owns every
command:

```bash
mise install
mise run setup              # build the daemon, download the VAD and speaker models
mise run start              # build and run this checkout with its window
mise run start -- --replay session.wav   # a 16 kHz mono WAV instead of a call
mise run start -- --headless             # no window; pair with `mise run overlay`
mise run start -- --no-save              # keep sessions in memory only
mise run overlay            # only the window, reloading QML on save
mise run check              # the gate: lint, test, cli:check, docs:check
mise run lint               # cargo fmt and clippy, every warning fatal
mise run test               # the domain tests, i18n, VAD and speaker parity
mise run cli:gen            # docs/cli.md, from the binary's help
mise run docs:check         # every relative link and picture resolves
mise run shots              # docs/img, from the real window, offscreen
mise run preview:controls   # the control lab: every kit component
mise run bench              # answer quality per model; benchmark/README.md
mise run bench:llm          # model latency on OpenRouter
mise run bench:diarization <dir>   # diarization error rate on WAV + RTTM
mise run readme:gif         # the README icon, from packaging/eco.omapixel
mise run install            # the release build, under ~/.local (PREFIX moves it)
mise run package            # the Arch package of HEAD, into target/arch/pkg
mise run hooks:install      # run the gate on every push, and post local-check
mise run local-check        # gate HEAD and post local-check by hand
mise run uninstall          # asks first, then takes that install back off
```

**A checkout is enough to try eco.** `mise run start` runs the daemon and its
window in the foreground, until Ctrl+C, and installs nothing. It prints the
transcript, notes and answers as they come; the user service prints none of
them, so the journal holds no conversation. `mise run
install` is §13: it puts everything under `~/.local`, and the Hyprland rules
are then loaded with
`dofile(os.getenv("HOME") .. "/.local/share/eco/hypr/eco.lua")` from
`~/.config/hypr/bindings.lua`. Installing again is the upgrade. `mise tasks ls`
lists the rest.

**Nothing checks the pictures.** `mise run shots` writes `docs/img`, and no task
compares them with what the window draws today, as `cli:check` does for
`docs/cli.md`. Run it after any change to the overlay;
[the window, screen by screen](screens.md#how-this-page-stays-true) says what
in a picture differs from a real screen.

---

## Tests and the gate

```bash
mise run check
```

That is the whole gate. It runs:

| step | what it refuses |
|---|---|
| `lint` | Rust not formatted by `cargo fmt`, any clippy warning (`-D warnings`, all targets) |
| `test` | a failing `cargo test`: the domain tests, the i18n checks (`tests/i18n.rs`), the answer Markdown (`overlay/markdown.js`) against its QML test case (`tests/overlay.rs`, run offscreen by Qt's `qmltestrunner`) and the VAD and speaker parity tests against the Python fixtures (these need the models from `mise run setup`) |
| `cli:check` | a `docs/cli.md` that is not what the binary's help generates |
| `docs:check` | a relative link or image in `README.md` or `docs/*.md` whose file, or whose heading for an `#anchor`, is missing |

**The gate guards `master`, from this machine.** Nothing on GitHub runs it: it
needs the pinned toolchain and the models. `mise run hooks:install` points git
at `.githooks`, whose `pre-push` runs `scripts/local-check --pre-push` for the
commit being pushed: a red gate refuses the push, and a green one posts the
`local-check` commit status. `master` is protected: a pull request merges only
when its head carries `local-check`, and nobody pushes to it directly or
rewrites it. An administrator can merge past the rule; that is how the release
pull request, which no hook ran on, is merged. To gate it instead, check its
branch out and run `mise run local-check`, which gates that commit and posts the
status. `ECO_SKIP_LOCAL_CHECK=1` or `git push --no-verify` skips the hook.

Recorded audio replays with `--replay <file.wav>` instead of joining a call.
`mise run preview:controls` opens the control lab, and `mise run shots`
regenerates `docs/img` from the real overlay against an isolated daemon.

---

## Decisions

- **Stack:** Rust (daemon, rewritten from the Python v1) + QML/Quickshell
  (overlay).
- **Architecture:** light hexagonal (ports/adapters) in the daemon.
- **Default STT:** Deepgram. Development: whisper.cpp + Vulkan on the LAN server
  (not faster-whisper).
- **Default trigger:** manual.
- **GPU:** no dedicated GPU on the laptop; Radeon 860M iGPU.
- **Interface:** QML overlay instead of a TUI.
- **Development:** on the laptop; the Windows machine only serves models.
- **Screen sharing:** a setting, off by default — `no_screen_share` also blacks
  out screenshots.
- **Sessions:** transcription and storage only inside a session (outside,
  inputs are only measured for the live trace); removed suggestions disappear
  and leave the context; an unfinished session is marked paused and shown as
  interrupted, never reopened by itself.
- **Answers:** user-defined actions (prompt + format + optional model) instead
  of fixed fast/deep modes.
- **Mic echo:** both answers are offered — dropping echoed lines (on by
  default) and PipeWire echo cancellation (off by default); headphones are
  still recommended.

## Open questions

- Validate whether OpenRouter accepts audio input for Gemini (decides whether a
  `gemini` backend exists).
- Cost strategy for the "me" channel: VAD + Deepgram vs. local STT.
- An automatic trigger: question heuristic on the "them" channel (sentence ends
  with `?` or patterns such as "o que você acha", "pode explicar", "how would
  you"), followed by end of speech (VAD), with debounce. Not built.
- A running recap: when a segment leaves the window, the default model updates a
  recap asynchronously, off the action's critical path. Not built.
