//! Windows: the daemon as a process of the user's own in the background, with
//! no console window, its output in a log under the data directory.
//!
//! A process started here inherits every inheritable handle of eco's, a pipe
//! a shell reads eco's output from among them, and that shell would wait for
//! the daemon to exit. So `eco start` has the shell (`ShellExecuteEx`, which
//! inherits nothing) run `eco detach`, and that one, holding no handle of the
//! caller's, starts the daemon.
//!
//! The daemon never runs elevated: a process of a lower integrity cannot open
//! the pipe of an elevated one, so a daemon started from an Administrator
//! terminal would shut out the Start menu entry and every window. An elevated
//! process, found by the integrity level `whoami` lists, starts none.

use std::fs::File;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::SystemTime;

use anyhow::{Context, Result, anyhow};
use futures::future::BoxFuture;
use winsafe::{SHELLEXECUTEINFO, ShellExecuteEx, co};

use crate::config::private_dir;
use crate::paths;
use crate::ports::ServiceManager;

/// A console of its own that is never shown. Without a console at all
/// (`DETACHED_PROCESS`), each console program the daemon runs, a hook's
/// `cmd` say, would open a console window of its own.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The mandatory level SID `whoami /groups` lists for an elevated process.
const HIGH_INTEGRITY: &str = "S-1-16-12288";

pub struct BackgroundProcess;

impl ServiceManager for BackgroundProcess {
    fn start(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async {
            let eco = eco_program(&std::env::current_exe()?)?;
            tokio::task::spawn_blocking(move || hidden(&eco, "detach")).await?
        })
    }

    fn elevated(&self) -> BoxFuture<'_, bool> {
        Box::pin(async {
            let output = tokio::process::Command::from(whoami()).output().await;
            output.is_ok_and(|output| is_elevated(&String::from_utf8_lossy(&output.stdout)))
        })
    }

    /// The daemon log, which each start empties: all of it is the last start's.
    fn output(&self, _: SystemTime) -> BoxFuture<'_, Option<String>> {
        Box::pin(async { read_log(&paths::daemon_log()) })
    }
}

/// The `eco` program, run as `eco detach`, for the eco program `exe`: `exe`
/// itself, or, for `eco-launch`, the `eco` of its install
/// (`<prefix>\bin\eco.exe` for `<prefix>\lib\eco\eco-launch.exe`) or of its
/// build, beside it. Never another program, which would not know `detach`.
fn eco_program(exe: &Path) -> Result<PathBuf> {
    let name = format!("eco{}", std::env::consts::EXE_SUFFIX);
    if exe.file_name().is_some_and(|file| file == name.as_str()) {
        return Ok(exe.to_path_buf());
    }
    let installed = exe
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(|prefix| prefix.join("bin").join(&name));
    installed
        .into_iter()
        .chain([exe.with_file_name(&name)])
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            anyhow!(
                "cannot start the daemon: no {name} beside {}",
                exe.display()
            )
        })
}

/// `whoami` listing the groups of this process's token.
fn whoami() -> Command {
    let mut command = Command::new("whoami");
    command
        .arg("/groups")
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    command
}

