//! The local socket the daemon listens on and its clients connect to: a Unix
//! domain socket at a path, readable and writable only by its owner. The
//! protocol on top of it lives in `control_socket`.

use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, Result, bail};
use tokio::net::{UnixListener, UnixStream};

pub use tokio::net::unix::{OwnedReadHalf as ReadHalf, OwnedWriteHalf as WriteHalf};

/// One connection, either end.
pub type Stream = UnixStream;

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("access to the eco socket was denied; check this process's sandbox permissions")]
    AccessDenied,
    #[error("cannot connect to the eco socket: {0}")]
    Unavailable(#[source] std::io::Error),
}

/// Connect to the socket at `path`; `None` when nothing listens there.
pub async fn connect(path: &Path) -> std::result::Result<Option<Stream>, ConnectError> {
    match UnixStream::connect(path).await {
        Ok(stream) => Ok(Some(stream)),
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::NotFound | ErrorKind::ConnectionRefused
            ) =>
        {
            Ok(None)
        }
        Err(error) if error.kind() == ErrorKind::PermissionDenied => {
            Err(ConnectError::AccessDenied)
        }
        Err(error) => Err(ConnectError::Unavailable(error)),
    }
}

/// Split a connection into halves that read and write independently.
pub fn split(stream: Stream) -> (ReadHalf, WriteHalf) {
    stream.into_split()
}

/// A socket accepting connections.
pub struct Listener(UnixListener);

impl Listener {
    /// Listen at `path`, refusing to start beside a live listener and clearing
    /// the file a crashed one left.
    pub async fn bind(path: &Path) -> Result<Self> {
        if path.exists() {
            if UnixStream::connect(path).await.is_ok() {
                bail!("a session is already running on {}", path.display());
            }
            std::fs::remove_file(path)
                .with_context(|| format!("cannot remove {}", path.display()))?;
        }
        let listener = UnixListener::bind(path)
            .with_context(|| format!("cannot listen on {}", path.display()))?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self(listener))
    }

    pub async fn accept(&self) -> std::io::Result<Stream> {
        Ok(self.0.accept().await?.0)
    }
}

/// Remove the socket at `path`, once its listener is gone.
pub fn release(path: &Path) {
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[tokio::test]
    async fn refuses_to_start_beside_a_live_listener() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("eco.sock");
        let _live = Listener::bind(&path).await.unwrap();
        let second = Listener::bind(&path).await;
        assert!(
            second
                .err()
                .unwrap()
                .to_string()
                .contains("already running")
        );
    }

    #[tokio::test]
    async fn replaces_a_stale_socket_and_keeps_it_private() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("eco.sock");
        drop(UnixListener::bind(&path).unwrap()); // a crashed session's leftover
        // A process another test forks may hold the listener until it execs.
        while UnixStream::connect(&path).await.is_ok() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let listener = Listener::bind(&path).await.unwrap();
        let mut stream = connect(&path).await.unwrap().unwrap();
        let mut accepted = listener.accept().await.unwrap();
        stream.write_all(b"ping\n").await.unwrap();
        let mut received = [0u8; 5];
        accepted.read_exact(&mut received).await.unwrap();
        assert_eq!(&received, b"ping\n");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        release(&path);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn nothing_listening_is_not_an_error() {
        let directory = tempfile::tempdir().unwrap();
        assert!(
            connect(&directory.path().join("eco.sock"))
                .await
                .unwrap()
                .is_none()
        );
    }
}
