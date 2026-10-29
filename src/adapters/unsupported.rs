//! The seams a platform has no adapter for yet (docs/design.md §15): each
//! says so when used, and none fails silently.

use std::path::PathBuf;

use futures::future::BoxFuture;

use crate::ports::{DesktopIntegration, InstallError, InstallProgress, Installer};

/// Why every use of this adapter fails: the platform has no adapter for the seam yet.
fn unsupported(what: &str) -> String {
    format!("{what} is not available on this system yet")
}

pub struct Unsupported;

impl DesktopIntegration for Unsupported {
    fn integrate(&self) -> Result<Option<String>, String> {
        Ok(Some(unsupported("starting eco from the desktop")))
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

#[cfg(test)]
mod tests {
    use super::*;

    const SAID: &str = "is not available on this system yet";

    #[test]
    fn setup_says_eco_is_not_tied_into_the_desktop() {
        let said = Unsupported.integrate().unwrap().unwrap();
        assert!(said.contains(SAID), "{said}");
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
