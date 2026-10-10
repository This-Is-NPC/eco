//! eco — voice sessions with an AI beside them, for Omarchy. The daemon: capture, VAD,
//! transcription, sessions and the LLM, served to the QML overlay over a Unix socket.

mod adapters;
mod bench;
mod cli;
mod config;
mod domain;
mod import;
mod lifecycle;
mod paths;
mod ports;
mod session;
mod setup;
mod skill;

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::ports::DesktopIntegration;

#[derive(Debug, Parser)]
#[command(
    name = "eco",
    version,
    about = "Voice sessions with an AI beside them, for Omarchy"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Download the models, load the Hyprland rules and keep the agent skill current.
    Setup {
        /// Also publish the `eco` skill to these agent harnesses: agents, claude-code.
        #[arg(long, value_delimiter = ',')]
        harnesses: Vec<String>,
    },
    /// Start the daemon service and open its window.
    Start {
        /// Do not open the overlay.
        #[arg(long)]
        headless: bool,
    },
    /// Stop the running daemon.
    Stop,
    /// Restart the daemon and reopen its window if it was visible.
    Restart,
    /// Show whether the daemon is running.
    Status {
        /// Fail if the daemon is running a different executable.
        #[arg(long, hide = true)]
        expect_current_exe: bool,
        /// Exit nonzero when the overlay is closed.
        #[arg(long, hide = true)]
        window_open: bool,
    },
    /// Run the daemon in the foreground (used by the user service and development).
    #[command(hide = true)]
    Daemon {
        /// 16 kHz mono WAV used instead of capture.
        #[arg(long)]
        replay: Option<PathBuf>,
        /// Do not open the overlay.
        #[arg(long)]
        headless: bool,
        /// Keep sessions only in memory.
        #[arg(long)]
        no_save: bool,
    },
    /// Tell the speakers of the regions on stdin apart (run by the daemon on import).
    #[command(hide = true)]
    Diarize { model: PathBuf },
    /// Print the usage spec (KDL) that docs/cli.md and the shell completions are generated from.
    #[command(hide = true)]
    Usage,
    /// Measure what docs/benchmarks.md reports.
    Bench {
        #[command(subcommand)]
        target: Bench,
    },
    /// List stored sessions, newest first, as JSON.
    Sessions {
        /// Only sessions of this kind.
        #[arg(long)]
        kind: Option<String>,
        /// Only sessions linked to this person id.
        #[arg(long)]
        person: Option<String>,
        /// Only sessions with this tag (any case).
        #[arg(long)]
        tag: Option<String>,
        /// Only sessions whose title, tags, lines, notes or answers hold this text (any case or accents).
        #[arg(long)]
        search: Option<String>,
    },
    /// Show a session's summary and timeline.
    Show { id: String },
    /// Permanently delete a paused or ended session.
    Delete { id: String },
    /// Give a session a new title and/or kind.
    Rename {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        kind: Option<String>,
    },
    /// Ask a session a question and wait for the answer.
    Ask {
        id: String,
        #[arg(required = true, trailing_var_arg = true)]
        question: Vec<String>,
    },
    /// Run a configured action on a session and wait for the answer.
    Action { id: String, name: String },
    /// Translate a session's lines and answers below each one, into a language from now on.
    #[command(group(clap::ArgGroup::new("into").required(true)))]
    Translate {
        id: String,
        /// The language code to translate into (en, pt, ja…), other than the session's own.
        #[arg(long, group = "into")]
        lang: Option<String>,
        /// Stop translating this session.
        #[arg(long, group = "into")]
        off: bool,
    },
    /// Run the hook of the action that gave an answer (its id in `eco show`) and wait for it.
    Send { id: String, answer: String },
    /// Keep a note in a session: a fact later answers use, not a question.
    Note {
        id: String,
        #[arg(required = true, trailing_var_arg = true)]
        text: Vec<String>,
    },
    /// Print a session's transcript as WebVTT, e.g. `eco export <id> > session.vtt`.
    Export { id: String },
    /// Say who a speaker of a session is (its label, as `eco show` lists it).
    #[command(group(clap::ArgGroup::new("who").required(true)))]
    Speaker {
        id: String,
        label: String,
        /// Their name: the known person called that, or a new one.
        #[arg(long, group = "who")]
        name: Option<String>,
        /// A known person, by id (see `eco people`).
        #[arg(long, group = "who")]
        person: Option<String>,
        /// No one: the speaker goes back to their label, or eco's guess of who they are is cleared.
        #[arg(long, group = "who")]
        clear: bool,
    },
    /// Assign one line or every speaker in a session to a person.
    Assign {
        id: String,
        #[command(subcommand)]
        target: AssignTarget,
    },
    /// Link a person to a session, independently of speaker identification.
    #[command(group(clap::ArgGroup::new("participant").required(true)))]
    Participant {
        id: String,
        #[arg(long, group = "participant")]
        name: Option<String>,
        #[arg(long, group = "participant")]
        person: Option<String>,
        #[arg(long, group = "participant")]
        remove: Option<String>,
    },
    /// Show or change which context slots a session has on.
    Context {
        id: String,
        /// Turn this context slot on (repeatable).
        #[arg(long)]
        add: Vec<String>,
        /// Turn this context slot off (repeatable).
        #[arg(long)]
        remove: Vec<String>,
    },
    /// Change one line of a session, named by who said it and its `at` (see `eco show`).
    #[command(group(clap::ArgGroup::new("change").required(true)))]
    Line {
        id: String,
        who: String,
        at: f64,
        /// Remove it: it leaves the session, its context and its export.
        #[arg(long, group = "change")]
        remove: bool,
        /// Its corrected text.
        #[arg(long, group = "change")]
        text: Option<String>,
    },
    /// The tags that group sessions, several per session.
    Tag {
        #[command(subcommand)]
        action: TagCommand,
    },
    /// The people linked to sessions; without a subcommand, list them.
    People {
        #[command(subcommand)]
        action: Option<PeopleCommand>,
    },
    /// Do what a shortcut does, on the newest eco window or the session shown; returns once sent.
    Window {
        #[command(subcommand)]
        action: WindowCommand,
    },
    /// Import an audio or video file into a new session.
    Import {
        path: PathBuf,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        language: Option<String>,
        /// Who the file is heard as; the user by default.
        #[arg(long)]
        participant: Option<String>,
        /// When the recording began, local time: YYYY-MM-DDTHH:MM[:SS]. By default
        /// the date the file was recorded with, else when it last changed.
        #[arg(long, value_parser = cli::local_date)]
        date: Option<f64>,
        /// Return once the import starts instead of when it ends.
        #[arg(long)]
        no_wait: bool,
    },
}

