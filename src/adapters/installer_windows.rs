//! The folder install.ps1 fills, `%LOCALAPPDATA%\Programs\eco`: a release's
//! Windows zip unpacked beside it and moved in, file by file. Windows lets no
//! one overwrite or delete a program that runs or a library one loaded — the
//! daemon's own eco.exe, the windows' Qt — but lets them be renamed: each file
//! in the way is renamed aside, `<name>.<id>.eco-old`, and deleted at once
//! when nothing holds it, else by `tidy` the next time the daemon starts. The
//! folder keeps its path, so the Start-menu entry and the PATH stay as they are.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use tokio::process::Command;

use crate::installer::{self, Scratch};
use crate::ports::{InstallError, InstallProgress, InstallStage, Installer};

/// What ends the name of a file moved aside.
const ASIDE: &str = ".eco-old";

/// How long a file another program has open is waited for, as install.ps1 waits.
const PATIENCE: Duration = Duration::from_secs(5);

/// Windows' ERROR_SHARING_VIOLATION: another program has the file open.
const SHARING_VIOLATION: i32 = 32;

#[derive(Clone)]
pub struct Folder {
    /// Where the release files are, under each tag.
    downloads: String,
    /// The installed eco: `%LOCALAPPDATA%\Programs\eco`.
    install: PathBuf,
    /// Windows' own tar, which unpacks zip files.
    tar: PathBuf,
}

impl Folder {
    pub fn new(downloads: &str) -> Self {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        Self {
            downloads: downloads.into(),
            install: var("LOCALAPPDATA")
                .unwrap_or_default()
                .join(r"Programs\eco"),
            tar: var("SystemRoot")
                .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
                .join(r"System32\tar.exe"),
        }
    }

    /// Delete the files an earlier update moved aside, and the folders left empty.
    pub fn tidy(&self) {
        for file in files(&self.install) {
            if is_aside(&file) {
                let _ = fs::remove_file(&file);
            }
        }
        prune(&self.install);
    }
}

/// The Windows zip of a release, as install.ps1 picks it:
/// `eco-<version>…-windows-x86_64.zip`.
fn is_package(name: &str) -> bool {
    name.strip_prefix("eco-")
        .and_then(|rest| rest.strip_suffix("-windows-x86_64.zip"))
        .is_some_and(|version| {
            version.starts_with(|c: char| c.is_ascii_digit()) && !version.contains(' ')
        })
}

/// What tar runs with to unpack `zip` into `into`.
fn arguments(zip: &Path, into: &Path) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = vec!["-x".into(), "-f".into(), zip.into()];
    arguments.extend(["-C".into(), into.into()]);
    arguments
}

fn is_aside(file: &Path) -> bool {
    file.to_string_lossy().ends_with(ASIDE)
}

/// Every file under `directory`, in its folders too; none when it is not there.
fn files(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => found.extend(files(&path)),
            Ok(_) => found.push(path),
            Err(_) => {}
        }
    }
    found
}

/// Delete the empty folders under `directory`, never `directory` itself.
fn prune(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            let path = entry.path();
            prune(&path);
            // Fails while it holds anything, which is kept.
            let _ = fs::remove_dir(&path);
        }
    }
}

/// Rename `from` to `to`, tried again while another program has `from` open
/// for a moment — a window reading a language pack it watches, a virus scan —
/// for up to `PATIENCE`.
fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
    let start = Instant::now();
    loop {
        match fs::rename(from, to) {
            Err(error)
                if error.raw_os_error() == Some(SHARING_VIOLATION)
                    && start.elapsed() < PATIENCE =>
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            done => return done,
        }
    }
}

/// Put back each file moved aside, as `(file, moved)`.
fn restore(aside: &[(PathBuf, PathBuf)]) {
    for (file, moved) in aside.iter().rev() {
        let _ = rename(moved, file);
    }
}

