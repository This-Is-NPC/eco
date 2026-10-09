//! The session commands of the `eco` CLI: they talk to the running daemon over its
//! socket and print one JSON object, `{"ok": true, "data": …}` or
//! `{"ok": false, "code": …, "message": …}`, exiting non-zero on failure.
//! `export` prints the document itself, to pipe or redirect.

use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDateTime, TimeZone};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};

use crate::adapters::local_socket::{self, ConnectError, ReadHalf, Stream, WriteHalf};
use crate::domain::session::same_tag;

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
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Run `request` against the daemon at `socket`, print the result, and return
/// the exit code.
pub async fn run(socket: &Path, request: Request) -> i32 {
    let document = matches!(request, Request::Export { .. });
    let answered = match Client::connect(socket).await {
        Ok(mut client) => client.handle(request).await,
        Err(failure) => Err(failure),
    };
    match answered {
        Ok(Value::String(text)) if document => {
            print!("{text}");
            0
        }
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
    async fn connect(socket: &Path) -> Result<Self, Failure> {
        let stream = local_socket::connect(socket)
            .await
            .map_err(connect_failure)?
            .ok_or_else(|| {
                Failure::new(
                    "daemon.offline",
                    "eco is not running; start it with `eco start` (or `eco start --headless`)",
                )
            })?;
        Ok(Self::over(stream))
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
            Request::Show { id } => self.show(&id).await,
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

    /// A session's summary, its timeline and its speakers.
    async fn show(&mut self, id: &str) -> Result<Value, Failure> {
        self.send(&format!("session.show {id}")).await?;
        self.until(|event| {
            if event["type"] == "session_detail" && event["session"]["id"] == id {
                let detail = json!({
                    "session": event["session"], "timeline": event["timeline"],
                    "speakers": event["speakers"],
                });
                return Some(Ok(detail));
            }
            failure_in(event, &["session.not_found"]).map(Err)
        })
        .await
    }

    async fn delete(&mut self, id: &str) -> Result<Value, Failure> {
        self.send(&format!("session.delete {id}")).await?;
        self.until(|event| {
            if event["type"] == "session_deleted" && event["id"] == id {
                return Some(Ok(json!({"id": id})));
            }
            failure_in(event, &["session.", "people.failed"]).map(Err)
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

    /// Remove or correct a line; returns what became of it.
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
        };
        self.send(&format!("{command} {line}")).await?;
        self.until(|event| {
            let ours =
                event["session"] == id && event["who"] == who && event["at"].as_f64() == Some(at);
            match event["type"].as_str() {
                Some("transcript_removed") if ours => Some(Ok(
                    json!({"session": id, "who": who, "at": at, "removed": true}),
                )),
                Some("transcript_edited") if ours => Some(Ok(
                    json!({"session": id, "who": who, "at": at, "text": event["text"]}),
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
                &["session.", "translation.unknown", "translation.own"],
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
            let failed = failure_in(event, &["session.", "action.unknown", "completion.failed"]);
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
        let (ours, theirs) = Stream::pair().unwrap();
        tokio::spawn(async move {
            let (mut reader, mut writer) = local_socket::split(theirs);
            let greeting = json!({"type": "snapshot", "kinds": ["meeting"]});
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
        });
        Client::over(ours)
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
        let (ours, theirs) = Stream::pair().unwrap();
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
        let change = LineChange::Edit("Windhawk".into());
        let line = client.line("n1", "Eles", 1.5, change).await.unwrap();
        assert_eq!(line["text"], "Windhawk");
    }

    #[tokio::test]
    async fn an_argument_with_a_line_break_is_never_sent() {
        for id in ["x\nconfig.set {}", "x\rstop"] {
            let (ours, mut theirs) = Stream::pair().unwrap();
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
    async fn a_daemon_that_is_not_running_is_reported() {
        let directory = tempfile::tempdir().unwrap();
        let failure = Client::connect(&directory.path().join("eco.sock"))
            .await
            .err()
            .unwrap();
        assert_eq!(failure.code, "daemon.offline");
    }

    #[test]
    fn a_restricted_socket_is_not_reported_as_an_offline_daemon() {
        let failure = connect_failure(ConnectError::AccessDenied);
        assert_eq!(failure.code, "daemon.access_denied");
    }
}
