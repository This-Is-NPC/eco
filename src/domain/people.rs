//! People the user has named, recognised by voice across sessions: a name and the
//! voices heard as them (speaker embeddings, never audio).

use crate::domain::diarization::normalized;
use crate::domain::session::short_id;

/// How many voices a person keeps; the oldest goes first.
const MAX_VOICEPRINTS: usize = 16;
/// A voice this close to a person is guessed to be them, until the user
/// confirms or clears it. On AMI the closest wrong person scored 0.52 and the
/// right ones 0.86–0.97.
pub const GUESSING: f32 = 0.70;
/// People at least this close are offered when a speaker is named by hand.
pub const SUGGESTING: f32 = 0.40;

/// A voice heard as a person, and the session speaker it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Voiceprint {
    pub session: String,
    pub label: String,
    /// Unit length.
    pub voice: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Person {
    pub id: String,
    pub name: String,
    pub color: String,
    /// Oldest first.
    pub voiceprints: Vec<Voiceprint>,
}

/// How close a voice is to a person.
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub person: String,
    pub score: f32,
}

impl Person {
    pub fn new(name: &str) -> Self {
        Self {
            id: short_id(12),
            name: name.into(),
            color: String::new(),
            voiceprints: Vec::new(),
        }
    }

    /// The cosine between `voice` and the mean of this person's voices.
    pub fn similarity(&self, voice: &[f32]) -> f32 {
        let Some(first) = self.voiceprints.first() else {
            return 0.0;
        };
        let mut mean = vec![0f32; first.voice.len()];
        for print in &self.voiceprints {
            mean.iter_mut().zip(&print.voice).for_each(|(m, x)| *m += x);
        }
        let mean = normalized(mean);
        let norm = voice.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm == 0.0 {
            return 0.0;
        }
        mean.iter().zip(voice).map(|(m, x)| m * x).sum::<f32>() / norm
    }

    /// Keep how the speaker `label` of `session` sounds as this person.
    pub fn enroll(&mut self, session: &str, label: &str, voice: Vec<f32>) {
        self.unenroll(session, label);
        self.voiceprints.push(Voiceprint {
            session: session.into(),
            label: label.into(),
            voice: normalized(voice),
        });
        let extra = self.voiceprints.len().saturating_sub(MAX_VOICEPRINTS);
        self.voiceprints.drain(..extra);
    }

    /// Drop the voice taken from the speaker `label` of `session`.
    pub fn unenroll(&mut self, session: &str, label: &str) {
        self.voiceprints
            .retain(|print| (print.session.as_str(), print.label.as_str()) != (session, label));
    }

    /// Take in `other`'s voices: the two were the same person.
    pub fn absorb(&mut self, other: Person) {
        for print in other.voiceprints {
            self.enroll(&print.session, &print.label, print.voice);
        }
    }
}

/// Everyone with a voice, closest to `voice` first.
pub fn ranked(people: &[Person], voice: &[f32]) -> Vec<Match> {
    let mut matches: Vec<Match> = people
        .iter()
        .filter(|p| !p.voiceprints.is_empty())
        .map(|p| Match {
            person: p.id.clone(),
            score: p.similarity(voice),
        })
        .collect();
    matches.sort_by(|a, b| b.score.total_cmp(&a.score));
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(name: &str, voice: Vec<f32>) -> Person {
        let mut person = Person::new(name);
        person.enroll("n0", "Speaker 1", voice);
        person
    }

    #[test]
    fn people_are_ranked_by_voice() {
        let ana = person("Ana", vec![1.0, 0.0, 0.0]);
        let bruno = person("Bruno", vec![0.0, 2.0, 0.0]);
        let people = [ana.clone(), bruno.clone(), Person::new("Carla")];
        let ranking = ranked(&people, &[0.1, 0.9, 0.0]);
        assert_eq!(ranking.len(), 2);
        assert_eq!(ranking[0].person, bruno.id);
        assert!(ranking[0].score > 0.99 && ranking[1].score < 0.2);
        assert_eq!(bruno.voiceprints[0].voice, [0.0, 1.0, 0.0]);
        assert_eq!(ana.similarity(&[0.0, 0.0, 0.0]), 0.0);
    }

    #[test]
    fn voices_are_bounded_replaced_undone_and_merged() {
        let mut ana = person("Ana", vec![1.0, 0.0]);
        for index in 0..MAX_VOICEPRINTS + 3 {
            ana.enroll(&format!("n{}", index + 1), "Speaker 1", vec![0.0, 1.0]);
        }
        assert_eq!(ana.voiceprints.len(), MAX_VOICEPRINTS);
        assert!(ana.voiceprints.iter().all(|v| v.voice == [0.0, 1.0]));
        ana.enroll("n5", "Speaker 1", vec![1.0, 0.0]); // the same speaker again
        assert_eq!(ana.voiceprints.len(), MAX_VOICEPRINTS);
        ana.unenroll("n5", "Speaker 1");
        assert_eq!(ana.voiceprints.len(), MAX_VOICEPRINTS - 1);
        ana.absorb(person("Ana Paula", vec![1.0, 0.0]));
        assert_eq!(ana.voiceprints.last().unwrap().voice, [1.0, 0.0]);
        assert_eq!(ana.name, "Ana");
        assert_ne!(ana.id, Person::new("Ana").id);
    }
}
