//! One append-only JSON Lines file per session, named by start time and title,
//! with `-2`, `-3`… when another session already holds that name; and beside
//! them the session index, `index.json` (docs/design.md §8).

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use chrono::{Local, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization;

use crate::config::{private_dir, private_file};
use crate::domain::session::{ENDED, IMPORTING, PAUSED, RECORDING, now};
use crate::ports::{
    LogAlert, LogHealth, Record, RecordSink, SessionLog, SessionStorage, StoreError,
};

/// The session index's file, in the sessions' directory.
const INDEX: &str = "index.json";

/// What the index keeps of one log.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct IndexedLog {
    /// The session's id, from the log's first record; "" when it has none.
    id: String,
    /// The log's size, and when it was last modified, in nanoseconds since
    /// the epoch, when `entry` was made.
    bytes: u64,
    modified: u64,
    /// What the domain made of the log then; `None` until it is asked for.
    entry: Option<Value>,
}

/// The index: each log's entry, by its file name.
type Logs = BTreeMap<String, IndexedLog>;

/// The index as the daemon holds it: `None` until it is first read from its file.
type Shared = Arc<Mutex<Option<Logs>>>;

/// Run `with` on the index of `directory`, read from its file the first time;
/// a file missing or not readable gives an empty index, so every log is read again.
fn indexed<R>(index: &Shared, directory: &Path, with: impl FnOnce(&mut Logs) -> R) -> R {
    let mut index = index.lock().expect("not poisoned");
    let logs = index.get_or_insert_with(|| {
        fs::read_to_string(directory.join(INDEX))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    });
    with(logs)
}

/// Write the index whole to a file then put it in place, so a reader never
/// meets it half written.
fn save(directory: &Path, logs: &Logs) {
    let path = directory.join(INDEX);
    let partial = directory.join(format!("{INDEX}.partial"));
    let text = serde_json::to_string(logs).expect("an index is JSON");
    let written = private_file()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&partial)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .and_then(|()| fs::rename(&partial, &path));
    if let Err(error) = written {
        eprintln!("eco: cannot write {}: {error}", path.display());
    }
}

/// A log's size and when it was last modified, in nanoseconds since the epoch.
fn stamp(path: &Path) -> Option<(u64, u64)> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some((metadata.len(), modified.as_nanos() as u64))
}

/// The session id in a log's first record, the only line read.
fn head_id(path: &Path) -> Option<String> {
    let mut line = String::new();
    BufReader::new(fs::File::open(path).ok()?)
        .read_line(&mut line)
        .ok()?;
    let head: Value = serde_json::from_str(&line).ok()?;
    head["id"].as_str().map(String::from)
}

/// A log's file name, the index's key.
fn name_of(path: &Path) -> Option<String> {
    path.file_name()?.to_str().map(String::from)
}

fn slug(title: &str) -> String {
    let ascii: String = title
        .nfkd()
        .filter(char::is_ascii)
        .collect::<String>()
        .to_lowercase();
    let mut slug = String::new();
    for c in ascii.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').chars().take(48).collect()
}

/// Records a log holds in memory while writes fail; the next ones are dropped,
/// so what reaches the file stays a prefix of the session.
const HELD: usize = 10_000;

fn append(path: &Path, record: &Record) -> std::io::Result<()> {
    // One write per line, so two writers appending to one log never split a line.
    let line = Value::Object(record.clone()).to_string() + "\n";
    private_file()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(line.as_bytes()))
}

/// One session's log file as a sink writes it: records that cannot be written
/// wait here, in order, and go first once a write works; `alert` hears it once
/// when writes start failing and once when they work again.
struct Journal {
    session: String,
    path: PathBuf,
    held: VecDeque<Record>,
    failing: bool,
    dropped: usize,
    alert: LogAlert,
}

impl Journal {
    fn new(session: String, path: PathBuf, alert: LogAlert) -> Self {
        Self {
            session,
            path,
            held: VecDeque::new(),
            failing: false,
            dropped: 0,
            alert,
        }
    }

