//! eco — voice sessions with an AI beside them, for Omarchy. The daemon: capture, VAD,
//! transcription, sessions and the LLM, served to the QML overlay over a Unix socket.

mod adapters;
mod bench;
mod cli;
mod config;
mod domain;
mod import;
mod lifecycle;
mod ports;
mod session;
mod setup;
mod skill;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

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

#[tokio::main]
async fn main() -> Result<()> {
    let ran = match Cli::parse().command {
        Command::Setup { harnesses } => set_up(harnesses).await,
        Command::Start { headless } => lifecycle::start(!headless).await,
        Command::Stop => lifecycle::stop().await,
        Command::Restart => lifecycle::restart().await,
        Command::Status {
            expect_current_exe,
            window_open,
        } => lifecycle::status(expect_current_exe, window_open).await,
        Command::Daemon {
            replay,
            headless,
            no_save,
        } => session::run(&config::config_file(), replay, headless, !no_save).await,
        Command::Diarize { model } => adapters::diarizer_process::serve(&model),
        Command::Bench { target } => match target {
            Bench::Llm => bench::llm::run().await,
            Bench::Diarization {
                dir,
                list,
                model,
                threshold,
                min_share,
                collar,
                output,
            } => {
                let options = bench::diarization::Options {
                    dir,
                    list,
                    model,
                    thresholds: threshold,
                    min_share,
                    collar,
                    output,
                };
                bench::diarization::run(options).await
            }
            Bench::People { dir, series } => bench::people::run(dir, series).await,
        },
        Command::Sessions {
            kind,
            person,
            tag,
            search,
        } => {
            sessions(cli::Request::Sessions {
                kind,
                person,
                // A tag given blank matches no session.
                tag: tag.map(|tag| domain::session::tag_name(&tag).unwrap_or_default()),
                search,
            })
            .await
        }
        Command::Show { id } => sessions(cli::Request::Show { id }).await,
        Command::Delete { id } => sessions(cli::Request::Delete { id }).await,
        Command::Rename { id, title, kind } => {
            sessions(cli::Request::Rename { id, title, kind }).await
        }
        Command::Ask { id, question } => {
            let question = question.join(" ");
            sessions(cli::Request::Ask { id, question }).await
        }
        Command::Action { id, name } => sessions(cli::Request::Action { id, name }).await,
        Command::Send { id, answer } => sessions(cli::Request::Send { id, answer }).await,
        Command::Translate { id, lang, off: _ } => {
            let language = lang.unwrap_or_default().trim().to_lowercase();
            sessions(cli::Request::Translate { id, language }).await
        }
        Command::Note { id, text } => {
            let text = text.join(" ");
            sessions(cli::Request::Note { id, text }).await
        }
        Command::Export { id } => sessions(cli::Request::Export { id }).await,
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
            sessions(cli::Request::Speaker { id, label, who }).await
        }
        Command::Assign { id, target } => {
            let request = match target {
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
            };
            sessions(request).await
        }
        Command::Context { id, add, remove } => {
            sessions(cli::Request::Context { id, add, remove }).await
        }
        Command::Participant {
            id,
            name,
            person,
            remove,
        } => {
            let removing = remove.is_some();
            sessions(cli::Request::Participant {
                id,
                person: remove.or(person),
                name,
                remove: removing,
            })
            .await
        }
        Command::Line {
            id,
            who,
            at,
            remove: _,
            text,
        } => {
            let change = text.map_or(cli::LineChange::Remove, cli::LineChange::Edit);
            sessions(cli::Request::Line {
                id,
                who,
                at,
                change,
            })
            .await
        }
        Command::People { action } => {
            let people = match action {
                None => cli::People::List,
                Some(PeopleCommand::Adopt) => cli::People::Adopt,
                Some(PeopleCommand::Add { name }) => cli::People::Add { name },
                Some(PeopleCommand::Rename { id, name }) => cli::People::Rename { id, name },
                Some(PeopleCommand::Merge { into, from }) => cli::People::Merge { into, from },
                Some(PeopleCommand::Forget { id }) => cli::People::Forget { id },
            };
            sessions(cli::Request::People(people)).await
        }
        Command::Tag { action } => {
            let tag = match action {
                TagCommand::List => cli::Tag::List,
                TagCommand::Add { id, tag } => cli::Tag::Add { id, tag },
                TagCommand::Remove { id, tag } => cli::Tag::Remove { id, tag },
                TagCommand::Rename { from, to } => cli::Tag::Rename { from, to },
                TagCommand::Delete { tag } => cli::Tag::Delete { tag },
            };
            sessions(cli::Request::Tag(tag)).await
        }
        Command::Import {
            path,
            title,
            kind,
            language,
            participant,
            date,
            no_wait,
        } => {
            let request = cli::Request::Import {
                path,
                title,
                kind,
                language,
                participant,
                started_at: date,
                wait: !no_wait,
            };
            sessions(request).await
        }
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

/// Download the models, load the Hyprland rules, then publish the skill to
/// `harnesses` and refresh it wherever it is already installed. A Hyprland
/// config it cannot write is a warning and exit status 1 after the rest ran.
async fn set_up(harnesses: Vec<String>) -> Result<()> {
    for done in setup::run().await? {
        println!("eco: {done}");
    }
    let rules = config::shipped("hypr/eco.lua");
    let hyprland = setup::hyprland(&config::hypr_bindings(), rules.as_deref());
    match &hyprland {
        Ok(done) => println!("eco: {done}"),
        Err(warning) => eprintln!("eco: warning: {warning}"),
    }
    let home = config::home();
    let mut targets = harnesses;
    for harness in skill::installed(&home) {
        if !targets.iter().any(|target| target == harness) {
            targets.push(harness.into());
        }
    }
    for harness in targets {
        println!("eco: {}", skill::install(&home, &harness)?);
    }
    // The rest of setup ran; the exit status still says the rules are not loaded.
    if hyprland.is_err() {
        std::process::exit(1);
    }
    Ok(())
}

/// Run a session command against the daemon; it prints its JSON and sets the exit code.
async fn sessions(request: cli::Request) -> Result<()> {
    let code = cli::run(&config::socket_path(), request).await;
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn the_cli_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn ask_takes_the_rest_of_the_line_as_the_question() {
        let cli =
            Cli::try_parse_from(["eco", "ask", "n1", "o", "que", "decidimos?"]).expect("parses");
        let Command::Ask { id, question } = cli.command else {
            panic!("expected ask")
        };
        assert_eq!(
            (id.as_str(), question.join(" ").as_str()),
            ("n1", "o que decidimos?")
        );
    }

    #[test]
    fn foreground_daemon_takes_replay_flags() {
        let cli = Cli::try_parse_from([
            "eco",
            "daemon",
            "--replay",
            "a.wav",
            "--headless",
            "--no-save",
        ])
        .expect("parses");
        match cli.command {
            Command::Daemon {
                replay,
                headless,
                no_save,
            } => {
                assert_eq!(replay, Some(PathBuf::from("a.wav")));
                assert!(headless && no_save);
            }
            _ => panic!("expected daemon"),
        }
    }
}