/// Make `install` hold the files under `staged`: every file there moved aside
/// first, then each staged file moved in. A failure puts back what was there.
/// What was moved aside is deleted unless something still holds it.
fn replace(staged: &Path, install: &Path) -> Result<(), InstallError> {
    let failed = |doing: &str, path: &Path, error: std::io::Error| {
        InstallError(format!("cannot {doing} {}: {error}", path.display()))
    };
    let id = uuid::Uuid::new_v4().simple().to_string();
    let mut aside = Vec::new();
    for file in files(install).into_iter().filter(|file| !is_aside(file)) {
        let mut moved = file.clone().into_os_string();
        moved.push(format!(".{id}{ASIDE}"));
        let moved = PathBuf::from(moved);
        if let Err(error) = rename(&file, &moved) {
            restore(&aside);
            return Err(failed("move aside", &file, error));
        }
        aside.push((file, moved));
    }
    let mut placed = Vec::new();
    for file in files(staged) {
        let target = install.join(file.strip_prefix(staged).expect("found under staged"));
        let moved = target
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| rename(&file, &target));
        if let Err(error) = moved {
            for placed in &placed {
                let _ = fs::remove_file(placed);
            }
            restore(&aside);
            return Err(failed("install", &target, error));
        }
        placed.push(target);
    }
    for (_, moved) in &aside {
        let _ = fs::remove_file(moved);
    }
    prune(install);
    Ok(())
}

impl Installer for Folder {
    fn program(&self) -> PathBuf {
        self.install.join(r"bin\eco.exe")
    }