    fn write(&mut self, record: Record) {
        if self.held.len() < HELD {
            self.held.push_back(record);
        } else {
            self.dropped += 1;
        }
        while let Some(next) = self.held.front() {
            if let Err(error) = append(&self.path, next) {
                if !self.failing {
                    self.failing = true;
                    (self.alert)(LogHealth::Unsaved {
                        session: self.session.clone(),
                        path: self.path.display().to_string(),
                        detail: error.to_string(),
                    });
                }
                return;
            }
            self.held.pop_front();
        }
        if self.failing {
            self.failing = false;
            (self.alert)(LogHealth::Saved {
                session: self.session.clone(),
                path: self.path.display().to_string(),
                dropped: std::mem::take(&mut self.dropped),
            });
        }
    }
}

/// A new file for `stem` in `directory`: `<stem>.jsonl`, or `<stem>-2.jsonl` and on
/// when another session already holds that name, so no two sessions share a log.
fn claim(directory: &Path, stem: &str) -> PathBuf {
    let mut n = 1;
    loop {
        let path = match n {
            1 => directory.join(format!("{stem}.jsonl")),
            _ => directory.join(format!("{stem}-{n}.jsonl")),
        };
        match private_file().write(true).create_new(true).open(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            _ => return path,
        }
    }
}

/// A file's records; an unreadable one gives none of them.
fn records(path: &Path) -> Vec<Record> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str::<Record>)
        .collect::<Result<_, _>>()
        .unwrap_or_default()
}

pub struct SessionFiles {
    directory: PathBuf,
    index: Shared,
}

impl SessionFiles {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            index: Shared::default(),
        }
    }

    /// Newest first: names start with the start time.
    fn paths(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = fs::read_dir(&self.directory)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok().map(|e| e.path()))
                    .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
                    .collect()
            })
            .unwrap_or_default();
        paths.sort();
        paths.reverse();
        paths
    }

    /// The log of `session_id`: the one the index names, when its first record
    /// says so, else the first log whose first record does.
    fn path_of(&self, session_id: &str) -> Option<PathBuf> {
        let named = indexed(&self.index, &self.directory, |logs| {
            logs.iter()
                .find(|(_, log)| log.id == session_id)
                .map(|(name, _)| self.directory.join(name))
        });
        let holds = |path: &PathBuf| head_id(path).is_some_and(|id| id == session_id);
        named
            .filter(holds)
            .or_else(|| self.paths().into_iter().find(holds))
    }

    /// Every stored session's records, newest session first.
    #[cfg(test)]
    pub fn all(&self) -> Vec<Vec<Record>> {
        self.paths()
            .iter()
            .filter(|path| fs::metadata(path).is_ok_and(|m| m.len() > 0))
            .map(|path| records(path))
            .collect()
    }

    /// Settle sessions a daemon that stopped left open: a recording one is paused,
    /// an interrupted import ends with what it had transcribed. Either stops at its
    /// last line or state, so the time the daemon was down does not count as run.
    pub fn pause_unfinished(&self) {
        for path in self.paths() {
            let records = records(&path);
            let last_state = records.iter().rev().find(|r| r["type"] == "state");
            let settled = match last_state.and_then(|r| r["state"].as_str()) {
                Some(RECORDING) => PAUSED,
                Some(IMPORTING) => ENDED,
                _ => continue,
            };
            let at = records
                .iter()
                .filter(|r| r["type"] == "speech" || r["type"] == "state")
                .filter_map(|r| r["at"].as_f64())
                .fold(0.0, f64::max);
            let record = json!({"type": "state", "state": settled, "at": at});
            if let Err(error) = append(&path, record.as_object().expect("an object")) {
                eprintln!("eco: cannot write {}: {error}", path.display());
            }
        }
    }
}

