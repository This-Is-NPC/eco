//! Windows: the daemon as a process of the user's own in the background, with
//! no console window, its output in a log under the data directory.
//!
//! A process started here inherits every inheritable handle of eco's, a pipe
//! a shell reads eco's output from among them, and that shell would wait for
//! the daemon to exit. So `eco start` has PowerShell's `Start-Process` (which
//! inherits nothing) run `eco detach`, and that one, holding no handle of the
//! caller's, starts the daemon.

use std::fs::File;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use futures::future::BoxFuture;

use crate::config::private_dir;
use crate::paths;
use crate::ports::ServiceManager;

/// A console of its own that is never shown. Without a console at all
/// (`DETACHED_PROCESS`), each console program the daemon runs, a hook's
/// `cmd` say, would open a console window of its own.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The variable that hands `Start-Process` the program to run, so its path
/// needs no quoting inside the script.
const PROGRAM: &str = "ECO_PROGRAM";

pub struct BackgroundProcess;

impl ServiceManager for BackgroundProcess {
    fn start(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async {
            let output =
                tokio::process::Command::from(hidden(&std::env::current_exe()?, &["detach"]))
                    .output()
                    .await
                    .context("cannot run powershell")?;
            if !output.status.success() {
                bail!(
                    "cannot start the daemon: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            Ok(())
        })
    }

    /// The daemon log, which each start empties: all of it is the last start's.
    fn output(&self, _: SystemTime) -> BoxFuture<'_, Option<String>> {
        Box::pin(async { read_log(&paths::daemon_log()) })
    }
}

/// The text of the daemon log at `log`, when there is one.
fn read_log(log: &Path) -> Option<String> {
    let bytes = std::fs::read(log).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Start the daemon from this executable, its output in the daemon log; run
/// by `eco detach`.
pub fn detach() -> Result<()> {
    spawn(&std::env::current_exe()?, &paths::daemon_log())?;
    Ok(())
}

/// PowerShell starting `program` with `arguments` (none holding a space or a
/// quote) in a hidden window, sharing no handle with this process.
fn hidden(program: &Path, arguments: &[&str]) -> Command {
    let list = arguments
        .iter()
        .map(|argument| format!("'{argument}'"))
        .collect::<Vec<_>>()
        .join(",");
    let mut command = Command::new("powershell");
    command
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(format!(
            "$ErrorActionPreference = 'Stop'; \
             Start-Process -WindowStyle Hidden -FilePath $env:{PROGRAM} -ArgumentList {list}"
        ))
        .env(PROGRAM, program)
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    command
}

/// The command that runs the daemon of `exe` in the background.
fn command(exe: &Path) -> Command {
    let mut command = Command::new(exe);
    command
        .args(["daemon", "--headless"])
        .creation_flags(CREATE_NO_WINDOW);
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
    use std::ffi::OsStr;
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn the_daemon_runs_headless() {
        let built = command(Path::new(r"C:\eco\eco.exe"));
        let args: Vec<_> = built.get_args().collect();
        assert_eq!(built.get_program(), r"C:\eco\eco.exe");
        assert_eq!(args, ["daemon", "--headless"]);
    }

    #[test]
    fn start_process_is_handed_the_program_and_its_arguments() {
        let built = hidden(Path::new(r"C:\Program Files\eco\eco.exe"), &["detach"]);
        let args: Vec<_> = built.get_args().collect();
        assert_eq!(built.get_program(), "powershell");
        assert!(
            args[3].to_string_lossy().ends_with(
                "Start-Process -WindowStyle Hidden -FilePath $env:ECO_PROGRAM -ArgumentList 'detach'"
            ),
            "{args:?}"
        );
        let program: Vec<_> = built.get_envs().collect();
        assert_eq!(
            program,
            [(
                OsStr::new(PROGRAM),
                Some(OsStr::new(r"C:\Program Files\eco\eco.exe"))
            )]
        );
    }

    #[test]
    fn what_it_starts_does_not_hold_the_callers_output() {
        // ping on the loopback lives for about 20 s; the caller reading
        // PowerShell's output returns once PowerShell exits.
        let ping = Path::new(&std::env::var_os("SystemRoot").unwrap()).join(r"System32\PING.EXE");
        let began = Instant::now();
        let output = hidden(&ping, &["-n", "20", "127.0.0.1"]).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(
            began.elapsed() < Duration::from_secs(15),
            "{:?}",
            began.elapsed()
        );
    }

    #[test]
    fn a_program_powershell_cannot_start_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let output = hidden(&directory.path().join("none.exe"), &["detach"])
            .output()
            .unwrap();
        assert!(!output.status.success());
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
    fn the_output_is_the_daemon_log() {
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join("daemon.log");
        assert_eq!(read_log(&log), None);
        std::fs::write(
            &log,
            "eco: no config at C:\\c.toml; see docs/design.md \u{a7}9\r\n",
        )
        .unwrap();
        assert_eq!(
            read_log(&log).as_deref(),
            Some("eco: no config at C:\\c.toml; see docs/design.md \u{a7}9\r\n")
        );
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
