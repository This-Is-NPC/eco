# The eco documentation

Every page in this directory, in the order somebody meets them, and what each
one is for. [The front door](../README.md) says what eco is and how to get it;
this page says which document answers which question.

---

## Start here

If eco is not on the laptop yet, read them in this order. Each page is written
to be followed on its own, so skipping one costs nothing but the thing it was
about.

1. [**How to install eco, and how to take it off again**](how-to-install-and-remove.md)
   — the installer; the first config, the Hyprland line, what the package puts
   on the machine, and what `sudo pacman -R eco` leaves.
2. [**How to register models**](how-to-register-models.md) — the keys, from the
   environment or omapass, one chat model and one transcription model, and which
   one does what.
3. [**How to set up audio sources**](how-to-set-up-audio-sources.md) — you on
   the microphone, everybody else on what the laptop plays, and what to do about
   echoes.
4. [**How to record a session**](how-to-record-a-session.md) — open, pause, end,
   and pick it up again.
5. [**How to ask, and how to use skills**](how-to-ask-and-use-skills.md) — a
   question, a skill, a note, and what the model is given to answer them.
6. [**How to name the speakers**](how-to-name-the-speakers.md) — `Speaker 1`
   becomes Ana, on this session and by voice on the next.
7. [**How to find a session**](how-to-find-a-session.md) — the search, the
   filters, the live ones and tags.

Each of these is its own step, and needs 1 to 4 done first:

8. [**How to write skills**](how-to-write-skills.md) — a request written once,
   on the model you choose, run with one key and sent to another program.
9. [**How to import a recording**](how-to-import-a-recording.md) — a Teams or
   Zoom transcript, or an audio or video file, as a session like any other.
10. [**How to translate a session**](how-to-translate-a-session.md) — another
    language under every line, live or stored.
11. [**How to read what a session cost**](how-to-read-what-a-session-cost.md) —
    which part was reported, which estimated, and why a part can be unknown.
12. [**How to use a model on the LAN**](how-to-use-a-model-on-the-lan.md) — a
    computer at home transcribing and answering, with no key and no cost.
13. [**How to use eco from an agent**](how-to-use-eco-from-an-agent.md) — the
    command line and the `eco` skill, which are the only way in.

**The shortest useful path** is 1, 2, 3, 4 — install it, give it a model to
transcribe and one to answer, tell it who it hears, and record — and then 5 in
the middle of the next call. Outside a session eco only measures the audio, so
nothing is kept until step 4.

## When you know what you want

| the page | what it holds |
|---|---|
| [the command line](cli.md) | every command, every flag, in full. **Generated** from the binary's help by `mise run cli:gen`; `mise run check` fails when the two have come apart, so it is never edited by hand. |
| [the window, in the order somebody meets it](screens.md) | every screen the overlay draws, as a walk from opening it to looking back at a session weeks later — with the keys, the dialogs a happy path never reaches, and what has no screen. |
| [how it is built](design.md) | for contributors: the model, the architecture, the socket, the state on disk and the gate. Its section numbers are cited from code, errors and other pages as `docs/design.md §N`, so they do not move. |
| [benchmarks](benchmarks.md) | the measurements, dated: model latency, the LAN models, the prompt cache, diarization, streaming transcription, the overlay's cost. Answer quality is [`benchmark/README.md`](../benchmark/README.md). |
| [translations](i18n.md) | the language packs: fixing a translation, adding a language, and the rules every key follows. |

## Conventions across every page

**Ids and paths are placeholders.** `dea44e92fd25` is a session's id and
`9bb9e6f8bd1e` a person's; yours come from `eco sessions` and `eco people`.
`/home/you` stands for your home directory. Each page says which placeholders it
uses where it first uses them.

**Outputs come from an isolated daemon, not from somebody's real sessions.**
Every command output was run against a test daemon with its own runtime, data
and config directories, never the user's socket. Where that shows — a log path
under a moved data directory — the page says what it reads on your machine.

**Pictures under `img/` are generated.** `mise run shots` writes `01` to `51`
from the real overlay, offscreen, against an isolated daemon seeded with
synthetic sessions and people; none of it is a real person, recording or key.
There is no `shots:check`: the live session runs on the clock of the run and the
software renderer places text differently each time, so two runs never write
the same bytes ([the window](screens.md#how-this-page-stays-true) says what is
pinned and what is not). `img/eco.gif` is the icon, drawn by
`mise run readme:gif`.

**What is measured is said to be measured.** A number in these pages came off a
machine, with its date in [benchmarks](benchmarks.md). Where something was only
read in the code or argued, the page says so in those words.