impl SessionLog for SessionFiles {
    fn writer(&self, alert: LogAlert) -> RecordSink {
        let directory = self.directory.clone();
        let index = Arc::clone(&self.index);
        let mut journal: Option<Journal> = None;
        Box::new(move |record: Record| {
            let journal = journal.get_or_insert_with(|| {
                let started = record
                    .get("started_at")
                    .and_then(Value::as_f64)
                    .unwrap_or_else(now);
                let stamp = Local
                    .timestamp_opt(started as i64, 0)
                    .single()
                    .unwrap_or_else(Local::now);
                // Named by its title, or by its kind when it has none.
                let name = ["title", "kind"]
                    .iter()
                    .filter_map(|key| record.get(*key).and_then(Value::as_str))
                    .map(slug)
                    .find(|name| !name.is_empty())
                    .unwrap_or_else(|| "session".into());
                let _ = private_dir(&directory);
                let claimed = claim(
                    &directory,
                    &format!("{}-{name}", stamp.format("%Y-%m-%d-%H%M%S")),
                );
                // The index names its log at once; its entry is made when asked for.
                if let (Some(name), Some(id)) =
                    (name_of(&claimed), record.get("id").and_then(Value::as_str))
                {
                    let log = IndexedLog {
                        id: id.into(),
                        ..IndexedLog::default()
                    };
                    indexed(&index, &directory, |logs| logs.insert(name, log));
                }
                let id = record.get("id").and_then(Value::as_str).unwrap_or_default();
                Journal::new(id.into(), claimed, alert.clone())
            });
            journal.write(record);
        })
    }

    fn index(
        &self,
        entry: &dyn Fn(&[Record]) -> Value,
        fresh: &dyn Fn(&Value) -> bool,
    ) -> Vec<Value> {
        indexed(&self.index, &self.directory, |logs| {
            let mut kept = Logs::new();
            let mut entries = Vec::new();
            let mut changed = false;
            // Measured before it is read: a log that grows meanwhile is read again next time.
            let measured = self.paths().into_iter().filter_map(|path| {
                let (bytes, modified) = stamp(&path)?;
                Some((name_of(&path)?, bytes, modified, path))
            });
            for (name, bytes, modified, path) in measured.filter(|(_, bytes, ..)| *bytes > 0) {
                let log = match logs.remove(&name) {
                    Some(log)
                        if (log.bytes, log.modified) == (bytes, modified)
                            && log.entry.as_ref().is_some_and(fresh) =>
                    {
                        log
                    }
                    _ => {
                        changed = true;
                        let records = records(&path);
                        let id = records
                            .first()
                            .and_then(|head| head.get("id"))
                            .and_then(Value::as_str);
                        IndexedLog {
                            id: id.unwrap_or_default().into(),
                            bytes,
                            modified,
                            entry: Some(entry(&records)),
                        }
                    }
                };
                entries.extend(log.entry.clone());
                kept.insert(name, log);
            }
            // A log no longer there leaves the index.
            changed |= !logs.is_empty();
            *logs = kept;
            if changed {
                save(&self.directory, logs);
            }
            entries
        })
    }

    fn read(&self, session_id: &str) -> Option<Vec<Record>> {
        self.path_of(session_id).map(|path| records(&path))
    }

    fn append_to(&self, session_id: &str, alert: LogAlert) -> Option<RecordSink> {
        let mut journal = Journal::new(session_id.into(), self.path_of(session_id)?, alert);
        Some(Box::new(move |record: Record| journal.write(record)))
    }

    fn storage(&self, session_id: &str) -> Option<SessionStorage> {
        let path = self.path_of(session_id)?;
        let bytes = fs::metadata(&path).ok()?.len();
        Some(SessionStorage {
            path: crate::paths::real(&path).ok()?.display().to_string(),
            bytes,
        })
    }

    fn delete(&self, session_id: &str) -> Result<bool, StoreError> {
        let Some(path) = self.path_of(session_id) else {
            return Ok(false);
        };
        fs::remove_file(&path).map_err(|error| StoreError(error.to_string()))?;
        indexed(&self.index, &self.directory, |logs| {
            if let Some(name) = name_of(&path)
                && logs.remove(&name).is_some()
            {
                save(&self.directory, logs);
            }
        });
        Ok(true)
    }
}

