# Agent rules

This file is the contract for anyone (human or agent) changing a
repository that uses it. It is self-contained: do not look up an
external skill to commit, and do not invent a second toolchain
beside mise.

The premises, mise rules, documentation rules, and commit rules below
are **not** project-specific. Copy them as-is. Architecture, layout, and
constraints of *this* tree live only under **This project**.

## Premises

- **Performance is a reason to change.** If a change is correct and
  makes sense, do it even when the measured gain is 0.01 ms. Do not
  skip a worthwhile optimisation because the delta is small. Measure
  when the cost of being wrong is high; do not use "nobody can
  perceive it" as a veto.
- **Zero dead code.** Delete unused functions, types, imports, flags,
  and files. Do not leave a replacement beside the thing it replaces.
- **Zero duplicated code.** One implementation of a behaviour. Extract
  or share rather than copy.
- **Zero legacy code.** No compatibility shims, deprecated aliases,
  dual paths, or "keep the old one until later" leftovers. Finish the
  cutover in the same change that introduces the new path.
- **Comments state the code.** A comment names what the next lines
  do, or a constraint the type system cannot. It is not a tutorial,
  a history, or a design essay.
- **Documentation lives in `docs/`.** Guides, measurements, language
  traps, and "how this is meant to be used" go there — not in
  comments, not in commit messages as a substitute for a doc, and not
  in this file except as rules.
- **English only.** Code, comments, commit messages, `docs/`,
  `README.md`, this file, mise task names and descriptions: English.

## Tooling: mise

mise owns the toolchain and every project command. Pin tools in
`mise.toml`. Run `mise install` before work. Invoke tools through
mise (`mise exec -- …` or, once defined, `mise run <task>`). Do not
call a system compiler or helper that is not the pinned one. Do not
add a parallel runner (Make, npm scripts, ad-hoc shell wrappers)
that duplicates a mise task.

### Tasks

Work lives in **scripts**, not in `mise.toml` bodies and not in
`mise-tasks/`. Each script is one job. mise **consumes** those
scripts: a `[tasks.name]` entry names the script, sets
`description` / `depends` / `env` / `sources` / `outputs` /
`usage` / `confirm`, and runs it. Git hooks and any other caller
run the same file. Do not duplicate the job as inline `run =`
shell, a file task, and a script.

- Put scripts under `scripts/`. Make them executable, with a
  shebang. Prefer `MISE_PROJECT_ROOT` when the script must know
  the repo root; otherwise relative paths from the project root
  (mise sets cwd to the directory of `mise.toml`).
- **Atomic:** one script, one responsibility. Compose larger work
  with `depends` (may run in parallel) or a `run` **array** of
  script invocations (sequential; a failure stops the rest,
  `set -e` for `sh`/`bash`/`zsh`). Aggregators (`check`, `bench`,
  `build`) only wire scripts together. The documented entry is
  `mise run <name>`; a hook calls `scripts/<name>`, not a second
  copy of the logic.
- **Idempotent:** running the same script twice with the same
  inputs leaves the tree in the same good state. Safe to re-run
  from a hook, CI, or `mise run`.
- Give every task a `description`. Use `alias` only when the short
  name is used often. Confirm destructive tasks with `confirm`.
- Declare `sources` and `outputs` when a task is skippable if
  inputs have not changed (builds). Use `sources` alone with
  `mise watch`.
- Arguments: a `usage` spec on the TOML task. Values arrive as
  `usage_*` environment variables. Do not use the deprecated Tera
  `arg()` / `option()` / `flag()` helpers.
- Do not add file tasks under `mise-tasks/` (or the other mise
  task directories). Do not keep a TOML task and a file task for
  the same name.
- List and inspect with `mise tasks ls` and
  `mise tasks info <name>`. Run with `mise run <name>`. Do not
  tell the user to `mise exec` a one-off that already has a task.
  Do not tell them to invoke `scripts/` when a task exists, except
  from git hooks and other non-mise entry points that must call
  the script directly.

## Documentation

Documentation is written for somebody using the project, one question
at a time, and kept true by the same tools that build it.

### The pages

