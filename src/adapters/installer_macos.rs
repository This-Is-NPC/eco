//! macOS: the folder `mise run install:macos` fills, `~/Applications/eco`: a
//! release's macOS archive unpacked beside it, then put in its place by a
//! rename. macOS lets a program that runs be renamed, so the daemon and its
//! windows keep running from the folder moved aside, which is deleted at once;
//! the folder keeps its path, so the PATH and the launchd agent stay as they are.
//! Removing eco is `eco apply-uninstall` in a process of its own, which stops
//! the daemon first and outlives it, then deletes the folder and the agent.

use std::ffi::OsString;
use std::fs;
use std::io::{self, IsTerminal};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use futures::future::BoxFuture;
use tokio::process::Command;

use crate::adapters::service_launchd;
use crate::installer::{self, Scratch};
use crate::paths;
use crate::ports::{InstallError, InstallProgress, InstallStage, Installed, Installer, Pending};

#[derive(Clone)]
pub struct Folder {
    /// Where the release files are, under each tag.
    downloads: String,
    /// The installed eco: `~/Applications/eco`.
    install: PathBuf,
    /// The system's tar, which unpacks the archive.
    tar: PathBuf,
    /// Whether eco was asked in a terminal, where a removal is then waited for.
    terminal: bool,
}

impl Folder {
    pub fn new(downloads: &str) -> Self {
        Self {
            downloads: downloads.into(),
            install: paths::app_dir(),
            tar: PathBuf::from("/usr/bin/tar"),
            terminal: io::stdin().is_terminal(),
        }
    }
}

/// What the removal runs: `eco apply-uninstall`, with `--everything` to delete
/// the user's files too.
fn removal_arguments(everything: bool) -> Vec<OsString> {
    let mut arguments = vec![OsString::from("apply-uninstall")];
    if everything {
        arguments.push("--everything".into());
    }
    arguments
}

/// A removal waiting for the daemon to stop.
#[derive(Debug)]
struct Removal {
    /// The eco that runs it, this one: macOS lets it be deleted as it runs.
    exe: PathBuf,
    everything: bool,
    /// Whether eco was asked in a terminal, which the removal then keeps and
    /// is waited for in.
    terminal: bool,
}

impl Pending for Removal {
    fn hand_off(&self) -> io::Result<()> {
        let mut command = std::process::Command::new(&self.exe);
        command.args(removal_arguments(self.everything));
        if self.terminal {
            return match command.status()? {
                status if status.success() => Ok(()),
                status => Err(io::Error::other(format!("the removal failed ({status})"))),
            };
        }
        // A group of its own, so it outlives the daemon it stops.
        command
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(drop)
    }
}

/// The macOS archive of a release: `eco-<version>…-macos-aarch64.tar.gz`.
fn is_package(name: &str) -> bool {
    name.strip_prefix("eco-")
        .and_then(|rest| rest.strip_suffix("-macos-aarch64.tar.gz"))
        .is_some_and(|version| {
            version.starts_with(|c: char| c.is_ascii_digit()) && !version.contains(' ')
        })
}

/// What tar runs with to unpack `archive` into `into`.
fn arguments(archive: &Path, into: &Path) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = vec!["-x".into(), "-z".into(), "-f".into(), archive.into()];
    arguments.extend(["-C".into(), into.into()]);
    arguments
}

/// Put `staged` where `install` is: the installed folder moved aside first,
/// then the staged one moved in, then the old one deleted. A failure puts
/// back what was there.
fn swap(staged: &Path, install: &Path) -> Result<(), InstallError> {
    let failed = |doing: &str, path: &Path, error: std::io::Error| {
        InstallError(format!("cannot {doing} {}: {error}", path.display()))
    };
    let mut aside = install.as_os_str().to_owned();
    aside.push(format!(".{}.eco-old", uuid::Uuid::new_v4().simple()));
    let aside = PathBuf::from(aside);
    let had = install.exists();
    if had {
        fs::rename(install, &aside).map_err(|error| failed("move aside", install, error))?;
    }
    if let Err(error) = fs::rename(staged, install) {
        if had {
            let _ = fs::rename(&aside, install);
        }
        return Err(failed("install", install, error));
    }
    if had {
        let _ = fs::remove_dir_all(&aside);
    }
    Ok(())
}

/// Delete the installed folder; nothing to do when it is not there.
fn remove_folder(install: &Path) -> Result<(), InstallError> {
    match fs::remove_dir_all(install) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(InstallError(format!(
            "cannot delete {}: {error}",
            install.display()
        ))),
        _ => Ok(()),
    }
}

