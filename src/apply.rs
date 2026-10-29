//! The apply step of an update the daemon could not install while it ran
//! (docs/design.md §13): `eco apply-update <package>`, started by the daemon
//! or by `eco update` in a copy of eco outside the install, so that the
//! installer can replace every file of it. It stops the daemon, installs the
//! package, starts eco again when a daemon ran and tells the user what failed.
//! The same eco starts again whether the install worked or not: it is the one
//! the install leaves in place.

use std::path::Path;

use futures::future::BoxFuture;

use crate::ports::InstallError;

/// What the apply step does, each a platform's own.
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use futures::FutureExt;

    use super::*;

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
}
