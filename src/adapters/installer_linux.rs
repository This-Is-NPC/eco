//! pacman: a release installed as the Arch package it carries, with `pacman
//! -U` run as root — through pkexec, which asks for the password in a window
//! of its own, or, on a system without pkexec, through sudo on the terminal
//! eco runs in, as install.sh does. The same for removing it, `pacman -Rns`.
//! The daemon is a systemd user service that the package does not stop, so the
//! removal is `eco apply-uninstall` in a transient unit of the user's systemd,
//! which stops the daemon first and outlives it.

use std::ffi::{OsStr, OsString};
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};

use futures::future::BoxFuture;
use tokio::process::Command;

use crate::installer::{self, Scratch};
use crate::ports::{InstallError, InstallProgress, InstallStage, Installed, Installer, Pending};

#[derive(Clone)]
pub struct Pacman {
    /// Where the release files are, under each tag.
    downloads: String,
    /// pkexec, when this system has it.
    pkexec: Option<PathBuf>,
    sudo: PathBuf,
    /// Whether eco runs in a terminal, where sudo can ask for the password.
    terminal: bool,
}

impl Pacman {
    pub fn new(downloads: &str) -> Self {
        Self {
            downloads: downloads.into(),
            pkexec: std::env::var_os("PATH").and_then(|path| on_path("pkexec", &path)),
            sudo: PathBuf::from("sudo"),
            terminal: std::io::stdin().is_terminal(),
        }
    }

    /// The program that runs pacman as root: pkexec, else sudo in a terminal.
    fn elevator(&self) -> Result<PathBuf, InstallError> {
        match (&self.pkexec, self.terminal) {
            (Some(pkexec), _) => Ok(pkexec.clone()),
            (None, true) => Ok(self.sudo.clone()),
            (None, false) => Err(InstallError(
                "pkexec is not installed, and there is no terminal for sudo to ask for the password; install polkit, or run eco in a terminal".into(),
            )),
        }
    }
}

/// `name` in the first directory of the search path `path` that has it.
fn on_path(name: &str, path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

/// The Arch package of a release, as install.sh picks it: `eco-<version>…pkg.tar.zst`.
fn is_package(name: &str) -> bool {
    name.strip_prefix("eco-")
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
        && name.ends_with(".pkg.tar.zst")
        && !name.contains(char::is_whitespace)
}

/// What the elevator runs: pacman installing `package`, without asking again.
fn arguments(package: &Path) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = ["pacman", "-U", "--noconfirm"].map(OsString::from).to_vec();
    arguments.push(package.into());
    arguments
}

/// What the elevator's exit `status` says of `pacman` with `operation`
/// (`-U`), which leaves nothing `done` (installed) when it fails.
fn outcome(status: ExitStatus, operation: &str, done: &str) -> Result<(), InstallError> {
    match status.code() {
        Some(0) => Ok(()),
        // pkexec's own: the password prompt was dismissed (126) or refused (127).
        Some(126 | 127) => Err(InstallError(format!(
            "the password was not given; nothing was {done}"
        ))),
        Some(code) => Err(InstallError(format!(
            "pacman {operation} failed with exit status {code}"
        ))),
        None => Err(InstallError(format!(
            "pacman {operation} was stopped by a signal"
        ))),
    }
}

/// Run pacman as root through `elevator` with `arguments`, its output on
/// stderr: stdout is the command's JSON.
async fn elevated(
    elevator: &Path,
    arguments: Vec<OsString>,
    operation: &str,
    done: &str,
) -> Result<(), InstallError> {
    let status = Command::new(elevator)
        .args(arguments)
        .stdin(Stdio::inherit())
        .stdout(Stdio::from(std::io::stderr()))
        .status()
        .await
        .map_err(|error| InstallError(format!("cannot run {}: {error}", elevator.display())))?;
    outcome(status, operation, done)
}

/// What the elevator runs to remove the package, without asking again and
/// with the files it made.
fn removal_arguments() -> Vec<OsString> {
    ["pacman", "-Rns", "--noconfirm", "eco"]
        .map(OsString::from)
        .to_vec()
}

