//! pacman: a release installed as the Arch package it carries, with `pacman
//! -U` run as root — through pkexec, which asks for the password in a window
//! of its own, or, on a system without pkexec, through sudo on the terminal
//! eco runs in, as install.sh does.

use std::ffi::{OsStr, OsString};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};

use futures::future::BoxFuture;
use tokio::process::Command;

use crate::installer::{self, Scratch};
use crate::ports::{InstallError, InstallProgress, InstallStage, Installer};

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
                "pkexec is not installed, and there is no terminal for sudo to ask for the password; install polkit, or run eco update in a terminal".into(),
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

/// What the elevator's exit `status` says: installed, or why not.
fn outcome(status: ExitStatus) -> Result<(), InstallError> {
    match status.code() {
        Some(0) => Ok(()),
        // pkexec's own: the password prompt was dismissed (126) or refused (127).
        Some(126 | 127) => Err(InstallError(
            "the password was not given; nothing was installed".into(),
        )),
        Some(code) => Err(InstallError(format!(
            "pacman -U failed with exit status {code}"
        ))),
        None => Err(InstallError("pacman -U was stopped by a signal".into())),
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
    ) -> BoxFuture<'static, Result<(), InstallError>> {
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
            .await?;
            progress(InstallStage::Installing);
            // pacman's output goes to stderr: stdout is the command's JSON.
            let status = Command::new(&elevator)
                .args(arguments(&package))
                .stdin(Stdio::inherit())
                .stdout(Stdio::from(std::io::stderr()))
                .status()
                .await
                .map_err(|error| {
                    InstallError(format!("cannot run {}: {error}", elevator.display()))
                })?;
            outcome(status)
        })
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
