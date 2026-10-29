//! The seams a platform has no adapter for yet (docs/design.md §15): each
//! says so when used, and none fails silently.

use std::path::PathBuf;

use futures::future::BoxFuture;
use futures::stream::{self, BoxStream};

use crate::adapters::devices;
use crate::ports::{
    AudioDevices, AudioError, AudioSource, DesktopIntegration, Device, EchoCancelling, EchoError,
    Frame, InstallError, InstallProgress, Installer, Notifier, ServiceManager, Urgency,
    WindowControl, WindowError,
};

/// Why every use of this adapter fails: the platform has no adapter for the seam yet.
fn unsupported(what: &str) -> String {
    format!("{what} is not available on this system yet")
}

pub struct Unsupported;

/// A source that fails at once, so the session reports its input down.
struct NoCapture;

impl AudioSource for NoCapture {
    fn frames(&mut self) -> BoxStream<'_, Result<Frame, AudioError>> {
        Box::pin(stream::once(async {
            Err(AudioError(unsupported("audio capture")))
        }))
    }
}

impl AudioDevices for Unsupported {
    fn list(&self) -> BoxFuture<'static, Vec<Device>> {
        Box::pin(async { devices::listed(Vec::new()) })
    }

    fn capture(&self, _device: &Device) -> Box<dyn AudioSource> {
        Box::new(NoCapture)
    }

    fn capture_cancelled(&self) -> Box<dyn AudioSource> {
        Box::new(NoCapture)
    }

    fn cancels_echo(&self) -> bool {
        false
    }

    fn cancel_echo(&self, _mic: &str) -> BoxFuture<'static, Result<EchoCancelling, EchoError>> {
        Box::pin(async { Err(EchoError::Unavailable) })
    }
}

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
    use futures::StreamExt;

    use super::*;

    const SAID: &str = "is not available on this system yet";

    #[tokio::test]
    async fn audio_lists_the_defaults_and_every_capture_fails() {
        assert_eq!(Unsupported.list().await, devices::defaults());
        let device = Device::new("@default-input", "Default microphone", "input");
        for mut source in [
            Unsupported.capture(&device),
            Unsupported.capture_cancelled(),
        ] {
            let first = source.frames().next().await.unwrap();
            assert!(first.unwrap_err().0.contains(SAID));
        }
        assert!(!Unsupported.cancels_echo());
        assert!(matches!(
            Unsupported.cancel_echo("mic").await,
            Err(EchoError::Unavailable)
        ));
    }

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
