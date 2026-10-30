//! A notification shown by a program the notifier runs, on Linux and macOS:
//! waited for on a thread of its own, and why it showed nothing written to
//! the daemon's stderr.

use std::process::Command;

/// Run `command`, the program `name`, on a thread of its own, so no caller
/// waits and no exited process is left unreaped.
pub fn show(name: &'static str, command: Command) {
    std::thread::spawn(move || {
        if let Some(why) = run(name, command) {
            eprintln!("eco: {why}");
        }
    });
}

/// Run `command`, the program `name`; why it showed nothing, if it did not.
fn run(name: &str, mut command: Command) -> Option<String> {
    match command.output() {
        Ok(output) if output.status.success() => None,
        Ok(output) => Some(format!(
            "{name} exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => Some(format!(
            "cannot run {name}, so no notification shows: {error}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::process::Stdio;

    use super::*;

    #[test]
    fn a_notification_not_shown_says_why() {
        let mut failing = Command::new("sh");
        failing
            .args(["-c", "echo no daemon >&2; exit 1"])
            .stderr(Stdio::piped());
        let why = run("notify-send", failing).unwrap();
        assert!(why.starts_with("notify-send exited with"), "{why}");
        assert!(why.ends_with(": no daemon"), "{why}");
        let missing = run("osascript", Command::new("/nonexistent/osascript")).unwrap();
        assert!(missing.starts_with("cannot run osascript"), "{missing}");
        assert_eq!(run("true", Command::new("true")), None);
    }
}
