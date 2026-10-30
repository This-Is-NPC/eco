//! The Windows Installer package of a release, `eco-<version>-windows-x86_64.msi`,
//! installed per machine in `%ProgramFiles%\eco`. Windows Installer cannot
//! replace the files of an eco that runs, so the package is downloaded and
//! checked while eco runs, then `eco apply-update` does the rest from a copy
//! of eco outside the install: it stops the daemon, runs `msiexec` unelevated
//! (Windows Installer asks for the elevation itself), and starts eco again.
//! The shell starts that copy (`ShellExecuteEx`), so it shares no handle with
//! the caller, whose output a terminal would otherwise wait on.
//!
//! Removing eco is the same: `eco apply-uninstall` from such a copy stops the
//! daemon and runs `msiexec /x` for every eco product the registry lists.
//!
//! The package waits in a folder of the user's, which any program of theirs
//! may write, and Windows Installer runs it elevated. So the apply step is
//! handed its SHA-256 too, opens the package so that no one may write, move or
//! delete it while it is open, checks it again, and holds it until `msiexec`
//! has ended.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use futures::future::BoxFuture;
use sha2::{Digest, Sha256};
use tokio::process::Command;

use windows_registry::LOCAL_MACHINE;

use crate::adapters::control_socket::Address;
use crate::adapters::notify_windows::{self, Toasts};
use crate::adapters::service_windows::{CREATE_NO_WINDOW, hidden};
use crate::apply::{self, Steps};
use crate::installer::{self, SCRATCH, Scratch};
use crate::lifecycle;
use crate::ports::{
    InstallError, InstallProgress, Installed, Installer, Notifier, Pending, Urgency,
};

/// Where Windows Installer lists what it installed.
const UNINSTALL_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

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

/// The sharing a held package allows others: reading, as `msiexec` does, and
/// no writing, renaming or deleting (`FILE_SHARE_READ`).
const SHARE_READ: u32 = 0x1;

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

/// What `msiexec` runs with to remove the product `code` (`{GUID}`): the
/// same UI and no restart.
fn removal_arguments(code: &str) -> Vec<OsString> {
    vec!["/x".into(), code.into(), "/qb".into(), "/norestart".into()]
}

/// What `msiexec` ending with `code` means for an install or, with
/// `cancelled` saying so, a removal.
fn outcome(code: Option<i32>, cancelled: &str) -> Result<(), InstallError> {
    let failed = |why: String| Err(InstallError(why));
    match code {
        Some(0 | REBOOT_PENDING) => Ok(()),
        Some(code) if CANCELLED.contains(&code) => failed(cancelled.into()),
        Some(BUSY) => failed("another installation is running; try again once it ends".into()),
        Some(code) => failed(format!("the installer failed with exit code {code}")),
        None => failed("the installer ended without an exit code".into()),
    }
}

/// Whether an entry of the uninstall list is eco's: named `eco`, published by
/// `This-Is-NPC` and installed by Windows Installer, as install.ps1 finds it.
fn is_eco(name: Option<String>, publisher: Option<String>, windows_installer: Option<u32>) -> bool {
    name.as_deref() == Some("eco")
        && publisher.as_deref() == Some("This-Is-NPC")
        && windows_installer == Some(1)
}

/// The product codes of every installed eco: more than one when an eco
/// installed twice at the same version by an older package left both.
fn products() -> Result<Vec<String>, InstallError> {
    let list = LOCAL_MACHINE
        .open(UNINSTALL_KEY)
        .map_err(|error| InstallError(format!("cannot read the installed programs: {error}")))?;
    let keys = list
        .keys()
        .map_err(|error| InstallError(format!("cannot read the installed programs: {error}")))?;
    Ok(keys
        .filter(|code| {
            list.open(code).is_ok_and(|entry| {
                is_eco(
                    entry.get_string("DisplayName").ok(),
                    entry.get_string("Publisher").ok(),
                    entry.get_u32("WindowsInstaller").ok(),
                )
            })
        })
        .collect())
}

