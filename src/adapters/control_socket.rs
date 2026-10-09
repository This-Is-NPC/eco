//! The daemon socket's protocol: clients send one command per line and receive
//! every event as a JSON line, over the local socket in `local_socket`. A new client first receives the greeting, so it can render the
//! current state.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};
use tokio::task::{JoinHandle, JoinSet};

use crate::adapters::local_socket::{self, Listener, Stream};
use crate::domain::events::Event;

/// The longest command line a client may send, in bytes; a longer one ends
/// its connection. A `config.set` with a full config is a few kilobytes.
const MAX_LINE: usize = 1 << 20;

/// How many lines may wait to be written to one client.
const MAX_QUEUED: usize = 4096;

/// Whether a client whose queue is full may miss `event` and stay connected:
/// only the live input meter, whose next reading replaces it.
fn transient(event: &Event) -> bool {
    event["type"] == "signal"
}

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
    /// Send an event to every client. A client that left is dropped; so is one
    /// whose queue is full, unless the event is transient and it skips it.
    pub fn emit(&self, event: &Event) {
        let line = format!("{event}\n");
        let transient = transient(event);
        self.0.lock().expect("not poisoned").retain(|client| {
            match client.lines.try_send(line.clone()) {
                Ok(()) => true,
                Err(mpsc::error::TrySendError::Full(_)) => transient,
                Err(mpsc::error::TrySendError::Closed(_)) => false,
            }
        });
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
        let listener = Listener::bind(path).await?;
        let accept = tokio::spawn(async move {
            // Connections live in this set, so stopping the accept loop ends them all.
            let mut connections = JoinSet::new();
            while let Ok(stream) = listener.accept().await {
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
        local_socket::release(&self.path);
    }
}

async fn serve(
    stream: Stream,
    clients: Clients,
    greeting: Greeting,
    commands: mpsc::UnboundedSender<String>,
) {
    let (reader, mut writer) = local_socket::split(stream);
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

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use tokio::io::AsyncReadExt;

    use super::*;

    async fn connect(path: &Path) -> Stream {
        local_socket::connect(path).await.unwrap().unwrap()
    }

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
        let stream = connect(&path).await;
        let (reader, mut writer) = local_socket::split(stream);
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
        let mut fits = connect(&path).await;
        let mut largest = vec![b'a'; MAX_LINE];
        largest.push(b'\n');
        fits.write_all(&largest).await.unwrap();
        assert_eq!(commands.recv().await.unwrap().len(), MAX_LINE);
        let mut huge = connect(&path).await;
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
        let mut stalled = connect(&path).await;
        let reading = connect(&path).await;
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
    async fn a_client_that_does_not_read_skips_signals_and_is_dropped_for_the_rest() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("eco.sock");
        let (_socket, clients, _commands) = bound(&path, Vec::new()).await;
        let _stalled = connect(&path).await;
        while clients.0.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        let text = "x".repeat(1024);
        for _ in 0..MAX_QUEUED * 2 {
            clients.emit(&json!({"type": "signal", "input": text, "level": 0.5, "speech": true}));
            tokio::task::yield_now().await;
        }
        assert_eq!(
            clients.0.lock().unwrap().len(),
            1,
            "a flood of signals keeps it"
        );
        clients.emit(&json!({"type": "suggestion_delta", "text": "olá"}));
        assert!(
            clients.0.lock().unwrap().is_empty(),
            "a lost real event drops it"
        );
    }
}
