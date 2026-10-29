//! The Windows Installer package of a release, `eco-<version>-windows-x86_64.msi`,
//! installed per machine in `%ProgramFiles%\eco`. Windows Installer cannot
//! replace the files of an eco that runs, so the package is downloaded and
//! checked while eco runs, then `eco apply-update` does the rest from a copy
//! of eco outside the install: it stops the daemon, runs `msiexec` unelevated
//! (Windows Installer asks for the elevation itself), and starts eco again.
//! The shell starts that copy (`ShellExecuteEx`), so it shares no handle with
//! the caller, whose output a terminal would otherwise wait on.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use futures::future::BoxFuture;
use tokio::process::Command;

use crate::adapters::control_socket::Address;
use crate::adapters::notify_windows::Toasts;
use crate::adapters::service_windows::{CREATE_NO_WINDOW, hidden};
use crate::apply::{self, Steps};
use crate::installer::{self, SCRATCH, Scratch};
use crate::lifecycle;
use crate::ports::{
    InstallError, InstallProgress, Installed, Installer, Notifier, Pending, Urgency,
};

/// What `msiexec` exits with when the install worked but Windows asks for a
/// restart, which eco does not need.
const REBOOT_PENDING: i32 = 3010;

/// What `msiexec` exits with when the user closed its window
/// (`ERROR_INSTALL_USEREXIT`) or dismissed the elevation prompt
/// (`ERROR_CANCELLED`).
const CANCELLED: [i32; 2] = [1602, 1223];

/// What `msiexec` exits with while another installation runs
/// (`ERROR_INSTALL_ALREADY_RUNNING`).
const BUSY: i32 = 1618;

/// The name of the copy of eco that applies an update, beside the package.
const COPY: &str = "eco.exe";

/// The installed eco: `%ProgramFiles%\eco\bin\eco.exe`, where the package puts
/// it, whichever eco asks.
fn installed_program() -> PathBuf {
    std::env::var_os("ProgramFiles")
        .map_or_else(|| PathBuf::from(r"C:\Program Files"), PathBuf::from)
        .join(r"eco\bin\eco.exe")
}

pub struct Msi {
    /// Where the release files are, under each tag.
    downloads: String,
    /// Where a package is downloaded to, in a folder of its own.
    scratch: PathBuf,
}

impl Msi {
    pub fn new(downloads: &str) -> Self {
        Self {
            downloads: downloads.into(),
            scratch: std::env::temp_dir(),
        }
    }
}

/// The Windows package of a release, as install.ps1 picks it:
/// `eco-<version>…-windows-x86_64.msi`.
fn is_package(name: &str) -> bool {
    name.strip_prefix("eco-")
        .and_then(|rest| rest.strip_suffix("-windows-x86_64.msi"))
        .is_some_and(|version| {
            version.starts_with(|c: char| c.is_ascii_digit()) && !version.contains(' ')
        })
}

/// What `msiexec` runs with to install `msi`: basic UI, which shows progress
/// and asks for the elevation, and no restart.
fn arguments(msi: &Path) -> Vec<OsString> {
    vec!["/i".into(), msi.into(), "/qb".into(), "/norestart".into()]
}

/// What `msiexec` ending with `code` means for the install.
fn outcome(code: Option<i32>) -> Result<(), InstallError> {
    let failed = |why: String| Err(InstallError(why));
    match code {
        Some(0 | REBOOT_PENDING) => Ok(()),
        Some(code) if CANCELLED.contains(&code) => failed("the installation was cancelled".into()),
        Some(BUSY) => failed("another installation is running; try again once it ends".into()),
        Some(code) => failed(format!("the installer failed with exit code {code}")),
        None => failed("the installer ended without an exit code".into()),
    }
}

/// The parameters that run `eco apply-update` on `msi`.
fn parameters(msi: &Path) -> String {
    format!("apply-update \"{}\"", msi.display())
}

/// Delete the folders of earlier updates under `temp`: the copy of eco that
/// applied one cannot be deleted while it runs, so the next daemon start does.
pub fn tidy(temp: &Path) {
    let Ok(entries) = fs::read_dir(temp) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(SCRATCH) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// A downloaded, verified package waiting for eco to stop.
#[derive(Debug)]
struct Package {
    msi: PathBuf,
}

impl Pending for Package {
    fn hand_off(&self) -> io::Result<()> {
        let copy = self.msi.with_file_name(COPY);
        fs::copy(std::env::current_exe()?, &copy)?;
        hidden(&copy, &parameters(&self.msi)).map_err(io::Error::other)
    }
}

impl Installer for Msi {
    fn program(&self) -> PathBuf {
        installed_program()
    }

    fn install(
        &self,
        tag: String,
        progress: InstallProgress,
    ) -> BoxFuture<'static, Result<Installed, InstallError>> {
        let downloads = self.downloads.clone();
        let parent = self.scratch.clone();
        Box::pin(async move {
            let scratch = Scratch::new(&parent)?;
            let msi =
                installer::fetch(&downloads, &tag, is_package, scratch.path(), &progress).await?;
            scratch.keep();
            Ok(Installed::Pending(Box::new(Package { msi })))
        })
    }
}

/// The apply step on this machine: the daemon of the user, the installed eco,
/// Windows Installer, and toasts for what goes wrong.
struct Applying {
    address: Address,
    program: PathBuf,
    msiexec: PathBuf,
    toasts: Toasts,
}

impl Applying {
    fn new() -> Self {
        let system = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        Self {
            address: Address::user(),
            program: installed_program(),
            msiexec: PathBuf::from(system).join(r"System32\msiexec.exe"),
            toasts: Toasts::default(),
        }
    }
}