- `README.md` is the front door, written for somebody installing the
  released program: what the project is in one paragraph, pictures of
  it working, Install (the release's installer, never development
  commands), Start here, the big ideas as short bold-led paragraphs
  each linking its page, the interface (keys, with a picture), What it
  does not do, Known limitations, What is broken (only while a defect
  is known), one line sending contributors to the design page,
  Requirements, license. Development and build commands live in the
  design page, not here.
- `docs/README.md` is the map: every page in the order somebody meets
  it, one line each on the question it answers, the shortest useful
  path, a "when you know what you want" table, and the conventions
  every page shares.
- `docs/how-to-*.md` holds one task per page. Each opens with
  "**The question:**" and says whether it is enough on its own, then
  "Before you start", then numbered steps, each with the command or
  the action and its real output, and ends with what can go wrong and
  where to go next. A page never explains a task that has its own page;
  it links it.
- `docs/screens.md` walks every screen in the order of use, with
  pictures, keys, and the screens that have none. It says how its
  pictures are made and what in them is pinned.
- `docs/cli.md` is generated from the program's own command
  definitions by a mise task, never edited by hand; `mise run check`
  fails when it is stale.
- `docs/what-does-not-work.md` exists only while a known defect is not
  fixed: every such defect, numbered, each with **What happens.** and
  **What to do in the meantime.**, and whether it was seen running or
  read in the code, with a "What is broken" section in `README.md`
  sampling it. No other page names a defect that is not there. When
  the last defect is fixed, the page and the README section go; when a
  known defect is left unfixed, both come back in the same change.
- Limits of the platform or of a provider are not defects: `README.md`
  lists them under "Known limitations", and each is said again on the
  page where somebody meets it.
- `docs/design.md` is for contributors: the model, the architecture,
  state on disk, protocols, packaging, what it is and is not, what was
  measured. Its sections are numbered and the numbers do not move: code
  and pages cite them as `docs/design.md §N`.

### Pictures

Pictures in `docs/img` are generated from the real program by a mise
task, against an isolated instance seeded with synthetic, versioned
fixtures — never the developer's own data, accounts or keys. Name them
`NN-<what>.png` in the order the screens page walks them. Look at every
picture a change produces. A picture that cannot be generated says so,
and where it came from.

### Discipline

- The documentation changes in the same change as the behaviour it
  describes: a new or changed command, screen, setting or defect
  updates its pages, pictures and `docs/cli.md` in that change.
- Facts come from the code or from running it. Outputs are real
  outputs. What was measured is said to be measured; what was only
  argued or read is said so in those words.
- Plain, short sentences: say what happens and what to do. No
  marketing, no promises of future work outside the defects page.
- Placeholders are named as placeholders the first time they appear.
- Every relative link, image and anchor resolves; `mise run check`
  runs a link check that fails when one does not.

## Commits (okt-task-commit)

Agents draft and create commits. The human owns publication: **never
`git push`**. When the tree is clean, say that a push is ready.

These rules inline the okt-task-commit playbook, the Conventional
Commits 1.0.0 grammar used with it, and the conventional-commits law.
There is nothing extra to fetch.

### Procedure

1. Read `git status` and `git diff --cached`. If nothing is staged,
   read the unstaged diff (`git diff` and untracked files).
2. Group hunks into **one intent per commit**. Split mixed trees with
   non-interactive staging only: `git add <path>` and
   `git restore --staged <path>`. Do not use `git add -p` or
   `git add -i`.
3. Derive **scope** from the paths touched (package, directory, or
   feature slug).
4. Draft the message (grammar below). Show every draft to the user
   **before** `git commit`.
5. Commit. Do not push. When the working tree is clean, suggest the
   user push when ready.

### Message grammar

```
<type>(<scope>)!: <subject>

<body>

<footer(s)>
```

- **type** (required): `feat`, `fix`, `docs`, `refactor`, `chore`,
  `test`, `build`, `ci`, `perf`. `feat` and `fix` are
  SemVer-significant. One type per commit: a feature and a fix are
  two commits.
- **scope**: noun in parentheses, from the paths. Omit only when no
  scope is honest.
- **!**: immediately before `:` for a breaking change
  (`feat(api)!: …`). Pair with a `BREAKING CHANGE:` footer.
- **subject**: English, imperative mood ("Add" not "Added"), ≤50
  characters, no trailing period, lowercase after the colon.
