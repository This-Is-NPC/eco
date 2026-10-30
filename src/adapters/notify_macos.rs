//! macOS: notifications through `osascript`, the system's AppleScript runner.
//! macOS has no urgency: a critical notification also plays the system's
//! alert sound.

use std::process::{Command, Stdio};

use crate::adapters::notify_program;
use crate::ports::{Notifier, Urgency};

pub struct Osascript;

/// `osascript` showing `title` and `body`, handed as arguments so no text
/// is read as AppleScript.
fn command(title: &str, body: &str, urgency: Urgency) -> Command {
    let sound = match urgency {
        Urgency::Normal => "",
        Urgency::Critical => " sound name \"Basso\"",
    };
    let mut command = Command::new("osascript");
    command
        .args(["-e", "on run argv"])
        .arg("-e")
        .arg(format!(
            "display notification (item 2 of argv) with title (item 1 of argv){sound}"
        ))
        .args(["-e", "end run", "--", title, body])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
}

impl Notifier for Osascript {
    fn notify(&self, title: &str, body: &str, urgency: Urgency) {
        notify_program::show("osascript", command(title, body, urgency));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notification_hands_its_title_and_body_as_arguments() {
        let built = command("Eles", "-- \"Transcription\" down", Urgency::Normal);
        let args: Vec<_> = built.get_args().collect();
        assert_eq!(built.get_program(), "osascript");
        assert_eq!(
            args,
            [
                "-e",
                "on run argv",
                "-e",
                "display notification (item 2 of argv) with title (item 1 of argv)",
                "-e",
                "end run",
                "--",
                "Eles",
                "-- \"Transcription\" down"
            ]
        );
    }

    #[test]
    fn a_critical_notification_plays_the_alert_sound() {
        let built = command("eco", "down", Urgency::Critical);
        let script = built.get_args().nth(3).unwrap();
        assert_eq!(
            script,
            "display notification (item 2 of argv) with title (item 1 of argv) \
             sound name \"Basso\""
        );
    }
}
