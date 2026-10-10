//! Windows: eco started at the user's login by a script in their Startup
//! folder, which Windows runs as the user logs in, and opened from an entry
//! in their Start menu.

use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::adapters::overlay;
use crate::adapters::service_windows::CREATE_NO_WINDOW;
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

/// Puts the shortcut "eco" into `folder`, the Start menu's.
pub struct StartMenu {
    folder: PathBuf,
}

impl StartMenu {
    pub fn new(folder: PathBuf) -> Self {
        Self { folder }
    }
}

impl DesktopIntegration for StartMenu {
    fn integrate(&self) -> Result<String, String> {
        let exe = std::env::current_exe().map_err(|error| {
            format!("cannot find this eco, so it is not in the Start menu: {error}")
        })?;
        let link = self.folder.join(SHORTCUT);
        let output = shortcut(&link, &exe, &overlay::program())
            .output()
            .map_err(|error| {
                format!("cannot run powershell, so eco is not in the Start menu: {error}")
            })?;
        said(&link, &output)
    }
}

/// The shortcut's name in the Start menu.
const SHORTCUT: &str = "eco.lnk";

/// Saves the shortcut `$env:ECO_LINK` through the shell's COM object, unless
/// it already holds these fields; prints `current`, `added` or `updated`.
/// The values come in variables, so no path is quoted inside the script.
const SAVE: &str = "$ErrorActionPreference = 'Stop'; \
     $found = Test-Path -LiteralPath $env:ECO_LINK; \
     $link = (New-Object -ComObject WScript.Shell).CreateShortcut($env:ECO_LINK); \
     if ($found -and $link.TargetPath -eq $env:ECO_TARGET -and $link.Arguments -eq $env:ECO_ARGUMENTS \
         -and $link.IconLocation -eq $env:ECO_ICON -and $link.WorkingDirectory -eq $env:ECO_DIRECTORY) \
     { 'current'; exit }; \
     New-Item -ItemType Directory -Force -Path (Split-Path -LiteralPath $env:ECO_LINK) | Out-Null; \
     $link.TargetPath = $env:ECO_TARGET; $link.Arguments = $env:ECO_ARGUMENTS; \
     $link.IconLocation = $env:ECO_ICON; $link.WorkingDirectory = $env:ECO_DIRECTORY; \
     $link.WindowStyle = 7; $link.Description = 'eco'; $link.Save(); \
     if ($found) { 'updated' } else { 'added' }";

/// PowerShell saving at `link` a shortcut that runs `exe start` through
/// `conhost --headless`, a console host that shows no window, with the icon
/// of the window program `window`.
fn shortcut(link: &Path, exe: &Path, window: &Path) -> Command {
    let system =
        std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    let mut icon = window.as_os_str().to_owned();
    icon.push(",0");
    let mut arguments = std::ffi::OsString::from("--headless \"");
    arguments.push(exe);
    arguments.push("\" start");
    let mut command = Command::new("powershell");
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", SAVE])
        .env("ECO_LINK", link)
        .env("ECO_TARGET", system.join(r"System32\conhost.exe"))
        .env("ECO_ARGUMENTS", arguments)
        .env("ECO_ICON", icon)
        .env("ECO_DIRECTORY", exe.parent().unwrap_or(exe))
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    command
}

