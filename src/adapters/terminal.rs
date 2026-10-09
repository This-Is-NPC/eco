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
}
