# Agent rules

The contract for anyone, human or agent, changing this repository. It is
self-contained: do not fetch an external skill to commit, and do not add
a second toolchain beside mise.

Every section except **This project** is shared across repositories and
copied as-is. Architecture, layout, and constraints of this tree live
only under **This project**.

## Premises

- **Performance is a reason to change.** A correct change that makes
  sense is made even when it saves 0.01 ms. Measure when being wrong is
  costly; "nobody will notice" is not a veto.
- **Zero dead code.** Delete unused functions, types, imports, flags,
  and files. A replacement never sits beside what it replaces.
- **Zero duplicated code.** One implementation per behaviour; extract
  and share instead of copying.
- **Zero legacy code.** No shims, deprecated aliases, dual paths, or
  "remove later" leftovers. A cutover finishes in the change that starts
  it.
- **Comments state the code.** A comment names what the next lines do,
  or a constraint the language cannot express. Not a tutorial, a
  history, or an essay.
- **English only.** Code, comments, commits, `docs/`, `README.md`, this
  file, and mise task names and descriptions.

## Working

- **No spin-busy.** Do not rebuild or rerun checks after each edit.
  Implement the whole change first and make it work, then run the
  project's full verification once: whatever build, tests, and checks
  the project defines (**This project** names them). A session that ends
  in several commits verifies the last one, not each commit.
- **Pictures are for releases.** Capture the whole program, on every
  target it ships to, only when preparing a release. Between releases,
  capture only what a change touches, and only when you need to see it.
- **Clean up after every run.** A build, test, or benchmark removes
  what it started, whether it passes or fails: processes, servers,
  devices, containers, temporary files, caches. Keep only the result it
  was meant to produce.

## Tooling: mise

mise pins every tool in `mise.toml` and owns every project command. Run
`mise install` before work, then invoke tools through `mise run <task>`
(or `mise exec -- …` when no task exists). Never call an unpinned system
tool. Never add a parallel runner (Make, npm scripts, shell wrappers).

### Tasks

The work lives in **scripts**; mise only consumes them. A
`[tasks.name]` entry names the script and sets `description`, `depends`,
`env`, `sources`, `outputs`, `usage`, and `confirm`. Git hooks and other
callers run the same script. A job is never duplicated as inline
`run =` shell, a file task, and a script.

- Scripts live in `scripts/`, executable, with a shebang. Use
  `MISE_PROJECT_ROOT` when the repo root matters; otherwise paths are
  relative to the project root.
- **Atomic:** one script, one job. Compose with `depends` (parallel) or
  a `run` array (sequential, stops on failure, `set -e` in shell).
  Aggregators such as `check`, `build`, or `bench` only wire scripts
  together.
- **Idempotent:** a second run with the same inputs leaves the same good
  state. Safe from a hook, CI, or `mise run`.
- Every task has a `description`. Use `alias` only for names used often.
  Destructive tasks use `confirm`.
- Skippable builds declare `sources` and `outputs`; `mise watch` needs
  `sources` alone.
- Arguments come from a `usage` spec as `usage_*` environment variables.
  Never use the deprecated Tera `arg()`, `option()`, or `flag()`.
- No file tasks under `mise-tasks/` or any other mise task directory.
- Inspect with `mise tasks ls` and `mise tasks info <name>`. Point users
  to `mise run <name>`, never to a script or `mise exec` one-off that a
  task already covers.

## Documentation

Documentation answers one question at a time for someone using the
project. The tools that build the project keep it true.

- `README.md` is the front door for someone using the project: what it
  is, how to install or obtain it, the main ideas with links to their
  pages, known limitations, requirements, and license, plus pictures
  when the project has a visible interface. Build and development
  commands live in `docs/`.
- `docs/` holds the guides, measurements, and language traps. They do
  not go in comments, in commit messages, or in this file except as
  rules. Each page answers one question and links to the page for any
  task it does not cover.
- Docs change in the same change as the behaviour they describe.
- Facts come from the code or from running it, and outputs are real.
  Say what was measured and what was only read or argued.
- Write short, plain sentences. No marketing and no promises of future
  work.
- Pictures come from the real program running on isolated, synthetic,
  versioned data, never personal data, accounts, or keys. They are
  regenerated for a release (see **Working**).
- Every relative link, image, and anchor resolves.

## Commits (okt-task-commit)

Agents draft and create commits. The human publishes: **never
`git push`**. When the tree is clean, say a push is ready.

### Procedure

1. Read `git status` and `git diff --cached`. If nothing is staged,
   read `git diff` and the untracked files.