impl Steps for Applying {
    fn stop(&self) -> BoxFuture<'_, Result<Option<bool>, InstallError>> {
        Box::pin(async {
            lifecycle::shut(&self.address)
                .await
                .map_err(|error| InstallError(format!("{error:#}")))
        })
    }

    fn install(&self, package: &Path) -> BoxFuture<'_, Result<(), InstallError>> {
        let package = package.to_path_buf();
        Box::pin(async move {
            let status = Command::new(&self.msiexec)
                .args(arguments(&package))
                .stdin(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .status()
                .await
                .map_err(|error| {
                    InstallError(format!("cannot run {}: {error}", self.msiexec.display()))
                })?;
            outcome(status.code())
        })
    }

    fn start(&self, window: bool) -> BoxFuture<'_, Result<(), InstallError>> {
        let program = self.program.clone();
        let parameters = if window { "start" } else { "start --headless" };
        Box::pin(async move {
            tokio::task::spawn_blocking(move || hidden(&program, parameters))
                .await
                .map_err(|error| InstallError(error.to_string()))?
                .map_err(|error| InstallError(error.to_string()))
        })
    }

    fn tell(&self, message: &str) {
        self.toasts.notify("eco", message, Urgency::Critical);
    }

    fn discard(&self, package: &Path) {
        let _ = fs::remove_file(package);
    }
}

/// `eco apply-update`: install `package` with the daemon stopped.
pub async fn apply(package: &Path) {
    apply::run(&Applying::new(), package).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::http::testing::serve;

    /// The SHA-256 of `abc`.
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const MSI: &str = "eco-0.5.0-windows-x86_64.msi";

    #[test]
    fn the_package_is_the_msi_of_the_windows_release() {
        assert!(is_package(MSI));
        assert!(is_package("eco-0.5.0-rc.1-windows-x86_64.msi"));
        for name in [
            "eco-0.5.0-windows-x86_64.zip",
            "eco-0.5.0-1-x86_64.pkg.tar.zst",
            "eco-windows-x86_64.msi",
            "eco-v0.5.0-windows-x86_64.msi",
            "eco-0.5.0 -windows-x86_64.msi",
            "eco-0.5.0-windows-aarch64.msi",
            "other-0.5.0-windows-x86_64.msi",
        ] {
            assert!(!is_package(name), "{name}");
        }
    }

    #[test]
    fn the_installer_shows_its_progress_and_never_restarts() {
        assert_eq!(
            arguments(Path::new(r"C:\Temp\eco.msi")),
            ["/i", r"C:\Temp\eco.msi", "/qb", "/norestart"]
        );
    }

    #[test]
    fn the_exit_code_tells_whether_it_installed() {
        assert!(outcome(Some(0)).is_ok());
        assert!(outcome(Some(3010)).is_ok());
        for (code, why) in [
            (1602, "the installation was cancelled"),
            (1223, "the installation was cancelled"),
            (
                1618,
                "another installation is running; try again once it ends",
            ),
            (1603, "the installer failed with exit code 1603"),
        ] {
            assert_eq!(outcome(Some(code)).unwrap_err().0, why, "{code}");
        }
        assert_eq!(
            outcome(None).unwrap_err().0,
            "the installer ended without an exit code"
        );
    }

    #[test]
    fn the_copy_is_told_the_quoted_package() {
        assert_eq!(
            parameters(Path::new(r"C:\Users\Zoe Lima\eco.msi")),
            r#"apply-update "C:\Users\Zoe Lima\eco.msi""#
        );
    }

    #[test]
    fn the_program_is_the_eco_in_the_installs_bin() {
        assert!(installed_program().ends_with(r"eco\bin\eco.exe"));
        assert_eq!(Msi::new("").program(), installed_program());
    }

    #[test]
    fn only_the_folders_of_earlier_updates_are_tidied() {
        let temp = tempfile::tempdir().unwrap();
        let old = temp.path().join(format!("{SCRATCH}1"));
        fs::create_dir(&old).unwrap();
        fs::write(old.join("eco.msi"), "").unwrap();
        let other = temp.path().join("other");
        fs::create_dir(&other).unwrap();
        tidy(temp.path());
        assert!(!old.exists());
        assert!(other.exists());
        tidy(&temp.path().join("missing"));
    }

    #[tokio::test]
    async fn a_verified_package_is_left_pending_in_its_own_folder() {
        let sums = format!("{ABC}  {MSI}\n").into_bytes();
        let (downloads, _) = serve(vec![(200, None, sums), (200, None, b"abc".to_vec())]).await;
        let temp = tempfile::tempdir().unwrap();
        let msi = Msi {
            downloads,
            scratch: temp.path().to_path_buf(),
        };
        let installed = msi
            .install("v0.5.0".into(), Box::new(|_| {}))
            .await
            .unwrap();
        assert!(matches!(installed, Installed::Pending(_)));
        let folder = fs::read_dir(temp.path()).unwrap().next().unwrap().unwrap();
        assert!(folder.file_name().to_string_lossy().starts_with(SCRATCH));
        assert_eq!(fs::read(folder.path().join(MSI)).unwrap(), b"abc");
    }

    #[tokio::test]
    async fn a_package_that_does_not_match_leaves_nothing() {
        let sums = format!("{ABC}  {MSI}\n").into_bytes();
        let (downloads, _) = serve(vec![(200, None, sums), (200, None, b"evil".to_vec())]).await;
        let temp = tempfile::tempdir().unwrap();
        let msi = Msi {
            downloads,
            scratch: temp.path().to_path_buf(),
        };
        let error = msi
            .install("v0.5.0".into(), Box::new(|_| {}))
            .await
            .unwrap_err();
        assert_eq!(
            error.0,
            format!("{MSI} does not match the release's SHA256SUMS; nothing was installed")
        );
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }
}
