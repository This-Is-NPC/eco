//! The Unix socket: clients send one command per line and receive every event as
//! a JSON line. A new client first receives the greeting, so it can render the
//! current state.

use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};
use tokio::task::{JoinHandle, JoinSet};

use crate::domain::events::Event;

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("access to the eco socket was denied; check this process's sandbox permissions")]
    AccessDenied,
    #[error("cannot connect to the eco socket: {0}")]
    Unavailable(#[source] std::io::Error),
}

pub async fn connect(path: &Path) -> std::result::Result<Option<UnixStream>, ConnectError> {
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

/// The longest command line a client may send, in bytes; a longer one ends
/// its connection. A `config.set` with a full config is a few kilobytes.
const MAX_LINE: usize = 1 << 20;

/// How many lines may wait to be written to one client; a client that falls
/// this far behind is dropped.
const MAX_QUEUED: usize = 4096;

/// A connected client: the queue of lines still to write to it, and a guard
/// whose drop ends its connection.
struct Client {
    lines: mpsc::Sender<String>,
    _kept: oneshot::Sender<()>,
}

/// Every connected client.
#[derive(Clone, Default)]
pub struct Clients(Arc<Mutex<Vec<Client>>>);

impl Clients {
    /// Send an event to every client; clients that left or whose queue is
    /// full are dropped.
    pub fn emit(&self, event: &Event) {
        let line = format!("{event}\n");
        self.0
            .lock()
            .expect("not poisoned")
            .retain(|client| client.lines.try_send(line.clone()).is_ok());
    }

    fn add(&self, client: Client) {
        self.0.lock().expect("not poisoned").push(client);
    }
}

pub type Greeting = Arc<dyn Fn() -> Vec<Event> + Send + Sync>;

/// The listening socket; dropping it stops serving and removes the file.
pub struct ControlSocket {
    path: PathBuf,
    accept: JoinHandle<()>,
}

impl ControlSocket {
    /// Listen at `path`, forwarding each command line to `commands`.
    pub async fn bind(
        path: &Path,
        clients: Clients,
        greeting: Greeting,
        commands: mpsc::UnboundedSender<String>,
    ) -> Result<Self> {
        remove_stale(path).await?;
        let listener = UnixListener::bind(path)
            .with_context(|| format!("cannot listen on {}", path.display()))?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        let accept = tokio::spawn(async move {
            // Connections live in this set, so stopping the accept loop ends them all.
            let mut connections = JoinSet::new();
            while let Ok((stream, _)) = listener.accept().await {
                connections.spawn(serve(
                    stream,
                    clients.clone(),
                    Arc::clone(&greeting),
                    commands.clone(),
                ));
            }
        });
        Ok(Self {
            path: path.into(),
            accept,
        })
    }
}

impl Drop for ControlSocket {
    fn drop(&mut self) {
        self.accept.abort();
        let _ = std::fs::remove_file(&self.path);
    }
}

async fn serve(
    stream: UnixStream,
    clients: Clients,
    greeting: Greeting,
    commands: mpsc::UnboundedSender<String>,
) {
    let (reader, mut writer) = stream.into_split();
    let (outgoing, mut queue) = mpsc::channel::<String>(MAX_QUEUED);
    for event in greeting() {
        let _ = outgoing.try_send(format!("{event}\n"));
    }
    let (kept, dropped) = oneshot::channel();
    clients.add(Client {
        lines: outgoing,
        _kept: kept,
    });
    let write = async move {
        while let Some(line) = queue.recv().await {
            if writer.write_all(line.as_bytes()).await.is_err() {
                break;
            }
        }
    };
    let read = async move {
        let mut reader = BufReader::new(reader);
        let mut line = Vec::new();
        loop {
            line.clear();
            // One byte past the cap tells a line that is too long from one that fits.
            let mut limited = (&mut reader).take(MAX_LINE as u64 + 1);
            if !matches!(limited.read_until(b'\n', &mut line).await, Ok(1..)) {
                break;
            }
            if line.len() > MAX_LINE && line.last() != Some(&b'\n') {
                break;
            }
            let Ok(line) = std::str::from_utf8(&line) else {
                break;
            };
            let command = line.trim();
            if !command.is_empty() && commands.send(command.to_string()).is_err() {
                break;
            }
        }
    };
    // The client is done when it stops reading, stops writing, sends a line
    // past the cap, or falls behind and its queue is dropped.
    tokio::select! { () = write => {}, () = read => {}, _ = dropped => {} }
}

/// Refuse to start beside a live session; clear the socket a crashed one left.
async fn remove_stale(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if UnixStream::connect(path).await.is_ok() {
        bail!("a session is already running on {}", path.display());
    }
    std::fs::remove_file(path).with_context(|| format!("cannot remove {}", path.display()))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use tokio::io::AsyncReadExt;

    use super::*;

    async fn bound(
        path: &Path,
        greeting: Vec<Event>,
    ) -> (ControlSocket, Clients, mpsc::UnboundedReceiver<String>) {
        let clients = Clients::default();
        let (commands, received) = mpsc::unbounded_channel();
        let socket = ControlSocket::bind(
            path,
            clients.clone(),
            Arc::new(move || greeting.clone()),
            commands,
        )
        .await
        .unwrap();
        (socket, clients, received)
    }

    #[tokio::test]
    async fn receives_commands_and_broadcasts_events() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("eco.sock");
        let greeting = vec![json!({"type": "snapshot", "actions": ["ask"]})];
        let (socket, clients, mut commands) = bound(&path, greeting.clone()).await;
        let stream = UnixStream::connect(&path).await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        let hello: Value =
            serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        writer.write_all(b"action probe\n\n").await.unwrap();
        assert_eq!(commands.recv().await.unwrap(), "action probe");
        clients.emit(&json!({"type": "suggestion_delta", "text": "olá"}));
        let event: Value =
            serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(hello, greeting[0]);
        assert_eq!(event, json!({"type": "suggestion_delta", "text": "olá"}));
        drop(socket);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn a_line_past_the_cap_ends_the_connection() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("eco.sock");
        let (_socket, _, mut commands) = bound(&path, Vec::new()).await;
        let mut fits = UnixStream::connect(&path).await.unwrap();
        let mut largest = vec![b'a'; MAX_LINE];
        largest.push(b'\n');
        fits.write_all(&largest).await.unwrap();
        assert_eq!(commands.recv().await.unwrap().len(), MAX_LINE);
        let mut huge = UnixStream::connect(&path).await.unwrap();
        // The daemon may close before every byte is written.
        let _ = huge.write_all(&vec![b'a'; MAX_LINE + 1]).await;
        let mut rest = Vec::new();
        let closed = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            huge.read_to_end(&mut rest),
        )
        .await;
        assert!(closed.is_ok(), "the daemon closes the connection");
        assert!(
            commands.try_recv().is_err(),
            "the long line is not a command"
        );
    }

    #[tokio::test]
    async fn a_client_that_does_not_read_is_dropped() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("eco.sock");
        let (_socket, clients, _commands) = bound(&path, Vec::new()).await;
        let mut stalled = UnixStream::connect(&path).await.unwrap();
        let reading = UnixStream::connect(&path).await.unwrap();
        while clients.0.lock().unwrap().len() < 2 {
            tokio::task::yield_now().await;
        }
        let total = MAX_QUEUED * 2;
        let reader = tokio::spawn(async move {
            let mut lines = BufReader::new(reading).lines();
            let mut count = 0;
            while count < total && lines.next_line().await.unwrap().is_some() {
                count += 1;
            }
            count
        });
        let text = "x".repeat(1024);
        for _ in 0..total {
            clients.emit(&json!({"type": "suggestion_delta", "text": text}));
            tokio::task::yield_now().await;
        }
        assert_eq!(clients.0.lock().unwrap().len(), 1);
        assert_eq!(reader.await.unwrap(), total);
        let mut received = Vec::new();
        let closed = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stalled.read_to_end(&mut received),
        )
        .await;
        assert!(closed.is_ok(), "the daemon closes the stalled connection");
    }

    #[tokio::test]
    async fn refuses_to_start_beside_a_live_session() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("eco.sock");
        let _live = bound(&path, Vec::new()).await;
        let (commands, _) = mpsc::unbounded_channel();
        let second =
            ControlSocket::bind(&path, Clients::default(), Arc::new(Vec::new), commands).await;
        assert!(
            second
                .err()
                .unwrap()
                .to_string()
                .contains("already running")
        );
    }

    #[tokio::test]
    async fn replaces_a_stale_socket() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("eco.sock");
        drop(UnixListener::bind(&path).unwrap()); // a crashed session's leftover
        // A process another test forks may hold the listener until it execs.
        while UnixStream::connect(&path).await.is_ok() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let (_socket, _, _) = bound(&path, Vec::new()).await;
        let mut stream = UnixStream::connect(&path).await.unwrap();
        stream.write_all(b"").await.unwrap();
        let mut nothing = [0u8; 1];
        let read = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            stream.read(&mut nothing),
        )
        .await;
        assert!(read.is_err(), "the new session keeps the connection open");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
