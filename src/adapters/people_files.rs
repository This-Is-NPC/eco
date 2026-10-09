//! The people registry on disk: `<dir>/<id>.json` per person and
//! `<dir>/voices/<session>.json` with the voices of a session's speakers.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::write_atomically;
use crate::domain::people::{Person, Voiceprint};
use crate::ports::{PeopleStore, StoreError};

#[derive(Serialize, Deserialize)]
struct Stored {
    id: String,
    name: String,
    #[serde(default)]
    color: String,
    voiceprints: Vec<StoredVoiceprint>,
}

#[derive(Serialize, Deserialize)]
struct StoredVoiceprint {
    session: String,
    label: String,
    voice: Vec<f32>,
}

pub struct PeopleFiles {
    dir: PathBuf,
}

impl PeopleFiles {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// A person's file, or `None` for an id that is not a plain name.
    fn file(&self, id: &str) -> Option<PathBuf> {
        plain(id).then(|| self.dir.join(format!("{id}.json")))
    }

    fn voices_file(&self, session_id: &str) -> Option<PathBuf> {
        plain(session_id).then(|| self.dir.join("voices").join(format!("{session_id}.json")))
    }
}

/// Ids are letters and digits, so none can point outside the directory.
fn plain(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric())
}

fn fail(error: impl std::fmt::Display) -> StoreError {
    StoreError(error.to_string())
}

/// Delete `path`; one already gone is fine.
fn remove(path: PathBuf) -> Result<(), StoreError> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(fail(e)),
        _ => Ok(()),
    }
}

impl PeopleStore for PeopleFiles {
    fn people(&self) -> Vec<Person> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut people: Vec<Person> = entries
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                (path.extension()? == "json").then_some(())?;
                let stored: Stored = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
                let voiceprints = stored.voiceprints.into_iter().map(|v| Voiceprint {
                    session: v.session,
                    label: v.label,
                    voice: v.voice,
                });
                Some(Person {
                    id: stored.id,
                    name: stored.name,
                    color: stored.color,
                    voiceprints: voiceprints.collect(),
                })
            })
            .collect();
        people.sort_by_key(|p| p.name.to_lowercase());
        people
    }

    fn save(&self, person: &Person) -> Result<(), StoreError> {
        let path = self.file(&person.id).ok_or_else(|| fail("bad person id"))?;
        let stored = Stored {
            id: person.id.clone(),
            name: person.name.clone(),
            color: person.color.clone(),
            voiceprints: person
                .voiceprints
                .iter()
                .map(|v| StoredVoiceprint {
                    session: v.session.clone(),
                    label: v.label.clone(),
                    voice: v.voice.clone(),
                })
                .collect(),
        };
        write_atomically(&path, &serde_json::to_vec(&stored).map_err(fail)?).map_err(fail)
    }

    fn forget(&self, person_id: &str) -> Result<(), StoreError> {
        remove(self.file(person_id).ok_or_else(|| fail("bad person id"))?)
    }

    fn voices(&self, session_id: &str) -> BTreeMap<String, Vec<f32>> {
        self.voices_file(session_id)
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn keep_voices(
        &self,
        session_id: &str,
        voices: &BTreeMap<String, Vec<f32>>,
    ) -> Result<(), StoreError> {
        let path = self
            .voices_file(session_id)
            .ok_or_else(|| fail("bad session id"))?;
        if voices.is_empty() {
            return remove(path);
        }
        write_atomically(&path, &serde_json::to_vec(voices).map_err(fail)?).map_err(fail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn people_and_voices_live_and_go_on_disk() {
        let directory = tempfile::tempdir().unwrap();
        let store = PeopleFiles::new(directory.path().join("people"));
        assert!(store.people().is_empty());
        let mut bruno = Person::new("bruno");
        bruno.color = "#ffb000".into();
        bruno.enroll("n1", "Speaker 2", vec![0.0, 1.0]);
        let ana = Person::new("Ana");
        store.save(&bruno).unwrap();
        store.save(&ana).unwrap();
        assert_eq!(store.people(), [ana.clone(), bruno.clone()]);

        let voices = BTreeMap::from([("Speaker 1".to_string(), vec![0.6, 0.8])]);
        store.keep_voices("n1", &voices).unwrap();
        assert_eq!(store.voices("n1"), voices);
        assert!(store.voices("n2").is_empty());

        let file = directory.path().join(format!("people/{}.json", ana.id));
        assert!(file.exists());
        store.forget(&ana.id).unwrap();
        assert!(!file.exists());
        assert_eq!(store.people(), [bruno]);
        store.keep_voices("n1", &BTreeMap::new()).unwrap();
        assert!(!directory.path().join("people/voices/n1.json").exists());
        assert!(store.forget("../etc").is_err());
    }
}
