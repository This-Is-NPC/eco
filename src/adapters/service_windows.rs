//! Windows: the daemon as a process of the user's own in the background, with
//! no console window, its output in a log under the data directory.

use std::fs::File;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result};
use futures::future::BoxFuture;

use crate::config::private_dir;
use crate::paths;
use crate::ports::ServiceManager;

/// A process group of its own, so a Ctrl-C in the terminal that ran
/// `eco start` does not reach the daemon.
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
/// A console of its own that is never shown. Without a console at all
/// (`DETACHED_PROCESS`), each console program the daemon runs, a hook's
/// `cmd` say, would open a console window of its own.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct BackgroundProcess;

impl ServiceManager for BackgroundProcess {
    fn start(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async {
            spawn(&std::env::current_exe()?, &paths::daemon_log())?;
            Ok(())
        })
    }
}

/// The command that runs the daemon of `exe` in the background.
fn command(exe: &Path) -> Command {
    let mut command = Command::new(exe);
    command
        .args(["daemon", "--headless"])
        .creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    command
}

/// Start the daemon of `exe` with its output in `log`, emptied first; it
/// outlives this process.
fn spawn(exe: &Path, log: &Path) -> Result<Child> {
    if let Some(directory) = log.parent() {
        private_dir(directory)?;
    }
    let output = File::create(log).with_context(|| format!("cannot write {}", log.display()))?;
    command(exe)
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(output)
        .spawn()
        .with_context(|| format!("cannot start {}", exe.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_daemon_runs_headless() {
        let built = command(Path::new(r"C:\eco\eco.exe"));
        let args: Vec<_> = built.get_args().collect();
        assert_eq!(built.get_program(), r"C:\eco\eco.exe");
        assert_eq!(args, ["daemon", "--headless"]);
    }

    #[test]
    fn its_output_goes_to_a_new_log() {
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join("eco").join("daemon.log");
        std::fs::create_dir(log.parent().unwrap()).unwrap();
        std::fs::write(&log, "an earlier run").unwrap();
        // cmd, handed `daemon --headless`, writes its banner and exits.
        let cmd = Path::new(&std::env::var_os("ComSpec").unwrap()).to_owned();
        let status = spawn(&cmd, &log).unwrap().wait().unwrap();
        assert!(status.success());
        let written = std::fs::read_to_string(&log).unwrap();
        assert!(written.contains("Microsoft"), "{written}");
        assert!(!written.contains("an earlier run"));
    }

    #[test]
    fn a_program_that_cannot_start_is_reported() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("none.exe");
        let error = spawn(&missing, &directory.path().join("daemon.log")).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("cannot start {}", missing.display())
        );
    }
}
