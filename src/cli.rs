//! The session commands of the `eco` CLI: they talk to the running daemon over its
//! socket and print one JSON object, `{"ok": true, "data": …}` or
//! `{"ok": false, "code": …, "message": …}`, exiting non-zero on failure.
//! `export` prints the document itself, to pipe or redirect.

use std::path::PathBuf;

use chrono::{Local, NaiveDateTime, TimeZone};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};

use crate::adapters::control_socket::{self, Address};
use crate::adapters::local_socket::{self, ConnectError, ReadHalf, Stream, WriteHalf};
use crate::domain::session::same_tag;

/// The timeline entries `eco session show` asks for at a time.
const READ_PAGE: usize = 1000;

/// What the CLI asks the daemon.
pub enum Request {
    Sessions {
        kind: Option<String>,
        person: Option<String>,
        /// Only sessions with this tag, whatever its case.
        tag: Option<String>,
        /// Only sessions whose title, tags, lines, notes or answers hold this text.
        search: Option<String>,
    },
    Show {
        id: String,
    },
    Delete {
        id: String,
    },
    Rename {
        id: String,
        title: Option<String>,
        kind: Option<String>,
    },
    Ask {
        id: String,
        question: String,
    },
    Action {
        id: String,
        name: String,
    },
    /// Run the hook of the action that gave an answer of a session.
    Send {
        id: String,
        answer: String,
    },
    /// Translate a session's lines and answers into `language` from now on, or stop for "".
    Translate {
        id: String,
        language: String,
    },
    /// Keep a note in a session: context later answers use.
    Note {
        id: String,
        text: String,
    },
    /// The session's transcript as WebVTT.
    Export {
        id: String,
    },
    Import {
        path: PathBuf,
        title: Option<String>,
        kind: Option<String>,
        language: Option<String>,
        participant: Option<String>,
        /// When the recording began, in seconds since the epoch.
        started_at: Option<f64>,
        wait: bool,
    },
    /// Say who a speaker of a session is.
    Speaker {
        id: String,
        label: String,
        who: Who,
    },
    AssignLine {
        id: String,
        who: String,
        at: f64,
        person: Option<String>,
        name: Option<String>,
    },
    AssignAll {
        id: String,
        person: Option<String>,
        name: Option<String>,
    },
    Participant {
        id: String,
        person: Option<String>,
        name: Option<String>,
        remove: bool,
    },
    /// The context slots a session has on; with changes, after them.
    Context {
        id: String,
        add: Vec<String>,
        remove: Vec<String>,
    },
    People(People),
    Tag(Tag),
    Window(Window),
    /// The names a model's key can come from: variables, credentials, omapass accounts.
    Keys,
    /// Change one line of a session, named by who said it and when (`at`).
    Line {
        id: String,
        who: String,
        at: f64,
        change: LineChange,
    },
}

/// What happens to a line.
pub enum LineChange {
    Remove,
    /// Its text becomes this.
    Edit(String),
    /// It and the line this speaker said at this time become one.
    Merge(String, f64),
    /// It splits before this character.
    Split(usize),
}

/// Who a speaker is.
pub enum Who {
    /// Called this: the known person of that name, or a new one for a live session.
    Name(String),
    /// A known person, by id.
    Person(String),
    /// No one: back to the label, or the guess cleared.
    Nobody,
}

/// The people linked to sessions.
pub enum People {
    List,
    Adopt,
    Add { name: String },
    Rename { id: String, name: String },
    Merge { into: String, from: String },
    Forget { id: String },
}

/// What a shortcut asks: of the newest eco window, or of the session shown.
pub enum Window {
    /// A `window.call`: "focus", "config", "new_session", "sessions", or
    /// "import" with the file's path.
    Call {
        call: &'static str,
        path: Option<PathBuf>,
    },
    /// Run a configured action on the session shown.
    Action { name: String },
    /// Pause the session shown if it records, else resume it.
    Toggle,
}

impl Window {
    /// The line the daemon reads for it, from `eco window` or a global shortcut.
    pub fn line(self) -> String {
        match self {
            Window::Call { call, path } => {
                let mut fields = json!({"call": call});
                if let Some(path) = path {
                    // The daemon resolves paths from its own directory, not ours.
                    fields["path"] = json!(std::path::absolute(&path).unwrap_or(path));
                }
                format!("window.call {fields}")
            }
            Window::Action { name } => format!("action {name}"),
            Window::Toggle => "session.toggle".into(),
        }
    }
}

/// The tags that group sessions.
pub enum Tag {
    List,
    Add { id: String, tag: String },
    Remove { id: String, tag: String },
    Rename { from: String, to: String },
    Delete { tag: String },
}

#[derive(Debug, PartialEq)]
pub struct Failure {
    code: String,
    message: String,
}

impl Failure {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Run `request` against the daemon at `address`, print the result, and
/// return the exit code.
pub async fn run(address: &Address, request: Request) -> i32 {
    let document = matches!(request, Request::Export { .. });
    let answered = match Client::connect(address).await {
        Ok(mut client) => client.handle(request).await,
        Err(failure) => Err(failure),
    };
    match answered {
        Ok(Value::String(text)) if document => {
            print!("{text}");
            0
        }
        answered => print(answered),
    }
}

/// Print `answered` as the one JSON object every command prints, and return
/// the exit code: 0 for `{"ok": true, "data": …}`, 1 for a failure.
pub fn print(answered: Result<Value, Failure>) -> i32 {
    match answered {
        Ok(data) => {
            println!("{}", json!({"ok": true, "data": data}));
            0
        }
        Err(failure) => {
            println!(
                "{}",
                json!({"ok": false, "code": failure.code, "message": failure.message})
            );
            1
        }
    }
}

/// Every event the daemon at `address` greets a client with: who it is
/// (`daemon`), its state, and what it reports, such as `update` and refused
/// shortcuts; `None` when no daemon answers.
pub async fn greeting(address: &Address) -> Result<Option<Vec<Value>>, Failure> {
    match Client::reach(address).await? {
        Some(mut client) => client.greeting().await.map(Some),
        None => Ok(None),
    }
}

/// The names the keys of the daemon at `address` can come from, as `eco key
/// list` prints them; `None` when no daemon answers.
pub async fn key_names(address: &Address) -> Result<Option<Value>, Failure> {
    match Client::reach(address).await? {
        Some(mut client) => client.keys().await.map(Some),
        None => Ok(None),
    }
}

/// Have the daemon at `address` install release `version`, saying each stage
/// it reaches on stderr; when it restarts: "now", or "when_session_ends" while
/// a session records. `None` when no daemon answers, or one older than
/// updates does not know the command, to install without it.
pub async fn update(address: &Address, version: &str) -> Result<Option<&'static str>, Failure> {
    match Client::reach(address).await? {
        Some(mut client) => match client.update(version).await {
            Err(failure) if failure.code == "command.unknown" => Ok(None),
            done => done.map(Some),
        },
        None => Ok(None),
    }
}

/// Have the daemon at `address` hand the removal of eco off, with the user's
/// files too when `everything`; `false` when no daemon answers, or one older
/// than removals does not know the command, to remove without it.
pub async fn uninstall(address: &Address, everything: bool) -> Result<bool, Failure> {
    match Client::reach(address).await? {
        Some(mut client) => match client.uninstall(everything).await {
            Err(failure) if failure.code == "command.unknown" => Ok(false),
            done => done.map(|()| true),
        },
        None => Ok(false),
    }
}

/// The error event among `codes` (prefixes), as a failure of this request.
fn failure_in(event: &Value, codes: &[&str]) -> Option<Failure> {
    let code = event.get("code").and_then(Value::as_str)?;
    if event["type"] != "error" || !codes.iter().any(|prefix| code.starts_with(prefix)) {
        return None;
    }
    let message = event["message"].as_str().unwrap_or(code);
    Some(Failure::new(code, message))
}

fn connect_failure(error: ConnectError) -> Failure {
    let code = match &error {
        ConnectError::AccessDenied => "daemon.access_denied",
        ConnectError::Unavailable(_) => "daemon.unavailable",
    };
    Failure::new(code, error.to_string())
}

/// Which stored sessions a listing keeps: of a kind, linked to a person, with a tag.
#[derive(Default)]
struct Filter<'a> {
    kind: Option<&'a str>,
    person: Option<&'a str>,
    tag: Option<&'a str>,
}

impl Filter<'_> {
    fn holds(&self, session: &Value) -> bool {
        let has = |key: &str, wanted: &dyn Fn(&str) -> bool| {
            session[key]
                .as_array()
                .is_some_and(|items| items.iter().filter_map(Value::as_str).any(wanted))
        };
        self.kind.is_none_or(|kind| session["kind"] == kind)
            && self
                .person
                .is_none_or(|person| has("people", &|id| id == person))
            && self
                .tag
                .is_none_or(|tag| has("tags", &|kept| same_tag(kept, tag)))
    }
}

struct Client {
    lines: Lines<BufReader<ReadHalf>>,
    writer: WriteHalf,
}

impl Client {
    async fn connect(address: &Address) -> Result<Self, Failure> {
        Self::reach(address).await?.ok_or_else(|| {
            Failure::new(
                "daemon.offline",
                "eco is not running; start it with `eco start` (or `eco start --headless`)",
            )
        })
    }

    /// The daemon at `address`, or `None` when none answers.
    async fn reach(address: &Address) -> Result<Option<Self>, Failure> {
        let stream = control_socket::connect(address)
            .await
            .map_err(connect_failure)?;
        Ok(stream.map(Self::over))
    }

    /// The events the daemon greeted this client with: those before the
    /// answer to `sync`, which the daemon reads after greeting it.
    async fn greeting(&mut self) -> Result<Vec<Value>, Failure> {
        self.send(control_socket::SYNC).await?;
        let mut events = Vec::new();
        self.until(|event| {
            if event["type"] == "synced" {
                return Some(Ok(()));
            }
            events.push(event.clone());
            None
        })
        .await?;
        Ok(events)
    }

