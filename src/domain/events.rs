//! Events the daemon sends to its clients, one JSON object per line.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};

use crate::domain::assistant::Emit;
use crate::domain::channel::Trouble;
use crate::ports::{LogAlert, LogHealth};

pub type Event = Value;

/// An error event: `code` and `params` let a client translate it; `message` is the
/// English text for the terminal and for clients without that translation.
pub fn error(code: &str, message: impl Into<String>, params: Value) -> Event {
    let params = if params.is_null() {
        Value::Object(Map::new())
    } else {
        params
    };
    json!({"type": "error", "code": code, "params": params, "message": message.into()})
}

/// A session log that cannot be written is an error with the path; one that can
/// be again is a `session_saved` event.
pub fn log_health(health: &LogHealth) -> Event {
    match health {
        LogHealth::Unsaved {
            session,
            path,
            detail,
        } => error(
            "session.unsaved",
            format!("cannot save session {session} to {path}: {detail}"),
            json!({"session": session, "path": path, "detail": detail}),
        ),
        LogHealth::Saved {
            session,
            path,
            dropped,
        } => json!({"type": "session_saved", "session": session, "path": path, "dropped": dropped}),
    }
}

/// An alert that tells the clients `emit` reaches.
pub fn log_alert(emit: &Emit) -> LogAlert {
    let emit = Arc::clone(emit);
    Arc::new(move |health| emit(log_health(&health)))
}

/// `who`'s trouble: speech lost is an error; a streaming transcription or the
/// source itself gone down carries the `code` a client translates and the
/// `detail` of the failure, and back, the seconds it was down and the seconds
/// of audio it dropped.
pub fn transcription(who: &str, trouble: &Trouble) -> Event {
    match trouble {
        Trouble::Failed(detail) => error(
            "transcription.failed",
            format!("{who}: transcription failed: {detail}"),
            json!({"who": who, "detail": detail}),
        ),
        Trouble::Down(detail) => json!({
            "type": "transcription", "who": who, "state": "down",
            "code": "stt.down", "detail": detail,
        }),
        Trouble::Unheard(detail) => json!({
            "type": "transcription", "who": who, "state": "down",
            "code": "capture.down", "detail": detail,
        }),
        Trouble::Back { down, dropped } => json!({
            "type": "transcription", "who": who, "state": "back",
            "down_s": down.as_secs_f64(), "dropped_s": dropped,
        }),
    }
}

/// `who`'s speech goes without a speaker while the diarizer is behind: `down`
/// until it catches up, then `back` with how many regions went without one.
pub fn diarization(who: &str, caught_up: Option<usize>) -> Event {
    match caught_up {
        None => json!({
            "type": "transcription", "who": who, "state": "down",
            "code": "diarization.behind", "detail": "",
        }),
        Some(count) => json!({
            "type": "transcription", "who": who, "state": "back",
            "code": "diarization.caught_up", "count": count,
        }),
    }
}

/// The sources whose streaming transcription is down now, each with the
/// `transcription` event that said so, for the clients that connect later.
#[derive(Clone, Default)]
pub struct Outages(Arc<Mutex<BTreeMap<String, Event>>>);

impl Outages {
    /// The event `trouble` makes for `who`, noted: down holds until back.
    pub fn note(&self, who: &str, trouble: &Trouble) -> Event {
        let event = transcription(who, trouble);
        let mut down = self.0.lock().expect("not poisoned");
        match trouble {
            Trouble::Down(_) | Trouble::Unheard(_) => {
                down.insert(who.to_string(), event.clone());
            }
            Trouble::Back { .. } => {
                down.remove(who);
            }
            Trouble::Failed(_) => {}
        }
        event
    }

    /// The event for `who`'s diarizer: behind holds until it catches up.
    pub fn diarizing(&self, who: &str, caught_up: Option<usize>) -> Event {
        let event = diarization(who, caught_up);
        let mut down = self.0.lock().expect("not poisoned");
        match caught_up {
            None => down.insert(diarizing(who), event.clone()),
            Some(_) => down.remove(&diarizing(who)),
        };
        event
    }

    /// The `transcription` down event of every source down now.
    pub fn events(&self) -> Vec<Event> {
        let down = self.0.lock().expect("not poisoned");
        down.values().cloned().collect()
    }

    /// A guard that forgets `who`'s outage once its transcriber or its capture
    /// stops, however it stops.
    pub fn until_stopped<'a>(&'a self, who: &'a str) -> Stopped<'a> {
        Stopped(self, who)
    }
}

