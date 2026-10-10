//! Windows: eco started at the user's login by a script in their Startup
//! folder, which Windows runs as the user logs in.

use std::fs;
use std::path::{Path, PathBuf};

use crate::ports::DesktopIntegration;

/// Writes the login script into `folder`.
pub struct StartupFolder {
    folder: PathBuf,
}

impl StartupFolder {
    pub fn new(folder: PathBuf) -> Self {
        Self { folder }
    }
}

impl DesktopIntegration for StartupFolder {
    fn integrate(&self) -> Result<String, String> {
        let exe = std::env::current_exe().map_err(|error| {
            format!("cannot find this eco, so it does not start at login: {error}")
        })?;
        install(&self.folder, &exe)
    }
}

/// The script's name in the Startup folder.
const SCRIPT: &str = "eco.cmd";

/// The script that starts the daemon of `exe` without a window: the window is
/// not built on Windows. cmd reads `%` in a script as a variable, so a `%` in
/// the path is doubled.
fn script(exe: &Path) -> String {
    let exe = exe.display().to_string().replace('%', "%%");
    format!(
        "@echo off\r\n\
         rem Written by `eco setup`: starts eco at login. Delete this file to stop it.\r\n\
         \"{exe}\" start --headless\r\n"
    )
}

/// Make `folder` hold the script that starts `exe` at login: write it when it
/// is missing or names another eco, and leave it alone when it is current.
/// `Err` is a warning: the script could not be written.
fn install(folder: &Path, exe: &Path) -> Result<String, String> {
    let path = folder.join(SCRIPT);
    let shown = path.display();
    let wanted = script(exe);
    let outcome = match fs::read_to_string(&path) {
        Ok(found) if found == wanted => return Ok(format!("login start in {shown} is up to date")),
        Ok(_) => "updated",
        Err(_) => "added",
    };
    fs::create_dir_all(folder)
        .and_then(|()| fs::write(&path, wanted))
        .map_err(|error| {
            format!("could not write {shown}, so eco does not start at login: {error}")
        })?;
    Ok(format!(
        "login start {outcome} in {shown}; eco starts with your next login"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_starts_the_daemon_quietly() {
        assert_eq!(
            script(Path::new(r"C:\Program Files\eco 100%\eco.exe")),
            "@echo off\r\n\
             rem Written by `eco setup`: starts eco at login. Delete this file to stop it.\r\n\
             \"C:\\Program Files\\eco 100%%\\eco.exe\" start --headless\r\n"
        );
    }

    #[test]
    fn adds_the_script_and_a_rerun_changes_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let folder = directory.path().join("Startup");
        let exe = Path::new(r"C:\eco\eco.exe");
        let path = folder.join("eco.cmd");
        assert_eq!(
            install(&folder, exe),
            Ok(format!(
                "login start added in {}; eco starts with your next login",
                path.display()
            ))
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), script(exe));
        assert_eq!(
            install(&folder, exe),
            Ok(format!("login start in {} is up to date", path.display()))
        );
    }

    #[test]
    fn rewrites_the_script_of_an_eco_that_moved() {
        let directory = tempfile::tempdir().unwrap();
        let folder = directory.path();
        install(folder, Path::new(r"C:\old\eco.exe")).unwrap();
        let moved = Path::new(r"C:\new\eco.exe");
        let path = folder.join("eco.cmd");
        assert_eq!(
            install(folder, moved),
            Ok(format!(
                "login start updated in {}; eco starts with your next login",
                path.display()
            ))
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), script(moved));
    }

    #[test]
    fn a_folder_it_cannot_write_is_a_warning() {
        let directory = tempfile::tempdir().unwrap();
        let folder = directory.path().join("Startup");
        fs::write(&folder, "a file, not a folder").unwrap();
        let warning = install(&folder, Path::new(r"C:\eco\eco.exe")).unwrap_err();
        assert!(
            warning.starts_with(&format!(
                "could not write {}, so eco does not start at login: ",
                folder.join("eco.cmd").display()
            )),
            "{warning}"
        );
    }

    #[test]
    fn the_script_runs_its_eco_with_start_headless() {
        let directory = tempfile::tempdir().unwrap();
        let said = directory.path().join("said.txt");
        // A stand-in eco that writes what it was asked.
        let exe = directory.path().join("eco 1%.cmd");
        fs::write(&exe, format!("@echo %*> \"{}\"\r\n", said.display())).unwrap();
        install(directory.path(), &exe).unwrap();
        let status = std::process::Command::new("cmd")
            .args([
                "/C",
                &directory.path().join("eco.cmd").display().to_string(),
            ])
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(fs::read_to_string(said).unwrap(), "start --headless\r\n");
    }
}
