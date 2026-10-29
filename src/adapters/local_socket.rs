//! The local socket the daemon listens on and its clients connect to. On
//! Linux it is a Unix domain socket at a path, readable and writable only by
//! its owner; on Windows a named pipe, whose name holds the user's, that
//! refuses clients on other machines. The protocol on top of it lives in
//! `control_socket`.

#[cfg(unix)]
pub use unix::{Listener, ReadHalf, Stream, WriteHalf, connect, release, split};
#[cfg(windows)]
pub use windows::{Listener, ReadHalf, Stream, WriteHalf, connect, release, split};

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[cfg_attr(
        unix,
        error("access to the eco socket was denied; check this process's sandbox permissions")
    )]
    #[cfg_attr(
        windows,
        error(
            "access to the eco socket was denied; if the daemon runs as administrator, run `eco stop` from an elevated terminal, then start eco again"
        )
    )]
    AccessDenied,
    #[error("cannot connect to the eco socket: {0}")]
    Unavailable(#[source] std::io::Error),
}

#[cfg(unix)]
mod unix {
    use std::io::ErrorKind;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use anyhow::{Context, Result, bail};
    use tokio::net::{UnixListener, UnixStream};

    pub use tokio::net::unix::{OwnedReadHalf as ReadHalf, OwnedWriteHalf as WriteHalf};

    use super::ConnectError;

    /// One connection, either end.
    pub type Stream = UnixStream;

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
}

#[cfg(windows)]
mod windows {
    use std::io::{self, ErrorKind};
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::task::{Context as Task, Poll};
    use std::time::Duration;

    use anyhow::{Context, Result, bail};
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
    use tokio::net::windows::named_pipe::{
        ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
    };
    use tokio::sync::Mutex;

    use super::ConnectError;

    /// Windows' error for a pipe whose every instance is taken.
    const ERROR_PIPE_BUSY: i32 = 231;

    /// How often, and how far apart, a client tries a pipe whose every
    /// instance is taken: the listener makes the next one as it accepts.
    const BUSY_TRIES: u32 = 40;
    const BUSY_WAIT: Duration = Duration::from_millis(50);

    /// One connection, either end.
    pub enum Stream {
        Server(NamedPipeServer),
        Client(NamedPipeClient),
    }

    impl AsyncRead for Stream {
        fn poll_read(
            self: Pin<&mut Self>,
            task: &mut Task<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            match self.get_mut() {
                Stream::Server(pipe) => Pin::new(pipe).poll_read(task, buf),
                Stream::Client(pipe) => Pin::new(pipe).poll_read(task, buf),
            }
        }
    }

    impl AsyncWrite for Stream {
        fn poll_write(
            self: Pin<&mut Self>,
            task: &mut Task<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            match self.get_mut() {
                Stream::Server(pipe) => Pin::new(pipe).poll_write(task, buf),
                Stream::Client(pipe) => Pin::new(pipe).poll_write(task, buf),
            }
        }

        fn poll_flush(self: Pin<&mut Self>, task: &mut Task<'_>) -> Poll<io::Result<()>> {
            match self.get_mut() {
                Stream::Server(pipe) => Pin::new(pipe).poll_flush(task),
                Stream::Client(pipe) => Pin::new(pipe).poll_flush(task),
            }
        }

        fn poll_shutdown(self: Pin<&mut Self>, task: &mut Task<'_>) -> Poll<io::Result<()>> {
            match self.get_mut() {
                Stream::Server(pipe) => Pin::new(pipe).poll_shutdown(task),
                Stream::Client(pipe) => Pin::new(pipe).poll_shutdown(task),
            }
        }
    }

    pub type ReadHalf = tokio::io::ReadHalf<Stream>;
    pub type WriteHalf = tokio::io::WriteHalf<Stream>;

