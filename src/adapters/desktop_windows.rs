//! Windows: eco's toasts named "eco" with eco's icon, and eco never started at
//! login: the script an earlier `eco setup` put into the user's Startup folder
//! is removed. The Start menu entry belongs to the installer.

use std::fs;
use std::path::{Path, PathBuf};

use crate::adapters::notify_windows::{self, APP_ID, Registered};
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
}