#[derive(Debug, Subcommand)]
enum AssignTarget {
    /// Assign exactly one transcript line.
    #[command(group(clap::ArgGroup::new("person_choice").required(true)))]
    Line {
        who: String,
        at: f64,
        #[arg(long, group = "person_choice")]
        person: Option<String>,
        #[arg(long, group = "person_choice")]
        name: Option<String>,
    },
    /// Assign all speech lines in the session.
    #[command(group(clap::ArgGroup::new("person_choice").required(true)))]
    All {
        #[arg(long, group = "person_choice")]
        person: Option<String>,
        #[arg(long, group = "person_choice")]
        name: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum PeopleCommand {
    /// Link named live speakers from older sessions to people.
    Adopt,
    /// Keep a new person by a name no one has yet.
    Add { name: String },
    /// Rename a person in every session that names them.
    Rename { id: String, name: String },
    /// `from` is the same person as `into`: merge their voices and sessions.
    Merge { into: String, from: String },
    /// Delete a person and the voices kept for them; sessions keep the name.
    Forget { id: String },
}

#[derive(Debug, Subcommand)]
enum TagCommand {
    /// Every tag, with how many sessions carry it.
    List,
    /// Tag a session; a tag another session has keeps the case it was first written in.
    Add { id: String, tag: String },
    /// Take a tag off a session.
    Remove { id: String, tag: String },
    /// Rename a tag in every session; renaming it to another tag joins the two.
    Rename { from: String, to: String },
    /// Take a tag off every session.
    Delete { tag: String },
}

#[derive(Debug, Subcommand)]
enum WindowCommand {
    /// Give the keyboard to the newest eco window, opening one when none is open.
    Focus,
    /// Open or close the settings, and give the window the keyboard.
    Config,
    /// Open the dialog that starts a session, and give the window the keyboard.
    New,
    /// List the sessions when no session is on screen, and give the window the keyboard.
    Sessions,
    /// Open the import dialog, with this file filled in, and give the window the keyboard.
    Import { path: Option<PathBuf> },
    /// Run a configured action on the session shown; its answer shows in the window.
    Action { name: String },
    /// Pause the session shown if it records, else resume it.
    Toggle,
}

#[derive(Debug, Subcommand)]
enum Bench {
    /// Time to first token of candidate models on OpenRouter (needs OPENROUTER_API_KEY).
    Llm,
    /// Diarization error rate over recordings with reference RTTMs.
    Diarization {
        /// Directory of `<name>.wav` (16 kHz mono) with `<name>.rttm` and optional `<name>.uem`.
        dir: PathBuf,
        /// File naming the recordings to run, one per line.
        #[arg(long)]
        list: Option<PathBuf>,
        /// Speaker embedding model (ONNX); the one `eco setup` downloads by default.
        #[arg(long)]
        model: Option<PathBuf>,
        /// Clustering thresholds to compare, e.g. 0.5,0.6,0.7.
        #[arg(long, value_delimiter = ',', default_value = "0.65")]
        threshold: Vec<f32>,
        /// Smallest share of the speech a speaker may hold.
        #[arg(long, default_value_t = 0.02)]
        min_share: f32,
        /// Seconds forgiven around reference boundaries, half on each side.
        #[arg(long, default_value_t = 0.0)]
        collar: f64,
        /// Directory to write eco's turns to as `<name>.rttm` (first threshold).
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// How safely people are named by voice across AMI meeting series.
    People {
        /// Directory of AMI `<series><a–d>.wav` with their `.rttm`.
        dir: PathBuf,
        /// Series whose meetings a–d hold the same people.
        #[arg(
            long,
            value_delimiter = ',',
            default_value = "ES2004,IS1009,TS3003,EN2002"
        )]
        series: Vec<String>,
    },
}

/// What `eco` runs for the command it was given.
enum Run {
    SetUp {
        harnesses: Vec<String>,
    },
    Start {
        show_window: bool,
    },
    Stop,
    Restart,
    Status {
        expect_current_exe: bool,
        window_open: bool,
    },
    Daemon {
        replay: Option<PathBuf>,
        headless: bool,
        save_sessions: bool,
    },
    Diarize {
        model: PathBuf,
    },
    Usage,
    BenchLlm,
    BenchDiarization(bench::diarization::Options),
    BenchPeople {
        dir: PathBuf,
        series: Vec<String>,
    },
    /// A request to the running daemon.
    Client(cli::Request),
}

impl From<Command> for Run {
    fn from(command: Command) -> Self {
        let request = match command {
            Command::Setup { harnesses } => return Run::SetUp { harnesses },
            Command::Start { headless } => {
                return Run::Start {
                    show_window: !headless,
                };
            }
            Command::Stop => return Run::Stop,
            Command::Restart => return Run::Restart,
            Command::Status {
                expect_current_exe,
                window_open,
            } => {
                return Run::Status {
                    expect_current_exe,
                    window_open,
                };
            }
            Command::Daemon {
                replay,
                headless,
                no_save,
            } => {
                return Run::Daemon {
                    replay,
                    headless,
                    save_sessions: !no_save,
                };
            }
            Command::Diarize { model } => return Run::Diarize { model },
            Command::Usage => return Run::Usage,
            Command::Bench { target } => {
                return match target {
                    Bench::Llm => Run::BenchLlm,
                    Bench::Diarization {
                        dir,
                        list,
                        model,
                        threshold,
                        min_share,
                        collar,
                        output,
                    } => Run::BenchDiarization(bench::diarization::Options {
                        dir,
                        list,
                        model,
                        thresholds: threshold,
                        min_share,
                        collar,
                        output,
                    }),
                    Bench::People { dir, series } => Run::BenchPeople { dir, series },
                };
            }
            Command::Sessions {
                kind,
                person,
                tag,
                search,
            } => cli::Request::Sessions {
                kind,
                person,
                // A tag given blank matches no session.
                tag: tag.map(|tag| domain::session::tag_name(&tag).unwrap_or_default()),
                search,
            },
            Command::Show { id } => cli::Request::Show { id },
            Command::Delete { id } => cli::Request::Delete { id },
            Command::Rename { id, title, kind } => cli::Request::Rename { id, title, kind },
            Command::Ask { id, question } => cli::Request::Ask {
                id,
                question: question.join(" "),
            },
            Command::Action { id, name } => cli::Request::Action { id, name },
            Command::Send { id, answer } => cli::Request::Send { id, answer },
            Command::Translate { id, lang, off: _ } => cli::Request::Translate {
                id,
                language: lang.unwrap_or_default().trim().to_lowercase(),
            },
            Command::Note { id, text } => cli::Request::Note {
                id,
                text: text.join(" "),
            },
            Command::Export { id } => cli::Request::Export { id },
            Command::Speaker {
                id,
                label,
                name,
                person,
                clear: _,
            } => {
                let who = match (name, person) {
                    (Some(name), _) => cli::Who::Name(name),
                    (_, Some(person)) => cli::Who::Person(person),
                    _ => cli::Who::Nobody,
                };
                cli::Request::Speaker { id, label, who }
            }
            Command::Assign { id, target } => match target {
                AssignTarget::Line {
                    who,
                    at,
                    person,
                    name,
                } => cli::Request::AssignLine {
                    id,
                    who,
                    at,
                    person,
                    name,
                },
                AssignTarget::All { person, name } => cli::Request::AssignAll { id, person, name },
            },
            Command::Context { id, add, remove } => cli::Request::Context { id, add, remove },
            Command::Participant {
                id,
                name,
                person,
                remove,
            } => cli::Request::Participant {
                id,
                remove: remove.is_some(),
                person: remove.or(person),
                name,
            },
            Command::Line {
                id,
                who,
                at,
                remove: _,
                text,
            } => cli::Request::Line {
                id,
                who,
                at,
                change: text.map_or(cli::LineChange::Remove, cli::LineChange::Edit),
            },
            Command::People { action } => cli::Request::People(match action {
                None => cli::People::List,
                Some(PeopleCommand::Adopt) => cli::People::Adopt,
                Some(PeopleCommand::Add { name }) => cli::People::Add { name },
                Some(PeopleCommand::Rename { id, name }) => cli::People::Rename { id, name },
                Some(PeopleCommand::Merge { into, from }) => cli::People::Merge { into, from },
                Some(PeopleCommand::Forget { id }) => cli::People::Forget { id },
            }),
            Command::Tag { action } => cli::Request::Tag(match action {
                TagCommand::List => cli::Tag::List,
                TagCommand::Add { id, tag } => cli::Tag::Add { id, tag },
                TagCommand::Remove { id, tag } => cli::Tag::Remove { id, tag },
                TagCommand::Rename { from, to } => cli::Tag::Rename { from, to },
                TagCommand::Delete { tag } => cli::Tag::Delete { tag },
            }),
            Command::Window { action } => {
                let call = |call| cli::Window::Call { call, path: None };
                cli::Request::Window(match action {
                    WindowCommand::Focus => call("focus"),
                    WindowCommand::Config => call("config"),
                    WindowCommand::New => call("new_session"),
                    WindowCommand::Sessions => call("sessions"),
                    WindowCommand::Import { path } => cli::Window::Call {
                        call: "import",
                        path,
                    },
                    WindowCommand::Action { name } => cli::Window::Action { name },
                    WindowCommand::Toggle => cli::Window::Toggle,
                })
            }
            Command::Import {
                path,
                title,
                kind,
                language,
                participant,
                date,
                no_wait,
            } => cli::Request::Import {
                path,
                title,
                kind,
                language,
                participant,
                started_at: date,
                wait: !no_wait,
            },
        };
        Run::Client(request)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let ran = match Run::from(Cli::parse().command) {
        Run::SetUp { harnesses } => set_up(harnesses).await,
        Run::Start { show_window } => lifecycle::start(show_window).await,
        Run::Stop => lifecycle::stop().await,
        Run::Restart => lifecycle::restart().await,
        Run::Status {
            expect_current_exe,
            window_open,
        } => lifecycle::status(expect_current_exe, window_open).await,
        Run::Daemon {
            replay,
            headless,
            save_sessions,
        } => session::run(&paths::config_file(), replay, headless, save_sessions).await,
        Run::Diarize { model } => adapters::diarizer_process::serve(&model),
        Run::Usage => {
            print!("{}", usage_spec());
            Ok(())
        }
        Run::BenchLlm => bench::llm::run().await,
        Run::BenchDiarization(options) => bench::diarization::run(options).await,
        Run::BenchPeople { dir, series } => bench::people::run(dir, series).await,
        // The client prints its JSON; its code is the exit status.
        Run::Client(request) => match cli::run(&paths::socket_path(), request).await {
            0 => Ok(()),
            code => std::process::exit(code),
        },
    };
    // A bad config is the user's to fix: say what is wrong, without a trace.
    if let Err(failure) = &ran
        && let Some(error) = failure.downcast_ref::<config::ConfigError>()
    {
        eprintln!("eco: {error}");
        std::process::exit(1);
    }
    ran
}

/// Download the models, load eco's rules into the desktop, then publish the
/// skill to `harnesses` and refresh it wherever it is already installed. A
/// desktop config it cannot write is a warning and exit status 1 after the
/// rest ran.
async fn set_up(harnesses: Vec<String>) -> Result<()> {
    for done in setup::run().await? {
        println!("eco: {done}");
    }
    let rules = setup::desktop().load_rules();
    match &rules {
        Ok(done) => println!("eco: {done}"),
        Err(warning) => eprintln!("eco: warning: {warning}"),
    }
    let home = paths::home();
    for harness in skill_targets(&home, harnesses) {
        println!("eco: {}", skill::install(&home, &harness)?);
    }
    // The rest of setup ran; the exit status still says the rules are not loaded.
    if rules.is_err() {
        std::process::exit(1);
    }
    Ok(())
}

/// The harnesses to publish the skill to: `asked`, then each harness under
/// `home` that already holds eco's skill and was not asked.
fn skill_targets(home: &Path, asked: Vec<String>) -> Vec<String> {
    let mut targets = asked;
    for harness in skill::installed(home) {
        if !targets.iter().any(|target| target == harness) {
            targets.push(harness.into());
        }
    }
    targets
}

/// The usage spec of the visible `eco` command line, generated from its clap
/// definition. It leaves out the version, which release-please moves, so that
/// docs/cli.md only changes with the commands.
fn usage_spec() -> String {
    let mut spec = clap_usage::spec(&mut <Cli as clap::CommandFactory>::command(), "eco");
    spec.version = None;
    spec.cmd.subcommands.retain(|_, command| !command.hide);
    spec.to_string()
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;
    use clap::error::ErrorKind;

    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("eco").chain(args.iter().copied()))
    }