/// The programs of eco that hold its install's files open while they run,
/// besides the daemon `lifecycle::shut` already waited for.
const WINDOWS: [&str; 2] = ["eco-window.exe", "eco-launch.exe"];

/// How long eco's windows are waited for to close before Windows Installer
/// runs; one still open past it keeps its files until a restart.
const WINDOWS_PATIENCE: std::time::Duration = std::time::Duration::from_secs(10);

/// `tasklist` listing every process by name, one CSV line each.
fn tasklist() -> std::process::Command {
    let mut command = std::process::Command::new("tasklist");
    command
        .args(["/FO", "CSV", "/NH"])
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    command
}

/// Whether the `tasklist` listing `listing` holds one of eco's windows.
fn windows_open(listing: &str) -> bool {
    listing.lines().any(|line| {
        WINDOWS
            .iter()
            .any(|name| line.starts_with(&format!("\"{name}\"")))
    })
}

/// Wait, up to `WINDOWS_PATIENCE`, for eco's windows to close after its daemon
/// stopped: Windows Installer cannot remove or replace a file one holds.
async fn windows_closed() {
    let until = tokio::time::Instant::now() + WINDOWS_PATIENCE;
    while tokio::time::Instant::now() < until {
        let listing = Command::from(tasklist()).output().await;
        if listing.is_ok_and(|output| !windows_open(&String::from_utf8_lossy(&output.stdout))) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

/// Run `msiexec` as the installer asks, removing the product `code`.
async fn remove_product(msiexec: &Path, code: &str) -> Result<(), InstallError> {
    let status = Command::new(msiexec)
        .args(removal_arguments(code))
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .await
        .map_err(|error| InstallError(format!("cannot run {}: {error}", msiexec.display())))?;
    outcome(status.code(), "the removal was cancelled; nothing changed")
}

/// The path of `msiexec` in this Windows.
fn msiexec_path() -> PathBuf {
    let system = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    PathBuf::from(system).join(r"System32\msiexec.exe")
}

/// The parameters that run `eco apply-uninstall`.
fn removal_parameters(everything: bool) -> &'static str {
    match everything {
        true => "apply-uninstall --everything",
        false => "apply-uninstall",
    }
}

/// A removal waiting for the daemon to stop.
#[derive(Debug)]
struct Removal {
    everything: bool,
    /// Where the copy of eco that removes it is put.
    scratch: PathBuf,
}

impl Pending for Removal {
    fn hand_off(&self) -> io::Result<()> {
        let folder = Scratch::new(&self.scratch)
            .map_err(|error| io::Error::other(error.0))?
            .keep();
        let copy = folder.join(COPY);
        fs::copy(std::env::current_exe()?, &copy)?;
        hidden(&copy, removal_parameters(self.everything)).map_err(io::Error::other)
    }
}

/// The parameters that run `eco apply-update` on `msi`, whose SHA-256 is
/// `sha256`.
fn parameters(msi: &Path, sha256: &[u8; 32]) -> String {
    format!("apply-update \"{}\" {}", msi.display(), hex(sha256))
}

/// The 64 lower-case hex digits of `digest`, as SHA256SUMS writes them.
fn hex(digest: &[u8; 32]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The SHA-256 of what `reader` holds, read to its end.
fn sha256_of(mut reader: impl std::io::Read) -> std::io::Result<[u8; 32]> {
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        match reader.read(&mut buffer)? {
            0 => break,
            read => hash.update(&buffer[..read]),
        }
    }
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&hash.finalize());
    Ok(digest)
}