    /// `update.install` of `version`: each stage said on stderr until the
    /// install is done; when the daemon restarts, as [`update`] says.
    async fn update(&mut self, version: &str) -> Result<&'static str, Failure> {
        self.send(&format!("update.install {}", json!({"version": version})))
            .await?;
        self.until(|event| {
            if event["type"] == "update_progress" {
                let detail = event["detail"].as_str().unwrap_or_default();
                return match event["stage"].as_str().unwrap_or_default() {
                    "failed" => Some(Err(Failure::new("update.failed", detail))),
                    "waiting" => Some(Ok("when_session_ends")),
                    "restarting" | "done" => Some(Ok("now")),
                    stage => {
                        eprintln!("{}", crate::installer::said(stage, version));
                        None
                    }
                };
            }
            failure_in(event, &["update.", "command."]).map(Err)
        })
        .await
    }

    /// `uninstall`: done once the daemon has handed the removal off.
    async fn uninstall(&mut self, everything: bool) -> Result<(), Failure> {
        self.send(&format!("uninstall {}", json!({"everything": everything})))
            .await?;
        self.until(|event| {
            if event["type"] == "uninstall" {
                return match event["stage"].as_str().unwrap_or_default() {
                    "failed" => Some(Err(Failure::new(
                        "uninstall.failed",
                        event["detail"].as_str().unwrap_or_default(),
                    ))),
                    _ => Some(Ok(())),
                };
            }
            failure_in(event, &["uninstall.", "update.", "command."]).map(Err)
        })
        .await
    }

    fn over(stream: Stream) -> Self {
        let (reader, writer) = local_socket::split(stream);
        Self {
            lines: BufReader::new(reader).lines(),
            writer,
        }
    }

    /// Write `command` as one line. A raw argument with a line feed or a
    /// carriage return could end the line early and smuggle a second command,
    /// so such a command is refused and nothing is written.
    async fn send(&mut self, command: &str) -> Result<(), Failure> {
        if command.contains(['\n', '\r']) {
            return Err(Failure::new(
                "argument.invalid",
                "an argument contains a line break",
            ));
        }
        let line = format!("{command}\n");
        self.writer
            .write_all(line.as_bytes())
            .await
            .map_err(|e| Failure::new("daemon.closed", e.to_string()))
    }

    /// Read events — the greeting and everything else the daemon broadcasts
    /// included — until `decide` settles the request.
    async fn until<T>(
        &mut self,
        mut decide: impl FnMut(&Value) -> Option<Result<T, Failure>>,
    ) -> Result<T, Failure> {
        loop {
            let line =
                self.lines.next_line().await.ok().flatten().ok_or_else(|| {
                    Failure::new("daemon.closed", "the daemon closed the connection")
                })?;
            let Ok(event) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if let Some(settled) = decide(&event) {
                return settled;
            }
        }
    }

    async fn handle(&mut self, request: Request) -> Result<Value, Failure> {
        match request {
            Request::Sessions {
                kind,
                person,
                tag,
                search,
            } => {
                let filter = Filter {
                    kind: kind.as_deref(),
                    person: person.as_deref(),
                    tag: tag.as_deref(),
                };
                self.sessions(filter, search.as_deref()).await
            }
            Request::Show { id } => self.read(&id).await,
            Request::Delete { id } => self.delete(&id).await,
            Request::Rename { id, title, kind } => self.rename(&id, title, kind).await,
            Request::Ask { id, question } => {
                let ask = json!({"id": id, "question": question});
                self.answer(&id, &format!("session.ask {ask}")).await
            }
            Request::Action { id, name } => {
                let action = json!({"id": id, "name": name});
                self.answer(&id, &format!("session.action {action}")).await
            }
            Request::Send { id, answer } => self.send_hook(&id, &answer).await,
            Request::Translate { id, language } => self.translate(&id, &language).await,
            Request::Note { id, text } => self.note(&id, &text).await,
            Request::Export { id } => self.export(&id).await,
            Request::Speaker { id, label, who } => self.speaker(&id, &label, who).await,
            Request::AssignLine {
                id,
                who,
                at,
                person,
                name,
            } => {
                self.assign_line(&id, &who, at, person.as_deref(), name.as_deref())
                    .await
            }
            Request::AssignAll { id, person, name } => {
                self.assign_all(&id, person.as_deref(), name.as_deref())
                    .await
            }
            Request::Context { id, add, remove } => self.context(&id, &add, &remove).await,
            Request::Participant {
                id,
                person,
                name,
                remove,
            } => {
                self.participant(&id, person.as_deref(), name.as_deref(), remove)
                    .await
            }
            Request::Line {
                id,
                who,
                at,
                change,
            } => self.line(&id, &who, at, change).await,
            Request::People(people) => {
                let command = match people {
                    People::List => "people".into(),
                    People::Adopt => return self.adopt_live_speakers().await,
                    People::Add { name } => format!("person.add {}", json!({"name": name})),
                    People::Rename { id, name } => {
                        format!("person.rename {}", json!({"id": id, "name": name}))
                    }
                    People::Merge { into, from } => {
                        format!("person.merge {}", json!({"into": into, "from": from}))
                    }
                    People::Forget { id } => format!("person.forget {id}"),
                };
                self.people(&command).await
            }
            Request::Tag(tag) => self.tag(tag).await,
            Request::Window(window) => self.window(window).await,
            Request::Keys => self.keys().await,
            Request::Import {
                path,
                title,
                kind,
                language,
                participant,
                started_at,
                wait,
            } => {
                // The daemon resolves paths from its own directory, not ours.
                let path = std::path::absolute(&path).unwrap_or(path);
                let mut import = json!({"path": path});
                let optional = [
                    ("title", title),
                    ("kind", kind),
                    ("language", language),
                    ("participant", participant),
                ];
                for (key, value) in optional {
                    if let Some(value) = value {
                        import[key] = json!(value);
                    }
                }
                if let Some(started_at) = started_at {
                    import["started_at"] = json!(started_at);
                }
                self.import(&import, wait).await
            }
        }
    }

    /// Stored sessions, newest first; optionally filtered by `filter` and what
    /// they hold.
    async fn sessions(
        &mut self,
        filter: Filter<'_>,
        search: Option<&str>,
    ) -> Result<Value, Failure> {
        let query = search
            .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|query| !query.is_empty());
        let found = match &query {
            Some(query) => {
                self.send(&format!("sessions.search {query}")).await?;
                let ids = self
                    .until(|event| {
                        (event["type"] == "sessions_found" && event["query"] == query.as_str())
                            .then(|| Ok(event["ids"].clone()))
                    })
                    .await?;
                Some(ids.as_array().cloned().unwrap_or_default())
            }
            None => None,
        };
        self.send("sessions").await?;
        let sessions = self
            .until(|event| (event["type"] == "sessions").then(|| Ok(event["sessions"].clone())))
            .await?;
        let sessions = sessions.as_array().cloned().unwrap_or_default();
        Ok(Value::Array(
            sessions
                .into_iter()
                .filter(|session| {
                    found
                        .as_ref()
                        .is_none_or(|ids| ids.contains(&session["id"]))
                        && filter.holds(session)
                })
                .collect(),
        ))
    }

    /// Send what a shortcut asks and return once the daemon has read it: what
    /// follows shows in the window.
    async fn window(&mut self, window: Window) -> Result<Value, Failure> {
        self.send(&window.line()).await?;
        self.send(control_socket::SYNC).await?;
        self.until(|event| (event["type"] == "synced").then_some(Ok(Value::Null)))
            .await
    }

    /// The names the daemon can read a key from, never a key: its variables, the
    /// Credential Manager's `eco:<name>` (`null` off Windows) and omapass's
    /// accounts (`null` when omapass is not there). A source that could not be
    /// listed adds its `credential_error` or `omapass_error`.
    async fn keys(&mut self) -> Result<Value, Failure> {
        self.send("keys").await?;
        let keys = self
            .until(|event| (event["type"] == "keys").then(|| Ok(event.clone())))
            .await?;
        self.send("omapass").await?;
        let omapass = self
            .until(|event| (event["type"] == "omapass").then(|| Ok(event.clone())))
            .await?;
        let accounts: Option<Vec<&Value>> =
            omapass["installed"].as_bool().unwrap_or(false).then(|| {
                omapass["accounts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|a| &a["account"])
                    .collect()
            });
        let mut listed = json!({
            "env": keys["env"], "credentials": keys["credentials"], "omapass": accounts,
        });
        let errors = [
            ("credential_error", &keys["credential_error"]),
            ("omapass_error", &omapass["error"]),
        ];
        for (field, error) in errors {
            if let Some(error) = error.as_str().filter(|e| !e.is_empty()) {
                listed[field] = json!(error);
            }
        }
        Ok(listed)
    }

    /// List the tags, tag or untag a session, or rename or delete a tag everywhere.
    async fn tag(&mut self, tag: Tag) -> Result<Value, Failure> {
        let (command, settled) = match tag {
            Tag::List => ("tags".into(), "tags"),
            Tag::Add { id, tag } => (
                format!("session.tag {}", json!({"id": id, "tag": tag})),
                "session_tags",
            ),
            Tag::Remove { id, tag } => (
                format!("session.untag {}", json!({"id": id, "tag": tag})),
                "session_tags",
            ),
            Tag::Rename { from, to } => (
                format!("tag.rename {}", json!({"from": from, "to": to})),
                "tag_renamed",
            ),
            Tag::Delete { tag } => (format!("tag.delete {}", json!({"tag": tag})), "tag_deleted"),
        };
        self.send(&command).await?;
        self.until(|event| {
            if event["type"] == settled {
                let mut answer = event.clone();
                answer.as_object_mut()?.remove("type");
                return Some(Ok(match settled {
                    "tags" => answer["tags"].take(),
                    _ => answer,
                }));
            }
            failure_in(event, &["session.", "tag."]).map(Err)
        })
        .await
    }

    /// A session's summary and its speakers.
    async fn show(&mut self, id: &str) -> Result<Value, Failure> {
        self.send(&format!("session.show {id}")).await?;
        self.until(|event| {
            if event["type"] == "session_detail" && event["session"]["id"] == id {
                let detail = json!({"session": event["session"], "speakers": event["speakers"]});
                return Some(Ok(detail));
            }
            failure_in(event, &["session.not_found"]).map(Err)
        })
        .await
    }

    /// A session's summary, its whole timeline, read in pages from the newest,
    /// and its speakers.
    async fn read(&mut self, id: &str) -> Result<Value, Failure> {
        let detail = self.show(id).await?;
        let mut pages: Vec<Vec<Value>> = Vec::new();
        let mut before = Value::Null;
        loop {
            let page = json!({"id": id, "before": before, "limit": READ_PAGE});
            self.send(&format!("session.timeline {page}")).await?;
            let (entries, more) = self
                .until(|event| {
                    if event["type"] == "session_timeline"
                        && event["session"] == id
                        && event["before"] == before
                        && event["after"].is_null()
                        && event["around"].is_null()
                    {
                        let entries = event["timeline"].as_array().cloned().unwrap_or_default();
                        return Some(Ok((entries, event["more_before"] == true)));
                    }
                    failure_in(event, &["session.not_found", "session.invalid"]).map(Err)
                })
                .await?;
            let oldest = entries.first().map(|entry| entry["at"].clone());
            pages.push(entries);
            match oldest {
                Some(at) if more => before = at,
                _ => break,
            }
        }
        let timeline: Vec<Value> = pages.into_iter().rev().flatten().collect();
        Ok(
            json!({"session": detail["session"], "timeline": timeline, "speakers": detail["speakers"]}),
        )
    }

    async fn delete(&mut self, id: &str) -> Result<Value, Failure> {
        self.send(&format!("session.delete {id}")).await?;
        self.until(|event| {
            if event["type"] == "session_deleted" && event["id"] == id {
                return Some(Ok(json!({"id": id})));
            }
            failure_in(event, &["session."]).map(Err)
        })
        .await
    }

    async fn assign_line(
        &mut self,
        id: &str,
        who: &str,
        at: f64,
        person: Option<&str>,
        name: Option<&str>,
    ) -> Result<Value, Failure> {
        let payload = json!({"id": id, "who": who, "at": at, "person": person.unwrap_or(""), "name": name.unwrap_or("")});
        self.send(&format!("person.assign_line {payload}")).await?;
        self.until(|event| {
            if event["type"] == "transcript_reassigned"
                && event["session"] == id
                && event["at"] == at
            {
                return Some(Ok(
                    json!({"session": id, "label": event["label"], "name": event["name"]}),
                ));
            }
            failure_in(event, &["session.", "line.", "person."]).map(Err)
        })
        .await
    }

    async fn assign_all(
        &mut self,
        id: &str,
        person: Option<&str>,
        name: Option<&str>,
    ) -> Result<Value, Failure> {
        let payload =
            json!({"session": id, "person": person.unwrap_or(""), "name": name.unwrap_or("")});
        self.send(&format!("person.assign_all {payload}")).await?;
        self.until(|event| {
            if event["type"] == "person_assigned_all" && event["session"] == id {
                return Some(Ok(json!({"session": id, "speakers": event["speakers"]})));
            }
            failure_in(event, &["session.", "person."]).map(Err)
        })
        .await
    }

    /// Say who the speaker `label` of session `id` is; returns what they go by now.
    async fn speaker(&mut self, id: &str, label: &str, who: Who) -> Result<Value, Failure> {
        let detail = self.show(id).await?;
        let speakers = detail["speakers"].as_array().cloned().unwrap_or_default();
        let Some(speaker) = speakers.into_iter().find(|s| s["label"] == label) else {
            let message = format!("session {id} has no speaker {label:?}");
            return Err(Failure::new("session.invalid", message));
        };
        if matches!(who, Who::Nobody) && speaker["person"].is_null() && speaker["guess"].is_object()
        {
            let payload = json!({"session": id, "label": label});
            self.send(&format!("person.guess.clear {payload}")).await?;
            return self
                .until(|event| {
                    if event["type"] == "session_speakers" && event["session"] == id {
                        return Some(Ok(
                            json!({"session": id, "label": label, "name": speaker["name"]}),
                        ));
                    }
                    failure_in(event, &["session.", "speaker.", "people."]).map(Err)
                })
                .await;
        }
        let rename = |name: &str| json!({"id": id, "label": label, "name": name});
        let assign = |person: &str, name: &str| json!({"session": id, "label": label, "person": person, "name": name});
        let command = match who {
            Who::Name(name) => format!("person.assign {}", assign("", &name)),
            Who::Person(person) => format!("person.assign {}", assign(&person, "")),
            Who::Nobody if speaker["person"].is_string() => {
                format!("person.unassign {}", json!({"session": id, "label": label}))
            }
            Who::Nobody => format!("session.speaker {}", rename("")),
        };
        self.send(&command).await?;
        self.until(|event| {
            if event["type"] == "speaker_renamed"
                && event["session"] == id
                && event["label"] == label
            {
                return Some(Ok(
                    json!({"session": id, "label": label, "name": event["name"]}),
                ));
            }
            failure_in(event, &["session.", "person.", "people."]).map(Err)
        })
        .await
    }

    /// The context slots session `id` has on — the ones chosen, or those of its
    /// kind — after turning on `add` and off `remove`; and every slot there is.
    async fn context(
        &mut self,
        id: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<Value, Failure> {
        // The greeting is the first line: it names the slots and their kinds.
        let slots = self
            .until(|event| (event["type"] == "snapshot").then(|| Ok(event["contexts"].clone())))
            .await?;
        let slots = slots.as_array().cloned().unwrap_or_default();
        let names: Vec<&str> = slots
            .iter()
            .filter_map(|slot| slot["name"].as_str())
            .collect();
        if let Some(unknown) = add
            .iter()
            .chain(remove)
            .find(|name| !names.contains(&name.as_str()))
        {
            let message = format!("no context slot {unknown:?}; there are {names:?}");
            return Err(Failure::new("context.unknown", message));
        }
        let detail = self.show(id).await?;
        let session = &detail["session"];
        let mut on: Vec<String> = match session["contexts"].as_array() {
            Some(chosen) => chosen
                .iter()
                .filter_map(|n| n.as_str().map(String::from))
                .collect(),
            None => slots
                .iter()
                .filter(|slot| {
                    slot["kinds"]
                        .as_array()
                        .is_some_and(|k| k.contains(&session["kind"]))
                })
                .filter_map(|slot| slot["name"].as_str().map(String::from))
                .collect(),
        };
        if add.is_empty() && remove.is_empty() {
            return Ok(json!({"session": id, "contexts": on, "available": names}));
        }
        on.retain(|name| !remove.contains(name));
        for name in add {
            if !on.contains(name) {
                on.push(name.clone());
            }
        }
        let payload = json!({"id": id, "contexts": on});
        self.send(&format!("session.context {payload}")).await?;
        self.until(|event| {
            if event["type"] == "session_context" && event["session"] == id {
                return Some(Ok(
                    json!({"session": id, "contexts": event["contexts"], "available": names}),
                ));
            }
            failure_in(event, &["session."]).map(Err)
        })
        .await
    }

    async fn participant(
        &mut self,
        id: &str,
        person: Option<&str>,
        name: Option<&str>,
        remove: bool,
    ) -> Result<Value, Failure> {
        let payload = (
            if remove {
                "person.leave"
            } else {
                "person.attend"
            },
            json!({"session": id, "person": person.unwrap_or(""), "name": name.unwrap_or("")}),
        );
        self.send(&format!("{} {}", payload.0, payload.1)).await?;
        self.until(|event| {
            if event["type"] == "attendees_changed" && event["session"] == id {
                return Some(Ok(event["people"].clone()));
            }
            failure_in(event, &["session.", "person.", "people."]).map(Err)
        })
        .await
    }

    /// Remove, correct, merge or split a line; returns what became of it.
    async fn line(
        &mut self,
        id: &str,
        who: &str,
        at: f64,
        change: LineChange,
    ) -> Result<Value, Failure> {
        let mut line = json!({"id": id, "who": who, "at": at});
        let command = match &change {
            LineChange::Remove => "session.line.remove",
            LineChange::Edit(text) => {
                line["text"] = json!(text);
                "session.line.edit"
            }
            LineChange::Merge(other, other_at) => {
                line = json!({"id": id, "into": {"who": who, "at": at},
                              "from": {"who": other, "at": other_at}});
                "session.line.merge"
            }
            LineChange::Split(offset) => {
                line["offset"] = json!(offset);
                "session.line.split"
            }
        };
        self.send(&format!("{command} {line}")).await?;
        // A merge or a split names its lines as `{"who", "at"}`.
        let named = |line: &Value| line["who"] == who && line["at"].as_f64() == Some(at);
        self.until(|event| {
            let session = event["session"] == id;
            let ours = session && named(event);
            match event["type"].as_str() {
                Some("transcript_removed") if ours => Some(Ok(
                    json!({"session": id, "who": who, "at": at, "removed": true}),
                )),
                Some("transcript_edited") if ours => Some(Ok(
                    json!({"session": id, "who": who, "at": at, "text": event["text"]}),
                )),
                Some("line_merged") if session && (named(&event["into"]) || named(&event["from"])) => {
                    Some(Ok(json!({"session": id, "into": event["into"],
                                   "from": event["from"], "text": event["text"]})))
                }
                Some("line_split") if session && named(&event["of"]) => Some(Ok(
                    json!({"session": id, "of": event["of"], "text": event["text"], "new": event["new"]}),
                )),
                _ => failure_in(event, &["session.", "line."]).map(Err),
            }
        })
        .await
    }

    /// Send a people command and return everyone known after it.
    async fn people(&mut self, command: &str) -> Result<Value, Failure> {
        self.send(command).await?;
        self.until(|event| {
            if event["type"] == "people" {
                return Some(Ok(event["people"].clone()));
            }
            failure_in(event, &["person.", "people."]).map(Err)
        })
        .await
    }

    async fn adopt_live_speakers(&mut self) -> Result<Value, Failure> {
        self.send("people.adopt").await?;
        self.until(|event| {
            if event["type"] == "people_adopted" {
                return Some(Ok(json!({"adopted": event["count"]})));
            }
            failure_in(event, &["session.", "person.", "people."]).map(Err)
        })
        .await
    }

    /// A session's transcript as a WebVTT document.
    async fn export(&mut self, id: &str) -> Result<Value, Failure> {
        self.send(&format!("session.export {id}")).await?;
        self.until(|event| {
            if event["type"] == "session_export" && event["id"] == id {
                return Some(Ok(event["text"].clone()));
            }
            failure_in(event, &["session.not_found"]).map(Err)
        })
        .await
    }

    /// Run the hook of answer `answer` of session `id` and wait until it exits.
    async fn send_hook(&mut self, id: &str, answer: &str) -> Result<Value, Failure> {
        let payload = json!({"session": id, "id": answer});
        self.send(&format!("hook.send {payload}")).await?;
        self.until(|event| {
            if event["type"] == "hook_sent" && event["session"] == id && event["id"] == answer {
                return Some(Ok(json!({"session": id, "id": answer, "sent": true})));
            }
            // Another answer's hook may fail meanwhile.
            if event["code"] == "hook.failed" && event["params"]["id"] != answer {
                return None;
            }
            failure_in(event, &["session.", "answer.", "hook."]).map(Err)
        })
        .await
    }

    /// Set a session's translation; returns the language it translates into now, or null.
    async fn translate(&mut self, id: &str, language: &str) -> Result<Value, Failure> {
        let payload = json!({"id": id, "language": language});
        self.send(&format!("session.translation {payload}")).await?;
        self.until(|event| {
            if event["type"] == "session_translation" && event["session"] == id {
                return Some(Ok(
                    json!({"session": id, "translating": event["translating"]}),
                ));
            }
            failure_in(
                event,
                &[
                    "session.",
                    "translation.unknown",
                    "translation.own",
                    "translation.none",
                ],
            )
            .map(Err)
        })
        .await
    }

    async fn note(&mut self, id: &str, text: &str) -> Result<Value, Failure> {
        let note = json!({"id": id, "text": text});
        self.send(&format!("session.note {note}")).await?;
        self.until(|event| {
            if event["type"] == "note" && event["session"] == id {
                let kept = json!({"session": id, "id": event["id"], "text": event["text"], "at": event["at"]});
                return Some(Ok(kept));
            }
            failure_in(event, &["session."]).map(Err)
        })
        .await
    }

    /// Give a session a new title and/or kind; what is not given stays.
    async fn rename(
        &mut self,
        id: &str,
        title: Option<String>,
        kind: Option<String>,
    ) -> Result<Value, Failure> {
        let (title, kind) = match (title, kind) {
            (Some(title), Some(kind)) => (title, kind),
            (title, kind) => {
                let current = self.show(id).await?;
                let field = |key: &str| current["session"][key].as_str().unwrap_or("").to_string();
                (
                    title.unwrap_or_else(|| field("title")),
                    kind.unwrap_or_else(|| field("kind")),
                )
            }
        };
        let rename = json!({"id": id, "title": title, "kind": kind});
        self.send(&format!("session.rename {rename}")).await?;
        self.until(|event| {
            if event["type"] == "session_renamed" && event["id"] == id {
                let renamed = json!({"id": id, "title": event["title"], "kind": event["kind"]});
                return Some(Ok(renamed));
            }
            failure_in(event, &["session."]).map(Err)
        })
        .await
    }

    /// Send a question or an action about session `id` and wait for the whole answer.
    async fn answer(&mut self, id: &str, command: &str) -> Result<Value, Failure> {
        self.send(command).await?;
        let mut answer: Option<Value> = None;
        self.until(|event| {
            let failed = failure_in(
                event,
                &[
                    "session.",
                    "action.unknown",
                    "chat.none",
                    "completion.failed",
                ],
            );
            if let Some(failure) = failed {
                return Some(Err(failure));
            }
            let kind = event["type"].as_str().unwrap_or_default();
            let ours = |answer: &Value| answer["id"] == event["id"];
            match (kind, answer.as_mut()) {
                ("suggestion_start", None) if event["session"] == id => {
                    let started = json!({
                        "session": id, "id": event["id"], "action": event["action"],
                        "prompt": event["prompt"], "model": event["model"], "text": "",
                    });
                    answer = Some(started);
                }
                ("suggestion_delta", Some(answer)) if ours(answer) => {
                    let text = answer["text"].as_str().unwrap_or_default().to_string()
                        + event["text"].as_str().unwrap_or_default();
                    answer["text"] = json!(text);
                }
                ("suggestion_end", Some(answer)) if ours(answer) => {
                    answer["ttft_ms"] = event["ttft_ms"].clone();
                    answer["total_ms"] = event["total_ms"].clone();
                    return Some(Ok(answer.clone()));
                }
                ("suggestion_removed", Some(answer)) if ours(answer) => {
                    let removed = "the answer was removed, or replaced by a newer request";
                    return Some(Err(Failure::new("suggestion.removed", removed)));
                }
                _ => {}
            }
            None
        })
        .await
    }

    /// Start importing a file; with `wait`, until it is transcribed.
    async fn import(&mut self, import: &Value, wait: bool) -> Result<Value, Failure> {
        self.send(&format!("session.import {import}")).await?;
        let started = self
            .until(|event| {
                if event["type"] == "import_started" {
                    return Some(Ok(event.clone()));
                }
                failure_in(event, &["import.", "session.invalid"]).map(Err)
            })
            .await?;
        let session = started["session"].clone();
        if !wait {
            return Ok(json!({"session": session, "total_s": started["total_s"]}));
        }
        let id = session["id"].clone();
        self.until(|event| {
            if let Some(failure) = failure_in(event, &["import."]) {
                return Some(Err(failure));
            }
            (event["type"] == "import_done" && event["id"] == id).then(|| {
                Ok(json!({"session": session, "total_s": started["total_s"], "complete": event["complete"]}))
            })
        })
        .await
    }
}

/// A local date and time in ISO 8601, `YYYY-MM-DDTHH:MM[:SS]` (a space may
/// stand for the `T`), as seconds since the epoch.
pub fn local_date(text: &str) -> Result<f64, String> {
    let text = text.trim().replacen(' ', "T", 1);
    let naive = ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"]
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(&text, format).ok())
        .ok_or_else(|| format!("{text:?} is not YYYY-MM-DDTHH:MM[:SS]"))?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|at| at.timestamp() as f64)
        .ok_or_else(|| format!("{text:?} does not exist here"))
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncReadExt;

    use super::*;

    #[test]
    fn a_date_is_local_iso_8601() {
        let at = Local
            .with_ymd_and_hms(2026, 9, 30, 14, 30, 0)
            .unwrap()
            .timestamp() as f64;
        assert_eq!(local_date("2026-09-30T14:30"), Ok(at));
        assert_eq!(local_date("2026-09-30 14:30:00"), Ok(at));
        assert_eq!(local_date("2026-09-30T14:30:15"), Ok(at + 15.0));
        assert!(local_date("30/09/2026 14:30").is_err());
        assert!(local_date("2026-09-30").is_err());
    }

    /// A client wired to a fake daemon that checks the command it receives and
    /// answers with `events`, one per line.
    fn talking_to(expected: &'static str, events: Vec<Value>) -> Client {
        conversing(vec![(expected, events)])
    }

    /// A client wired to a fake daemon that, turn by turn, checks the command it
    /// receives and answers with that turn's events.
    fn conversing(turns: Vec<(&'static str, Vec<Value>)>) -> Client {
        greeted(json!({"type": "snapshot", "kinds": ["meeting"]}), turns)
    }

    /// `conversing`, with the daemon greeting the client with `greeting`.
    fn greeted(greeting: Value, turns: Vec<(&'static str, Vec<Value>)>) -> Client {
        let (ours, theirs) = local_socket::pair();
        tokio::spawn(daemon(theirs, greeting, turns));
        Client::over(ours)
    }

    /// The fake daemon's end of one connection: greet, then answer each turn.
    async fn daemon(theirs: Stream, greeting: Value, turns: Vec<(&'static str, Vec<Value>)>) {
        let (mut reader, mut writer) = local_socket::split(theirs);
        writer
            .write_all(format!("{greeting}\n").as_bytes())
            .await
            .unwrap();
        for (expected, events) in turns {
            let mut received = vec![0u8; expected.len() + 1];
            reader.read_exact(&mut received).await.unwrap();
            assert_eq!(String::from_utf8_lossy(&received), format!("{expected}\n"));
            for event in events {
                writer
                    .write_all(format!("{event}\n").as_bytes())
                    .await
                    .unwrap();
            }
        }
        // Hold the connection open, as the daemon does.
        let _ = reader.read(&mut [0u8; 1]).await;
    }

    /// A client wired to a fake daemon that checks the command it receives,
    /// writes `lines` as they are, and closes the connection.
    fn closing_after(expected: &'static str, lines: &'static [&'static str]) -> Client {
        let (ours, theirs) = local_socket::pair();
        tokio::spawn(async move {
            let (reader, mut writer) = local_socket::split(theirs);
            let mut commands = BufReader::new(reader).lines();
            assert_eq!(commands.next_line().await.unwrap().unwrap(), expected);
            for line in lines {
                writer
                    .write_all(format!("{line}\n").as_bytes())
                    .await
                    .unwrap();
            }
        });
        Client::over(ours)
    }

    fn refusal(code: &str) -> Value {
        json!({"type": "error", "code": code, "message": format!("{code} happened")})
    }

    fn signal() -> Value {
        json!({"type": "signal", "input": "@default-input", "level": 0.1, "speech": false})
    }

    #[tokio::test]
    async fn sessions_skip_the_greeting_and_filter_by_kind() {
        let sessions = json!({"type": "sessions", "sessions": [{"id": "a", "kind": "idea"}, {"id": "b", "kind": "meeting"}]});
        let mut client = talking_to("sessions", vec![signal(), sessions]);
        let found = client
            .sessions(
                Filter {
                    kind: Some("idea"),
                    ..Filter::default()
                },
                None,
            )
            .await
            .unwrap();
        assert_eq!(found, json!([{"id": "a", "kind": "idea"}]));
    }

    #[tokio::test]
    async fn sessions_filter_by_linked_person() {
        let sessions = json!({"type": "sessions", "sessions": [
            {"id": "a", "kind": "meeting", "people": ["p1", "p2"]},
            {"id": "b", "kind": "meeting", "people": ["p2"]}
        ]});
        let mut client = talking_to("sessions", vec![sessions]);
        let found = client
            .sessions(
                Filter {
                    person: Some("p1"),
                    ..Filter::default()
                },
                None,
            )
            .await
            .unwrap();
        assert_eq!(found.as_array().unwrap().len(), 1);
        assert_eq!(found[0]["id"], "a");
    }

    #[tokio::test]
    async fn sessions_filter_by_tag_whatever_its_case() {
        let sessions = json!({"type": "sessions", "sessions": [
            {"id": "a", "kind": "meeting", "tags": ["Client X", "Q3"]},
            {"id": "b", "kind": "idea", "tags": ["client x"]},
            {"id": "c", "kind": "meeting", "tags": []}
        ]});
        let mut client = talking_to("sessions", vec![sessions]);
        let filter = Filter {
            kind: Some("meeting"),
            tag: Some("CLIENT X"),
            ..Filter::default()
        };
        let found = client.sessions(filter, None).await.unwrap();
        assert_eq!(found.as_array().unwrap().len(), 1);
        assert_eq!(found[0]["id"], "a");
    }

    #[tokio::test]
    async fn tag_commands_settle_on_their_own_event() {
        let tagged = json!({"type": "session_tags", "session": "n1", "tags": ["Q3"]});
        let mut client = talking_to(
            r#"session.tag {"id":"n1","tag":"Q3"}"#,
            vec![signal(), tagged],
        );
        let tag = Tag::Add {
            id: "n1".into(),
            tag: "Q3".into(),
        };
        assert_eq!(
            client.tag(tag).await.unwrap(),
            json!({"session": "n1", "tags": ["Q3"]})
        );

        let listed = json!({"type": "tags", "tags": [{"tag": "Q3", "sessions": 2}]});
        let mut client = talking_to("tags", vec![listed]);
        assert_eq!(
            client.tag(Tag::List).await.unwrap(),
            json!([{"tag": "Q3", "sessions": 2}])
        );

        let gone = json!({"type": "error", "code": "tag.not_found", "message": "no session is tagged \"x\""});
        let mut client = talking_to(r#"tag.delete {"tag":"x"}"#, vec![gone]);
        let failure = client
            .tag(Tag::Delete { tag: "x".into() })
            .await
            .unwrap_err();
        assert_eq!(failure.code, "tag.not_found");
    }

    #[tokio::test]
    async fn key_list_joins_the_key_names_and_the_omapass_accounts() {
        let keys = json!({"type": "keys", "env": ["ECO_DEEPGRAM"], "credentials": null, "credential_error": ""});
        let accounts = json!({"type": "omapass", "installed": true,
                              "accounts": [{"account": "openrouter", "folder": "AI"}]});
        let mut client = conversing(vec![
            ("keys", vec![signal(), keys]),
            ("omapass", vec![accounts]),
        ]);
        assert_eq!(
            client.handle(Request::Keys).await,
            Ok(json!({"env": ["ECO_DEEPGRAM"], "credentials": null, "omapass": ["openrouter"]}))
        );
    }

    #[tokio::test]
    async fn key_list_says_which_source_could_not_be_listed() {
        let keys =
            json!({"type": "keys", "env": [], "credentials": [], "credential_error": "no store"});
        let missing = json!({"type": "omapass", "installed": false, "accounts": []});
        let mut client = conversing(vec![
            ("keys", vec![keys.clone()]),
            ("omapass", vec![missing]),
        ]);
        assert_eq!(
            client.keys().await.unwrap(),
            json!({"env": [], "credentials": [], "omapass": null, "credential_error": "no store"})
        );
        let locked = json!({"type": "omapass", "installed": true, "accounts": [], "error": "omapass: locked"});
        let mut client = conversing(vec![("keys", vec![keys]), ("omapass", vec![locked])]);
        assert_eq!(
            client.keys().await.unwrap(),
            json!({"env": [], "credentials": [], "omapass": [],
                   "credential_error": "no store", "omapass_error": "omapass: locked"})
        );
    }

    #[tokio::test]
    async fn sessions_filter_by_what_they_hold() {
        let sessions = json!({"type": "sessions", "sessions": [
            {"id": "a", "kind": "meeting"}, {"id": "b", "kind": "idea"}, {"id": "c", "kind": "meeting"}
        ]});
        let mut client = conversing(vec![
            (
                "sessions.search prazo do contrato",
                vec![
                    json!({"type": "sessions_found", "query": "other", "ids": ["a"]}),
                    json!({"type": "sessions_found", "query": "prazo do contrato", "ids": ["b", "c"]}),
                ],
            ),
            ("sessions", vec![sessions]),
        ]);
        let found = client
            .sessions(
                Filter {
                    kind: Some("meeting"),
                    ..Filter::default()
                },
                Some("  prazo do\ncontrato "),
            )
            .await
            .unwrap();
        assert_eq!(found, json!([{"id": "c", "kind": "meeting"}]));
    }

    #[tokio::test]
    async fn an_answer_collects_only_its_own_deltas() {
        let events = vec![
            json!({"type": "suggestion_start", "id": "other", "session": "open1", "action": "ask"}),
            json!({"type": "suggestion_delta", "id": "other", "text": "não"}),
            json!({"type": "suggestion_start", "id": "s1", "session": "n1", "action": "chat", "prompt": "E aí?", "model": "m"}),
            json!({"type": "error", "code": "transcription.failed", "message": "x"}),
            json!({"type": "suggestion_delta", "id": "s1", "text": "Decidimos "}),
            signal(),
            json!({"type": "suggestion_delta", "id": "s1", "text": "Redis."}),
            json!({"type": "suggestion_end", "id": "s1", "ttft_ms": 200, "total_ms": 900}),
        ];
        let mut client = talking_to(r#"session.ask {"id":"n1","question":"E aí?"}"#, events);
        let answer = client
            .handle(Request::Ask {
                id: "n1".into(),
                question: "E aí?".into(),
            })
            .await
            .unwrap();
        assert_eq!(answer["text"], "Decidimos Redis.");
        assert_eq!(
            (answer["id"].clone(), answer["prompt"].clone()),
            (json!("s1"), json!("E aí?"))
        );
        assert_eq!(answer["total_ms"], 900);
    }

    #[tokio::test]
    async fn a_note_is_kept_once_its_session_says_so() {
        let events = vec![
            json!({"type": "note", "id": "x", "session": "other", "text": "não"}),
            json!({"type": "note", "id": "k1", "session": "n1", "text": "Migrei para MVVM.", "at": 3.0}),
        ];
        let mut client = talking_to(
            r#"session.note {"id":"n1","text":"Migrei para MVVM."}"#,
            events,
        );
        let kept = client
            .handle(Request::Note {
                id: "n1".into(),
                text: "Migrei para MVVM.".into(),
            })
            .await
            .unwrap();
        assert_eq!(
            kept,
            json!({"session": "n1", "id": "k1", "text": "Migrei para MVVM.", "at": 3.0})
        );
    }

    #[tokio::test]
    async fn translation_settles_on_its_session() {
        let events = vec![
            json!({"type": "session_translation", "session": "other", "translating": "ja"}),
            json!({"type": "session_translation", "session": "n1", "translating": "en"}),
        ];
        let mut client = talking_to(r#"session.translation {"id":"n1","language":"en"}"#, events);
        let request = Request::Translate {
            id: "n1".into(),
            language: "en".into(),
        };
        assert_eq!(
            client.handle(request).await.unwrap(),
            json!({"session": "n1", "translating": "en"})
        );
    }

    #[tokio::test]
    async fn a_hook_is_sent_once_its_own_run_ends() {
        let events = vec![
            json!({"type": "hook_started", "session": "n1", "id": "s1"}),
            json!({"type": "error", "code": "hook.failed", "message": "hook: down", "params": {"session": "n2", "id": "s9"}}),
            json!({"type": "hook_sent", "session": "n1", "id": "s1"}),
        ];
        let mut client = talking_to(r#"hook.send {"session":"n1","id":"s1"}"#, events);
        let request = Request::Send {
            id: "n1".into(),
            answer: "s1".into(),
        };
        assert_eq!(
            client.handle(request).await.unwrap(),
            json!({"session": "n1", "id": "s1", "sent": true})
        );

        let failed = json!({"type": "error", "code": "hook.failed", "message": "hook: down", "params": {"session": "n1", "id": "s1"}});
        let mut client = talking_to(r#"hook.send {"session":"n1","id":"s1"}"#, vec![failed]);
        let request = Request::Send {
            id: "n1".into(),
            answer: "s1".into(),
        };
        assert_eq!(
            client.handle(request).await.unwrap_err(),
            Failure::new("hook.failed", "hook: down")
        );
    }

    #[tokio::test]
    async fn without_a_model_asking_and_translating_fail_at_once() {
        let none = json!({"type": "error", "code": "chat.none", "message": "no chat model"});
        let mut client = talking_to(r#"session.ask {"id":"n1","question":"E aí?"}"#, vec![none]);
        let request = Request::Ask {
            id: "n1".into(),
            question: "E aí?".into(),
        };
        assert_eq!(client.handle(request).await.unwrap_err().code, "chat.none");

        let none = json!({"type": "error", "code": "translation.none", "message": "no translator"});
        let mut client = talking_to(
            r#"session.translation {"id":"n1","language":"en"}"#,
            vec![none],
        );
        let request = Request::Translate {
            id: "n1".into(),
            language: "en".into(),
        };
        assert_eq!(
            client.handle(request).await.unwrap_err().code,
            "translation.none"
        );
    }

    #[tokio::test]
    async fn a_failed_or_replaced_answer_is_a_failure() {
        let failed = json!({"type": "error", "code": "completion.failed", "message": "chat: 429"});
        let mut client = talking_to(r#"session.action {"id":"n1","name":"ask"}"#, vec![failed]);
        let request = Request::Action {
            id: "n1".into(),
            name: "ask".into(),
        };
        assert_eq!(
            client.handle(request).await.unwrap_err(),
            Failure::new("completion.failed", "chat: 429")
        );

        let events = vec![
            json!({"type": "suggestion_start", "id": "s1", "session": "n1"}),
            json!({"type": "suggestion_removed", "id": "s1"}),
        ];
        let mut client = talking_to(r#"session.action {"id":"n1","name":"ask"}"#, events);
        let request = Request::Action {
            id: "n1".into(),
            name: "ask".into(),
        };
        assert_eq!(
            client.handle(request).await.unwrap_err().code,
            "suggestion.removed"
        );
    }

    #[tokio::test]
    async fn an_import_waits_for_its_session() {
        let events = vec![
            json!({"type": "import_started", "session": {"id": "i1", "title": "talk"}, "total_s": 60.0}),
            json!({"type": "import_progress", "id": "i1", "done_s": 30.0, "total_s": 60.0}),
            json!({"type": "import_done", "id": "i1", "complete": true}),
        ];
        let mut client = talking_to(r#"session.import {"path":"/talk.mp4"}"#, events);
        let done = client
            .import(&json!({"path": "/talk.mp4"}), true)
            .await
            .unwrap();
        assert_eq!(
            (done["session"]["id"].clone(), done["complete"].clone()),
            (json!("i1"), json!(true))
        );

        let busy = json!({"type": "error", "code": "import.busy", "message": "an import is already running"});
        let mut client = talking_to(r#"session.import {"path":"/talk.mp4"}"#, vec![busy]);
        let failure = client
            .import(&json!({"path": "/talk.mp4"}), true)
            .await
            .unwrap_err();
        assert_eq!(failure.code, "import.busy");
    }

    #[tokio::test]
    async fn an_export_is_the_document() {
        let other = json!({"type": "session_export", "id": "other", "format": "vtt", "text": "no"});
        let ours =
            json!({"type": "session_export", "id": "n1", "format": "vtt", "text": "WEBVTT\n"});
        let mut client = talking_to("session.export n1", vec![other, ours]);
        let request = Request::Export { id: "n1".into() };
        assert_eq!(client.handle(request).await.unwrap(), json!("WEBVTT\n"));
    }

    /// A fake daemon that answers `session.show` with these speakers, then each
    /// command it expects, in order, with its events.
    fn with_speakers(
        source: &'static str,
        speakers: Value,
        steps: Vec<(&'static str, Vec<Value>)>,
    ) -> Client {
        let (ours, theirs) = local_socket::pair();
        tokio::spawn(async move {
            let (reader, mut writer) = local_socket::split(theirs);
            let mut lines = BufReader::new(reader).lines();
            assert_eq!(lines.next_line().await.unwrap().unwrap(), "session.show n1");
            let detail = json!({"type": "session_detail", "session": {"id": "n1", "source": source}, "timeline": [], "speakers": speakers});
            let mut replies = vec![(None, vec![detail])];
            replies.extend(
                steps
                    .into_iter()
                    .map(|(command, events)| (Some(command), events)),
            );
            for (command, events) in replies {
                if let Some(command) = command {
                    assert_eq!(lines.next_line().await.unwrap().unwrap(), command);
                }
                for event in events {
                    writer
                        .write_all(format!("{event}\n").as_bytes())
                        .await
                        .unwrap();
                }
            }
            let _ = lines.next_line().await;
        });
        Client::over(ours)
    }

    #[tokio::test]
    async fn a_speaker_becomes_a_known_person_or_a_name() {
        let renamed = json!({"type": "speaker_renamed", "session": "n1", "label": "Speaker 1", "name": "Ana"});
        let voiced = json!([{"label": "Speaker 1", "voice": true, "person": null}]);
        let assign =
            r#"person.assign {"session":"n1","label":"Speaker 1","person":"","name":"Ana"}"#;
        let mut client = with_speakers("import", voiced, vec![(assign, vec![renamed.clone()])]);
        let named = client
            .speaker("n1", "Speaker 1", Who::Name("Ana".into()))
            .await
            .unwrap();
        assert_eq!(
            named,
            json!({"session": "n1", "label": "Speaker 1", "name": "Ana"})
        );

        // A WebVTT speaker without a voice also becomes a person.
        let plain = json!([{"label": "Speaker 1", "voice": false, "person": null}]);
        let steps = vec![(assign, vec![renamed.clone()])];
        let mut client = with_speakers("import", plain.clone(), steps);
        let named = client
            .speaker("n1", "Speaker 1", Who::Name("Ana".into()))
            .await;
        assert!(named.is_ok());
        let steps = vec![(assign, vec![renamed])];
        let mut client = with_speakers("live", plain, steps);
        assert!(
            client
                .speaker("n1", "Speaker 1", Who::Name("Ana".into()))
                .await
                .is_ok()
        );

        let mut client = with_speakers("live", json!([]), vec![]);
        let missing = client
            .speaker("n1", "Speaker 9", Who::Nobody)
            .await
            .unwrap_err();
        assert_eq!(missing.code, "session.invalid");
    }

    #[tokio::test]
    async fn people_commands_answer_with_everyone() {
        let gone = json!({"type": "error", "code": "person.not_found", "message": "no person"});
        let mut client = talking_to("person.forget p9", vec![gone]);
        let request = Request::People(People::Forget { id: "p9".into() });
        assert_eq!(
            client.handle(request).await.unwrap_err().code,
            "person.not_found"
        );

        let everyone = json!({"type": "people", "people": [{"id": "p1", "name": "Ana"}]});
        let mut client = talking_to(r#"person.merge {"into":"p1","from":"p2"}"#, vec![everyone]);
        let request = Request::People(People::Merge {
            into: "p1".into(),
            from: "p2".into(),
        });
        assert_eq!(client.handle(request).await.unwrap()[0]["name"], "Ana");

        let everyone = json!({"type": "people", "people": [{"id": "p3", "name": "Sadao"}]});
        let mut client = talking_to(r#"person.add {"name":"Sadao"}"#, vec![everyone]);
        let request = Request::People(People::Add {
            name: "Sadao".into(),
        });
        assert_eq!(client.handle(request).await.unwrap()[0]["name"], "Sadao");
    }

    #[tokio::test]
    async fn a_line_is_removed_by_who_and_when() {
        let other =
            json!({"type": "transcript_removed", "session": "n1", "who": "Eles", "at": 2.0});
        let ours = json!({"type": "transcript_removed", "session": "n1", "who": "Eles", "at": 1.5});
        let mut client = talking_to(
            r#"session.line.remove {"id":"n1","who":"Eles","at":1.5}"#,
            vec![other, ours],
        );
        let removed = client
            .line("n1", "Eles", 1.5, LineChange::Remove)
            .await
            .unwrap();
        assert_eq!(removed["removed"], true);
        assert_eq!(removed["at"], 1.5);
    }

    #[tokio::test]
    async fn a_line_is_corrected() {
        let edited = json!({"type": "transcript_edited", "session": "n1", "who": "Eles", "at": 1.5, "text": "Windhawk"});
        let mut client = talking_to(
            r#"session.line.edit {"id":"n1","who":"Eles","at":1.5,"text":"Windhawk"}"#,
            vec![edited],
        );
        let request = Request::Line {
            id: "n1".into(),
            who: "Eles".into(),
            at: 1.5,
            change: LineChange::Edit("Windhawk".into()),
        };
        let line = client.handle(request).await.unwrap();
        assert_eq!(line["text"], "Windhawk");
    }

    #[tokio::test]
    async fn lines_are_merged_and_split_by_who_and_when() {
        let other = json!({"type": "line_merged", "session": "n1", "into": {"who": "Eu", "at": 9.0},
                           "from": {"who": "Eu", "at": 9.5}, "text": "x"});
        let ours = json!({"type": "line_merged", "session": "n1", "into": {"who": "Eu", "at": 1.0},
                          "from": {"who": "Eles", "at": 1.5}, "text": "Oi. Tudo bem?"});
        let mut client = talking_to(
            r#"session.line.merge {"id":"n1","into":{"who":"Eles","at":1.5},"from":{"who":"Eu","at":1.0}}"#,
            vec![other, ours],
        );
        let merged = client
            .line("n1", "Eles", 1.5, LineChange::Merge("Eu".into(), 1.0))
            .await
            .unwrap();
        assert_eq!(merged["text"], "Oi. Tudo bem?");
        assert_eq!(merged["into"], json!({"who": "Eu", "at": 1.0}));

        let split = json!({"type": "line_split", "session": "n1", "of": {"who": "Eles", "at": 1.5},
                           "text": "Oi.", "new": {"who": "Eles", "name": "Ana", "at": 1.501, "text": "Tudo bem?"}});
        let mut client = talking_to(
            r#"session.line.split {"id":"n1","who":"Eles","at":1.5,"offset":3}"#,
            vec![split],
        );
        let split = client
            .line("n1", "Eles", 1.5, LineChange::Split(3))
            .await
            .unwrap();
        assert_eq!(split["new"]["text"], "Tudo bem?");

        let invalid = json!({"type": "error", "code": "line.invalid", "message": "no room"});
        let mut client = talking_to(
            r#"session.line.split {"id":"n1","who":"Eles","at":1.5,"offset":0}"#,
            vec![invalid],
        );
        let failed = client.line("n1", "Eles", 1.5, LineChange::Split(0)).await;
        assert_eq!(failed.unwrap_err().code, "line.invalid");
    }

    #[tokio::test]
    async fn an_argument_with_a_line_break_is_never_sent() {
        for id in ["x\nconfig.set {}", "x\rstop"] {
            let (ours, mut theirs) = local_socket::pair();
            let mut client = Client::over(ours);
            let failure = client.show(id).await.err().unwrap();
            assert_eq!(failure.code, "argument.invalid");
            drop(client);
            let mut received = Vec::new();
            theirs.read_to_end(&mut received).await.unwrap();
            assert!(received.is_empty(), "sent {received:?}");
        }
    }

    #[tokio::test]
    async fn other_control_characters_are_sent() {
        let detail = json!({"type": "session_detail", "session": {"id": "a\u{7f}\u{85}\0b"}});
        let mut client = talking_to("session.show a\u{7f}\u{85}\0b", vec![detail]);
        let shown = client.show("a\u{7f}\u{85}\0b").await.unwrap();
        assert_eq!(shown["session"]["id"], "a\u{7f}\u{85}\0b");
    }

    #[tokio::test]
    async fn a_line_break_inside_a_json_argument_is_kept() {
        let note = json!({"type": "note", "id": "c1", "session": "n1", "text": "a\nb", "at": 1.0});
        let mut client = talking_to(r#"session.note {"id":"n1","text":"a\nb"}"#, vec![note]);
        let kept = client.note("n1", "a\nb").await.unwrap();
        assert_eq!(kept["text"], "a\nb");
    }

    #[tokio::test]
    async fn window_commands_send_what_a_shortcut_asks_and_return_once_read() {
        let path = std::path::absolute("/tmp/retro.vtt").unwrap();
        let imported = json!({"call": "import", "path": path});
        let imported: &'static str = format!("window.call {imported}").leak();
        let asks = [
            (
                Window::Call {
                    call: "config",
                    path: None,
                },
                r#"window.call {"call":"config"}"#,
            ),
            (
                Window::Call {
                    call: "import",
                    path: Some("/tmp/retro.vtt".into()),
                },
                imported,
            ),
            (Window::Action { name: "ask".into() }, "action ask"),
            (Window::Toggle, "session.toggle"),
        ];
        for (window, expected) in asks {
            let synced = json!({"type": "synced"});
            let mut client = conversing(vec![(expected, vec![signal()]), ("sync", vec![synced])]);
            assert_eq!(client.window(window).await, Ok(Value::Null));
        }
    }

    #[tokio::test]
    async fn a_window_command_the_daemon_closes_on_before_it_is_read_fails() {
        let mut client = closing_after("session.toggle", &[]);
        let failure = client.window(Window::Toggle).await.unwrap_err();
        assert_eq!(failure.code, "daemon.closed");
    }

    #[tokio::test]
    async fn a_window_command_reaches_a_running_daemon() {
        let directory = tempfile::tempdir().unwrap();
        let address = Address::scratch(directory.path());
        let (commands, mut received) = tokio::sync::mpsc::unbounded_channel();
        let _socket = control_socket::ControlSocket::bind(
            &address,
            "token".into(),
            control_socket::Clients::default(),
            std::sync::Arc::new(Vec::new),
            commands,
        )
        .await
        .unwrap();
        let request = Request::Window(Window::Call {
            call: "focus",
            path: None,
        });
        let ran = tokio::time::timeout(std::time::Duration::from_secs(10), run(&address, request));
        assert_eq!(ran.await, Ok(0));
        assert_eq!(
            received.try_recv().unwrap(),
            r#"window.call {"call":"focus"}"#
        );
    }

    #[tokio::test]
    async fn a_daemon_that_is_not_running_is_reported() {
        let directory = tempfile::tempdir().unwrap();
        let failure = Client::connect(&Address::scratch(directory.path()))
            .await
            .err()
            .unwrap();
        assert_eq!(failure.code, "daemon.offline");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_socket_path_the_system_refuses_is_unavailable() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let failure = Client::connect(&Address::scratch(file.path()))
            .await
            .err()
            .unwrap();
        assert_eq!(failure.code, "daemon.unavailable");
    }

    /// A socket at a fresh path whose daemon answers one connection.
    async fn listening(turns: Vec<(&'static str, Vec<Value>)>) -> (tempfile::TempDir, Address) {
        let directory = tempfile::tempdir().unwrap();
        let path = Address::scratch(directory.path());
        let listener = control_socket::FakeListener::bind(&path).await;
        tokio::spawn(async move {
            let stream = listener.accept().await.unwrap();
            let greeting = json!({"type": "snapshot", "kinds": ["meeting"]});
            daemon(stream, greeting, turns).await;
        });
        (directory, path)
    }

    #[tokio::test]
    async fn run_exits_zero_once_the_daemon_answers() {
        let sessions = json!({"type": "sessions", "sessions": [{"id": "a", "kind": "idea"}]});
        let (_directory, socket) = listening(vec![("sessions", vec![sessions])]).await;
        let request = Request::Sessions {
            kind: None,
            person: None,
            tag: None,
            search: None,
        };
        assert_eq!(run(&socket, request).await, 0);

        let vtt = json!({"type": "session_export", "id": "n1", "text": "WEBVTT\n"});
        let (_directory, socket) = listening(vec![("session.export n1", vec![vtt])]).await;
        assert_eq!(run(&socket, Request::Export { id: "n1".into() }).await, 0);
    }

    #[tokio::test]
    async fn run_exits_one_when_the_daemon_refuses_or_is_offline() {
        let (_directory, socket) = listening(vec![(
            "session.delete n9",
            vec![refusal("session.not_found")],
        )])
        .await;
        assert_eq!(run(&socket, Request::Delete { id: "n9".into() }).await, 1);

        let directory = tempfile::tempdir().unwrap();
        let offline = Address::scratch(directory.path());
        assert_eq!(run(&offline, Request::Show { id: "n1".into() }).await, 1);
    }

    #[tokio::test]
    async fn a_line_that_is_not_json_is_skipped() {
        let mut client = closing_after(
            "session.delete n1",
            &["not json", r#"{"type":"session_deleted","id":"n1"}"#],
        );
        assert_eq!(client.delete("n1").await, Ok(json!({"id": "n1"})));
    }

    #[tokio::test]
    async fn a_daemon_that_closes_before_settling_is_a_failure() {
        let mut client = closing_after("session.delete n1", &[]);
        assert_eq!(client.delete("n1").await.unwrap_err().code, "daemon.closed");
    }

    #[tokio::test]
    async fn a_command_to_a_gone_daemon_is_a_failure() {
        let (ours, theirs) = local_socket::pair();
        drop(theirs);
        let mut client = Client::over(ours);
        assert_eq!(client.delete("n1").await.unwrap_err().code, "daemon.closed");
    }

    #[tokio::test]
    async fn a_delete_settles_on_its_own_session() {
        let events = vec![
            json!({"type": "session_deleted", "id": "other"}),
            refusal("transcription.failed"),
            // Its voice links failing does not undo the deletion.
            refusal("people.failed"),
            json!({"type": "session_deleted", "id": "n1"}),
        ];
        let mut client = talking_to("session.delete n1", events);
        let request = Request::Delete { id: "n1".into() };
        assert_eq!(client.handle(request).await, Ok(json!({"id": "n1"})));

        let refused = vec![refusal("session.delete_failed")];
        let mut client = talking_to("session.delete n1", refused);
        let request = Request::Delete { id: "n1".into() };
        assert_eq!(
            client.handle(request).await.unwrap_err(),
            Failure::new("session.delete_failed", "session.delete_failed happened")
        );
    }

    #[tokio::test]
    async fn a_line_is_given_to_a_person() {
        let events = vec![
            json!({"type": "transcript_reassigned", "session": "n1", "at": 9.0, "label": "x", "name": "x"}),
            json!({"type": "transcript_reassigned", "session": "n1", "who": "Eles", "at": 1.5, "label": "Ana", "name": "Ana"}),
        ];
        let mut client = talking_to(
            r#"person.assign_line {"id":"n1","who":"Eles","at":1.5,"person":"","name":"Ana"}"#,
            events,
        );
        let request = Request::AssignLine {
            id: "n1".into(),
            who: "Eles".into(),
            at: 1.5,
            person: None,
            name: Some("Ana".into()),
        };
        assert_eq!(
            client.handle(request).await,
            Ok(json!({"session": "n1", "label": "Ana", "name": "Ana"}))
        );

        let mut client = talking_to(
            r#"person.assign_line {"id":"n1","who":"Eles","at":1.5,"person":"p1","name":""}"#,
            vec![refusal("line.not_found")],
        );
        let request = Request::AssignLine {
            id: "n1".into(),
            who: "Eles".into(),
            at: 1.5,
            person: Some("p1".into()),
            name: None,
        };
        assert_eq!(
            client.handle(request).await.unwrap_err().code,
            "line.not_found"
        );
    }

    #[tokio::test]
    async fn every_speaker_is_given_to_one_person() {
        let events = vec![
            json!({"type": "person_assigned_all", "session": "other", "speakers": 5}),
            json!({"type": "person_assigned_all", "session": "n1", "speakers": 2}),
        ];
        let mut client = talking_to(
            r#"person.assign_all {"session":"n1","person":"p1","name":""}"#,
            events,
        );
        let request = Request::AssignAll {
            id: "n1".into(),
            person: Some("p1".into()),
            name: None,
        };
        assert_eq!(
            client.handle(request).await,
            Ok(json!({"session": "n1", "speakers": 2}))
        );

        let mut client = talking_to(
            r#"person.assign_all {"session":"n1","person":"","name":"Ana"}"#,
            vec![refusal("person.not_found")],
        );
        let request = Request::AssignAll {
            id: "n1".into(),
            person: None,
            name: Some("Ana".into()),
        };
        assert_eq!(
            client.handle(request).await.unwrap_err().code,
            "person.not_found"
        );
    }

    #[tokio::test]
    async fn a_speaker_is_given_a_known_person_by_id() {
        let speakers = json!([{"label": "Speaker 1", "name": "Speaker 1", "person": null}]);
        let assign =
            r#"person.assign {"session":"n1","label":"Speaker 1","person":"p1","name":""}"#;
        let events = vec![
            json!({"type": "speaker_renamed", "session": "n1", "label": "Speaker 2", "name": "Bia"}),
            json!({"type": "speaker_renamed", "session": "n1", "label": "Speaker 1", "name": "Ana"}),
        ];
        let mut client = with_speakers("live", speakers.clone(), vec![(assign, events)]);
        let request = Request::Speaker {
            id: "n1".into(),
            label: "Speaker 1".into(),
            who: Who::Person("p1".into()),
        };
        assert_eq!(
            client.handle(request).await,
            Ok(json!({"session": "n1", "label": "Speaker 1", "name": "Ana"}))
        );

        let steps = vec![(assign, vec![refusal("people.failed")])];
        let mut client = with_speakers("live", speakers, steps);
        let failure = client
            .speaker("n1", "Speaker 1", Who::Person("p1".into()))
            .await
            .unwrap_err();
        assert_eq!(failure.code, "people.failed");
    }

    #[tokio::test]
    async fn nobody_unassigns_a_person_or_clears_a_name() {
        let renamed = json!({"type": "speaker_renamed", "session": "n1", "label": "Speaker 1", "name": "Speaker 1"});
        let assigned = json!([{"label": "Speaker 1", "name": "Ana", "person": "p1"}]);
        let unassign = r#"person.unassign {"session":"n1","label":"Speaker 1"}"#;
        let mut client = with_speakers("live", assigned, vec![(unassign, vec![renamed.clone()])]);
        let nobody = client.speaker("n1", "Speaker 1", Who::Nobody).await;
        assert_eq!(
            nobody,
            Ok(json!({"session": "n1", "label": "Speaker 1", "name": "Speaker 1"}))
        );

        let named = json!([{"label": "Speaker 1", "name": "Chefe", "person": null, "guess": null}]);
        let clear = r#"session.speaker {"id":"n1","label":"Speaker 1","name":""}"#;
        let mut client = with_speakers("live", named, vec![(clear, vec![renamed])]);
        let nobody = client.speaker("n1", "Speaker 1", Who::Nobody).await;
        assert_eq!(nobody.unwrap()["name"], "Speaker 1");
    }

    #[tokio::test]
    async fn nobody_clears_a_guess() {
        let guessed = json!([{"label": "Speaker 1", "name": "Speaker 1", "person": null,
            "guess": {"person": "p1", "name": "Ana", "score": 0.9}}]);
        let clear = r#"person.guess.clear {"session":"n1","label":"Speaker 1"}"#;
        let events = vec![
            json!({"type": "session_speakers", "session": "other", "speakers": []}),
            json!({"type": "session_speakers", "session": "n1", "speakers": []}),
        ];
        let mut client = with_speakers("live", guessed.clone(), vec![(clear, events)]);
        let cleared = client.speaker("n1", "Speaker 1", Who::Nobody).await;
        assert_eq!(
            cleared,
            Ok(json!({"session": "n1", "label": "Speaker 1", "name": "Speaker 1"}))
        );

        let steps = vec![(clear, vec![refusal("speaker.invalid")])];
        let mut client = with_speakers("live", guessed, steps);
        let failure = client
            .speaker("n1", "Speaker 1", Who::Nobody)
            .await
            .unwrap_err();
        assert_eq!(failure.code, "speaker.invalid");
    }

    /// The greeting of a daemon with two context slots: one for meetings, one for ideas.
    fn with_slots() -> Value {
        json!({"type": "snapshot", "contexts": [
            {"name": "cv", "kinds": ["meeting"]},
            {"name": "notes", "kinds": ["idea"]},
        ]})
    }

    #[tokio::test]
    async fn a_session_without_chosen_contexts_has_those_of_its_kind() {
        let detail = json!({"type": "session_detail", "session": {"id": "n1", "kind": "meeting"}});
        let mut client = greeted(with_slots(), vec![("session.show n1", vec![detail])]);
        let request = Request::Context {
            id: "n1".into(),
            add: vec![],
            remove: vec![],
        };
        assert_eq!(
            client.handle(request).await,
            Ok(json!({"session": "n1", "contexts": ["cv"], "available": ["cv", "notes"]}))
        );
    }

    #[tokio::test]
    async fn contexts_are_turned_on_and_off() {
        let detail = json!({"type": "session_detail", "session": {"id": "n1", "kind": "meeting", "contexts": ["cv"]}});
        let events = vec![
            json!({"type": "session_context", "session": "other", "contexts": []}),
            json!({"type": "session_context", "session": "n1", "contexts": ["notes"]}),
        ];
        let mut client = greeted(
            with_slots(),
            vec![
                ("session.show n1", vec![detail.clone()]),
                (
                    r#"session.context {"id":"n1","contexts":["notes"]}"#,
                    events,
                ),
            ],
        );
        let changed = client
            .context("n1", &["notes".into(), "notes".into()], &["cv".into()])
            .await;
        assert_eq!(
            changed,
            Ok(json!({"session": "n1", "contexts": ["notes"], "available": ["cv", "notes"]}))
        );

        let mut client = greeted(
            with_slots(),
            vec![
                ("session.show n1", vec![detail]),
                (
                    r#"session.context {"id":"n1","contexts":["cv","notes"]}"#,
                    vec![refusal("session.not_found")],
                ),
            ],
        );
        let failure = client.context("n1", &["notes".into()], &[]).await;
        assert_eq!(failure.unwrap_err().code, "session.not_found");
    }

    #[tokio::test]
    async fn an_unknown_context_slot_is_refused_before_asking() {
        let mut client = greeted(with_slots(), vec![]);
        let failure = client
            .context("n1", &[], &["cvv".into()])
            .await
            .unwrap_err();
        assert_eq!(failure.code, "context.unknown");
        assert_eq!(
            failure.message,
            r#"no context slot "cvv"; there are ["cv", "notes"]"#
        );
    }

    #[tokio::test]
    async fn a_person_joins_or_leaves_a_session() {
        let events = vec![
            json!({"type": "attendees_changed", "session": "other", "people": []}),
            json!({"type": "attendees_changed", "session": "n1", "people": [{"id": "p1", "name": "Ana"}]}),
        ];
        let mut client = talking_to(
            r#"person.attend {"session":"n1","person":"","name":"Ana"}"#,
            events,
        );
        let request = Request::Participant {
            id: "n1".into(),
            person: None,
            name: Some("Ana".into()),
            remove: false,
        };
        assert_eq!(
            client.handle(request).await,
            Ok(json!([{"id": "p1", "name": "Ana"}]))
        );

        let mut client = talking_to(
            r#"person.leave {"session":"n1","person":"p1","name":""}"#,
            vec![refusal("person.not_found")],
        );
        let request = Request::Participant {
            id: "n1".into(),
            person: Some("p1".into()),
            name: None,
            remove: true,
        };
        assert_eq!(
            client.handle(request).await.unwrap_err().code,
            "person.not_found"
        );
    }

    #[tokio::test]
    async fn people_are_listed_renamed_and_adopted() {
        let everyone = json!({"type": "people", "people": [{"id": "p1", "name": "Ana"}]});
        let mut client = talking_to("people", vec![signal(), everyone.clone()]);
        let request = Request::People(People::List);
        assert_eq!(
            client.handle(request).await,
            Ok(json!([{"id": "p1", "name": "Ana"}]))
        );

        let mut client = talking_to(r#"person.rename {"id":"p1","name":"Ana"}"#, vec![everyone]);
        let request = Request::People(People::Rename {
            id: "p1".into(),
            name: "Ana".into(),
        });
        assert_eq!(client.handle(request).await.unwrap()[0]["name"], "Ana");

        let adopted = json!({"type": "people_adopted", "count": 3});
        let mut client = talking_to("people.adopt", vec![signal(), adopted]);
        let request = Request::People(People::Adopt);
        assert_eq!(client.handle(request).await, Ok(json!({"adopted": 3})));

        let mut client = talking_to("people.adopt", vec![refusal("people.failed")]);
        let request = Request::People(People::Adopt);
        assert_eq!(
            client.handle(request).await.unwrap_err().code,
            "people.failed"
        );
    }

    #[tokio::test]
    async fn a_rename_with_title_and_kind_asks_nothing_first() {
        let events = vec![
            json!({"type": "session_renamed", "id": "other", "title": "x", "kind": "idea"}),
            json!({"type": "session_renamed", "id": "n1", "title": "Retro", "kind": "meeting"}),
        ];
        let mut client = talking_to(
            r#"session.rename {"id":"n1","title":"Retro","kind":"meeting"}"#,
            events,
        );
        let request = Request::Rename {
            id: "n1".into(),
            title: Some("Retro".into()),
            kind: Some("meeting".into()),
        };
        assert_eq!(
            client.handle(request).await,
            Ok(json!({"id": "n1", "title": "Retro", "kind": "meeting"}))
        );
    }

    #[tokio::test]
    async fn a_rename_keeps_what_is_not_given() {
        let detail = json!({"type": "session_detail", "session": {"id": "n1", "title": "Old", "kind": "idea"}});
        let renamed =
            json!({"type": "session_renamed", "id": "n1", "title": "Retro", "kind": "idea"});
        let mut client = conversing(vec![
            ("session.show n1", vec![detail.clone()]),
            (
                r#"session.rename {"id":"n1","title":"Retro","kind":"idea"}"#,
                vec![renamed],
            ),
        ]);
        let renamed = client.rename("n1", Some("Retro".into()), None).await;
        assert_eq!(renamed.unwrap()["title"], "Retro");

        let mut client = conversing(vec![
            ("session.show n1", vec![detail]),
            (
                r#"session.rename {"id":"n1","title":"Old","kind":"meeting"}"#,
                vec![refusal("session.invalid")],
            ),
        ]);
        let failure = client.rename("n1", None, Some("meeting".into())).await;
        assert_eq!(failure.unwrap_err().code, "session.invalid");

        let mut client = talking_to("session.show n9", vec![refusal("session.not_found")]);
        let failure = client.rename("n9", None, None).await.unwrap_err();
        assert_eq!(failure.code, "session.not_found");
    }

    #[tokio::test]
    async fn a_session_is_shown_with_its_whole_timeline_read_in_pages_and_its_speakers() {
        let detail =
            json!({"type": "session_detail", "session": {"id": "n1"}, "speakers": [], "extra": 1});
        let other = json!({"type": "session_detail", "session": {"id": "other"}});
        let page = |before: Value, ats: &[f64], more: bool| {
            let entries: Vec<Value> = ats
                .iter()
                .map(|at| json!({"who": "Eles", "at": at}))
                .collect();
            json!({"type": "session_timeline", "session": "n1", "before": before, "after": null,
                "around": null, "timeline": entries, "more_before": more, "more_after": false})
        };
        // Another window's page is not this one's.
        let elsewhere = json!({"type": "session_timeline", "session": "n1", "before": 9.0,
            "after": null, "around": null, "timeline": [{"at": 8.0}], "more_before": false});
        let mut client = conversing(vec![
            ("session.show n1", vec![other, detail]),
            (
                r#"session.timeline {"id":"n1","before":null,"limit":1000}"#,
                vec![elsewhere, page(Value::Null, &[3.0, 4.0], true)],
            ),
            (
                r#"session.timeline {"id":"n1","before":3.0,"limit":1000}"#,
                vec![page(json!(3.0), &[1.0, 2.0], false)],
            ),
        ]);
        let request = Request::Show { id: "n1".into() };
        let ats: Vec<f64> = [1.0, 2.0, 3.0, 4.0].into();
        let timeline: Vec<Value> = ats
            .iter()
            .map(|at| json!({"who": "Eles", "at": at}))
            .collect();
        assert_eq!(
            client.handle(request).await,
            Ok(json!({"session": {"id": "n1"}, "timeline": timeline, "speakers": []}))
        );
    }

    #[tokio::test]
    async fn a_session_without_entries_is_shown_with_an_empty_timeline() {
        let detail = json!({"type": "session_detail", "session": {"id": "n1"}, "speakers": []});
        let empty = json!({"type": "session_timeline", "session": "n1", "before": null, "after": null,
            "around": null, "timeline": [], "more_before": false, "more_after": false});
        let mut client = conversing(vec![
            ("session.show n1", vec![detail]),
            (
                r#"session.timeline {"id":"n1","before":null,"limit":1000}"#,
                vec![empty],
            ),
        ]);
        let shown = client
            .handle(Request::Show { id: "n1".into() })
            .await
            .unwrap();
        assert_eq!(shown["timeline"], json!([]));
    }

    #[tokio::test]
    async fn sessions_with_a_blank_search_are_not_searched() {
        let sessions = json!({"type": "sessions", "sessions": [{"id": "a", "kind": "idea"}]});
        let mut client = talking_to("sessions", vec![sessions]);
        let request = Request::Sessions {
            kind: None,
            person: None,
            tag: None,
            search: Some(" \n ".into()),
        };
        assert_eq!(
            client.handle(request).await,
            Ok(json!([{"id": "a", "kind": "idea"}]))
        );
    }

    #[tokio::test]
    async fn a_tag_is_removed_or_renamed() {
        let untagged = json!({"type": "session_tags", "session": "n1", "tags": []});
        let mut client = talking_to(r#"session.untag {"id":"n1","tag":"Q3"}"#, vec![untagged]);
        let request = Request::Tag(Tag::Remove {
            id: "n1".into(),
            tag: "Q3".into(),
        });
        assert_eq!(
            client.handle(request).await,
            Ok(json!({"session": "n1", "tags": []}))
        );

        let renamed = json!({"type": "tag_renamed", "from": "Q3", "to": "Q4", "sessions": 2});
        let mut client = talking_to(r#"tag.rename {"from":"Q3","to":"Q4"}"#, vec![renamed]);
        let request = Request::Tag(Tag::Rename {
            from: "Q3".into(),
            to: "Q4".into(),
        });
        assert_eq!(
            client.handle(request).await,
            Ok(json!({"from": "Q3", "to": "Q4", "sessions": 2}))
        );
    }

    #[tokio::test]
    async fn an_import_sends_its_options_and_an_absolute_path() {
        let path = std::path::absolute("talk.mp4").unwrap();
        let expected = json!({"path": path, "title": "Talk", "kind": "meeting",
            "language": "pt", "participant": "Ana", "started_at": 1.5});
        let expected: &'static str = format!("session.import {expected}").leak();
        let started = json!({"type": "import_started", "session": {"id": "i1"}, "total_s": 60.0});
        let mut client = talking_to(expected, vec![signal(), started]);
        let request = Request::Import {
            path: "talk.mp4".into(),
            title: Some("Talk".into()),
            kind: Some("meeting".into()),
            language: Some("pt".into()),
            participant: Some("Ana".into()),
            started_at: Some(1.5),
            wait: false,
        };
        assert_eq!(
            client.handle(request).await,
            Ok(json!({"session": {"id": "i1"}, "total_s": 60.0}))
        );
    }

    #[tokio::test]
    async fn a_waited_import_fails_with_its_transcription() {
        let events = vec![
            json!({"type": "import_started", "session": {"id": "i1"}, "total_s": 60.0}),
            json!({"type": "import_done", "id": "other", "complete": true}),
            refusal("transcription.failed"),
            refusal("import.failed"),
        ];
        let mut client = talking_to(r#"session.import {"path":"/talk.mp4"}"#, events);
        let failure = client.import(&json!({"path": "/talk.mp4"}), true).await;
        assert_eq!(failure.unwrap_err().code, "import.failed");
    }

    #[tokio::test]
    async fn the_greeting_is_every_event_before_synced() {
        let update = json!({"type": "update", "latest": "0.5.0"});
        let synced = json!({"type": "synced"});
        let mut client = conversing(vec![("sync", vec![update.clone(), synced])]);
        assert_eq!(
            client.greeting().await,
            Ok(vec![
                json!({"type": "snapshot", "kinds": ["meeting"]}),
                update
            ])
        );
    }

    #[tokio::test]
    async fn without_a_daemon_there_is_no_greeting_or_update() {
        let directory = tempfile::tempdir().unwrap();
        let address = Address::scratch(directory.path());
        assert_eq!(greeting(&address).await, Ok(None));
        assert_eq!(update(&address, "0.5.0").await, Ok(None));
    }

    #[tokio::test]
    async fn a_running_daemon_greets_lists_its_keys_and_installs() {
        let synced = json!({"type": "synced"});
        let (_directory, socket) = listening(vec![("sync", vec![synced])]).await;
        assert_eq!(
            greeting(&socket).await,
            Ok(Some(vec![
                json!({"type": "snapshot", "kinds": ["meeting"]})
            ]))
        );

        let keys = json!({"type": "keys", "env": ["OPENROUTER_API_KEY"], "credentials": null});
        let omapass = json!({"type": "omapass", "installed": false});
        let (_directory, socket) =
            listening(vec![("keys", vec![keys]), ("omapass", vec![omapass])]).await;
        let listed = key_names(&socket).await.unwrap().unwrap();
        assert_eq!(listed["env"], json!(["OPENROUTER_API_KEY"]));

        let command = r#"update.install {"version":"0.5.0"}"#;
        let (_directory, socket) = listening(vec![(command, vec![stage("done", "")])]).await;
        assert_eq!(update(&socket, "0.5.0").await, Ok(Some("now")));
        // A daemon from before updates leaves the install to the command.
        let unknown = refusal("command.unknown");
        let (_directory, socket) = listening(vec![(command, vec![unknown])]).await;
        assert_eq!(update(&socket, "0.5.0").await, Ok(None));
    }

    fn stage(stage: &str, detail: &str) -> Value {
        json!({"type": "update_progress", "stage": stage, "version": "0.5.0", "detail": detail})
    }

    #[tokio::test]
    async fn an_update_through_the_daemon_ends_when_it_restarts_or_waits() {
        let command = r#"update.install {"version":"0.5.0"}"#;
        let stages = vec![
            stage("downloading", ""),
            stage("verifying", ""),
            stage("installing", ""),
        ];
        let mut now = stages.clone();
        now.push(stage("restarting", ""));
        let mut client = talking_to(command, now);
        assert_eq!(client.update("0.5.0").await, Ok("now"));
        let mut later = stages;
        later.push(stage("waiting", ""));
        let mut client = talking_to(command, later);
        assert_eq!(client.update("0.5.0").await, Ok("when_session_ends"));
    }

    #[tokio::test]
    async fn an_update_through_the_daemon_fails_with_its_reason() {
        let command = r#"update.install {"version":"0.5.0"}"#;
        let failed = stage("failed", "eco-0.5.0-1-x86_64.pkg.tar.zst does not match");
        let mut client = talking_to(command, vec![stage("downloading", ""), failed]);
        assert_eq!(
            client.update("0.5.0").await,
            Err(Failure::new(
                "update.failed",
                "eco-0.5.0-1-x86_64.pkg.tar.zst does not match"
            ))
        );
        let mut client = talking_to(command, vec![refusal("update.busy")]);
        assert_eq!(
            client.update("0.5.0").await.unwrap_err().code,
            "update.busy"
        );
    }

    fn removal(stage: &str, detail: &str) -> Value {
        json!({"type": "uninstall", "stage": stage, "everything": true, "detail": detail})
    }

    #[tokio::test]
    async fn a_removal_through_the_daemon_ends_when_it_is_handed_off() {
        let command = r#"uninstall {"everything":true}"#;
        let mut client = talking_to(command, vec![removal("removing", "")]);
        assert_eq!(client.uninstall(true).await, Ok(()));
        let (_directory, socket) = listening(vec![(command, vec![removal("removing", "")])]).await;
        assert_eq!(uninstall(&socket, true).await, Ok(true));
        // A daemon from before removals leaves it to the command.
        let unknown = refusal("command.unknown");
        let (_directory, socket) = listening(vec![(command, vec![unknown])]).await;
        assert_eq!(uninstall(&socket, true).await, Ok(false));
        let directory = tempfile::tempdir().unwrap();
        let address = Address::scratch(directory.path());
        assert_eq!(uninstall(&address, true).await, Ok(false));
    }

    #[tokio::test]
    async fn a_removal_through_the_daemon_fails_with_its_reason_or_its_refusal() {
        let command = r#"uninstall {"everything":false}"#;
        let failed = removal("failed", "no systemd-run");
        let mut client = talking_to(command, vec![failed]);
        assert_eq!(
            client.uninstall(false).await,
            Err(Failure::new("uninstall.failed", "no systemd-run"))
        );
        let mut client = talking_to(command, vec![refusal("uninstall.recording")]);
        assert_eq!(
            client.uninstall(false).await.unwrap_err().code,
            "uninstall.recording"
        );
    }

    #[test]
    fn a_restricted_socket_is_not_reported_as_an_offline_daemon() {
        let failure = connect_failure(ConnectError::AccessDenied);
        assert_eq!(failure.code, "daemon.access_denied");
    }
}
