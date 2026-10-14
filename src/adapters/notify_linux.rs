//! Linux: notifications through `notify-send`, under the app name and the
//! icon eco installs.

use std::process::{Command, Stdio};

use crate::ports::{Notifier, Urgency};

pub struct NotifySend;

/// `notify-send` showing `title` and `body` as eco's, with `urgency`.
fn command(title: &str, body: &str, urgency: Urgency) -> Command {
    let urgency = match urgency {
        Urgency::Normal => "normal",
        Urgency::Critical => "critical",
    };
    let mut command = Command::new("notify-send");
    command
        .arg("--app-name=eco")
        .arg("--icon=eco")
        .arg(format!("--urgency={urgency}"))
        .arg("--")
        .args([title, body])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
}

/// Run `command`; why it showed nothing, if it did not.
fn run(mut command: Command) -> Option<String> {
    match command.output() {
        Ok(output) if output.status.success() => None,
        Ok(output) => Some(format!(
            "notify-send exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => Some(format!(
            "cannot run notify-send, so no notification shows: {error}"
        )),
    }
}

impl Notifier for NotifySend {
    fn notify(&self, title: &str, body: &str, urgency: Urgency) {
        let command = command(title, body, urgency);
        // Waited for on a thread of its own, so no caller waits and no exited
        // process is left unreaped.
        std::thread::spawn(move || {
            if let Some(why) = run(command) {
                eprintln!("eco: {why}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notification_is_eco_s_with_its_icon_urgency_title_and_body() {
        let built = command("Eles", "-- Transcription down", Urgency::Critical);
        let args: Vec<_> = built.get_args().collect();
        assert_eq!(built.get_program(), "notify-send");
        assert_eq!(
            args,
            [
                "--app-name=eco",
                "--icon=eco",
                "--urgency=critical",
                "--",
                "Eles",
                "-- Transcription down"
            ]
        );
        let normal = command("eco", "ready", Urgency::Normal);
        assert!(normal.get_args().any(|arg| arg == "--urgency=normal"));
    }

    #[test]
    fn a_notification_not_shown_says_why() {
        let mut failing = Command::new("sh");
        failing
            .args(["-c", "echo no daemon >&2; exit 1"])
            .stderr(Stdio::piped());
        let why = run(failing).unwrap();
        assert!(why.starts_with("notify-send exited with"), "{why}");
        assert!(why.ends_with(": no daemon"), "{why}");
        let missing = run(Command::new("/nonexistent/notify-send")).unwrap();
        assert!(missing.starts_with("cannot run notify-send"), "{missing}");
        assert_eq!(run(Command::new("true")), None);
    }
}