/// `package`, opened so that it stays as it is while it is held, once it is
/// checked to have the SHA-256 `sha256`.
fn hold(package: &Path, sha256: &[u8; 32]) -> Result<File, InstallError> {
    let name = package.display();
    let file = fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ)
        .open(package)
        .map_err(|error| InstallError(format!("cannot open {name}: {error}")))?;
    let found =
        sha256_of(&file).map_err(|error| InstallError(format!("cannot read {name}: {error}")))?;
    if found != *sha256 {
        return Err(InstallError(format!(
            "{name} changed after it was checked; nothing was installed"
        )));
    }
    Ok(file)
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
    sha256: [u8; 32],
}

impl Pending for Package {
    fn hand_off(&self) -> io::Result<()> {
        let copy = self.msi.with_file_name(COPY);
        fs::copy(std::env::current_exe()?, &copy)?;
        hidden(&copy, &parameters(&self.msi, &self.sha256)).map_err(io::Error::other)
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
            let fetched =
                installer::fetch(&downloads, &tag, is_package, scratch.path(), &progress).await?;
            scratch.keep();
            Ok(Installed::Pending(Box::new(Package {
                msi: fetched.path,
                sha256: fetched.sha256,
            })))
        })
    }

    fn uninstall(&self, everything: bool) -> Box<dyn Pending> {
        Box::new(Removal {
            everything,
            scratch: self.scratch.clone(),
        })
    }

    fn remove(&self) -> BoxFuture<'static, Result<(), InstallError>> {
        Box::pin(async {
            let codes = products()?;
            if codes.is_empty() {
                return Err(InstallError(
                    "eco is not installed with Windows Installer; nothing to remove".into(),
                ));
            }
            windows_closed().await;
            let msiexec = msiexec_path();
            for code in codes {
                remove_product(&msiexec, &code).await?;
            }
            // The toasts registered eco's name under the user's hive.
            notify_windows::unregister(notify_windows::APP_ID);
            Ok(())
        })
    }
}

/// The apply step on this machine: the daemon of the user, the installed eco,
/// Windows Installer, and toasts for what goes wrong.
struct Applying {
    address: Address,
    /// The SHA-256 the package was checked to have.
    sha256: [u8; 32],
    program: PathBuf,
    msiexec: PathBuf,
    toasts: Toasts,
}