/// What saving the shortcut at `link` did, from PowerShell's `output`.
fn said(link: &Path, output: &std::process::Output) -> Result<String, String> {
    let shown = link.display();
    if !output.status.success() {
        return Err(format!(
            "could not write {shown}, so eco is not in the Start menu: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    match String::from_utf8_lossy(&output.stdout).trim() {
        "current" => Ok(format!("Start menu entry {shown} is up to date")),
        done => Ok(format!("Start menu entry {done} in {shown}")),
    }
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

    /// The fields of the shortcut at `link`, read back through the shell.
    fn read_shortcut(link: &Path) -> Vec<String> {
        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "$l = (New-Object -ComObject WScript.Shell).CreateShortcut($env:ECO_LINK); \
                 $l.TargetPath; $l.Arguments; $l.IconLocation; $l.WorkingDirectory; $l.WindowStyle",
            ])
            .env("ECO_LINK", link)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(String::from)
            .collect()
    }

    #[test]
    fn the_shortcut_runs_eco_start_in_a_hidden_console() {
        let built = shortcut(
            Path::new(r"C:\menu\eco.lnk"),
            Path::new(r"C:\Program Files\eco\bin\eco.exe"),
            Path::new(r"C:\Program Files\eco\lib\eco\eco-window.exe"),
        );
        assert_eq!(built.get_program(), "powershell");
        let args: Vec<_> = built.get_args().collect();
        assert_eq!(args, ["-NoProfile", "-NonInteractive", "-Command", SAVE]);
        let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap());
        let conhost = system.join(r"System32\conhost.exe");
        let envs: Vec<_> = built.get_envs().collect();
        let value = |name: &str| {
            envs.iter()
                .find(|(key, _)| *key == name)
                .and_then(|(_, value)| *value)
                .unwrap()
                .to_owned()
        };
        assert_eq!(value("ECO_LINK"), r"C:\menu\eco.lnk");
        assert_eq!(value("ECO_TARGET"), conhost.as_os_str());
        assert_eq!(
            value("ECO_ARGUMENTS"),
            r#"--headless "C:\Program Files\eco\bin\eco.exe" start"#
        );
        assert_eq!(
            value("ECO_ICON"),
            r"C:\Program Files\eco\lib\eco\eco-window.exe,0"
        );
        assert_eq!(value("ECO_DIRECTORY"), r"C:\Program Files\eco\bin");
    }

    #[test]
    fn adds_the_shortcut_and_a_rerun_changes_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let link = directory.path().join("Programs").join("eco.lnk");
        let exe = directory.path().join("eco.exe");
        let window = directory.path().join("eco-window.exe");
        let save = |exe: &Path| said(&link, &shortcut(&link, exe, &window).output().unwrap());
        assert_eq!(
            save(&exe),
            Ok(format!("Start menu entry added in {}", link.display()))
        );
        let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap());
        let conhost = system.join(r"System32\conhost.exe").display().to_string();
        let mut fields = read_shortcut(&link);
        // The shell gives the Windows folder back in its own case.
        assert!(fields[0].eq_ignore_ascii_case(&conhost), "{fields:?}");
        fields[0] = conhost;
        assert_eq!(
            fields,
            [
                system.join(r"System32\conhost.exe").display().to_string(),
                format!("--headless \"{}\" start", exe.display()),
                format!("{},0", window.display()),
                directory.path().display().to_string(),
                "7".into(),
            ]
        );
        assert_eq!(
            save(&exe),
            Ok(format!("Start menu entry {} is up to date", link.display()))
        );
        let moved = directory.path().join("new").join("eco.exe");
        assert_eq!(
            save(&moved),
            Ok(format!("Start menu entry updated in {}", link.display()))
        );
        assert_eq!(
            read_shortcut(&link)[1],
            format!("--headless \"{}\" start", moved.display())
        );
    }

    #[test]
    fn a_shortcut_it_cannot_write_is_a_warning() {
        let directory = tempfile::tempdir().unwrap();
        let folder = directory.path().join("Programs");
        fs::write(&folder, "a file, not a folder").unwrap();
        let link = folder.join("eco.lnk");
        let output = shortcut(&link, Path::new(r"C:\eco\eco.exe"), Path::new(r"C:\w.exe"))
            .output()
            .unwrap();
        let warning = said(&link, &output).unwrap_err();
        assert!(
            warning.starts_with(&format!(
                "could not write {}, so eco is not in the Start menu: ",
                link.display()
            )),
            "{warning}"
        );
    }
}
