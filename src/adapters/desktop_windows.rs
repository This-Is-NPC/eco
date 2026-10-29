//! Windows: eco opened from an entry in the user's Start menu, its toasts
//! named "eco" with eco's icon, and never started at login: the script an
//! earlier `eco setup` put into their Startup folder is removed.

use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::adapters::notify_windows::{self, APP_ID, Registered};
use crate::adapters::overlay;
use crate::adapters::service_windows::CREATE_NO_WINDOW;
use crate::paths;
use crate::ports::DesktopIntegration;

/// Removes the login script an earlier eco wrote into `folder`, the Startup folder.
pub struct OldLoginScript {
    folder: PathBuf,
}

impl OldLoginScript {
    pub fn new(folder: PathBuf) -> Self {
        Self { folder }
    }
}

impl DesktopIntegration for OldLoginScript {
    fn integrate(&self) -> Result<Option<String>, String> {
        remove_script(&self.folder)
    }
}

/// The login script's name in the Startup folder.
const SCRIPT: &str = "eco.cmd";

/// Remove the login script from `folder`, if it is there; `Err` is a warning:
/// it is there and could not be removed.
fn remove_script(folder: &Path) -> Result<Option<String>, String> {
    let path = folder.join(SCRIPT);
    match fs::remove_file(&path) {
        Ok(()) => Ok(Some(format!(
            "removed {}; eco no longer starts at login",
            path.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "could not remove {}, so eco still starts at login: {error}",
            path.display()
        )),
    }
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
    fn integrate(&self) -> Result<Option<String>, String> {
        let exe = std::env::current_exe().map_err(|error| {
            format!("cannot find this eco, so it is not in the Start menu: {error}")
        })?;
        let link = self.folder.join(SHORTCUT);
        let output = shortcut(&link, &launcher(&exe), &overlay::program())
            .output()
            .map_err(|error| {
                format!("cannot run powershell, so eco is not in the Start menu: {error}")
            })?;
        said(&link, &output).map(Some)
    }
}

/// Registers eco's app id, which names eco's toasts "eco" with its icon.
pub struct ToastName;

impl DesktopIntegration for ToastName {
    fn integrate(&self) -> Result<Option<String>, String> {
        toast_name(APP_ID, notify_windows::icon().as_deref())
    }
}

/// Register the app id `id` with the icon at `icon`, saying what was done.
fn toast_name(id: &str, icon: Option<&Path>) -> Result<Option<String>, String> {
    Ok(match notify_windows::register(id, icon)? {
        Registered::Current => None,
        Registered::Added => Some(format!("notifications registered as eco's ({id})")),
        Registered::Updated => Some(format!("notifications updated as eco's ({id})")),
    })
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

/// The program the shortcut runs, which opens eco with no console window:
/// installed beside the window program (`<prefix>/lib/eco/eco-launch.exe`),
/// or beside `exe` when eco runs from a checkout's build.
fn launcher(exe: &Path) -> PathBuf {
    let name = format!("eco-launch{}", std::env::consts::EXE_SUFFIX);
    paths::shipped_program(&name).unwrap_or_else(|| exe.with_file_name(name))
}

/// PowerShell saving at `link` a shortcut that runs the program `launcher`,
/// with the icon of the window program `window`.
fn shortcut(link: &Path, launcher: &Path, window: &Path) -> Command {
    let mut icon = window.as_os_str().to_owned();
    icon.push(",0");
    let mut command = Command::new("powershell");
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", SAVE])
        .env("ECO_LINK", link)
        .env("ECO_TARGET", launcher)
        .env("ECO_ARGUMENTS", "")
        .env("ECO_ICON", icon)
        .env("ECO_DIRECTORY", launcher.parent().unwrap_or(launcher))
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
    fn an_old_login_script_is_removed_and_none_is_left_alone() {
        let directory = tempfile::tempdir().unwrap();
        let folder = directory.path();
        let path = folder.join("eco.cmd");
        fs::write(
            &path,
            "@echo off\r\n\"C:\\eco\\eco.exe\" start --headless\r\n",
        )
        .unwrap();
        fs::write(folder.join("other.cmd"), "rem not eco's").unwrap();
        assert_eq!(
            OldLoginScript::new(folder.into()).integrate(),
            Ok(Some(format!(
                "removed {}; eco no longer starts at login",
                path.display()
            )))
        );
        assert!(!path.exists());
        assert!(folder.join("other.cmd").exists());
        assert_eq!(remove_script(folder), Ok(None));
        assert_eq!(remove_script(&folder.join("no such folder")), Ok(None));
    }

    #[test]
    fn a_script_it_cannot_remove_is_a_warning() {
        let directory = tempfile::tempdir().unwrap();
        // A folder named like the script cannot be removed as a file.
        let path = directory.path().join("eco.cmd");
        fs::create_dir(&path).unwrap();
        let warning = remove_script(directory.path()).unwrap_err();
        assert!(
            warning.starts_with(&format!(
                "could not remove {}, so eco still starts at login: ",
                path.display()
            )),
            "{warning}"
        );
    }

    #[test]
    fn setup_says_what_it_did_to_the_toasts_name() {
        let test = notify_windows::testing::TestId::new("This-Is-NPC.eco-test-setup");
        let icon = Path::new(r"C:\eco\share\eco\eco.ico");
        assert_eq!(
            toast_name(test.0, Some(icon)),
            Ok(Some(
                "notifications registered as eco's (This-Is-NPC.eco-test-setup)".into()
            ))
        );
        assert_eq!(toast_name(test.0, Some(icon)), Ok(None));
        assert_eq!(
            toast_name(test.0, Some(Path::new(r"D:\eco.ico"))),
            Ok(Some(
                "notifications updated as eco's (This-Is-NPC.eco-test-setup)".into()
            ))
        );
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
    fn the_shortcut_runs_the_launcher_with_the_window_icon() {
        let built = shortcut(
            Path::new(r"C:\menu\eco.lnk"),
            Path::new(r"C:\Program Files\eco\lib\eco\eco-launch.exe"),
            Path::new(r"C:\Program Files\eco\lib\eco\eco-window.exe"),
        );
        assert_eq!(built.get_program(), "powershell");
        let args: Vec<_> = built.get_args().collect();
        assert_eq!(args, ["-NoProfile", "-NonInteractive", "-Command", SAVE]);
        let envs: Vec<_> = built.get_envs().collect();
        let value = |name: &str| {
            envs.iter()
                .find(|(key, _)| *key == name)
                .and_then(|(_, value)| *value)
                .unwrap()
                .to_owned()
        };
        assert_eq!(value("ECO_LINK"), r"C:\menu\eco.lnk");
        assert_eq!(
            value("ECO_TARGET"),
            r"C:\Program Files\eco\lib\eco\eco-launch.exe"
        );
        assert_eq!(value("ECO_ARGUMENTS"), "");
        assert_eq!(
            value("ECO_ICON"),
            r"C:\Program Files\eco\lib\eco\eco-window.exe,0"
        );
        assert_eq!(value("ECO_DIRECTORY"), r"C:\Program Files\eco\lib\eco");
    }

    #[test]
    fn the_launcher_of_a_build_is_beside_eco() {
        // Not installed, so no lib/eco beside the test binary.
        assert_eq!(
            launcher(Path::new(r"C:\build\eco.exe")),
            Path::new(r"C:\build\eco-launch.exe")
        );
    }

    #[test]
    fn adds_the_shortcut_and_a_rerun_changes_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let link = directory.path().join("Programs").join("eco.lnk");
        let launch = directory.path().join("eco-launch.exe");
        let window = directory.path().join("eco-window.exe");
        let save = |launch: &Path| said(&link, &shortcut(&link, launch, &window).output().unwrap());
        assert_eq!(
            save(&launch),
            Ok(format!("Start menu entry added in {}", link.display()))
        );
        assert_eq!(
            read_shortcut(&link),
            [
                launch.display().to_string(),
                String::new(),
                format!("{},0", window.display()),
                directory.path().display().to_string(),
                "7".into(),
            ]
        );
        assert_eq!(
            save(&launch),
            Ok(format!("Start menu entry {} is up to date", link.display()))
        );
        let moved = directory.path().join("new").join("eco-launch.exe");
        assert_eq!(
            save(&moved),
            Ok(format!("Start menu entry updated in {}", link.display()))
        );
        assert_eq!(read_shortcut(&link)[0], moved.display().to_string());
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