/// The key of `who`'s diarizer in the outages, apart from its transcription.
fn diarizing(who: &str) -> String {
    format!("{who}\0diarization")
}

/// Forgets a source's outage when dropped.
pub struct Stopped<'a>(&'a Outages, &'a str);

impl Drop for Stopped<'_> {
    fn drop(&mut self) {
        let mut down = self.0.0.lock().expect("not poisoned");
        down.remove(self.1);
        down.remove(&diarizing(self.1));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_diarizer_behind_is_replayed_until_it_catches_up_apart_from_the_transcription() {
        let outages = Outages::default();
        let down = outages.note("Eles", &Trouble::Down("reset".into()));
        let behind = outages.diarizing("Eles", None);
        assert_eq!(
            behind.to_string(),
            r#"{"type":"transcription","who":"Eles","state":"down","code":"diarization.behind","detail":""}"#
        );
        assert_eq!(outages.events(), [down.clone(), behind]);
        assert_eq!(
            outages.diarizing("Eles", Some(3)).to_string(),
            r#"{"type":"transcription","who":"Eles","state":"back","code":"diarization.caught_up","count":3}"#
        );
        assert_eq!(outages.events(), [down]);
        outages.diarizing("Eles", None);
        drop(outages.until_stopped("Eles"));
        assert_eq!(outages.events(), [] as [Value; 0]);
    }

    #[test]
    fn a_transcription_reads_lost_as_an_error_down_with_its_code_and_back_with_its_seconds() {
        let failed = transcription("Eles", &Trouble::Failed("timed out".into()));
        assert_eq!(
            failed.to_string(),
            r#"{"type":"error","code":"transcription.failed","params":{"who":"Eles","detail":"timed out"},"message":"Eles: transcription failed: timed out"}"#
        );
        let down = transcription("Eles", &Trouble::Down("connection reset".into()));
        assert_eq!(
            down.to_string(),
            r#"{"type":"transcription","who":"Eles","state":"down","code":"stt.down","detail":"connection reset"}"#
        );
        let unheard = transcription("Eu", &Trouble::Unheard("not connected".into()));
        assert_eq!(
            unheard.to_string(),
            r#"{"type":"transcription","who":"Eu","state":"down","code":"capture.down","detail":"not connected"}"#
        );
        let back = Trouble::Back {
            down: Duration::from_millis(7500),
            dropped: 2.5,
        };
        assert_eq!(
            transcription("Eles", &back).to_string(),
            r#"{"type":"transcription","who":"Eles","state":"back","down_s":7.5,"dropped_s":2.5}"#
        );
    }

    #[test]
    fn speech_lost_is_told_but_not_kept_for_later_clients() {
        let outages = Outages::default();
        let event = outages.note("Eles", &Trouble::Failed("timed out".into()));
        assert_eq!(event["code"], "transcription.failed");
        assert_eq!(outages.events(), [] as [Value; 0]);
    }

    #[test]
    fn a_source_unheard_is_kept_for_later_clients_until_it_is_back() {
        let outages = Outages::default();
        let unheard = outages.note("Eu", &Trouble::Unheard("not connected".into()));
        assert_eq!(outages.events(), [unheard]);
        let back = Trouble::Back {
            down: Duration::from_secs(3),
            dropped: 0.0,
        };
        outages.note("Eu", &back);
        assert_eq!(outages.events(), [] as [Value; 0]);
    }

    #[test]
    fn errors_carry_code_params_and_message_in_order() {
        let event = error("action.unknown", "unknown action 'x'", json!({"name": "x"}));
        assert_eq!(
            event.to_string(),
            r#"{"type":"error","code":"action.unknown","params":{"name":"x"},"message":"unknown action 'x'"}"#
        );
        assert_eq!(
            error("session.none", "start a session first", Value::Null)["params"],
            json!({})
        );
    }

    #[test]
    fn a_log_that_cannot_be_written_is_an_error_with_its_path_and_a_saved_event_after() {
        let down = log_health(&LogHealth::Unsaved {
            session: "s1".into(),
            path: "/data/s1.jsonl".into(),
            detail: "No space left on device".into(),
        });
        assert_eq!(down["code"], "session.unsaved");
        assert_eq!(down["params"]["path"], "/data/s1.jsonl");
        assert_eq!(down["params"]["detail"], "No space left on device");
        let back = log_health(&LogHealth::Saved {
            session: "s1".into(),
            path: "/data/s1.jsonl".into(),
            dropped: 3,
        });
        assert_eq!(
            back,
            json!({"type": "session_saved", "session": "s1", "path": "/data/s1.jsonl", "dropped": 3})
        );
    }
}
