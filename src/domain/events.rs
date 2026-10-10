//! Events the daemon sends to its clients, one JSON object per line.

use serde_json::{Map, Value, json};

use crate::domain::channel::Trouble;

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

/// `who`'s trouble: speech lost is an error; a streaming transcription gone
/// down carries the `code` a client translates and the provider's `detail`,
/// and back, the seconds it was down and the seconds of audio it dropped.
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
        Trouble::Back { down, dropped } => json!({
            "type": "transcription", "who": who, "state": "back",
            "down_s": down.as_secs_f64(), "dropped_s": dropped,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

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
}
