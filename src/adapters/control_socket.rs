//! The Unix socket: clients send one command per line and receive every event as
//! a JSON line. A new client first receives the greeting, so it can render the
//! current state.

use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
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

/// Every connected client, as the queue of lines still to write to it.
#[derive(Clone, Default)]
pub struct Clients(Arc<Mutex<Vec<mpsc::UnboundedSender<String>>>>);

impl Clients {
    /// Send an event to every client; clients that left are dropped.
    pub fn emit(&self, event: &Event) {
        let line = format!("{event}\n");
        self.0
            .lock()
            .expect("not poisoned")
            .retain(|client| client.send(line.clone()).is_ok());
    }

    fn add(&self, client: mpsc::UnboundedSender<String>) {
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
    let (outgoing, mut queue) = mpsc::unbounded_channel::<String>();
    for event in greeting() {
        let _ = outgoing.send(format!("{event}\n"));
    }
    clients.add(outgoing);
    let write = async move {
        while let Some(line) = queue.recv().await {
            if writer.write_all(line.as_bytes()).await.is_err() {
                break;
            }
        }
    };
    let read = async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let command = line.trim();
            if !command.is_empty() && commands.send(command.to_string()).is_err() {
                break;
            }
        }
    };
    // The client is done when it stops reading or stops writing.
    tokio::select! { () = write => {}, () = read => {} }
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