2. Make **one intent per commit**. Split with non-interactive staging
   only: `git add <path>` and `git restore --staged <path>`. Never
   `git add -p` or `git add -i`.
3. Take the **scope** from the paths touched.
4. Show every draft message to the user **before** `git commit`.
5. Commit without pushing.

### Message grammar (Conventional Commits 1.0.0)

```
<type>(<scope>)!: <subject>

<body>

<footer(s)>
```

- **type**: `feat`, `fix`, `docs`, `refactor`, `chore`, `test`,
  `build`, `ci`, or `perf`. One type per commit; `feat` and `fix` drive
  SemVer.
- **scope**: a noun from the paths. Omit only when no scope is honest.
- **!**: marks a breaking change, together with a `BREAKING CHANGE:`
  footer.
- **subject**: imperative, at most 50 characters, lowercase after the
  colon, no trailing period.
- **body**: optional, wrapped at 72 columns. Explain why, not the patch.
- **footers**: `Token: value` lines, such as `Refs: #123`.

Never attribute a commit to an agent: no `Co-Authored-By: <model>`, no
`Generated with <tool>`, no model name anywhere. The human running the
session is the author, with no opt-out. Add other `Co-Authored-By`
trailers only for people the user named in this conversation.

Bad: `feat: add filter and fix duplicate insert`.
Good: `feat(filter): add priority option`, then
`fix(insert): prevent duplicates`.

## This project

eco keeps voice sessions for Omarchy (Arch Linux, Hyprland/Wayland,
PipeWire): meetings, conversations, other sessions, spoken ideas — each a session
with a kind the user picks. It captures the audio, transcribes it, and
streams short answers from an LLM into an overlay, live or afterwards.
It runs on Linux and on Windows; macOS (Apple Silicon only, macOS 14.6 or
later) is in progress. Development happens on the Omarchy laptop.
Windows is built and tested by hand on a Windows machine, and macOS on a
Mac (`mise run check` on each); there is no CI for either. A
Windows machine on the LAN may also serve models over HTTP, reached
through the same adapters as any provider.

Two processes. The daemon (`src/`, Rust, toolchain pinned by mise,
built with cargo) does capture, VAD, transcription, triggers, prompts and the
LLM call. The overlay (`overlay/`, QML) only renders events. It runs in
`eco-window`, a small C++ host on plain Qt 6 in `window/` (cmake and ninja
pinned by mise, `mise run window:build`), one process per window, launched by
the daemon. The host is a bridge and holds no logic: it gives QML the socket,
files, the environment and the clipboard (`EcoHost`), and nothing in it knows
the compositor — focus, raising and screen-share hiding go through the
daemon's `WindowControl` port; a call only Windows or macOS needs from a
window is a bridge method there, behind `Q_OS_WIN` or `Q_OS_MACOS`. Both
talk over one local socket (a Unix socket at `$XDG_RUNTIME_DIR/eco.sock` on
Linux and at `$TMPDIR/eco.sock` on macOS, a named pipe on Windows, behind
`src/adapters/local_socket.rs`), one
JSON object per line; the overlay
never calls a provider, never holds a secret and never writes the
config — it sends a draft and the daemon validates and saves it. QML
views compose the kit in `overlay/Eco/Kit/` (clean cyberpunk HUD:
monospace, hairlines, outlined surfaces, theme accent) and keep no
logic beyond binding the `Eco` singleton. The overlay is QML modules by
role under `overlay/Eco/` (`docs/design.md §3`), imported by name, never by
relative path: Core imports none of them, Kit only Core, and Live,
Sessions, Dialogs and Settings only Kit and Core — never each other; a
piece two of them share goes to Kit, and Window composes them; `mise run
check` runs `scripts/overlay-check` to enforce this. The control lab
(`mise run preview:controls`) shows every kit component: a new or
changed component is shown there in the same change, and a view
reuses a kit component instead of drawing its own variant — also enforced by
`mise run check`. Interface text lives only in
the language packs (`overlay/Eco/Core/i18n/*.json`, read through `I18n.t`):
add a key to every pack, never a literal in QML; daemon errors carry a
`code` with an `error.<code>` text. Motion must mean something —
a state, new content, or the user's action; nothing animates on a timer
for its own sake.

