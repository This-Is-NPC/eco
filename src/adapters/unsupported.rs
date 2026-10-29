//! The seams a platform has no adapter for yet (docs/design.md §15): each
//! says so when used, and none fails silently.

use std::path::PathBuf;

use futures::future::BoxFuture;

use crate::ports::{
    DesktopIntegration, InstallError, InstallProgress, Installer, Notifier, ServiceManager,
    Urgency, WindowControl, WindowError,
};

/// Why every use of this adapter fails: the platform has no adapter for the seam yet.
fn unsupported(what: &str) -> String {
    format!("{what} is not available on this system yet")
}

pub struct Unsupported;

impl ServiceManager for Unsupported {
    fn start(&self) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async {
            anyhow::bail!(
                "{}; run `eco daemon` in a terminal instead",
                unsupported("running eco in the background")
            )
        })
    }

    fn output(&self, _since: std::time::SystemTime) -> BoxFuture<'_, Option<String>> {
        Box::pin(async { None })
    }
}

impl DesktopIntegration for Unsupported {
    fn integrate(&self) -> Result<Option<String>, String> {
        Ok(Some(unsupported("starting eco from the desktop")))
    }
}

impl WindowControl for Unsupported {
    fn focus(&self, _pid: u32) -> BoxFuture<'static, Result<(), WindowError>> {
        Box::pin(async { Err(WindowError(unsupported("focusing eco's window"))) })
    }

    fn hide_from_share(
        &self,
        _pids: Vec<u32>,
        _hidden: bool,
    ) -> BoxFuture<'static, Result<(), WindowError>> {
        Box::pin(async {
            Err(WindowError(unsupported(
                "hiding eco's windows from screen sharing",
            )))
        })
    }
}

impl Installer for Unsupported {
    fn install(
        &self,
        _tag: String,
        _progress: InstallProgress,
    ) -> BoxFuture<'static, Result<(), InstallError>> {
        Box::pin(async { Err(InstallError(unsupported("installing a release"))) })
    }

    fn program(&self) -> PathBuf {
        std::env::current_exe().unwrap_or_default()
    }
}

impl Notifier for Unsupported {
    fn notify(&self, _title: &str, _body: &str, _urgency: Urgency) {
        eprintln!("eco: {}", unsupported("a system notification"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAID: &str = "is not available on this system yet";

    #[tokio::test]
    async fn the_service_refuses_to_start_and_says_what_to_run() {
        let error = Unsupported.start().await.unwrap_err().to_string();
        assert!(error.contains(SAID), "{error}");
        assert!(error.contains("eco daemon"), "{error}");
        assert_eq!(Unsupported.output(std::time::SystemTime::now()).await, None);
    }

    #[test]
    fn setup_says_eco_is_not_tied_into_the_desktop() {
        let said = Unsupported.integrate().unwrap().unwrap();
        assert!(said.contains(SAID), "{said}");
    }

    #[tokio::test]
    async fn windows_are_neither_focused_nor_hidden() {
        assert!(Unsupported.focus(1).await.unwrap_err().0.contains(SAID));
        let hidden = Unsupported.hide_from_share(vec![1], true).await;
        assert!(hidden.unwrap_err().0.contains(SAID));
    }

    #[tokio::test]
    async fn an_update_installs_nothing() {
        let error = Unsupported
            .install("v1.0.0".into(), Box::new(|_| panic!("no stage starts")))
            .await
            .unwrap_err();
        assert!(error.0.contains(SAID), "{}", error.0);
        assert_eq!(Unsupported.program(), std::env::current_exe().unwrap());
    }
}
