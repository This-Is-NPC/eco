//! macOS: the folder `mise run install:macos` fills, `~/Applications/eco`: a
//! release's macOS archive unpacked beside it, then put in its place by a
//! rename. macOS lets a program that runs be renamed, so the daemon and its
//! windows keep running from the folder moved aside, which is deleted at once;
//! the folder keeps its path, so the PATH and the launchd agent stay as they are.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use futures::future::BoxFuture;
use tokio::process::Command;

use crate::installer::{self, Scratch};
use crate::paths;
use crate::ports::{InstallError, InstallProgress, InstallStage, Installer};

#[derive(Clone)]
pub struct Folder {
    /// Where the release files are, under each tag.
    downloads: String,
    /// The installed eco: `~/Applications/eco`.
    install: PathBuf,
    /// The system's tar, which unpacks the archive.
    tar: PathBuf,
}

impl Folder {
    pub fn new(downloads: &str) -> Self {
        Self {
            downloads: downloads.into(),
            install: paths::app_dir(),
            tar: PathBuf::from("/usr/bin/tar"),
        }
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

impl Installer for Folder {
    fn program(&self) -> PathBuf {
        self.install.join("bin/eco")
    }

    fn install(
        &self,
        tag: String,
        progress: InstallProgress,
    ) -> BoxFuture<'static, Result<(), InstallError>> {
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
            .await?;
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
            swap(&staged, &folder.install)
        })
    }
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;
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
        };
        assert_eq!(
            folder.program(),
            PathBuf::from("/Users/a/Applications/eco/bin/eco")
        );
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