/// Whether the `whoami /groups` listing holds the high mandatory level.
fn is_elevated(groups: &str) -> bool {
    groups.contains(HIGH_INTEGRITY)
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

/// Have the shell start `program` with `parameters` in a hidden window, in
/// the program's own folder, as a process that shares no handle with this
/// one, and return once it has. A folder a process works in cannot be
/// removed, so the process never inherits this one's.
pub(crate) fn hidden(program: &Path, parameters: &str) -> Result<()> {
    let file = program
        .to_str()
        .ok_or_else(|| anyhow!("cannot start {}: the path is not text", program.display()))?;
    ShellExecuteEx(&SHELLEXECUTEINFO {
        mask: co::SEE_MASK::FLAG_NO_UI | co::SEE_MASK::NOASYNC,
        file,
        parameters: Some(parameters),
        directory: program.parent().and_then(Path::to_str),
        show: co::SW::HIDE,
        ..Default::default()
    })
    .map_err(|error| anyhow!("cannot start {file}: {error}"))
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
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn the_daemon_is_started_by_eco_itself_or_the_eco_of_the_launcher() {
        let directory = tempfile::tempdir().unwrap();
        let file = |path: &Path| {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
            path.to_path_buf()
        };
        let eco = file(&directory.path().join(r"bin\eco.exe"));
        let launcher = file(&directory.path().join(r"lib\eco\eco-launch.exe"));
        assert_eq!(eco_program(&eco).unwrap(), eco);
        assert_eq!(eco_program(&launcher).unwrap(), eco);
        let build = directory.path().join("release");
        let built = file(&build.join("eco.exe"));
        assert_eq!(
            eco_program(&file(&build.join("eco-launch.exe"))).unwrap(),
            built
        );
    }

    #[test]
    fn a_launcher_with_no_eco_beside_it_starts_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let launcher = directory.path().join("eco-launch.exe");
        std::fs::write(&launcher, "").unwrap();
        let error = eco_program(&launcher).unwrap_err().to_string();
        assert!(
            error.starts_with("cannot start the daemon: no eco.exe beside"),
            "{error}"
        );
    }

    #[test]
    fn whoami_lists_the_groups() {
        let built = whoami();
        let args: Vec<_> = built.get_args().collect();
        assert_eq!(built.get_program(), "whoami");
        assert_eq!(args, ["/groups"]);
    }

    #[test]
    fn the_integrity_level_tells_elevation() {
        let elevated =
            "Mandatory Label\\High Mandatory Level  Label  S-1-16-12288  Mandatory group\r\n";
        let limited =
            "Mandatory Label\\Medium Mandatory Level  Label  S-1-16-8192  Mandatory group\r\n";
        assert!(is_elevated(elevated));
        assert!(!is_elevated(limited));
        assert!(!is_elevated(""));
    }

    #[test]
    fn the_daemon_runs_headless() {
        let built = command(Path::new(r"C:\eco\eco.exe"));
        let args: Vec<_> = built.get_args().collect();
        assert_eq!(built.get_program(), r"C:\eco\eco.exe");
        assert_eq!(args, ["daemon", "--headless"]);
    }

    #[test]
    fn what_it_starts_does_not_hold_the_callers_output() {
        // ping on the loopback lives for about 20 s; the shell returns once
        // it has started it.
        let ping = Path::new(&std::env::var_os("SystemRoot").unwrap()).join(r"System32\PING.EXE");
        let began = Instant::now();
        hidden(&ping, "-n 20 127.0.0.1").unwrap();
        assert!(
            began.elapsed() < Duration::from_secs(15),
            "{:?}",
            began.elapsed()
        );
    }

    #[test]
    fn what_it_starts_works_in_its_own_folder() {
        let directory = tempfile::tempdir().unwrap();
        let cmd = directory.path().join("cmd.exe");
        std::fs::copy(std::env::var_os("ComSpec").unwrap(), &cmd).unwrap();
        let written = directory.path().join("folder.txt");
        hidden(&cmd, &format!("/c cd > \"{}\"", written.display())).unwrap();
        let began = Instant::now();
        let folder = loop {
            match std::fs::read_to_string(&written) {
                Ok(text) if text.ends_with('\n') => break text,
                _ if began.elapsed() < Duration::from_secs(10) => {
                    std::thread::sleep(Duration::from_millis(50))
                }
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(Path::new(folder.trim()), directory.path());
    }

    #[test]
    fn a_program_the_shell_cannot_start_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let error = hidden(&directory.path().join("none.exe"), "detach").unwrap_err();
        assert!(
            error.to_string().starts_with("cannot start ")
                && error.to_string().contains("none.exe"),
            "{error}"
        );
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
            "eco: missing C:\\Users\\Zo\u{eb}\\silero_vad.onnx; run `eco setup`\r\n",
        )
        .unwrap();
        assert_eq!(
            read_log(&log).as_deref(),
            Some("eco: missing C:\\Users\\Zo\u{eb}\\silero_vad.onnx; run `eco setup`\r\n")
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
