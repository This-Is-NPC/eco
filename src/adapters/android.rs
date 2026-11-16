//! Android's answers for the seams its app owns (docs/design.md §15): the app
//! starts and stops the daemon, shows the windows, notifies and updates eco, so
//! the daemon reports each of these as not available instead of doing it.

use std::path::PathBuf;
use std::time::SystemTime;

use futures::future::BoxFuture;

use super::unsupported::unsupported;
use crate::ports::{
    InstallError, InstallProgress, Installed, Installer, Notifier, Pending, ServiceManager,
    Urgency, WindowControl, WindowError,
};

/// The service lifecycle: the app starts the daemon from its foreground service.
pub struct NoService;

impl ServiceManager for NoService {
    fn start(&self) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async { anyhow::bail!(unsupported("starting the daemon from the shell")) })
    }

    fn output(&self, _: SystemTime) -> BoxFuture<'_, Option<String>> {
        Box::pin(async { None })
    }
}

/// Installing and removing eco: the system's package installer does both.
pub struct NoInstaller;

fn refuse() -> InstallError {
    InstallError(unsupported("updating or removing eco from the shell"))
}

#[derive(Debug)]
struct NoRemoval;

impl Pending for NoRemoval {
    fn hand_off(&self) -> std::io::Result<()> {
        Err(std::io::Error::other(refuse().0))
    }
}

impl Installer for NoInstaller {
    fn install(
        &self,
        _: String,
        _: InstallProgress,
    ) -> BoxFuture<'static, Result<Installed, InstallError>> {
        Box::pin(async { Err(refuse()) })
    }

    fn program(&self) -> PathBuf {
        std::env::current_exe().unwrap_or_default()
    }

    fn uninstall(&self, _: bool) -> Box<dyn Pending> {
        Box::new(NoRemoval)
    }

    fn remove(&self) -> BoxFuture<'static, Result<(), InstallError>> {
        Box::pin(async { Err(refuse()) })
    }
}

/// System notifications: the app shows them, the daemon says so in its log.
pub struct NoNotifications;

impl Notifier for NoNotifications {
    fn notify(&self, _: &str, _: &str, _: Urgency) {
        eprintln!("eco: {}", unsupported("a system notification"));
    }
}

/// Window control: the app owns every window, none is a process of the daemon.
pub struct NoWindowControl;

impl WindowControl for NoWindowControl {
    fn focus(&self, _: u32) -> BoxFuture<'static, Result<(), WindowError>> {
        Box::pin(async { Err(WindowError(unsupported("focusing a window"))) })
    }

    fn hide_from_share(&self, _: Vec<u32>, _: bool) -> BoxFuture<'static, Result<(), WindowError>> {
        Box::pin(async {
            Err(WindowError(unsupported(
                "hiding a window from screen sharing",
            )))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAID: &str = "is not available on this system yet";

    #[tokio::test]
    async fn the_shell_cannot_start_the_daemon() {
        let error = NoService.start().await.unwrap_err().to_string();
        assert!(error.contains(SAID), "{error}");
        assert_eq!(NoService.output(SystemTime::now()).await, None);
    }

    #[tokio::test]
    async fn the_shell_cannot_update_or_remove_eco() {
        let installer = NoInstaller;
        let error = installer.install("v1.0.0".into(), Box::new(|_| {})).await;
        assert!(error.unwrap_err().0.contains(SAID));
        assert!(installer.remove().await.unwrap_err().0.contains(SAID));
        let removal = installer.uninstall(true).hand_off().unwrap_err();
        assert!(removal.to_string().contains(SAID));
    }

    #[tokio::test]
    async fn windows_are_not_the_daemon_s_to_control() {
        let error = NoWindowControl.focus(1).await.unwrap_err();
        assert!(error.0.contains(SAID));
        let error = NoWindowControl.hide_from_share(vec![1], true).await;
        assert!(error.unwrap_err().0.contains(SAID));
    }
}
