//! Events the daemon sends to its clients, one JSON object per line.

use serde_json::{Map, Value, json};

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

/// A segment `who` said that the STT could not transcribe.
pub fn transcription_failed(who: &str, detail: &str) -> Event {
    error(
        "transcription.failed",
        format!("{who}: transcription failed: {detail}"),
        json!({"who": who, "detail": detail}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