/// What `systemd-run` runs: `eco apply-uninstall`, in the terminal that asked
/// and until it ends, or on its own, past the daemon.
fn unit_arguments(exe: &Path, everything: bool, terminal: bool) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = ["--user", "--collect", "--quiet"]
        .map(OsString::from)
        .to_vec();
    if terminal {
        arguments.extend(["--pty", "--wait"].map(OsString::from));
    }
    arguments.push(exe.into());
    arguments.push("apply-uninstall".into());
    if everything {
        arguments.push("--everything".into());
    }
    arguments
}

/// A removal waiting for the daemon to stop.
#[derive(Debug)]
struct Removal {
    systemd_run: PathBuf,
    everything: bool,
    /// Whether eco was asked in a terminal, which the removal then keeps and
    /// is waited for in.
    terminal: bool,
}

impl Pending for Removal {
    fn hand_off(&self) -> io::Result<()> {
        let mut command = std::process::Command::new(&self.systemd_run);
        command.args(unit_arguments(
            &std::env::current_exe()?,
            self.everything,
            self.terminal,
        ));
        if !self.terminal {
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
        }
        // Without `--wait`, systemd-run ends once the unit has started.
        match command.status()? {
            status if status.success() => Ok(()),
            status => Err(io::Error::other(format!(
                "systemd-run could not run the removal ({status})"
            ))),
        }
    }
}

impl Installer for Pacman {
    fn program(&self) -> PathBuf {
        PathBuf::from("/usr/bin/eco")
    }

