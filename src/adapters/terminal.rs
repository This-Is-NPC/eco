//! Daemon events rendered in the terminal, suggestions streaming inline.

use std::io::{IsTerminal, Write};
use std::sync::Mutex;

use serde_json::Value;

use crate::domain::events::Event;

/// Prints events live to a terminal.
pub struct Terminal<W>(Mutex<W>);

/// A printer for `out` only when it is a terminal: under the user service stdout
/// is the journal, which must never hold transcript, note or answer text.
pub fn terminal<W: Write + IsTerminal>(out: W) -> Option<Terminal<W>> {
    out.is_terminal().then(|| Terminal(Mutex::new(out)))
}

impl<W: Write> Terminal<W> {
    pub fn print(&self, event: &Event) {
        let Some(line) = line(event) else { return };
        let mut out = self.0.lock().expect("not poisoned");
        let _ = out.write_all(line.as_bytes());
        let _ = out.flush();
    }
}

fn line(event: &Event) -> Option<String> {
    let text = |key: &str| event.get(key).and_then(Value::as_str).unwrap_or_default();
    let number = |key: &str| event.get(key).and_then(Value::as_u64);
    let line = match text("type") {
        "transcript" => format!(
            "[{}] {}  ({} ms)\n",
            text("who"),
            text("text"),
            number("latency_ms").unwrap_or(0)
        ),
        "note" => format!("[nota] {}\n", text("text")),
        "suggestion_start" => format!("\n── {} · {} ──\n", text("action"), text("model")),
        "suggestion_delta" => text("text").to_string(),
        "suggestion_end" => {
            let cache = match (number("cached_tokens"), number("prompt_tokens")) {
                (Some(cached), Some(prompt)) if prompt > 0 => {
                    format!(" · cache {cached}/{prompt} tokens")
                }
                _ => String::new(),
            };
            let (ttft, total) = (
                number("ttft_ms").unwrap_or(0),
                number("total_ms").unwrap_or(0),
            );
            format!("\n── 1º token {ttft} ms · total {total} ms{cache} ──\n\n")
        }
        "session" => match event.get("session") {
            Some(Value::Object(session)) => {
                let title = session
                    .get("title")
                    .and_then(Value::as_str)
                    .filter(|t| !t.is_empty())
                    .unwrap_or("(sem título)");
                let field =
                    |key: &str| session.get(key).and_then(Value::as_str).unwrap_or_default();
                format!("\n── {} {title}: {} ──\n\n", field("kind"), field("state"))
            }
            _ => "\n── nota encerrada ──\n\n".into(),
        },
        "suggestion_removed" => format!("\n── sugestão {} removida ──\n", text("id")),
        "note_removed" => format!("\n── nota {} removida ──\n", text("id")),
        "error" => format!("\n!! {}\n", text("message")),
        "transcription" => {
            let seconds = |key: &str| event.get(key).and_then(Value::as_f64).unwrap_or(0.0);
            match text("state") {
                "down" => format!(
                    "\n!! {}: transcription down: {}\n",
                    text("who"),
                    text("detail")
                ),
                _ => format!(
                    "\n── {}: transcription back after {:.0} s, {:.0} s of audio dropped ──\n",
                    text("who"),
                    seconds("down_s"),
                    seconds("dropped_s")
                ),
            }
        }
        _ => return None,
    };
    Some(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prints_nothing_when_out_is_not_a_terminal() {
        let journal = tempfile::tempfile().unwrap();
        assert!(terminal(journal).is_none());
    }

    #[test]
    fn a_transcription_down_and_back_reads_as_a_line_each() {
        let down = serde_json::json!({
            "type": "transcription", "who": "Eles", "state": "down",
            "code": "stt.down", "detail": "connection reset",
        });
        assert_eq!(
            line(&down).unwrap(),
            "\n!! Eles: transcription down: connection reset\n"
        );
        let back = serde_json::json!({
            "type": "transcription", "who": "Eles", "state": "back",
            "down_s": 331.0, "dropped_s": 30.976,
        });
        assert_eq!(
            line(&back).unwrap(),
            "\n── Eles: transcription back after 331 s, 31 s of audio dropped ──\n"
        );
    }

    #[test]
    fn each_event_prints_as_its_line_and_others_print_nothing() {
        use serde_json::json;

        let printed = Terminal(Mutex::new(Vec::new()));
        for event in [
            json!({"type": "transcript", "who": "Você", "text": "oi", "latency_ms": 120}),
            json!({"type": "transcript", "who": "Eles", "text": "olá"}),
            json!({"type": "note", "text": "ligar amanhã"}),
            json!({"type": "suggestion_start", "action": "resposta", "model": "m"}),
            json!({"type": "suggestion_delta", "text": "Diga "}),
            json!({"type": "suggestion_delta", "text": "sim."}),
            json!({"type": "suggestion_end", "ttft_ms": 300, "total_ms": 900,
                   "cached_tokens": 50, "prompt_tokens": 200}),
            json!({"type": "suggestion_end", "cached_tokens": 0, "prompt_tokens": 0}),
            json!({"type": "session", "session": {"kind": "meeting", "title": "", "state": "live"}}),
            json!({"type": "session", "session": {"kind": "idea", "title": "Plano", "state": "ended"}}),
            json!({"type": "session", "session": null}),
            json!({"type": "suggestion_removed", "id": "s3"}),
            json!({"type": "note_removed", "id": "n2"}),
            json!({"type": "error", "message": "no key"}),
            json!({"type": "levels"}),
        ] {
            printed.print(&event);
        }
        let printed = String::from_utf8(printed.0.into_inner().unwrap()).unwrap();
        assert_eq!(
            printed,
            "[Você] oi  (120 ms)\n\
             [Eles] olá  (0 ms)\n\
             [nota] ligar amanhã\n\
             \n── resposta · m ──\n\
             Diga sim.\
             \n── 1º token 300 ms · total 900 ms · cache 50/200 tokens ──\n\n\
             \n── 1º token 0 ms · total 0 ms ──\n\n\
             \n── meeting (sem título): live ──\n\n\
             \n── idea Plano: ended ──\n\n\
             \n── nota encerrada ──\n\n\
             \n── sugestão s3 removida ──\n\
             \n── nota n2 removida ──\n\
             \n!! no key\n"
        );
    }
}
