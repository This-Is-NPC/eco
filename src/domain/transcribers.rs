//! The transcribers running on the inputs: one per input, model and language
//! some recording session listens with, shared by every session that does.

use std::collections::{BTreeMap, BTreeSet};

use futures::future::BoxFuture;
use tokio::task::{AbortHandle, JoinSet};

/// What a recording session listens with: a transcription model, by name, and
/// the language it transcribes in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Listening {
    pub model: String,
    pub language: String,
}

/// The transcribers of a pipeline's inputs, started and stopped as what the
/// recording sessions listen with changes.
pub struct Transcribers {
    inputs: Vec<String>,
    running: BTreeMap<(String, Listening), AbortHandle>,
    tasks: JoinSet<()>,
}

impl Transcribers {
    pub fn new(inputs: Vec<String>) -> Self {
        Self {
            inputs,
            running: BTreeMap::new(),
            tasks: JoinSet::new(),
        }
    }

    /// Run a transcriber on every input for each of `wanted`: `start` makes the
    /// ones missing, and the ones no session listens with any more stop.
    pub fn follow(
        &mut self,
        wanted: &BTreeSet<Listening>,
        start: &mut dyn FnMut(&str, &Listening) -> BoxFuture<'static, ()>,
    ) {
        self.running.retain(|(_, listening), task| {
            let kept = wanted.contains(listening);
            if !kept {
                task.abort();
            }
            kept
        });
        for listening in wanted {
            for input in &self.inputs {
                let key = (input.clone(), listening.clone());
                if !self.running.contains_key(&key) {
                    let task = self.tasks.spawn(start(input, listening));
                    self.running.insert(key, task);
                }
            }
        }
    }

    /// Wait for every transcriber to end on its own, once their captures ended.
    pub async fn finish(&mut self) {
        while self.tasks.join_next().await.is_some() {}
        self.running.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use futures::FutureExt;

    use super::*;

    fn listening(model: &str, language: &str) -> Listening {
        Listening {
            model: model.into(),
            language: language.into(),
        }
    }

    type Log = Arc<Mutex<Vec<String>>>;

    /// Notes when the transcriber holding it stops.
    struct Stops(Log, String);

    impl Drop for Stops {
        fn drop(&mut self) {
            self.0.lock().unwrap().push(format!("stopped {}", self.1));
        }
    }

    /// Transcribers that run until stopped, each noted as it starts and stops.
    fn pending(log: &Log) -> impl FnMut(&str, &Listening) -> BoxFuture<'static, ()> {
        let log = Arc::clone(log);
        move |input, listening| {
            let name = format!("{input} {} {}", listening.model, listening.language);
            log.lock().unwrap().push(format!("started {name}"));
            let stops = Stops(Arc::clone(&log), name);
            async move {
                let _stops = stops;
                std::future::pending::<()>().await;
            }
            .boxed()
        }
    }

    #[tokio::test]
    async fn sessions_listening_alike_share_one_transcriber_per_input() {
        let log = Log::default();
        let mut transcribers = Transcribers::new(vec!["mic".into(), "speakers".into()]);
        // Two sessions listening alike ask for one listening.
        let wanted = BTreeSet::from([listening("deepgram", "pt"), listening("deepgram", "pt")]);
        transcribers.follow(&wanted, &mut pending(&log));
        transcribers.follow(&wanted, &mut pending(&log));
        assert_eq!(
            *log.lock().unwrap(),
            ["started mic deepgram pt", "started speakers deepgram pt"]
        );
    }

    #[tokio::test]
    async fn two_transcribers_run_side_by_side_on_one_input() {
        let log = Log::default();
        let mut transcribers = Transcribers::new(vec!["mic".into()]);
        let wanted = BTreeSet::from([listening("deepgram", "pt"), listening("whisper", "en")]);
        transcribers.follow(&wanted, &mut pending(&log));
        tokio::task::yield_now().await;
        assert_eq!(
            *log.lock().unwrap(),
            ["started mic deepgram pt", "started mic whisper en"]
        );
    }

    #[tokio::test]
    async fn a_transcriber_stops_with_its_last_listener() {
        let log = Log::default();
        let mut transcribers = Transcribers::new(vec!["mic".into()]);
        let both = BTreeSet::from([listening("deepgram", "pt"), listening("whisper", "en")]);
        transcribers.follow(&both, &mut pending(&log));
        let whisper = BTreeSet::from([listening("whisper", "en")]);
        transcribers.follow(&whisper, &mut pending(&log));
        tokio::task::yield_now().await;
        assert_eq!(
            log.lock().unwrap().last().unwrap(),
            "stopped mic deepgram pt"
        );
        transcribers.follow(&BTreeSet::new(), &mut pending(&log));
        // Every one stopped: waiting for the rest ends at once.
        transcribers.finish().await;
        assert_eq!(
            log.lock().unwrap().last().unwrap(),
            "stopped mic whisper en"
        );
    }
}
