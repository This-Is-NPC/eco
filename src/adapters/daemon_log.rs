//! The log of the daemon started in the background on Windows and macOS: each
//! start empties it, so all of it is the last start's.

use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result};

use crate::config::private_dir;

/// The log at `log`, emptied, in a private directory.
pub fn emptied(log: &Path) -> Result<File> {
    if let Some(directory) = log.parent() {
        private_dir(directory)?;
    }
    File::create(log).with_context(|| format!("cannot write {}", log.display()))
}

/// The text of the log at `log`, when there is one.
pub fn read(log: &Path) -> Option<String> {
    let bytes = std::fs::read(log).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_start_empties_the_log_in_a_new_directory() {
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join("eco").join("daemon.log");
        emptied(&log).unwrap();
        std::fs::write(&log, "an earlier run").unwrap();
        emptied(&log).unwrap();
        assert_eq!(read(&log).as_deref(), Some(""));
    }

    #[test]
    fn the_output_is_the_whole_log() {
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join("daemon.log");
        assert_eq!(read(&log), None);
        std::fs::write(
            &log,
            "eco: missing C:\\Users\\Zo\u{eb}\\silero_vad.onnx; run `eco setup`\r\n",
        )
        .unwrap();
        assert_eq!(
            read(&log).as_deref(),
            Some("eco: missing C:\\Users\\Zo\u{eb}\\silero_vad.onnx; run `eco setup`\r\n")
        );
    }

    #[test]
    fn a_log_that_cannot_be_written_says_where() {
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join("daemon.log");
        std::fs::create_dir(&log).unwrap();
        let error = emptied(&log).unwrap_err();
        assert_eq!(error.to_string(), format!("cannot write {}", log.display()));
    }
}
