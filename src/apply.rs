//! The apply step of an update the daemon could not install while it ran
//! (docs/design.md §13): `eco apply-update <package>`, started by the daemon
//! or by `eco update` in a copy of eco outside the install, so that the
//! installer can replace every file of it. It stops the daemon, installs the
//! package, starts eco again when a daemon ran and tells the user what failed.
//! The same eco starts again whether the install worked or not: it is the one
//! the install leaves in place.
//!
//! `eco apply-uninstall` is the removal's: it stops the daemon, removes the
//! package, deletes the user's files when asked, and starts nothing.

#[cfg(any(windows, test))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use futures::future::BoxFuture;

use crate::adapters::control_socket::Address;
use crate::lifecycle;
use crate::paths;
use crate::ports::{InstallError, Installer, Notifier, Urgency};

/// What the apply step does, each a platform's own.
#[cfg(any(windows, test))]
pub trait Steps: Send + Sync {
    /// Stop the daemon and wait until it is gone, within a bound; whether its
    /// window was open, or `None` when no daemon ran.
    fn stop(&self) -> BoxFuture<'_, Result<Option<bool>, InstallError>>;
    /// Run the installer on `package` until it ends.
    fn install(&self, package: &Path) -> BoxFuture<'_, Result<(), InstallError>>;
    /// Start the installed eco, with its window when `window`.
    fn start(&self, window: bool) -> BoxFuture<'_, Result<(), InstallError>>;
    /// Show the user `message`; the daemon that would say it is gone.
    fn tell(&self, message: &str);
    /// Delete `package`.
    fn discard(&self, package: &Path);
}

/// Stop the daemon, install `package`, start eco again if a daemon ran, and
/// tell the user of each step that failed.
#[cfg(any(windows, test))]
pub async fn run(steps: &dyn Steps, package: &Path) {
    let window = match steps.stop().await {
        Ok(window) => window,
        Err(error) => {
            steps.discard(package);
            return steps.tell(&format!(
                "eco was not updated: it could not stop the running daemon ({})",
                error.0
            ));
        }
    };
    let installed = steps.install(package).await;
    steps.discard(package);
    if let Some(window) = window
        && let Err(error) = steps.start(window).await
    {
        steps.tell(&format!("eco could not start again: {}", error.0));
    }
    if let Err(error) = installed {
        steps.tell(&format!("eco was not updated: {}", error.0));
    }
}

/// What the removal does.
pub trait Removal: Send + Sync {
    /// Stop the daemon and wait until it is gone, within a bound.
    fn stop(&self) -> BoxFuture<'_, Result<Option<bool>, InstallError>>;
    /// Remove the installed package.
    fn remove(&self) -> BoxFuture<'_, Result<(), InstallError>>;
    /// Delete the user's config, sessions, people, models and state.
    fn erase(&self) -> Result<(), InstallError>;
    /// Show the user `message`; the daemon that would say it is gone.
    fn tell(&self, message: &str);
}

/// Stop the daemon, remove the package and, with `everything`, the user's
/// files, telling the user how it ended; whether eco was removed. Nothing
/// starts again, and a removal that fails or is cancelled leaves the user's
/// files alone.
pub async fn uninstall(steps: &dyn Removal, everything: bool) -> bool {
    if let Err(error) = steps.stop().await {
        steps.tell(&format!(
            "eco was not removed: it could not stop the running daemon ({})",
            error.0
        ));
        return false;
    }
    if let Err(error) = steps.remove().await {
        steps.tell(&format!("eco was not removed: {}", error.0));
        return false;
    }
    if everything && let Err(error) = steps.erase() {
        steps.tell(&format!(
            "eco was removed, but its data was not deleted: {}",
            error.0
        ));
        return false;
    }
    steps.tell(match everything {
        true => "eco and its data were removed",
        false => "eco was removed; its data was kept",
    });
    true
}

/// The removal on this machine: the user's daemon, the platform's installer,
/// the user's directories, and the desktop's notifications.
pub struct Local {
    address: Address,
    installer: Arc<dyn Installer>,
    notifier: Arc<dyn Notifier>,
    /// What `erase` deletes.
    kept: Vec<PathBuf>,
}

impl Local {
    pub fn new(installer: Arc<dyn Installer>, notifier: Arc<dyn Notifier>) -> Self {
        Self {
            address: Address::user(),
            installer,
            notifier,
            kept: paths::kept(),
        }
    }
}