    fn install(
        &self,
        tag: String,
        progress: InstallProgress,
    ) -> BoxFuture<'static, Result<(), InstallError>> {
        let folder = self.clone();
        Box::pin(async move {
            // Beside the install, so every file moves in by a rename.
            let parent = folder
                .install
                .parent()
                .ok_or_else(|| InstallError("LOCALAPPDATA is not set".into()))?;
            let scratch = Scratch::new(parent)?;
            let zip = installer::fetch(
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
                .args(arguments(&zip, &unpacked))
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
            // The zip holds one folder, eco, laid out as the install.
            let staged = unpacked.join("eco");
            if !staged.join(r"bin\eco.exe").is_file() {
                return Err(InstallError(format!(
                    "the zip of {tag} holds no eco\\bin\\eco.exe; nothing was installed"
                )));
            }
            let install = folder.install.clone();
            tokio::task::spawn_blocking(move || replace(&staged, &install))
                .await
                .map_err(|error| InstallError(error.to_string()))?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::http::testing::serve;

    /// The SHA-256 of `abc`.
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const ZIP: &str = "eco-0.5.0-windows-x86_64.zip";

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn the_zip_is_the_one_install_ps1_takes() {
        assert!(is_package(ZIP));
        assert!(!is_package("eco-0.5.0-1-x86_64.pkg.tar.zst"));
        assert!(!is_package("eco-windows-x86_64.zip"));
        assert_eq!(
            arguments(Path::new(r"C:\t\e.zip"), Path::new(r"C:\t\u")),
            ["-x", "-f", r"C:\t\e.zip", "-C", r"C:\t\u"].map(OsString::from)
        );
    }

    #[test]
    fn the_new_files_take_the_place_of_the_old_ones() {
        let directory = tempfile::tempdir().unwrap();
        let install = directory.path().join("eco");
        let staged = directory.path().join("staged");
        write(&install.join(r"bin\eco.exe"), "old");
        write(&install.join(r"lib\eco\gone.dll"), "old");
        write(&staged.join(r"bin\eco.exe"), "new");
        write(&staged.join(r"share\eco\overlay\shell.qml"), "new");
        replace(&staged, &install).unwrap();
        assert_eq!(
            fs::read_to_string(install.join(r"bin\eco.exe")).unwrap(),
            "new"
        );
        assert!(install.join(r"share\eco\overlay\shell.qml").is_file());
        assert!(!install.join("lib").exists());
        assert!(files(&install).iter().all(|file| !is_aside(file)));
    }

    #[test]
    fn a_file_another_program_holds_for_a_moment_is_waited_for() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = tempfile::tempdir().unwrap();
        let install = directory.path().join("eco");
        let staged = directory.path().join("staged");
        let pack = install.join(r"share\eco\overlay\Eco\Core\i18n\pt-BR.json");
        write(&pack, "old");
        write(
            &staged.join(r"share\eco\overlay\Eco\Core\i18n\pt-BR.json"),
            "new",
        );
        // Open as Qt opens a file it reads: shared for reading and writing, not
        // for deleting, which a rename needs.
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x2)
            .open(&pack)
            .unwrap();
        let reader = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(held);
        });
        let replaced = replace(&staged, &install);
        reader.join().unwrap();
        replaced.unwrap();
        assert_eq!(fs::read_to_string(&pack).unwrap(), "new");
        assert!(files(&install).iter().all(|file| !is_aside(file)));
    }

    /// What the copy below runs: it lasts until its stdin closes.
    #[test]
    #[ignore = "run by a_running_program_is_moved_aside_and_tidied_next_time"]
    fn lasts_until_stdin_closes() {
        let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut Vec::new());
    }

    #[test]
    fn a_running_program_is_moved_aside_and_tidied_next_time() {
        let directory = tempfile::tempdir().unwrap();
        let install = directory.path().join("eco");
        let staged = directory.path().join("staged");
        // A copy of this test's own executable, running from the install.
        let program = install.join(r"bin\eco.exe");
        fs::create_dir_all(program.parent().unwrap()).unwrap();
        fs::copy(std::env::current_exe().unwrap(), &program).unwrap();
        let mut running = std::process::Command::new(&program)
            .args(["--ignored", "--exact"])
            .arg("adapters::installer_windows::tests::lasts_until_stdin_closes")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        write(&staged.join(r"bin\eco.exe"), "new");
        let replaced = replace(&staged, &install);
        let aside: Vec<PathBuf> = files(&install)
            .into_iter()
            .filter(|f| is_aside(f))
            .collect();
        drop(running.stdin.take());
        running.wait().unwrap();
        replaced.unwrap();
        assert_eq!(fs::read_to_string(&program).unwrap(), "new");
        // Running, it could not be deleted: it waits aside for the next start.
        assert_eq!(aside.len(), 1, "{aside:?}");
        let folder = Folder {
            downloads: String::new(),
            install: install.clone(),
            tar: PathBuf::new(),
        };
        folder.tidy();
        assert_eq!(files(&install), [program]);
    }

    #[test]
    fn the_program_is_the_eco_in_the_install_bin() {
        let folder = Folder {
            downloads: String::new(),
            install: PathBuf::from(r"C:\Users\a\AppData\Local\Programs\eco"),
            tar: PathBuf::new(),
        };
        assert_eq!(
            folder.program(),
            PathBuf::from(r"C:\Users\a\AppData\Local\Programs\eco\bin\eco.exe")
        );
    }

    #[tokio::test]
    async fn a_zip_that_does_not_match_leaves_the_install_as_it_was() {
        let directory = tempfile::tempdir().unwrap();
        let install = directory.path().join("eco");
        write(&install.join(r"bin\eco.exe"), "old");
        let sums = format!("{ABC}  {ZIP}\n").into_bytes();
        let (downloads, _) = serve(vec![(200, None, sums), (200, None, b"evil".to_vec())]).await;
        let folder = Folder {
            downloads,
            install: install.clone(),
            // Never run: the zip does not match.
            tar: directory.path().join("no-tar.exe"),
        };
        let error = folder
            .install("v0.5.0".into(), Box::new(|_| {}))
            .await
            .unwrap_err();
        assert_eq!(
            error.0,
            format!("{ZIP} does not match the release's SHA256SUMS; nothing was installed")
        );
        assert_eq!(files(&install), [install.join(r"bin\eco.exe")]);
        assert_eq!(
            fs::read_to_string(install.join(r"bin\eco.exe")).unwrap(),
            "old"
        );
        // The scratch folder beside the install is gone.
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