    fn install(
        &self,
        tag: String,
        progress: InstallProgress,
    ) -> BoxFuture<'static, Result<Installed, InstallError>> {
        let pacman = self.clone();
        Box::pin(async move {
            let elevator = pacman.elevator()?;
            let scratch = Scratch::new(&std::env::temp_dir())?;
            let package = installer::fetch(
                &pacman.downloads,
                &tag,
                is_package,
                scratch.path(),
                &progress,
            )
            .await?
            .path;
            progress(InstallStage::Installing);
            elevated(&elevator, arguments(&package), "-U", "installed")
                .await
                .map(|()| Installed::Done)
        })
    }

    fn uninstall(&self, everything: bool) -> Box<dyn Pending> {
        Box::new(Removal {
            systemd_run: PathBuf::from("systemd-run"),
            everything,
            terminal: self.terminal,
        })
    }

    fn remove(&self) -> BoxFuture<'static, Result<(), InstallError>> {
        let elevator = self.elevator();
        Box::pin(async move { elevated(&elevator?, removal_arguments(), "-Rns", "removed").await })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::adapters::fake_program::fake_program;
    use crate::adapters::http::testing::serve;

    /// The SHA-256 of `abc`.
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const PACKAGE: &str = "eco-0.5.0-1-x86_64.pkg.tar.zst";

    fn pacman(downloads: &str, pkexec: Option<PathBuf>, terminal: bool) -> Pacman {
        Pacman {
            downloads: downloads.into(),
            pkexec,
            sudo: PathBuf::from("/usr/bin/sudo"),
            terminal,
        }
    }

    #[test]
    fn the_program_is_the_one_the_package_installs() {
        assert_eq!(
            pacman("", None, false).program(),
            PathBuf::from("/usr/bin/eco")
        );
    }

    fn recorded() -> (InstallProgress, Arc<Mutex<Vec<InstallStage>>>) {
        let stages = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&stages);
        (
            Box::new(move |stage| kept.lock().unwrap().push(stage)),
            stages,
        )
    }

    /// A release whose SHA256SUMS names the package as `abc`, served as `body`.
    async fn release(body: &[u8]) -> String {
        let sums = format!("{ABC}  {PACKAGE}\n").into_bytes();
        serve(vec![(200, None, sums), (200, None, body.to_vec())])
            .await
            .0
    }

    /// A pkexec that writes its arguments to `ran` and exits with `code`.
    fn fake_pkexec(directory: &Path, code: i32) -> (PathBuf, PathBuf) {
        let ran = directory.join("ran");
        let body = format!("printf '%s\\n' \"$@\" > {}\nexit {code}", ran.display());
        (fake_program(directory, "pkexec", &body), ran)
    }

    #[test]
    fn pacman_runs_through_pkexec_else_sudo_in_a_terminal() {
        let pkexec = PathBuf::from("/usr/bin/pkexec");
        assert_eq!(
            pacman("", Some(pkexec.clone()), false).elevator().unwrap(),
            pkexec
        );
        assert_eq!(
            pacman("", None, true).elevator().unwrap(),
            PathBuf::from("/usr/bin/sudo")
        );
        let error = pacman("", None, false).elevator().unwrap_err();
        assert!(
            error.0.starts_with("pkexec is not installed"),
            "{}",
            error.0
        );
        assert_eq!(
            arguments(Path::new("/tmp/x/eco.pkg.tar.zst")),
            ["pacman", "-U", "--noconfirm", "/tmp/x/eco.pkg.tar.zst"].map(OsString::from)
        );
    }

    #[test]
    fn pkexec_is_found_in_the_first_directory_that_has_it() {
        let empty = tempfile::tempdir().unwrap();
        let with = tempfile::tempdir().unwrap();
        let (pkexec, _) = fake_pkexec(with.path(), 0);
        let path = std::env::join_paths([empty.path(), with.path()]).unwrap();
        assert_eq!(on_path("pkexec", &path), Some(pkexec));
        let path = std::env::join_paths([empty.path()]).unwrap();
        assert_eq!(on_path("pkexec", &path), None);
    }

    #[test]
    fn the_package_is_the_one_install_sh_takes() {
        assert!(is_package(PACKAGE));
        assert!(!is_package("eco-debug-0.5.0-1-x86_64.pkg.tar.zst"));
        assert!(!is_package("eco-0.5.0-windows-x86_64.zip"));
        assert!(!is_package("eco-0.5.0-1-x86_64.pkg.tar.zst.sig"));
    }

    #[tokio::test]
    async fn the_verified_package_is_handed_to_pacman_through_pkexec() {
        let directory = tempfile::tempdir().unwrap();
        let (pkexec, ran) = fake_pkexec(directory.path(), 0);
        let downloads = release(b"abc").await;
        let (progress, stages) = recorded();
        pacman(&downloads, Some(pkexec), false)
            .install("v0.5.0".into(), progress)
            .await
            .unwrap();
        let said = fs::read_to_string(&ran).unwrap();
        let lines: Vec<&str> = said.lines().collect();
        assert_eq!(lines[..3], ["pacman", "-U", "--noconfirm"]);
        assert!(lines[3].ends_with(&format!("/{PACKAGE}")), "{said}");
        assert_eq!(
            *stages.lock().unwrap(),
            [
                InstallStage::Downloading,
                InstallStage::Verifying,
                InstallStage::Installing
            ]
        );
        // The download goes with the install.
        assert!(!Path::new(lines[3]).exists());
    }

    #[tokio::test]
    async fn a_package_that_does_not_match_never_reaches_pacman() {
        let directory = tempfile::tempdir().unwrap();
        let (pkexec, ran) = fake_pkexec(directory.path(), 0);
        let downloads = release(b"evil").await;
        let (progress, stages) = recorded();
        let error = pacman(&downloads, Some(pkexec), false)
            .install("v0.5.0".into(), progress)
            .await
            .unwrap_err();
        assert_eq!(
            error.0,
            format!("{PACKAGE} does not match the release's SHA256SUMS; nothing was installed")
        );
        assert!(!ran.exists());
        assert!(!stages.lock().unwrap().contains(&InstallStage::Installing));
    }

    #[tokio::test]
    async fn without_pkexec_or_a_terminal_nothing_is_downloaded() {
        // Nothing listens here: a download would fail otherwise.
        let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let downloads = format!("http://{}/v1", closed.local_addr().unwrap());
        drop(closed);
        let (progress, stages) = recorded();
        let error = pacman(&downloads, None, false)
            .install("v0.5.0".into(), progress)
            .await
            .unwrap_err();
        assert!(
            error.0.starts_with("pkexec is not installed"),
            "{}",
            error.0
        );
        assert!(stages.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_pkexec_that_cannot_run_or_is_killed_installs_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let downloads = release(b"abc").await;
        let (progress, _) = recorded();
        let missing = directory.path().join("gone").join("pkexec");
        let error = pacman(&downloads, Some(missing.clone()), false)
            .install("v0.5.0".into(), progress)
            .await
            .unwrap_err();
        assert!(
            error
                .0
                .starts_with(&format!("cannot run {}: ", missing.display())),
            "{}",
            error.0
        );
        let killed = fake_program(directory.path(), "pkexec", "kill -9 $$");
        let downloads = release(b"abc").await;
        let (progress, _) = recorded();
        let error = pacman(&downloads, Some(killed), false)
            .install("v0.5.0".into(), progress)
            .await
            .unwrap_err();
        assert_eq!(error.0, "pacman -U was stopped by a signal");
    }

    #[test]
    fn a_removal_waits_in_the_terminal_that_asked_for_it() {
        let pending = pacman("", None, true).uninstall(true);
        let said = format!("{pending:?}");
        assert!(said.contains("everything: true") && said.contains("terminal: true"));
    }

    #[test]
    fn the_package_is_removed_with_its_files_without_asking() {
        assert_eq!(
            removal_arguments(),
            ["pacman", "-Rns", "--noconfirm", "eco"].map(OsString::from)
        );
    }

    #[test]
    fn the_removal_runs_in_a_unit_that_waits_only_in_a_terminal() {
        let exe = Path::new("/usr/bin/eco");
        assert_eq!(
            unit_arguments(exe, false, false),
            [
                "--user",
                "--collect",
                "--quiet",
                "/usr/bin/eco",
                "apply-uninstall"
            ]
            .map(OsString::from)
        );
        assert_eq!(
            unit_arguments(exe, true, true),
            [
                "--user",
                "--collect",
                "--quiet",
                "--pty",
                "--wait",
                "/usr/bin/eco",
                "apply-uninstall",
                "--everything"
            ]
            .map(OsString::from)
        );
    }

    fn removal(systemd_run: PathBuf, terminal: bool) -> Box<dyn Pending> {
        Box::new(Removal {
            systemd_run,
            everything: true,
            terminal,
        })
    }

    #[test]
    fn handing_off_the_removal_runs_systemd_run_with_the_unit() {
        let directory = tempfile::tempdir().unwrap();
        let ran = directory.path().join("ran");
        let body = format!("printf '%s\\n' \"$@\" > {}", ran.display());
        let systemd_run = fake_program(directory.path(), "systemd-run", &body);
        removal(systemd_run, false).hand_off().unwrap();
        let said = fs::read_to_string(&ran).unwrap();
        let lines: Vec<&str> = said.lines().collect();
        assert_eq!(lines[..3], ["--user", "--collect", "--quiet"]);
        assert_eq!(
            lines[lines.len() - 2..],
            ["apply-uninstall", "--everything"]
        );
    }

    #[test]
    fn a_systemd_run_that_fails_or_is_missing_fails_the_hand_off() {
        let directory = tempfile::tempdir().unwrap();
        let failing = fake_program(directory.path(), "systemd-run", "exit 1");
        let error = removal(failing, true).hand_off().unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("systemd-run could not run the removal")
        );
        let missing = directory.path().join("gone").join("systemd-run");
        assert!(removal(missing, false).hand_off().is_err());
    }

    #[tokio::test]
    async fn removing_runs_pacman_through_pkexec() {
        let directory = tempfile::tempdir().unwrap();
        let (pkexec, ran) = fake_pkexec(directory.path(), 0);
        pacman("", Some(pkexec), false).remove().await.unwrap();
        assert_eq!(
            fs::read_to_string(&ran)
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            ["pacman", "-Rns", "--noconfirm", "eco"]
        );
    }

    #[tokio::test]
    async fn a_removal_that_is_refused_or_cannot_be_elevated_is_said() {
        for (code, said) in [
            (126, "the password was not given; nothing was removed"),
            (1, "pacman -Rns failed with exit status 1"),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let (pkexec, _) = fake_pkexec(directory.path(), code);
            let error = pacman("", Some(pkexec), false).remove().await.unwrap_err();
            assert_eq!(error.0, said);
        }
        let error = pacman("", None, false).remove().await.unwrap_err();
        assert!(
            error.0.starts_with("pkexec is not installed"),
            "{}",
            error.0
        );
    }

    #[tokio::test]
    async fn a_dismissed_password_prompt_or_a_failed_pacman_is_said() {
        for (code, said) in [
            (126, "the password was not given; nothing was installed"),
            (1, "pacman -U failed with exit status 1"),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let (pkexec, _) = fake_pkexec(directory.path(), code);
            let downloads = release(b"abc").await;
            let (progress, _) = recorded();
            let error = pacman(&downloads, Some(pkexec), false)
                .install("v0.5.0".into(), progress)
                .await
                .unwrap_err();
            assert_eq!(error.0, said);
        }
    }
}