/// --no-save: sessions live only in memory, and none is listed.
pub struct NoSessionFiles;

impl SessionLog for NoSessionFiles {
    fn writer(&self, _: LogAlert) -> RecordSink {
        Box::new(|_| {})
    }

    fn index(&self, _: &dyn Fn(&[Record]) -> Value, _: &dyn Fn(&Value) -> bool) -> Vec<Value> {
        Vec::new()
    }

    fn read(&self, _: &str) -> Option<Vec<Record>> {
        None
    }

    fn append_to(&self, _: &str, _: LogAlert) -> Option<RecordSink> {
        None
    }

    fn storage(&self, _: &str) -> Option<SessionStorage> {
        None
    }

    fn delete(&self, _: &str) -> Result<bool, StoreError> {
        Ok(false)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An alert nobody hears.
    pub(crate) fn quiet() -> LogAlert {
        Arc::new(|_| {})
    }
    use crate::domain::session::{ENDED, IMPORT, LIVE, Session, now};

    impl SessionFiles {
        fn with_index<R>(&self, with: impl FnOnce(&mut Logs) -> R) -> R {
            indexed(&self.index, &self.directory, with)
        }
    }

    /// Who may read, write or enter `path`.
    #[cfg(unix)]
    pub(crate) fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn names(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(directory)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn slugs_drop_accents_and_symbols() {
        assert_eq!(slug("Entrevista técnica: Acme"), "entrevista-tecnica-acme");
        assert_eq!(slug("  "), "");
        assert_eq!(slug("日本語"), "");
        assert_eq!(slug(&"a".repeat(60)).len(), 48);
    }

    #[test]
    fn a_line_comes_back_at_the_very_time_it_was_heard() {
        let directory = tempfile::tempdir().unwrap();
        let files = SessionFiles::new(directory.path().into());
        let mut session = Session::begin("", "meeting", LIVE, "pt", files.writer(quiet()));
        // A time the default float parse reads one step off.
        let at = 1792034916.0726755;
        session.hear_at("Recrutador", "Oi.", at);
        let records = files.read(&session.id).unwrap();
        let heard = records.iter().find(|r| r["type"] == "speech").unwrap();
        assert_eq!(heard["at"].as_f64(), Some(at));
    }

    #[test]
    fn files_name_sessions_and_pause_the_unfinished() {
        let directory = tempfile::tempdir().unwrap();
        let files = SessionFiles::new(directory.path().into());
        let mut open = Session::begin(
            "Entrevista técnica: Acme",
            "meeting",
            LIVE,
            "pt",
            files.writer(quiet()),
        );
        open.hear_at("Recrutador", "Oi.", now());
        let mut closed = Session::begin("", "idea", LIVE, "en", files.writer(quiet()));
        closed.set_state(ENDED);
        let imported = Session::begin("Palestra", "other", IMPORT, "pt", files.writer(quiet()));
        let names = names(directory.path());
        assert!(
            names
                .iter()
                .any(|n| n.ends_with("-entrevista-tecnica-acme.jsonl")),
            "{names:?}"
        );
        assert!(
            names.iter().any(|n| n.ends_with("-idea.jsonl")),
            "{names:?}"
        );

        files.pause_unfinished();
        let state_of = |id: &str| {
            let records = files.read(id).unwrap();
            records.iter().rev().find(|r| r["type"] == "state").unwrap()["state"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(state_of(&open.id), "paused");
        // The time the daemon was down does not count as run.
        let heard = files.read(&open.id).unwrap();
        assert_eq!(
            heard.last().unwrap()["at"],
            heard.iter().rev().find(|r| r["type"] == "speech").unwrap()["at"]
        );
        assert_eq!(state_of(&closed.id), "ended");
        assert_eq!(state_of(&imported.id), "ended");
    }

    #[cfg(unix)]
    #[test]
    fn a_session_file_that_cannot_be_written_is_left_as_it_was() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let files = SessionFiles::new(directory.path().into());
        let mut open = Session::begin("Call", "meeting", LIVE, "pt", files.writer(quiet()));
        open.hear_at("Eles", "Oi.", now());
        let path = fs::read_dir(directory.path())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        files.pause_unfinished();
        let records = files.read(&open.id).unwrap();
        let state = records.iter().rev().find(|r| r["type"] == "state").unwrap();
        assert_eq!(state["state"], "recording");
    }

    #[test]
    fn same_title_sessions_in_one_second_keep_their_own_files() {
        let directory = tempfile::tempdir().unwrap();
        let files = SessionFiles::new(directory.path().into());
        for id in ["a", "b", "c"] {
            let mut sink = files.writer(quiet());
            let head = json!({"type": "session", "id": id, "title": "Call", "started_at": 1.0e9});
            sink(head.as_object().unwrap().clone());
            let line = json!({"type": "speech", "text": id, "at": 1.0e9});
            sink(line.as_object().unwrap().clone());
        }
        let names = names(directory.path());
        assert_eq!(names.len(), 3, "{names:?}");
        for id in ["a", "b", "c"] {
            let records = files.read(id).unwrap();
            assert_eq!(records.len(), 2);
            assert_eq!(records[1]["text"], id);
        }
    }

    #[test]
    fn files_list_read_and_continue_sessions() {
        let directory = tempfile::tempdir().unwrap();
        let files = SessionFiles::new(directory.path().into());
        let mut first = Session::begin("Primeira", "meeting", LIVE, "pt", files.writer(quiet()));
        first.set_state(PAUSED);
        let second = Session::begin("Segunda", "meeting", LIVE, "en", files.writer(quiet()));
        let titles: Vec<String> = files
            .all()
            .iter()
            .map(|r| r[0]["title"].as_str().unwrap().into())
            .collect();
        assert_eq!(titles.len(), 2);
        assert!(titles.contains(&"Primeira".into()) && titles.contains(&"Segunda".into()));

        assert_eq!(files.read(&first.id).unwrap()[0]["title"], "Primeira");
        let mut sink = files.append_to(&first.id, quiet()).unwrap();
        sink(
            json!({"type": "state", "state": "recording", "at": 1.0})
                .as_object()
                .unwrap()
                .clone(),
        );
        assert_eq!(
            files.read(&first.id).unwrap().last().unwrap()["state"],
            "recording"
        );
        assert!(files.read("nope").is_none());
        assert!(files.append_to("nope", quiet()).is_none());
        assert_eq!(files.read(&second.id).unwrap()[0]["language"], "en");
    }

    #[test]
    fn storage_reports_the_real_file_and_delete_removes_it() {
        let directory = tempfile::tempdir().unwrap();
        let sessions = directory.path().join("sessions");
        let files = SessionFiles::new(sessions.clone());
        let mut session = Session::begin("Call", "meeting", LIVE, "pt", files.writer(quiet()));
        session.hear_at("Eles", "Hello", now());
        let storage = files.storage(&session.id).unwrap();
        let real = crate::paths::real(directory.path())
            .unwrap()
            .join("sessions");
        assert!(storage.path.starts_with(real.to_str().unwrap()));
        #[cfg(unix)]
        {
            assert_eq!(mode(&sessions), 0o700);
            assert_eq!(mode(Path::new(&storage.path)), 0o600);
        }
        assert_eq!(storage.bytes, fs::metadata(&storage.path).unwrap().len());
        assert!(storage.bytes > 0);
        assert!(files.delete(&session.id).unwrap());
        assert!(!Path::new(&storage.path).exists());
        assert!(!files.delete(&session.id).unwrap());
    }

    #[test]
    fn an_unreadable_file_is_listed_empty() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("2026-01-01-000000-quebrada.jsonl"),
            "{not json\n",
        )
        .unwrap();
        fs::write(directory.path().join("2026-01-02-000000-vazia.jsonl"), "").unwrap();
        fs::write(
            directory.path().join("2026-01-03-000000-binaria.jsonl"),
            [0xff, 0xfe],
        )
        .unwrap();
        let all = SessionFiles::new(directory.path().into()).all();
        assert_eq!(all, vec![Vec::<Record>::new(); 2]);
    }

    #[test]
    fn no_session_files_keeps_nothing() {
        let mut sink = NoSessionFiles.writer(quiet());
        sink(Record::new());
        let entry = |_: &[Record]| json!({});
        assert!(NoSessionFiles.index(&entry, &|_| true).is_empty());
        assert!(NoSessionFiles.read("x").is_none());
        assert!(
            NoSessionFiles.append_to("x", quiet()).is_none()
                && NoSessionFiles.storage("x").is_none()
        );
        assert!(!NoSessionFiles.delete("x").unwrap());
    }

    #[test]
    fn a_directory_that_cannot_be_made_loses_the_session_without_failing() {
        let directory = tempfile::tempdir().unwrap();
        let blocked = directory.path().join("sessions");
        fs::write(&blocked, "a file where the directory goes").unwrap();
        let files = SessionFiles::new(blocked);
        let mut sink = files.writer(quiet());
        sink(
            json!({"type": "session", "id": "s"})
                .as_object()
                .unwrap()
                .clone(),
        );
        assert!(files.all().is_empty());
    }

    /// The index of `files`, each entry the log's id and how many records it
    /// had, and how many entries were made from a log for it.
    fn listed(files: &SessionFiles, fresh: &dyn Fn(&Value) -> bool) -> (Vec<Value>, usize) {
        let made = std::cell::Cell::new(0);
        let entry = |records: &[Record]| {
            made.set(made.get() + 1);
            json!({"id": records.first().map(|head| head["id"].clone()), "records": records.len()})
        };
        let entries = files.index(&entry, fresh);
        (entries, made.get())
    }

    fn always(_: &Value) -> bool {
        true
    }

    /// What the index file says of each log, by the session's id.
    fn on_disk(directory: &Path) -> BTreeMap<String, Value> {
        let text = fs::read_to_string(directory.join(INDEX)).unwrap();
        let logs: Logs = serde_json::from_str(&text).unwrap();
        logs.into_values()
            .map(|log| (log.id, log.entry.unwrap_or_default()))
            .collect()
    }

    #[test]
    fn the_index_follows_every_write_edit_import_and_delete() {
        let directory = tempfile::tempdir().unwrap();
        let files = SessionFiles::new(directory.path().into());
        let mut live = Session::begin("Call", "meeting", LIVE, "pt", files.writer(quiet()));
        // Named from the first write: found without reading the other logs.
        assert_eq!(
            files.with_index(|logs| logs.values().map(|log| log.id.clone()).collect::<Vec<_>>()),
            [live.id.clone()]
        );
        let imported = Session::begin("Talk", "other", IMPORT, "en", files.writer(quiet()));
        let (entries, made) = listed(&files, &always);
        assert_eq!(made, 2);
        assert_eq!(entries.len(), 2);
        #[cfg(unix)]
        assert_eq!(mode(&directory.path().join(INDEX)), 0o600);
        let disk = on_disk(directory.path());
        assert_eq!(disk[&imported.id]["records"], 2, "an import is indexed");
        assert_eq!(disk[&live.id]["records"], 2);

        // Unchanged, nothing is read again.
        assert_eq!(listed(&files, &always).1, 0);
        // A line heard: that log alone is read again.
        live.hear_at("Eles", "Oi.", now());
        let (entries, made) = listed(&files, &always);
        assert_eq!(made, 1);
        assert!(entries.contains(&json!({"id": live.id, "records": 3})));
        // An edit appended to a stored log, the same.
        let mut sink = files.append_to(&imported.id, quiet()).unwrap();
        sink(
            json!({"type": "meta", "title": "Palestra", "kind": "other", "at": 1.0})
                .as_object()
                .unwrap()
                .clone(),
        );
        assert_eq!(listed(&files, &always).1, 1);
        assert_eq!(on_disk(directory.path())[&imported.id]["records"], 3);

        assert!(files.delete(&imported.id).unwrap());
        assert!(!on_disk(directory.path()).contains_key(&imported.id));
        let (entries, made) = listed(&files, &always);
        assert_eq!((entries.len(), made), (1, 0));
    }

    #[test]
    fn a_log_whose_first_record_has_no_id_is_kept_and_indexed() {
        let directory = tempfile::tempdir().unwrap();
        let files = SessionFiles::new(directory.path().into());
        let mut sink = files.writer(quiet());
        sink(
            json!({"type": "note", "text": "Oi"})
                .as_object()
                .unwrap()
                .clone(),
        );
        // Not named in the index when written; read when asked for, under no id.
        assert!(files.with_index(|logs| logs.is_empty()));
        let entry = |records: &[Record]| json!(records.len());
        assert_eq!(files.index(&entry, &always), [json!(1)]);
        assert_eq!(on_disk(directory.path()).keys().collect::<Vec<_>>(), [""]);
    }

    #[test]
    fn a_missing_or_broken_index_is_rebuilt_and_a_kept_one_is_read() {
        let directory = tempfile::tempdir().unwrap();
        let files = SessionFiles::new(directory.path().into());
        for title in ["One", "Two"] {
            Session::begin(title, "meeting", LIVE, "pt", files.writer(quiet()));
        }
        let (entries, made) = listed(&files, &always);
        assert_eq!(made, 2);
        // Another daemon reads the index its file keeps.
        let again = SessionFiles::new(directory.path().into());
        assert_eq!(listed(&again, &always), (entries.clone(), 0));
        fs::remove_file(directory.path().join(INDEX)).unwrap();
        let rebuilt = SessionFiles::new(directory.path().into());
        assert_eq!(listed(&rebuilt, &always), (entries.clone(), 2));
        assert_eq!(on_disk(directory.path()).len(), 2, "written again");
        fs::write(directory.path().join(INDEX), "{half").unwrap();
        let broken = SessionFiles::new(directory.path().into());
        assert_eq!(listed(&broken, &always), (entries, 2));
    }

    #[test]
    fn a_log_modified_since_or_an_entry_no_longer_fresh_is_read_again() {
        let directory = tempfile::tempdir().unwrap();
        let files = SessionFiles::new(directory.path().into());
        let kept = Session::begin("Kept", "meeting", LIVE, "pt", files.writer(quiet()));
        let touched = Session::begin("Touched", "meeting", LIVE, "pt", files.writer(quiet()));
        fs::write(
            directory.path().join("2026-01-01-000000-quebrada.jsonl"),
            "{not json\n",
        )
        .unwrap();
        let (entries, made) = listed(&files, &always);
        assert_eq!(made, 3);
        assert!(
            entries.contains(&json!({"id": null, "records": 0})),
            "an unreadable log has its entry too"
        );
        assert_eq!(listed(&files, &always).1, 0);

        // The same bytes, modified at another time.
        let path = files.path_of(&touched.id).unwrap();
        let earlier = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(earlier)
            .unwrap();
        assert_eq!(listed(&files, &always).1, 1);

        // An entry the domain no longer takes as it is.
        let stale = |entry: &Value| entry["id"] != kept.id.as_str();
        assert_eq!(listed(&files, &stale).1, 1);
    }

    #[test]
    fn an_index_that_cannot_be_written_still_lists_the_sessions() {
        let directory = tempfile::tempdir().unwrap();
        let files = SessionFiles::new(directory.path().into());
        Session::begin("One", "meeting", LIVE, "pt", files.writer(quiet()));
        fs::create_dir(directory.path().join(format!("{INDEX}.partial"))).unwrap();
        let (entries, made) = listed(&files, &always);
        assert_eq!((entries.len(), made), (1, 1));
        assert!(!directory.path().join(INDEX).exists());
        // Held in memory, it is not read again.
        assert_eq!(listed(&files, &always).1, 0);
    }

    #[test]
    fn a_session_the_index_misnames_is_still_found() {
        let directory = tempfile::tempdir().unwrap();
        let files = SessionFiles::new(directory.path().into());
        let first = Session::begin("One", "meeting", LIVE, "pt", files.writer(quiet()));
        let second = Session::begin("Two", "meeting", LIVE, "pt", files.writer(quiet()));
        listed(&files, &always);
        // The index says each log holds the other session.
        files.with_index(|logs| {
            for log in logs.values_mut() {
                log.id = if log.id == first.id {
                    second.id.clone()
                } else {
                    first.id.clone()
                };
            }
        });
        assert_eq!(files.read(&first.id).unwrap()[0]["title"], "One");
        assert_eq!(files.read(&second.id).unwrap()[0]["title"], "Two");
    }

    /// A journal whose file cannot be created (its directory is missing), the
    /// alerts it told, and the directory to make for writes to work.
    fn failing_journal() -> (Journal, Arc<Mutex<Vec<LogHealth>>>, PathBuf) {
        let directory = tempfile::tempdir().unwrap().keep();
        let told = Arc::new(Mutex::new(Vec::new()));
        let hear = Arc::clone(&told);
        let alert: LogAlert = Arc::new(move |health| hear.lock().unwrap().push(health));
        let journal = Journal::new("s1".into(), directory.join("gone/s1.jsonl"), alert);
        (journal, told, directory)
    }

    fn numbered(n: usize) -> Record {
        json!({"type": "speech", "n": n})
            .as_object()
            .unwrap()
            .clone()
    }

    #[test]
    fn a_log_that_cannot_be_written_tells_once_then_once_when_it_can() {
        let (mut journal, told, directory) = failing_journal();
        for n in 0..3 {
            journal.write(numbered(n));
        }
        let told_now = told.lock().unwrap().clone();
        assert_eq!(told_now.len(), 1);
        assert!(
            matches!(&told_now[0], LogHealth::Unsaved { session, path, .. }
            if session == "s1" && path.ends_with("gone/s1.jsonl"))
        );

        fs::create_dir(directory.join("gone")).unwrap();
        journal.write(numbered(3));
        journal.write(numbered(4));
        let told_now = told.lock().unwrap().clone();
        assert_eq!(told_now.len(), 2);
        assert!(matches!(&told_now[1], LogHealth::Saved { session, .. } if session == "s1"));

        let written: Vec<_> = records(&directory.join("gone/s1.jsonl"))
            .iter()
            .map(|record| record["n"].as_u64().unwrap())
            .collect();
        assert_eq!(written, [0, 1, 2, 3, 4]);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_log_failing_again_after_it_recovered_tells_again() {
        let (mut journal, told, directory) = failing_journal();
        journal.write(numbered(0));
        fs::create_dir(directory.join("gone")).unwrap();
        journal.write(numbered(1));
        fs::remove_dir_all(directory.join("gone")).unwrap();
        journal.write(numbered(2));
        assert_eq!(told.lock().unwrap().len(), 3);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn records_held_while_writes_fail_are_bounded_to_the_first_ones() {
        let (mut journal, told, directory) = failing_journal();
        for n in 0..HELD + 5 {
            journal.write(numbered(n));
        }
        fs::create_dir(directory.join("gone")).unwrap();
        journal.write(numbered(HELD + 5));
        let written = records(&directory.join("gone/s1.jsonl"));
        assert_eq!(written.len(), HELD);
        assert_eq!(written[0]["n"], 0);
        assert_eq!(written[HELD - 1]["n"], HELD - 1);
        assert!(matches!(
            told.lock().unwrap().last(),
            Some(LogHealth::Saved { dropped: 6, .. })
        ));
        fs::remove_dir_all(directory).unwrap();
    }
}