impl Installer for Folder {
    fn program(&self) -> PathBuf {
        self.install.join("bin/eco")
    }

    fn uninstall(&self, everything: bool) -> Box<dyn Pending> {
        Box::new(Removal {
            exe: std::env::current_exe().unwrap_or_else(|_| self.program()),
            everything,
            terminal: self.terminal,
        })
    }

    fn remove(&self) -> BoxFuture<'static, Result<(), InstallError>> {
        let install = self.install.clone();
        Box::pin(async move {
            service_launchd::take_off()
                .await
                .map_err(|error| InstallError(format!("{error:#}")))?;
            remove_folder(&install)
        })
    }

    fn install(
        &self,
        tag: String,
        progress: InstallProgress,
    ) -> BoxFuture<'static, Result<Installed, InstallError>> {
        let folder = self.clone();
        Box::pin(async move {
            // Beside the install, so the folder moves in by a rename.
            let parent = folder
                .install
                .parent()
                .ok_or_else(|| InstallError("HOME is not set".into()))?;
            fs::create_dir_all(parent).map_err(|error| {
                InstallError(format!("cannot create {}: {error}", parent.display()))
            })?;
            let scratch = Scratch::new(parent)?;
            let archive = installer::fetch(
                &folder.downloads,
                &tag,
                is_package,
                scratch.path(),
                &progress,
            )
            .await?
            .path;
            progress(InstallStage::Installing);
            let unpacked = scratch.path().join("unpacked");
            fs::create_dir(&unpacked)
                .map_err(|error| InstallError(format!("cannot unpack {tag}: {error}")))?;
            let output = Command::new(&folder.tar)
                .args(arguments(&archive, &unpacked))
                .stdin(Stdio::null())
                .output()
                .await
                .map_err(|error| {
                    InstallError(format!("cannot run {}: {error}", folder.tar.display()))
                })?;
            if !output.status.success() {
                let said = String::from_utf8_lossy(&output.stderr);
                let last = said.lines().rfind(|line| !line.trim().is_empty());
                return Err(InstallError(format!(
                    "cannot unpack {tag}: {}",
                    last.unwrap_or("tar failed").trim()
                )));
            }
            // The archive holds one folder, eco, laid out as the install.
            let staged = unpacked.join("eco");
            if !staged.join("bin/eco").is_file() {
                return Err(InstallError(format!(
                    "the archive of {tag} holds no eco/bin/eco; nothing was installed"
                )));
            }
            swap(&staged, &folder.install).map(|()| Installed::Done)
        })
    }
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::adapters::fake_program::fake_program;
    use crate::adapters::http::testing::serve;

    const ARCHIVE: &str = "eco-0.5.0-macos-aarch64.tar.gz";

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// A release archive of one folder, eco, holding `files`, as `(path, text)`.
    fn archive(dir: &Path, files: &[(&str, &str)]) -> Vec<u8> {
        let source = dir.join("source");
        for (path, text) in files {
            write(&source.join("eco").join(path), text);
        }
        let archive = dir.join(ARCHIVE);
        let made = std::process::Command::new("/usr/bin/tar")
            .args(["-c", "-z", "-f"])
            .arg(&archive)
            .arg("-C")
            .arg(&source)
            .arg("eco")
            .status()
            .unwrap();
        assert!(made.success());
        fs::read(archive).unwrap()
    }

    fn sums(archive: &[u8]) -> Vec<u8> {
        let digest: String = Sha256::digest(archive)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!("{digest}  {ARCHIVE}\n").into_bytes()
    }

    #[test]
    fn the_archive_is_the_macos_one_and_tar_unpacks_it() {
        assert!(is_package(ARCHIVE));
        assert!(!is_package("eco-0.5.0-windows-x86_64.zip"));
        assert!(!is_package("eco-macos-aarch64.tar.gz"));
        assert_eq!(
            arguments(Path::new("/t/e.tar.gz"), Path::new("/t/u")),
            ["-x", "-z", "-f", "/t/e.tar.gz", "-C", "/t/u"].map(OsString::from)
        );
    }

    #[test]
    fn the_program_is_the_eco_in_the_install_bin() {
        let folder = Folder {
            downloads: String::new(),
            install: PathBuf::from("/Users/a/Applications/eco"),
            tar: PathBuf::new(),
            terminal: false,
        };
        assert_eq!(
            folder.program(),
            PathBuf::from("/Users/a/Applications/eco/bin/eco")
        );
    }

    #[test]
    fn the_removal_is_apply_uninstall_and_deletes_the_data_only_when_asked() {
        assert_eq!(removal_arguments(false), ["apply-uninstall"]);
        assert_eq!(removal_arguments(true), ["apply-uninstall", "--everything"]);
    }

    #[test]
    fn a_removal_asked_in_a_terminal_is_waited_for_and_says_when_it_failed() {
        let directory = tempfile::tempdir().unwrap();
        let calls = directory.path().join("calls");
        let eco = fake_program(
            directory.path(),
            "eco",
            &format!(
                "echo \"$*\" > '{}'\n[ \"$2\" = --everything ]",
                calls.display()
            ),
        );
        let removal = |everything| Removal {
            exe: eco.clone(),
            everything,
            terminal: true,
        };
        removal(true).hand_off().unwrap();
        assert_eq!(
            fs::read_to_string(&calls).unwrap(),
            "apply-uninstall --everything\n"
        );
        let error = removal(false).hand_off().unwrap_err();
        assert!(
            error.to_string().starts_with("the removal failed"),
            "{error}"
        );
    }

    #[test]
    fn a_removal_asked_elsewhere_runs_on_its_own() {
        let directory = tempfile::tempdir().unwrap();
        let calls = directory.path().join("calls");
        let eco = fake_program(
            directory.path(),
            "eco",
            &format!("echo \"$*\" > '{}'", calls.display()),
        );
        let removal = Removal {
            exe: eco,
            everything: false,
            terminal: false,
        };
        removal.hand_off().unwrap();
        let start = std::time::Instant::now();
        while fs::read_to_string(&calls).unwrap_or_default() != "apply-uninstall\n" {
            assert!(start.elapsed() < std::time::Duration::from_secs(5));
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn the_folder_is_deleted_and_a_missing_one_is_nothing_to_do() {
        let directory = tempfile::tempdir().unwrap();
        let install = directory.path().join("eco");
        write(&install.join("bin/eco"), "old");
        remove_folder(&install).unwrap();
        assert!(!install.exists());
        remove_folder(&install).unwrap();
    }

    #[test]
    fn a_folder_that_cannot_move_in_leaves_the_install_as_it_was() {
        let directory = tempfile::tempdir().unwrap();
        let install = directory.path().join("eco");
        write(&install.join("bin/eco"), "old");
        let error = swap(&directory.path().join("gone"), &install).unwrap_err();
        assert!(error.0.starts_with("cannot install"), "{}", error.0);
        assert_eq!(fs::read_to_string(install.join("bin/eco")).unwrap(), "old");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn a_release_takes_the_place_of_the_install() {
        let directory = tempfile::tempdir().unwrap();
        let install = directory.path().join("Applications/eco");
        write(&install.join("bin/eco"), "old");
        write(&install.join("lib/eco/gone"), "old");
        let packed = archive(
            directory.path(),
            &[("bin/eco", "new"), ("share/eco/overlay/shell.qml", "new")],
        );
        let (downloads, _) = serve(vec![(200, None, sums(&packed)), (200, None, packed)]).await;
        let folder = Folder {
            downloads,
            install: install.clone(),
            tar: PathBuf::from("/usr/bin/tar"),
            terminal: false,
        };
        folder
            .install("v0.5.0".into(), Box::new(|_| {}))
            .await
            .unwrap();
        assert_eq!(fs::read_to_string(install.join("bin/eco")).unwrap(), "new");
        assert!(install.join("share/eco/overlay/shell.qml").is_file());
        assert!(!install.join("lib").exists());
        // Only the install is left beside it: no scratch, nothing moved aside.
        let beside: Vec<_> = fs::read_dir(install.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(beside, ["eco"]);
    }

    #[tokio::test]
    async fn an_archive_without_eco_installs_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let install = directory.path().join("Applications/eco");
        write(&install.join("bin/eco"), "old");
        let packed = archive(directory.path(), &[("README.txt", "no program")]);
        let (downloads, _) = serve(vec![(200, None, sums(&packed)), (200, None, packed)]).await;
        let folder = Folder {
            downloads,
            install: install.clone(),
            tar: PathBuf::from("/usr/bin/tar"),
            terminal: false,
        };
        let error = folder
            .install("v0.5.0".into(), Box::new(|_| {}))
            .await
            .unwrap_err();
        assert_eq!(
            error.0,
            "the archive of v0.5.0 holds no eco/bin/eco; nothing was installed"
        );
        assert_eq!(fs::read_to_string(install.join("bin/eco")).unwrap(), "old");
    }
}
