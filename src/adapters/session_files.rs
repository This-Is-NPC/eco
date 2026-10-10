//! One append-only JSON Lines file per session, named by start time and title,
//! with `-2`, `-3`… when another session already holds that name.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{Local, TimeZone};
use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization;

use crate::config::{private_dir, private_file};
use crate::domain::session::{ENDED, IMPORTING, PAUSED, RECORDING, now};
use crate::ports::{Record, RecordSink, SessionLog, SessionStorage, StoreError};

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

fn append(path: &Path, record: &Record) {
    // One write per line, so two writers appending to one log never split a line.
    let line = Value::Object(record.clone()).to_string() + "\n";
    let written = private_file()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(line.as_bytes()));
    if let Err(error) = written {
        eprintln!("eco: cannot write {}: {error}", path.display());
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
}

impl SessionFiles {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
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

    fn path_of(&self, session_id: &str) -> Option<PathBuf> {
        self.paths().into_iter().find(|path| {
            let text = fs::read_to_string(path).unwrap_or_default();
            let head = text
                .lines()
                .next()
                .and_then(|line| serde_json::from_str::<Value>(line).ok());
            head.is_some_and(|head| head["id"] == session_id)
        })
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
            append(&path, record.as_object().expect("an object"));
        }
    }
}

impl SessionLog for SessionFiles {
    fn writer(&self) -> RecordSink {
        let directory = self.directory.clone();
        let path: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
        Box::new(move |record: Record| {
            let mut path = path.lock().expect("not poisoned");
            let target = path.get_or_insert_with(|| {
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
                claim(
                    &directory,
                    &format!("{}-{name}", stamp.format("%Y-%m-%d-%H%M%S")),
                )
            });
            append(target, &record);
        })
    }

    fn all(&self) -> Vec<Vec<Record>> {
        self.paths()
            .iter()
            .filter(|path| fs::metadata(path).is_ok_and(|m| m.len() > 0))
            .map(|path| records(path))
            .collect()
    }

    fn read(&self, session_id: &str) -> Option<Vec<Record>> {
        self.path_of(session_id).map(|path| records(&path))
    }

    fn append_to(&self, session_id: &str) -> Option<RecordSink> {
        let path = self.path_of(session_id)?;
        Some(Box::new(move |record: Record| append(&path, &record)))
    }

    fn storage(&self, session_id: &str) -> Option<SessionStorage> {
        let path = self.path_of(session_id)?;
        let bytes = fs::metadata(&path).ok()?.len();
        Some(SessionStorage {
            path: path.canonicalize().ok()?.display().to_string(),
            bytes,
        })
    }

    fn delete(&self, session_id: &str) -> Result<bool, StoreError> {
        let Some(path) = self.path_of(session_id) else {
            return Ok(false);
        };
        fs::remove_file(path).map_err(|error| StoreError(error.to_string()))?;
        Ok(true)
    }
}

/// --no-save: sessions live only in memory, and none is listed.
pub struct NoSessionFiles;

impl SessionLog for NoSessionFiles {
    fn writer(&self) -> RecordSink {
        Box::new(|_| {})
    }

    fn all(&self) -> Vec<Vec<Record>> {
        Vec::new()
    }

    fn read(&self, _: &str) -> Option<Vec<Record>> {
        None
    }

    fn append_to(&self, _: &str) -> Option<RecordSink> {
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
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::domain::session::{ENDED, IMPORT, LIVE, Session, now};

    /// Who may read, write or enter `path`.
    pub(crate) fn mode(path: &Path) -> u32 {
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
    fn files_name_sessions_and_pause_the_unfinished() {
        let directory = tempfile::tempdir().unwrap();
        let files = SessionFiles::new(directory.path().into());
        let mut open = Session::begin(
            "Entrevista técnica: Acme",
            "meeting",
            LIVE,
            "pt",
            files.writer(),
        );
        open.hear_at("Recrutador", "Oi.", now());
        let mut closed = Session::begin("", "idea", LIVE, "en", files.writer());
        closed.set_state(ENDED);
        let imported = Session::begin("Palestra", "other", IMPORT, "pt", files.writer());
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

    #[test]
    fn same_title_sessions_in_one_second_keep_their_own_files() {
        let directory = tempfile::tempdir().unwrap();
        let files = SessionFiles::new(directory.path().into());
        for id in ["a", "b", "c"] {
            let mut sink = files.writer();
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
        let mut first = Session::begin("Primeira", "meeting", LIVE, "pt", files.writer());
        first.set_state(PAUSED);
        let second = Session::begin("Segunda", "meeting", LIVE, "en", files.writer());
        let titles: Vec<String> = files
            .all()
            .iter()
            .map(|r| r[0]["title"].as_str().unwrap().into())
            .collect();
        assert_eq!(titles.len(), 2);
        assert!(titles.contains(&"Primeira".into()) && titles.contains(&"Segunda".into()));

        assert_eq!(files.read(&first.id).unwrap()[0]["title"], "Primeira");
        let mut sink = files.append_to(&first.id).unwrap();
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
        assert!(files.append_to("nope").is_none());
        assert_eq!(files.read(&second.id).unwrap()[0]["language"], "en");
    }

    #[test]
    fn storage_reports_the_real_file_and_delete_removes_it() {
        let directory = tempfile::tempdir().unwrap();
        let sessions = directory.path().join("sessions");
        let files = SessionFiles::new(sessions.clone());
        let mut session = Session::begin("Call", "meeting", LIVE, "pt", files.writer());
        session.hear_at("Eles", "Hello", now());
        let storage = files.storage(&session.id).unwrap();
        assert!(storage.path.starts_with(sessions.to_str().unwrap()));
        assert_eq!(mode(&sessions), 0o700);
        assert_eq!(mode(Path::new(&storage.path)), 0o600);
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
        let mut sink = NoSessionFiles.writer();
        sink(Record::new());
        assert!(NoSessionFiles.all().is_empty() && NoSessionFiles.read("x").is_none());
        assert!(NoSessionFiles.append_to("x").is_none() && NoSessionFiles.storage("x").is_none());
        assert!(!NoSessionFiles.delete("x").unwrap());
    }

    #[test]
    fn a_directory_that_cannot_be_made_loses_the_session_without_failing() {
        let directory = tempfile::tempdir().unwrap();
        let blocked = directory.path().join("sessions");
        fs::write(&blocked, "a file where the directory goes").unwrap();
        let files = SessionFiles::new(blocked);
        let mut sink = files.writer();
        sink(
            json!({"type": "session", "id": "s"})
                .as_object()
                .unwrap()
                .clone(),
        );
        assert!(files.all().is_empty());
    }
}