The daemon is ports and adapters. The core (`src/domain/`) imports
no provider, audio, or network library; it depends only on the
traits in `src/ports.rs`. Every external system
is an adapter under `src/adapters/`, composed from the config in
`src/session.rs`. No `unsafe`: the crate forbids it; a Windows or macOS
API is reached through a crate with a safe interface, or from the C++ host. Add a port only when two real adapters exist or
a test needs the seam. A *platform seam* is a port in `src/ports.rs`
(or the window host's bridge) with a Linux, a Windows and a macOS adapter, for
paths, the daemon socket, audio devices and capture, service lifecycle
and desktop setup, window control, shortcuts and the shell that runs
hooks. Its adapter is chosen with `#[cfg]` in one place — the
composition in `src/session.rs` or the module that owns the seam;
nothing else in the tree names a platform. Code that is POSIX (the Unix
socket, file modes, signals, `sh -c`) is `#[cfg(unix)]` and shared by
Linux and macOS; code that is Linux alone (PipeWire, Hyprland, systemd,
`/proc`, pacman) is `#[cfg(target_os = "linux")]`. A capability one platform
lacks (echo cancellation, the omapass keyring on Windows) is a known
limitation the adapter reports, not a silent no-op. The LLM has one adapter: the OpenAI-compatible
API, selected by `base_url`. Agents reach eco only through the `eco` CLI
and the `skills/eco/SKILL.md` skill (no MCP); a change to the CLI updates the
skill in the same commit.

A connection to a provider that drops, closes or stalls while a session
runs is opened again, with backoff, for as long as the session captures;
transcription never stops silently, the overlay says when it is down and
when it is back, and audio heard meanwhile is kept in memory only, within a
bound, and sent once it is back.

Tests. A change to behaviour comes with a test that fails without it.
Failure paths are behaviour: a network that is down, a provider that closes
or stalls, a process that exits, a file that is half written — each tested
deterministically with fake adapters and tokio's paused clock, never against
a real provider, the network, the user's socket or data. A test pins
behaviour (a line sent, an event, an exit code, an error); none restates the
code. Adapters that only run a program (`hyprctl`, `pw-*`, `systemctl`) are
tested for the arguments they build and the output they read. A platform's
adapters are tested on that platform: tests that need Linux are
`#[cfg(target_os = "linux")]`, those that need macOS
`#[cfg(target_os = "macos")]`, those that need either `#[cfg(unix)]`, those
that need Windows `#[cfg(windows)]`, and the rest run on all three. `mise run
coverage` measures lines per file: the target is 100% of `src/` except
`src/bench/`, and every line left without a test is listed with its reason
in `docs/benchmarks.md`, in the same change that leaves it; until a file
reaches the target, no change lowers its coverage.

Never write raw audio to disk. Outside a session, audio is only measured
for the input monitor — never transcribed or stored.
Sessions are append-only logs in the data directory's `sessions/`
(`~/.local/share/eco` on Linux, `%LOCALAPPDATA%\eco` on Windows,
`~/Library/Application Support/eco` on macOS) unless
`--no-save`; a removed suggestion must also leave the context of later
actions. Secrets come only from the
environment (`OPENROUTER_API_KEY`, `DEEPGRAM_API_KEY`,
`ELEVEN_LABS_API_KEY`, `GROQ_API_KEY`) or from the user's omapass
keyring (`api_key_omapass`), read when a provider is built; never log them or transcript text at info level.
No telemetry.

The Hyprland config on the target machine is Lua (`o.bind`,
`o.window`); do not ship legacy `hyprland.conf` syntax. On Linux, global
shortcuts run `eco window` commands, which reach the running daemon; on
Windows the daemon registers them itself. They never start a second
daemon.

The answer benchmark is the one exception to the scripts and docs rules: it
is a self-contained tool in `benchmark/` — one Python script on the standard
library (pinned by mise), its README, config, synthetic data and run folders —
and `mise run bench` calls `benchmark/bench.py` directly. Its data is
synthetic; never put a real person's résumé, sessions or keys there.

`mise tasks ls` lists the commands. Verify with `mise run check`; the
pre-push hook (`mise run hooks:install`) runs it and posts the `local-check`
status `master` requires, and release-please cuts releases from merged pull
requests (`docs/design.md §13`).
Replay recorded audio with `--replay <file.wav>` instead of joining a
call.

Guides: `docs/README.md` (the map: every page and which question it
answers), the `docs/how-to-*.md` pages (one task each: install and
remove, register models, audio sources, record, ask and use skills,
write skills, name the speakers, find, import, translate, cost, a
model on the LAN, eco from an agent), `docs/cli.md` (every command,
generated by `mise run cli:gen` from the usage spec `eco usage` prints, the
same spec the shell completions use), `docs/screens.md` (every screen, with
the pictures `mise run shots` writes to `docs/img`),
`docs/design.md` (how it
is built: model, architecture, protocol, state on disk; cited as
`docs/design.md §N`), `docs/benchmarks.md` (measurements),
`docs/i18n.md` (translations). Setup: `README.md`.