impl Removal for Local {
    fn stop(&self) -> BoxFuture<'_, Result<Option<bool>, InstallError>> {
        Box::pin(async {
            lifecycle::shut(&self.address)
                .await
                .map_err(|error| InstallError(format!("{error:#}")))
        })
    }

    fn remove(&self) -> BoxFuture<'_, Result<(), InstallError>> {
        self.installer.remove()
    }

    fn erase(&self) -> Result<(), InstallError> {
        for dir in &self.kept {
            match std::fs::remove_dir_all(dir) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(InstallError(format!(
                        "cannot delete {}: {error}",
                        dir.display()
                    )));
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn tell(&self, message: &str) {
        self.notifier.notify("eco", message, Urgency::Critical);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use futures::FutureExt;

    use super::*;
    use crate::session::update::testing::FakeInstaller;

    /// Steps that log what they were asked, failing where told.
    #[derive(Default)]
    struct Fake {
        /// The window of the daemon that runs, or none.
        daemon: Option<bool>,
        stops: Option<&'static str>,
        installs: Option<&'static str>,
        starts: Option<&'static str>,
        log: Mutex<Vec<String>>,
    }

    impl Fake {
        fn log(&self, line: impl Into<String>) {
            self.log.lock().unwrap().push(line.into());
        }

        fn failing(why: Option<&'static str>) -> Result<(), InstallError> {
            why.map_or(Ok(()), |why| Err(InstallError(why.into())))
        }

        fn said(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
    }

    impl Steps for Fake {
        fn stop(&self) -> BoxFuture<'_, Result<Option<bool>, InstallError>> {
            self.log("stop");
            let done = Self::failing(self.stops).map(|()| self.daemon);
            async move { done }.boxed()
        }

        fn install(&self, package: &Path) -> BoxFuture<'_, Result<(), InstallError>> {
            self.log(format!("install {}", package.display()));
            async move { Self::failing(self.installs) }.boxed()
        }

        fn start(&self, window: bool) -> BoxFuture<'_, Result<(), InstallError>> {
            self.log(format!("start window={window}"));
            async move { Self::failing(self.starts) }.boxed()
        }

        fn tell(&self, message: &str) {
            self.log(format!("tell {message}"));
        }

        fn discard(&self, package: &Path) {
            self.log(format!("discard {}", package.display()));
        }
    }

    fn package() -> PathBuf {
        PathBuf::from("eco.msi")
    }

    #[tokio::test]
    async fn the_daemon_stops_then_the_package_installs_then_eco_starts() {
        let steps = Fake {
            daemon: Some(true),
            ..Fake::default()
        };
        run(&steps, &package()).await;
        assert_eq!(
            steps.said(),
            [
                "stop",
                "install eco.msi",
                "discard eco.msi",
                "start window=true"
            ]
        );
    }

    #[tokio::test]
    async fn a_window_that_was_closed_stays_closed() {
        let steps = Fake {
            daemon: Some(false),
            ..Fake::default()
        };
        run(&steps, &package()).await;
        assert_eq!(steps.said().last().unwrap(), "start window=false");
    }

    #[tokio::test]
    async fn with_no_daemon_nothing_starts() {
        let steps = Fake::default();
        run(&steps, &package()).await;
        assert_eq!(steps.said(), ["stop", "install eco.msi", "discard eco.msi"]);
    }

    #[tokio::test]
    async fn a_failed_install_starts_the_old_eco_and_tells_why() {
        let steps = Fake {
            daemon: Some(true),
            installs: Some("the installation was cancelled"),
            ..Fake::default()
        };
        run(&steps, &package()).await;
        assert_eq!(
            steps.said(),
            [
                "stop",
                "install eco.msi",
                "discard eco.msi",
                "start window=true",
                "tell eco was not updated: the installation was cancelled"
            ]
        );
    }

    #[tokio::test]
    async fn an_eco_that_cannot_start_again_is_told_too() {
        let steps = Fake {
            daemon: Some(true),
            starts: Some("no eco.exe"),
            ..Fake::default()
        };
        run(&steps, &package()).await;
        assert_eq!(
            steps.said().last().unwrap(),
            "tell eco could not start again: no eco.exe"
        );
    }

    #[tokio::test]
    async fn a_daemon_that_does_not_stop_installs_nothing() {
        let steps = Fake {
            daemon: Some(true),
            stops: Some("the daemon did not stop within 10 seconds"),
            ..Fake::default()
        };
        run(&steps, &package()).await;
        assert_eq!(
            steps.said(),
            [
                "stop",
                "discard eco.msi",
                "tell eco was not updated: it could not stop the running daemon (the daemon did not stop within 10 seconds)"
            ]
        );
    }

    /// A removal that logs what it was asked, failing where told.
    #[derive(Default)]
    struct Removing {
        stops: Option<&'static str>,
        removes: Option<&'static str>,
        erases: Option<&'static str>,
        log: Mutex<Vec<String>>,
    }

    impl Removal for Removing {
        fn stop(&self) -> BoxFuture<'_, Result<Option<bool>, InstallError>> {
            self.log.lock().unwrap().push("stop".into());
            let done = Fake::failing(self.stops).map(|()| Some(true));
            async move { done }.boxed()
        }

        fn remove(&self) -> BoxFuture<'_, Result<(), InstallError>> {
            self.log.lock().unwrap().push("remove".into());
            async move { Fake::failing(self.removes) }.boxed()
        }

        fn erase(&self) -> Result<(), InstallError> {
            self.log.lock().unwrap().push("erase".into());
            Fake::failing(self.erases)
        }

        fn tell(&self, message: &str) {
            self.log.lock().unwrap().push(format!("tell {message}"));
        }
    }

    fn said_by(steps: &Removing) -> Vec<String> {
        steps.log.lock().unwrap().clone()
    }

    #[tokio::test]
    async fn the_daemon_stops_then_the_package_goes_and_its_data_stays() {
        let steps = Removing::default();
        assert!(uninstall(&steps, false).await);
        assert_eq!(
            said_by(&steps),
            ["stop", "remove", "tell eco was removed; its data was kept"]
        );
    }

    #[tokio::test]
    async fn everything_deletes_the_data_after_the_package() {
        let steps = Removing::default();
        assert!(uninstall(&steps, true).await);
        assert_eq!(
            said_by(&steps),
            [
                "stop",
                "remove",
                "erase",
                "tell eco and its data were removed"
            ]
        );
    }

    #[tokio::test]
    async fn a_cancelled_removal_keeps_the_data_even_when_everything_is_asked() {
        let steps = Removing {
            removes: Some("the removal was cancelled; nothing changed"),
            ..Removing::default()
        };
        assert!(!uninstall(&steps, true).await);
        assert_eq!(
            said_by(&steps),
            [
                "stop",
                "remove",
                "tell eco was not removed: the removal was cancelled; nothing changed"
            ]
        );
    }

    #[tokio::test]
    async fn a_daemon_that_does_not_stop_removes_nothing() {
        let steps = Removing {
            stops: Some("the daemon did not stop within 10 seconds"),
            ..Removing::default()
        };
        assert!(!uninstall(&steps, true).await);
        assert_eq!(
            said_by(&steps),
            [
                "stop",
                "tell eco was not removed: it could not stop the running daemon (the daemon did not stop within 10 seconds)"
            ]
        );
    }

    #[tokio::test]
    async fn data_that_cannot_be_deleted_is_told() {
        let steps = Removing {
            erases: Some("cannot delete /x: busy"),
            ..Removing::default()
        };
        assert!(!uninstall(&steps, true).await);
        assert_eq!(
            said_by(&steps).last().unwrap(),
            "tell eco was removed, but its data was not deleted: cannot delete /x: busy"
        );
    }

    /// A notifier that keeps what it is told.
    #[derive(Default)]
    struct Shown(Mutex<Vec<(String, Urgency)>>);

    impl Notifier for Shown {
        fn notify(&self, _: &str, body: &str, urgency: Urgency) {
            self.0.lock().unwrap().push((body.into(), urgency));
        }
    }

    fn local(directory: &Path, removes: Option<&'static str>) -> (Local, Arc<Shown>) {
        let shown = Arc::new(Shown::default());
        let local = Local {
            address: Address::scratch(directory),
            installer: Arc::new(FakeInstaller {
                remove_fails: removes,
                ..FakeInstaller::default()
            }),
            notifier: shown.clone(),
            kept: vec![directory.join("config"), directory.join("data")],
        };
        (local, shown)
    }

    #[tokio::test]
    async fn the_machine_removes_through_its_installer_and_tells_by_notification() {
        let directory = tempfile::tempdir().unwrap();
        let (local, shown) = local(directory.path(), Some("the removal was cancelled"));
        assert_eq!(local.stop().await.unwrap(), None);
        assert_eq!(
            local.remove().await.unwrap_err().0,
            "the removal was cancelled"
        );
        local.tell("hello");
        assert_eq!(
            *shown.0.lock().unwrap(),
            [("hello".to_string(), Urgency::Critical)]
        );
        let (local, _) = self::local(directory.path(), None);
        local.remove().await.unwrap();
    }

    #[test]
    fn the_user_s_machine_deletes_the_directories_eco_keeps() {
        let local = Local::new(
            Arc::new(FakeInstaller::default()),
            Arc::new(Shown::default()),
        );
        assert_eq!(local.kept, paths::kept());
    }

    #[test]
    fn erasing_deletes_the_directories_and_ignores_the_missing() {
        let directory = tempfile::tempdir().unwrap();
        let (local, _) = local(directory.path(), None);
        std::fs::create_dir_all(directory.path().join("data/sessions")).unwrap();
        std::fs::write(directory.path().join("data/sessions/a"), "x").unwrap();
        local.erase().unwrap();
        assert!(!directory.path().join("data").exists());
        local.erase().unwrap();
    }

    #[test]
    fn a_directory_that_cannot_be_deleted_is_said() {
        let directory = tempfile::tempdir().unwrap();
        let (mut local, _) = local(directory.path(), None);
        let file = directory.path().join("file");
        std::fs::write(&file, "x").unwrap();
        // Not a directory: deleting it as one fails, and it is not "missing".
        local.kept = vec![file.clone()];
        let error = local.erase().unwrap_err();
        assert!(
            error
                .0
                .starts_with(&format!("cannot delete {}: ", file.display())),
            "{}",
            error.0
        );
    }
}