- **body** (optional): wrap at 72 columns. Explain the *why* the
  diff does not. Not a paste of the patch.
- **footers**: `Token: value` lines (`Refs: #123`,
  `BREAKING CHANGE: drops X`).

Never attribute a commit to an agent: no `Co-Authored-By: <model>`,
no `Generated with <tool>`, no model name in trailer or body. The
human running the session is the author. No opt-out. Other
`Co-Authored-By` trailers only if the user asked for that person in
this conversation.

Bad: `feat: add filter and fix duplicate insert` (two intents).
Good: `feat(filter): add priority option` then
`fix(insert): prevent duplicates`.

## This project

eco keeps voice sessions for Omarchy (Arch Linux, Hyprland/Wayland,
PipeWire): meetings, conversations, other sessions, spoken ideas — each a session
with a kind the user picks. It captures the audio, transcribes it, and
streams short answers from an LLM into an overlay, live or afterwards.
Linux only; nothing in this tree may special-case macOS or Windows.
Development happens on the Omarchy laptop. A separate Windows machine
on the LAN only serves models over HTTP; it holds no code from this
repository and is reached through the same adapters as any provider.

Two processes. The daemon (`src/`, Rust, toolchain pinned by mise,
built with cargo) does capture, VAD, transcription, triggers, prompts and the
LLM call. The overlay (`overlay/`, QML on Quickshell, launched by the daemon) only renders
events. They talk over one Unix socket at
`$XDG_RUNTIME_DIR/eco.sock`, one JSON object per line; the overlay
never calls a provider, never holds a secret and never writes the
config — it sends a draft and the daemon validates and saves it. QML
views compose the kit in `overlay/` (clean cyberpunk HUD:
monospace, hairlines, outlined surfaces, theme accent) and keep no
logic beyond binding the `Eco` singleton. The control lab
(`mise run preview:controls`) shows every kit component: a new or
changed component is shown there in the same change, and a view
reuses a kit component instead of drawing its own variant. Interface text lives only in
the language packs (`overlay/i18n/*.json`, read through `I18n.t`):
add a key to every pack, never a literal in QML; daemon errors carry a
`code` with an `error.<code>` text. Motion must mean something —
a state, new content, or the user's action; nothing animates on a timer
for its own sake.

The daemon is ports and adapters. The core (`src/domain/`) imports
no provider, audio, or network library; it depends only on the
traits in `src/ports.rs`. Every external system
is an adapter under `src/adapters/`, composed from the config in
`src/session.rs`. No `unsafe`: the crate forbids it. Add a port only when two real adapters exist or
a test needs the seam. The stated exception is a *platform seam*: a
port in `src/ports.rs` (or the window host's bridge) with one Linux
adapter, for paths, the daemon socket, audio devices and capture,
service lifecycle and desktop setup, window control, and shortcuts.
Its adapter is chosen in one place — the composition in
`src/session.rs` or the module that owns the seam. The LLM has one adapter: the OpenAI-compatible
API, selected by `base_url`. Agents reach eco only through the `eco` CLI
and the `skills/eco/SKILL.md` skill (no MCP); a change to the CLI updates the
skill in the same commit.

Never write raw audio to disk. Outside a session, audio is only measured
for the input monitor — never transcribed or stored.
Sessions are append-only logs in `~/.local/share/eco/sessions/` unless
`--no-save`; a removed suggestion must also leave the context of later
actions. Secrets come only from the
environment (`OPENROUTER_API_KEY`, `DEEPGRAM_API_KEY`,
`ELEVEN_LABS_API_KEY`, `GROQ_API_KEY`) or from the user's omapass
keyring (`api_key_omapass`), read when a provider is built; never log them or transcript text at info level.
No telemetry.

The Hyprland config on the target machine is Lua (`o.bind`,
`o.window`); do not ship legacy `hyprland.conf` syntax. Global
shortcuts reach the daemon through `socat`, not by starting a second daemon.

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
generated by `mise run cli:gen`), `docs/screens.md` (every screen, with
the pictures `mise run shots` writes to `docs/img`),
`docs/design.md` (how it
is built: model, architecture, protocol, state on disk; cited as
`docs/design.md §N`), `docs/benchmarks.md` (measurements),
`docs/i18n.md` (translations). Setup: `README.md`.
