//! The Windows adapter for the platform seam whose Windows side is not written
//! yet: window control. It says it is not supported on Windows, so the daemon
//! starts and the command line works without it (docs/design.md §15).

use futures::FutureExt;
use futures::future::BoxFuture;

use crate::ports::{WindowControl, WindowError};

/// What `what` cannot do yet on Windows.
fn unsupported(what: &str) -> String {
    format!("{what} is not supported on Windows yet")
}

/// Window control, unsupported.
#[derive(Default)]
pub struct Unsupported;

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
    async fn window_control_says_it_is_unsupported() {
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
    }
}