impl Applying {
    fn new(sha256: [u8; 32]) -> Self {
        Self {
            address: Address::user(),
            sha256,
            program: installed_program(),
            msiexec: msiexec_path(),
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
            // Held until msiexec has ended: what it installs is what was checked.
            let _held = hold(&package, &self.sha256)?;
            windows_closed().await;
            let status = Command::new(&self.msiexec)
                .args(arguments(&package))
                .stdin(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .status()
                .await
                .map_err(|error| {
                    InstallError(format!("cannot run {}: {error}", self.msiexec.display()))
                })?;
            outcome(status.code(), "the installation was cancelled")
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

/// `eco apply-update`: install `package`, which has the SHA-256 `sha256`, with
/// the daemon stopped.
pub async fn apply(package: &Path, sha256: [u8; 32]) {
    apply::run(&Applying::new(sha256), package).await;
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
        let cancelled = "the installation was cancelled";
        assert!(outcome(Some(0), cancelled).is_ok());
        assert!(outcome(Some(3010), cancelled).is_ok());
        for (code, why) in [
            (1602, "the installation was cancelled"),
            (1223, "the installation was cancelled"),
            (
                1618,
                "another installation is running; try again once it ends",
            ),
            (1603, "the installer failed with exit code 1603"),
        ] {
            assert_eq!(outcome(Some(code), cancelled).unwrap_err().0, why, "{code}");
        }
        assert_eq!(
            outcome(None, cancelled).unwrap_err().0,
            "the installer ended without an exit code"
        );
    }

    #[test]
    fn a_cancelled_removal_says_nothing_changed() {
        let cancelled = "the removal was cancelled; nothing changed";
        assert_eq!(outcome(Some(1602), cancelled).unwrap_err().0, cancelled);
        assert!(outcome(Some(0), cancelled).is_ok());
    }

    #[test]
    fn the_installer_removes_a_product_with_its_ui_and_no_restart() {
        assert_eq!(
            removal_arguments("{ABC}"),
            ["/x", "{ABC}", "/qb", "/norestart"]
        );
    }

    #[test]
    fn only_eco_s_own_package_is_eco() {
        let name = || Some("eco".to_string());
        let publisher = || Some("This-Is-NPC".to_string());
        assert!(is_eco(name(), publisher(), Some(1)));
        assert!(!is_eco(Some("ecosystem".into()), publisher(), Some(1)));
        assert!(!is_eco(name(), Some("Someone".into()), Some(1)));
        assert!(!is_eco(name(), publisher(), Some(0)));
        assert!(!is_eco(name(), publisher(), None));
        assert!(!is_eco(None, None, None));
    }

    #[test]
    fn the_removal_runs_apply_uninstall_with_or_without_the_data() {
        assert_eq!(removal_parameters(false), "apply-uninstall");
        assert_eq!(removal_parameters(true), "apply-uninstall --everything");
    }

    #[test]
    fn msiexec_is_in_the_system_directory() {
        assert!(msiexec_path().ends_with(r"System32\msiexec.exe"));
    }

    #[test]
    fn a_removal_keeps_what_it_was_asked_to_delete() {
        let pending = Msi::new("").uninstall(true);
        assert!(format!("{pending:?}").contains("everything: true"));
    }

    #[test]
    fn the_copy_is_told_the_quoted_package_and_its_checksum() {
        let sha256 = installer::checksum(ABC).unwrap();
        assert_eq!(
            parameters(Path::new(r"C:\Users\Zoe Lima\eco.msi"), &sha256),
            format!(r#"apply-update "C:\Users\Zoe Lima\eco.msi" {ABC}"#)
        );
    }

    #[test]
    fn tasklist_lists_every_process_by_name() {
        let built = tasklist();
        let args: Vec<_> = built.get_args().collect();
        assert_eq!(built.get_program(), "tasklist");
        assert_eq!(args, ["/FO", "CSV", "/NH"]);
    }

    #[test]
    fn a_window_of_eco_in_the_listing_is_one_still_open() {
        let idle = "\"System\",\"4\",\"Services\",\"0\",\"144 K\"\r\n\"eco.exe\",\"7\",\"Console\",\"2\",\"9 K\"\r\n";
        assert!(!windows_open(idle));
        for name in WINDOWS {
            let open = format!("{idle}\"{name}\",\"9\",\"Console\",\"2\",\"40 K\"\r\n");
            assert!(windows_open(&open), "{name}");
        }
    }

    #[test]
    fn a_digest_reads_back_as_its_hex_and_a_file_hashes_to_it() {
        let digest = sha256_of(&b"abc"[..]).unwrap();
        assert_eq!(hex(&digest), ABC);
        assert_eq!(installer::checksum(&hex(&digest)), Some(digest));
    }

    #[test]
    fn a_held_package_cannot_be_written_moved_or_deleted() {
        let directory = tempfile::tempdir().unwrap();
        let msi = directory.path().join(MSI);
        fs::write(&msi, "abc").unwrap();
        let held = hold(&msi, &installer::checksum(ABC).unwrap()).unwrap();
        assert!(fs::write(&msi, "evil").is_err());
        assert!(fs::rename(&msi, directory.path().join("moved.msi")).is_err());
        assert!(fs::remove_file(&msi).is_err());
        // What msiexec does: read it beside the hold.
        assert_eq!(fs::read(&msi).unwrap(), b"abc");
        drop(held);
        fs::write(&msi, "free").unwrap();
    }

    #[test]
    fn a_package_changed_after_its_check_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let msi = directory.path().join(MSI);
        fs::write(&msi, "abd").unwrap();
        let error = hold(&msi, &installer::checksum(ABC).unwrap()).unwrap_err();
        assert!(
            error
                .0
                .ends_with("changed after it was checked; nothing was installed"),
            "{error}"
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
