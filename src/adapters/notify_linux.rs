//! Linux: notifications through `notify-send`, under the app name and the
//! icon eco installs.

use std::process::{Command, Stdio};

use crate::adapters::notify_program;
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

impl Notifier for NotifySend {
    fn notify(&self, title: &str, body: &str, urgency: Urgency) {
        notify_program::show("notify-send", command(title, body, urgency));
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
}
