//! Windows adapters for the platform seams whose Windows side is not written
//! yet: audio capture, the background service, the desktop's rules and window
//! control. Each says it is not supported on Windows, so the daemon starts and
//! the command line works without them (docs/design.md §15).

use futures::future::BoxFuture;
use futures::stream::{self, BoxStream};
use futures::{FutureExt, StreamExt};

use crate::ports::{
    AudioDevices, AudioError, AudioSource, DesktopIntegration, Device, EchoCancelling, Frame,
    ServiceManager, WindowControl, WindowError,
};

/// What `what` cannot do yet on Windows.
fn unsupported(what: &str) -> String {
    format!("{what} is not supported on Windows yet")
}

/// Every seam above, unsupported.
#[derive(Default)]
pub struct Unsupported;

impl AudioDevices for Unsupported {
    /// No device: nothing can be captured.
    fn list(&self) -> BoxFuture<'static, Vec<Device>> {
        async { Vec::new() }.boxed()
    }

    fn capture(&self, _: &Device) -> Box<dyn AudioSource> {
        Box::new(Unsupported)
    }

    fn capture_cancelled(&self) -> Box<dyn AudioSource> {
        Box::new(Unsupported)
    }

    fn cancel_echo(&self, _: &str) -> BoxFuture<'static, Result<EchoCancelling, AudioError>> {
        async { Err(AudioError(unsupported("echo cancellation"))) }.boxed()
    }
}

impl AudioSource for Unsupported {
    fn frames(&mut self) -> BoxStream<'_, Result<Frame, AudioError>> {
        stream::once(async { Err(AudioError(unsupported("audio capture"))) }).boxed()
    }
}

impl ServiceManager for Unsupported {
    fn start(&self) -> BoxFuture<'_, anyhow::Result<()>> {
        async {
            Err(anyhow::anyhow!(
                "{}; run `eco daemon` in a terminal",
                unsupported("running the daemon in the background")
            ))
        }
        .boxed()
    }
}

impl DesktopIntegration for Unsupported {
    fn load_rules(&self) -> Result<String, String> {
        Ok(format!(
            "{}; nothing loaded",
            unsupported("loading eco's window rules and shortcuts")
        ))
    }
}

impl WindowControl for Unsupported {
    fn focus(&self, _: u32) -> BoxFuture<'static, Result<(), WindowError>> {
        async { Err(WindowError(unsupported("focusing an eco window"))) }.boxed()
    }

    fn hide_from_share(&self, _: Vec<u32>, _: bool) -> BoxFuture<'static, Result<(), WindowError>> {
        async { Err(WindowError(unsupported("hiding eco from screen sharing"))) }.boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn audio_lists_nothing_and_capture_says_it_is_unsupported() {
        assert!(Unsupported.list().await.is_empty());
        let device = Device::new("mic", "Mic", "input");
        for mut source in [
            Unsupported.capture(&device),
            Unsupported.capture_cancelled(),
        ] {
            let frames: Vec<_> = source.frames().collect().await;
            let [Err(AudioError(said))] = &frames[..] else {
                panic!("{frames:?}");
            };
            assert_eq!(said, "audio capture is not supported on Windows yet");
        }
        let Err(AudioError(said)) = Unsupported.cancel_echo("mic").await else {
            panic!("echo cancellation started");
        };
        assert_eq!(said, "echo cancellation is not supported on Windows yet");
    }

    #[tokio::test]
    async fn the_service_window_control_and_desktop_say_they_are_unsupported() {
        let started = Unsupported.start().await.unwrap_err().to_string();
        assert_eq!(
            started,
            "running the daemon in the background is not supported on Windows yet; \
             run `eco daemon` in a terminal"
        );
        assert_eq!(
            Unsupported.focus(1).await.unwrap_err().0,
            "focusing an eco window is not supported on Windows yet"
        );
        assert_eq!(
            Unsupported
                .hide_from_share(vec![1], true)
                .await
                .unwrap_err()
                .0,
            "hiding eco from screen sharing is not supported on Windows yet"
        );
        assert_eq!(
            Unsupported.load_rules(),
            Ok(
                "loading eco's window rules and shortcuts is not supported on Windows yet; \
                nothing loaded"
                    .into()
            )
        );
    }
}