    /// Connect to the pipe named `path`; `None` when nothing listens there.
    pub async fn connect(path: &Path) -> std::result::Result<Option<Stream>, ConnectError> {
        let mut tries = 0;
        loop {
            match ClientOptions::new().open(path) {
                Ok(client) => return Ok(Some(Stream::Client(client))),
                Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                    tries += 1;
                    if tries == BUSY_TRIES {
                        return Err(ConnectError::Unavailable(error));
                    }
                    tokio::time::sleep(BUSY_WAIT).await;
                }
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                Err(error) if error.kind() == ErrorKind::PermissionDenied => {
                    return Err(ConnectError::AccessDenied);
                }
                Err(error) => return Err(ConnectError::Unavailable(error)),
            }
        }
    }

    /// Split a connection into halves that read and write independently.
    pub fn split(stream: Stream) -> (ReadHalf, WriteHalf) {
        tokio::io::split(stream)
    }

    /// A pipe accepting connections: one instance waits for the next client.
    pub struct Listener {
        path: PathBuf,
        next: Mutex<NamedPipeServer>,
    }

    /// A new instance of the pipe named `path`, for clients on this machine only.
    pub(super) fn instance(path: &Path, first: bool) -> io::Result<NamedPipeServer> {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create(path)
    }

    impl Listener {
        /// Listen on the pipe named `path`, refusing to start beside a live
        /// listener. A pipe goes with its last handle, so a crashed one leaves
        /// nothing behind.
        pub async fn bind(path: &Path) -> Result<Self> {
            let first = match instance(path, true) {
                Ok(first) => first,
                Err(error) if error.kind() == ErrorKind::PermissionDenied => {
                    bail!("a session is already running on {}", path.display());
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("cannot listen on {}", path.display()));
                }
            };
            Ok(Self {
                path: path.into(),
                next: Mutex::new(first),
            })
        }

        /// The next client, on its own instance; a new instance waits for the
        /// one after before this one is handed out, so the pipe never vanishes.
        pub async fn accept(&self) -> io::Result<Stream> {
            let mut next = self.next.lock().await;
            next.connect().await?;
            let waiting = instance(&self.path, false)?;
            Ok(Stream::Server(std::mem::replace(&mut *next, waiting)))
        }
    }

    /// Nothing to remove: the pipe goes with its last handle.
    pub fn release(_path: &Path) {}
}

/// A socket of a test's own: in `directory`.
#[cfg(all(test, unix))]
pub fn scratch(directory: &std::path::Path) -> std::path::PathBuf {
    directory.join("eco.sock")
}

/// Two ends of one connection, for a test.
#[cfg(all(test, unix))]
pub fn pair() -> (Stream, Stream) {
    Stream::pair().unwrap()
}

/// A socket of a test's own: a pipe named after `directory`.
#[cfg(all(test, windows))]
pub fn scratch(directory: &std::path::Path) -> std::path::PathBuf {
    let mut name = std::ffi::OsString::from(r"\\.\pipe\eco-test-");
    name.push(directory.file_name().unwrap_or_default());
    name.into()
}

/// Two ends of one connection, for a test: a pipe of its own, connected.
#[cfg(all(test, windows))]
pub fn pair() -> (Stream, Stream) {
    use tokio::net::windows::named_pipe::ClientOptions;
    let name = format!(r"\\.\pipe\eco-test-{}", uuid::Uuid::new_v4().simple());
    let server = windows::instance(std::path::Path::new(&name), true).unwrap();
    let client = ClientOptions::new().open(&name).unwrap();
    // The client is in already, so this completes at once.
    futures::executor::block_on(server.connect()).unwrap();
    (Stream::Client(client), Stream::Server(server))
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[tokio::test]
    async fn refuses_to_start_beside_a_live_listener() {
        let directory = tempfile::tempdir().unwrap();
        let path = scratch(directory.path());
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
    async fn nothing_listening_is_not_an_error() {
        let directory = tempfile::tempdir().unwrap();
        assert!(connect(&scratch(directory.path())).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn each_client_gets_its_own_connection_both_ways() {
        let directory = tempfile::tempdir().unwrap();
        let path = scratch(directory.path());
        let listener = Listener::bind(&path).await.unwrap();
        let mut kept = Vec::new();
        for _ in 0..2 {
            let mut client = connect(&path).await.unwrap().unwrap();
            let mut accepted = listener.accept().await.unwrap();
            client.write_all(b"ping\n").await.unwrap();
            let mut received = [0u8; 5];
            accepted.read_exact(&mut received).await.unwrap();
            assert_eq!(&received, b"ping\n");
            let (_, mut writer) = split(accepted);
            writer.write_all(b"pong\n").await.unwrap();
            client.read_exact(&mut received).await.unwrap();
            assert_eq!(&received, b"pong\n");
            kept.push(client);
        }
    }

    #[tokio::test]
    async fn a_listener_that_is_gone_leaves_nothing_listening() {
        let directory = tempfile::tempdir().unwrap();
        let path = scratch(directory.path());
        let listener = Listener::bind(&path).await.unwrap();
        drop(listener);
        release(&path);
        assert!(connect(&path).await.unwrap().is_none());
        let _again = Listener::bind(&path).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn replaces_a_stale_socket_and_keeps_it_private() {
        use std::os::unix::fs::PermissionsExt;
        use tokio::net::{UnixListener, UnixStream};

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

    #[cfg(unix)]
    #[tokio::test]
    async fn a_socket_the_process_may_not_open_is_denied_or_unavailable() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("eco.sock");
        let _listener = Listener::bind(&path).await.unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let denied = connect(&path).await;
        // Root connects through any mode.
        let root = std::fs::metadata("/proc/self").unwrap().uid() == 0;
        assert!(root || matches!(denied, Err(ConnectError::AccessDenied)));

        let file = directory.path().join("plain");
        std::fs::write(&file, "").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(
            connect(&file.join("below")).await,
            Err(ConnectError::Unavailable(_))
        ));
    }
}