    fn run(args: &[&str]) -> Run {
        Run::from(parse(args).expect("parses").command)
    }

    fn request(args: &[&str]) -> cli::Request {
        let Run::Client(request) = run(args) else {
            panic!("expected a request to the daemon")
        };
        request
    }

    fn rejected(args: &[&str]) -> ErrorKind {
        parse(args).expect_err("rejected").kind()
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn the_cli_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn the_usage_spec_names_every_visible_top_level_command() {
        let spec = usage_spec();
        for command in Cli::command().get_subcommands() {
            let line = format!("\ncmd {} ", command.get_name());
            assert_eq!(
                spec.contains(&line),
                !command.is_hide_set(),
                "{line:?} in the spec"
            );
        }
    }

    #[test]
    fn a_command_is_required_and_must_exist() {
        assert_eq!(
            rejected(&[]),
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
        assert_eq!(rejected(&["record"]), ErrorKind::InvalidSubcommand);
        assert_eq!(rejected(&["--version"]), ErrorKind::DisplayVersion);
    }

    #[test]
    fn setup_takes_harnesses_split_on_commas() {
        let Run::SetUp { harnesses } = run(&["setup"]) else {
            panic!("expected setup")
        };
        assert!(harnesses.is_empty());
        let Run::SetUp { harnesses } = run(&["setup", "--harnesses", "agents,claude-code"]) else {
            panic!("expected setup")
        };
        assert_eq!(harnesses, strings(&["agents", "claude-code"]));
    }

    #[test]
    fn setup_publishes_the_skill_where_it_is_installed_once() {
        let home = tempfile::tempdir().unwrap();
        assert!(skill_targets(home.path(), Vec::new()).is_empty());
        skill::install(home.path(), "claude-code").unwrap();
        assert_eq!(
            skill_targets(home.path(), strings(&["agents"])),
            strings(&["agents", "claude-code"])
        );
        assert_eq!(
            skill_targets(home.path(), strings(&["claude-code"])),
            strings(&["claude-code"])
        );
    }

    #[test]
    fn start_opens_the_window_unless_headless() {
        assert!(matches!(run(&["start"]), Run::Start { show_window: true }));
        assert!(matches!(
            run(&["start", "--headless"]),
            Run::Start { show_window: false }
        ));
    }

    #[test]
    fn stop_and_restart_take_no_arguments() {
        assert!(matches!(run(&["stop"]), Run::Stop));
        assert!(matches!(run(&["restart"]), Run::Restart));
        assert_eq!(rejected(&["stop", "now"]), ErrorKind::UnknownArgument);
    }

    #[test]
    fn status_checks_only_what_its_hidden_flags_ask() {
        assert!(matches!(
            run(&["status"]),
            Run::Status {
                expect_current_exe: false,
                window_open: false
            }
        ));
        assert!(matches!(
            run(&["status", "--expect-current-exe", "--window-open"]),
            Run::Status {
                expect_current_exe: true,
                window_open: true
            }
        ));
    }

    #[test]
    fn the_daemon_captures_shows_and_saves_by_default() {
        let Run::Daemon {
            replay,
            headless,
            save_sessions,
        } = run(&["daemon"])
        else {
            panic!("expected daemon")
        };
        assert_eq!((replay, headless, save_sessions), (None, false, true));
    }

    #[test]
    fn foreground_daemon_takes_replay_flags() {
        let Run::Daemon {
            replay,
            headless,
            save_sessions,
        } = run(&["daemon", "--replay", "a.wav", "--headless", "--no-save"])
        else {
            panic!("expected daemon")
        };
        assert_eq!(
            (replay, headless, save_sessions),
            (Some(PathBuf::from("a.wav")), true, false)
        );
    }

    #[test]
    fn diarize_needs_its_model() {
        assert!(matches!(
            run(&["diarize", "embed.onnx"]),
            Run::Diarize { model } if model == Path::new("embed.onnx")
        ));
        assert_eq!(rejected(&["diarize"]), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn usage_prints_the_spec() {
        assert!(matches!(run(&["usage"]), Run::Usage));
    }

    #[test]
    fn bench_needs_a_target() {
        assert!(matches!(run(&["bench", "llm"]), Run::BenchLlm));
        assert_eq!(
            rejected(&["bench"]),
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn bench_diarization_defaults() {
        let Run::BenchDiarization(options) = run(&["bench", "diarization", "ami"]) else {
            panic!("expected bench diarization")
        };
        assert_eq!(options.dir, PathBuf::from("ami"));
        assert_eq!(
            (options.list, options.model, options.output),
            (None, None, None)
        );
        assert_eq!(options.thresholds, [0.65]);
        assert_eq!((options.min_share, options.collar), (0.02, 0.0));
    }

    #[test]
    fn bench_diarization_takes_its_options_and_thresholds_split_on_commas() {
        let Run::BenchDiarization(options) = run(&[
            "bench",
            "diarization",
            "ami",
            "--list",
            "dev.txt",
            "--model",
            "embed.onnx",
            "--threshold",
            "0.5,0.7",
            "--min-share",
            "0.1",
            "--collar",
            "0.25",
            "--output",
            "out",
        ]) else {
            panic!("expected bench diarization")
        };
        assert_eq!(options.list, Some(PathBuf::from("dev.txt")));
        assert_eq!(options.model, Some(PathBuf::from("embed.onnx")));
        assert_eq!(options.output, Some(PathBuf::from("out")));
        assert_eq!(options.thresholds, [0.5, 0.7]);
        assert_eq!((options.min_share, options.collar), (0.1, 0.25));
        assert_eq!(
            rejected(&["bench", "diarization", "ami", "--threshold", "high"]),
            ErrorKind::ValueValidation
        );
    }

    #[test]
    fn bench_people_runs_four_series_by_default() {
        let Run::BenchPeople { dir, series } = run(&["bench", "people", "ami"]) else {
            panic!("expected bench people")
        };
        assert_eq!(dir, PathBuf::from("ami"));
        assert_eq!(series, strings(&["ES2004", "IS1009", "TS3003", "EN2002"]));
        let Run::BenchPeople { series, .. } =
            run(&["bench", "people", "ami", "--series", "ES2004,TS3003"])
        else {
            panic!("expected bench people")
        };
        assert_eq!(series, strings(&["ES2004", "TS3003"]));
    }

    #[test]
    fn sessions_filter_nothing_by_default() {
        assert!(matches!(
            request(&["sessions"]),
            cli::Request::Sessions {
                kind: None,
                person: None,
                tag: None,
                search: None
            }
        ));
    }

    #[test]
    fn sessions_take_filters_and_a_tag_with_its_spaces_collapsed() {
        let cli::Request::Sessions {
            kind,
            person,
            tag,
            search,
        } = request(&[
            "sessions",
            "--kind",
            "meeting",
            "--person",
            "p1",
            "--tag",
            " Big   Deal ",
            "--search",
            "budget",
        ])
        else {
            panic!("expected sessions")
        };
        assert_eq!(
            (kind, person, tag, search),
            (
                Some("meeting".into()),
                Some("p1".into()),
                Some("Big Deal".into()),
                Some("budget".into())
            )
        );
    }

    #[test]
    fn a_blank_tag_matches_no_session() {
        assert!(matches!(
            request(&["sessions", "--tag", "  "]),
            cli::Request::Sessions { tag: Some(tag), .. } if tag.is_empty()
        ));
    }

    #[test]
    fn show_delete_and_export_name_one_session() {
        assert!(matches!(request(&["show", "n1"]), cli::Request::Show { id } if id == "n1"));
        assert!(matches!(request(&["delete", "n1"]), cli::Request::Delete { id } if id == "n1"));
        assert!(matches!(request(&["export", "n1"]), cli::Request::Export { id } if id == "n1"));
        assert_eq!(rejected(&["show"]), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn rename_changes_only_what_it_is_given() {
        assert!(matches!(
            request(&["rename", "n1", "--title", "Standup"]),
            cli::Request::Rename { id, title: Some(title), kind: None }
                if id == "n1" && title == "Standup"
        ));
        assert!(matches!(
            request(&["rename", "n1", "--kind", "idea"]),
            cli::Request::Rename { title: None, kind: Some(kind), .. } if kind == "idea"
        ));
    }

    #[test]
    fn ask_takes_the_rest_of_the_line_as_the_question() {
        assert!(matches!(
            request(&["ask", "n1", "o", "que", "decidimos?"]),
            cli::Request::Ask { id, question } if id == "n1" && question == "o que decidimos?"
        ));
        assert_eq!(rejected(&["ask", "n1"]), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn note_takes_the_rest_of_the_line_as_the_text() {
        assert!(matches!(
            request(&["note", "n1", "budget", "is", "10k"]),
            cli::Request::Note { id, text } if id == "n1" && text == "budget is 10k"
        ));
        assert_eq!(
            rejected(&["note", "n1"]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn action_and_send_name_the_session_and_what_to_run() {
        assert!(matches!(
            request(&["action", "n1", "summary"]),
            cli::Request::Action { id, name } if id == "n1" && name == "summary"
        ));
        assert!(matches!(
            request(&["send", "n1", "a3"]),
            cli::Request::Send { id, answer } if id == "n1" && answer == "a3"
        ));
        assert_eq!(
            rejected(&["send", "n1"]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn translate_lowercases_the_language_and_off_stops() {
        assert!(matches!(
            request(&["translate", "n1", "--lang", " PT "]),
            cli::Request::Translate { id, language } if id == "n1" && language == "pt"
        ));
        assert!(matches!(
            request(&["translate", "n1", "--off"]),
            cli::Request::Translate { language, .. } if language.is_empty()
        ));
        assert_eq!(
            rejected(&["translate", "n1"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            rejected(&["translate", "n1", "--lang", "pt", "--off"]),
            ErrorKind::ArgumentConflict
        );
    }

    #[test]
    fn speaker_is_a_name_a_person_or_nobody() {
        assert!(matches!(
            request(&["speaker", "n1", "S1", "--name", "Ana"]),
            cli::Request::Speaker { id, label, who: cli::Who::Name(name) }
                if id == "n1" && label == "S1" && name == "Ana"
        ));
        assert!(matches!(
            request(&["speaker", "n1", "S1", "--person", "p1"]),
            cli::Request::Speaker { who: cli::Who::Person(person), .. } if person == "p1"
        ));
        assert!(matches!(
            request(&["speaker", "n1", "S1", "--clear"]),
            cli::Request::Speaker {
                who: cli::Who::Nobody,
                ..
            }
        ));
        assert_eq!(
            rejected(&["speaker", "n1", "S1"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            rejected(&["speaker", "n1", "S1", "--name", "Ana", "--clear"]),
            ErrorKind::ArgumentConflict
        );
    }

    #[test]
    fn assign_line_names_the_line_by_speaker_and_time() {
        assert!(matches!(
            request(&["assign", "n1", "line", "S1", "12.5", "--person", "p1"]),
            cli::Request::AssignLine { id, who, at, person: Some(person), name: None }
                if id == "n1" && who == "S1" && at == 12.5 && person == "p1"
        ));
        assert_eq!(
            rejected(&["assign", "n1", "line", "S1", "soon", "--person", "p1"]),
            ErrorKind::ValueValidation
        );
        assert_eq!(
            rejected(&["assign", "n1", "line", "S1", "12.5"]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn assign_all_takes_one_person() {
        assert!(matches!(
            request(&["assign", "n1", "all", "--name", "Ana"]),
            cli::Request::AssignAll { id, person: None, name: Some(name) }
                if id == "n1" && name == "Ana"
        ));
        assert_eq!(
            rejected(&["assign", "n1", "all", "--name", "Ana", "--person", "p1"]),
            ErrorKind::ArgumentConflict
        );
    }

    #[test]
    fn participant_links_by_name_or_person_and_removes_by_person() {
        assert!(matches!(
            request(&["participant", "n1", "--name", "Ana"]),
            cli::Request::Participant { id, person: None, name: Some(name), remove: false }
                if id == "n1" && name == "Ana"
        ));
        assert!(matches!(
            request(&["participant", "n1", "--person", "p1"]),
            cli::Request::Participant { person: Some(person), name: None, remove: false, .. }
                if person == "p1"
        ));
        assert!(matches!(
            request(&["participant", "n1", "--remove", "p1"]),
            cli::Request::Participant { person: Some(person), name: None, remove: true, .. }
                if person == "p1"
        ));
        assert_eq!(
            rejected(&["participant", "n1"]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn context_flags_repeat() {
        assert!(matches!(
            request(&["context", "n1"]),
            cli::Request::Context { id, add, remove } if id == "n1" && add.is_empty() && remove.is_empty()
        ));
        let cli::Request::Context { add, remove, .. } = request(&[
            "context", "n1", "--add", "resume", "--add", "notes", "--remove", "web",
        ]) else {
            panic!("expected context")
        };
        assert_eq!(
            (add, remove),
            (strings(&["resume", "notes"]), strings(&["web"]))
        );
    }

    #[test]
    fn line_is_removed_or_given_new_text() {
        assert!(matches!(
            request(&["line", "n1", "me", "3.5", "--remove"]),
            cli::Request::Line { id, who, at, change: cli::LineChange::Remove }
                if id == "n1" && who == "me" && at == 3.5
        ));
        assert!(matches!(
            request(&["line", "n1", "me", "3.5", "--text", "fixed"]),
            cli::Request::Line { change: cli::LineChange::Edit(text), .. } if text == "fixed"
        ));
        assert_eq!(
            rejected(&["line", "n1", "me", "3.5"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            rejected(&["line", "n1", "me", "3.5", "--remove", "--text", "fixed"]),
            ErrorKind::ArgumentConflict
        );
    }

    #[test]
    fn tag_needs_an_action() {
        assert!(matches!(
            request(&["tag", "list"]),
            cli::Request::Tag(cli::Tag::List)
        ));
        assert!(matches!(
            request(&["tag", "add", "n1", "work"]),
            cli::Request::Tag(cli::Tag::Add { id, tag }) if id == "n1" && tag == "work"
        ));
        assert!(matches!(
            request(&["tag", "remove", "n1", "work"]),
            cli::Request::Tag(cli::Tag::Remove { id, tag }) if id == "n1" && tag == "work"
        ));
        assert!(matches!(
            request(&["tag", "rename", "work", "job"]),
            cli::Request::Tag(cli::Tag::Rename { from, to }) if from == "work" && to == "job"
        ));
        assert!(matches!(
            request(&["tag", "delete", "work"]),
            cli::Request::Tag(cli::Tag::Delete { tag }) if tag == "work"
        ));
        assert_eq!(
            rejected(&["tag"]),
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn people_without_an_action_lists_them() {
        assert!(matches!(
            request(&["people"]),
            cli::Request::People(cli::People::List)
        ));
        assert!(matches!(
            request(&["people", "adopt"]),
            cli::Request::People(cli::People::Adopt)
        ));
        assert!(matches!(
            request(&["people", "add", "Ana"]),
            cli::Request::People(cli::People::Add { name }) if name == "Ana"
        ));
        assert!(matches!(
            request(&["people", "rename", "p1", "Ana"]),
            cli::Request::People(cli::People::Rename { id, name }) if id == "p1" && name == "Ana"
        ));
        assert!(matches!(
            request(&["people", "merge", "p1", "p2"]),
            cli::Request::People(cli::People::Merge { into, from }) if into == "p1" && from == "p2"
        ));
        assert!(matches!(
            request(&["people", "forget", "p1"]),
            cli::Request::People(cli::People::Forget { id }) if id == "p1"
        ));
    }

    #[test]
    fn window_commands_name_their_window_call() {
        for (command, name) in [
            ("focus", "focus"),
            ("config", "config"),
            ("new", "new_session"),
            ("sessions", "sessions"),
            ("import", "import"),
        ] {
            assert!(
                matches!(
                    request(&["window", command]),
                    cli::Request::Window(cli::Window::Call { call, path: None }) if call == name
                ),
                "eco window {command}"
            );
        }
        assert!(matches!(
            request(&["window", "import", "talk.mp4"]),
            cli::Request::Window(cli::Window::Call { call: "import", path: Some(path) })
                if path == Path::new("talk.mp4")
        ));
        assert!(matches!(
            request(&["window", "action", "summary"]),
            cli::Request::Window(cli::Window::Action { name }) if name == "summary"
        ));
        assert!(matches!(
            request(&["window", "toggle"]),
            cli::Request::Window(cli::Window::Toggle)
        ));
    }

    #[test]
    fn import_waits_and_guesses_everything_by_default() {
        assert!(matches!(
            request(&["import", "talk.mp4"]),
            cli::Request::Import {
                path,
                title: None,
                kind: None,
                language: None,
                participant: None,
                started_at: None,
                wait: true,
            } if path == Path::new("talk.mp4")
        ));
    }

    #[test]
    fn import_takes_its_options_and_a_local_date() {
        let cli::Request::Import {
            title,
            kind,
            language,
            participant,
            started_at,
            wait,
            ..
        } = request(&[
            "import",
            "talk.mp4",
            "--title",
            "Talk",
            "--kind",
            "meeting",
            "--language",
            "pt",
            "--participant",
            "p1",
            "--date",
            "2026-09-30 14:30",
            "--no-wait",
        ])
        else {
            panic!("expected import")
        };
        assert_eq!(
            (title, kind, language, participant),
            (
                Some("Talk".into()),
                Some("meeting".into()),
                Some("pt".into()),
                Some("p1".into())
            )
        );
        assert_eq!(started_at, cli::local_date("2026-09-30T14:30").ok());
        assert!(!wait);
        assert_eq!(
            rejected(&["import", "talk.mp4", "--date", "30/09/2026"]),
            ErrorKind::ValueValidation
        );
    }
}
