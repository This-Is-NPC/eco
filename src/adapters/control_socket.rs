//! The daemon socket's protocol: clients send one command per line and receive
//! every event as a JSON line, over the local socket in `local_socket`. A new
//! client first receives the greeting, so it can render the current state. On
//! Windows a client is admitted first, by the daemon's token (docs/design.md §10).

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};
use tokio::task::{JoinHandle, JoinSet};

use crate::adapters::local_socket::{self, ConnectError, Listener, Stream};
use crate::config::{private_dir, private_file};
use crate::domain::events::Event;
use crate::paths;

/// The longest command line a client may send, in bytes; a longer one ends
/// its connection. A `config.set` with a full config is a few kilobytes.
const MAX_LINE: usize = 1 << 20;

/// How many lines may wait to be written to one client.
const MAX_QUEUED: usize = 4096;

/// The command a client sends after others to learn the daemon has read them:
/// its connection answers `SYNCED` to that client alone (docs/design.md §10).
pub const SYNC: &str = "sync";

/// The answer to `SYNC`.
const SYNCED: &str = "{\"type\":\"synced\"}\n";

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

/// Where the daemon listens, and the file that holds its token for the run.
pub struct Address {
    pub socket: PathBuf,
    pub token: PathBuf,
}

impl Address {
    /// The user's daemon.
    pub fn user() -> Self {
        Self {
            socket: paths::socket_path(),
            token: paths::token_path(),
        }
    }

    /// A daemon of a test's own: its socket and token in `directory`.
    #[cfg(test)]
    pub fn scratch(directory: &Path) -> Self {
        Self {
            socket: local_socket::scratch(directory),
            token: directory.join("eco.token"),
        }
    }
}

/// Connect to the daemon at `address` as a client; `None` when nothing
/// listens there, or on Windows when its token file is not there, as while
/// the daemon starts or stops.
pub async fn connect(address: &Address) -> Result<Option<Stream>, ConnectError> {
    match local_socket::connect(&address.socket).await? {
        Some(stream) => introduce(stream, &address.token).await,
        None => Ok(None),
    }
}

/// A client on Linux needs no introduction: the socket is `0600`.
#[cfg(unix)]
async fn introduce(stream: Stream, _token: &Path) -> Result<Option<Stream>, ConnectError> {
    Ok(Some(stream))
}

/// Admit a client on Linux: only its user can open the `0600` socket.
#[cfg(unix)]
async fn admitted(_stream: &mut Stream, _token: &str) -> bool {
    true
}

/// A client on Windows first sends `auth <token>`, the token read from the file
/// `token`.
#[cfg(windows)]
async fn introduce(mut stream: Stream, token: &Path) -> Result<Option<Stream>, ConnectError> {
    let token = match fs::read_to_string(token) {
        Ok(token) => token,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ConnectError::Unavailable(error)),
    };
    stream
        .write_all(format!("{AUTH}{}\n", token.trim()).as_bytes())
        .await
        .map_err(ConnectError::Unavailable)?;
    Ok(Some(stream))
}

/// How a client's first line on Windows starts, before the token.
#[cfg(windows)]
const AUTH: &str = "auth ";

/// How long a client on Windows has to send its first line.
#[cfg(windows)]
const ADMIT_WITHIN: std::time::Duration = std::time::Duration::from_secs(5);

/// Admit a client on Windows, whose pipe any local account can open: its first
/// line must be `auth <token>`, within `ADMIT_WITHIN`. The line is read byte by
/// byte, so nothing after it is taken from the commands.
#[cfg(windows)]
async fn admitted(stream: &mut Stream, token: &str) -> bool {
    let expected = format!("{AUTH}{token}\n");
    let read = async {
        let mut line = Vec::with_capacity(expected.len());
        while line.len() < expected.len() && line.last() != Some(&b'\n') {
            line.push(stream.read_u8().await.ok()?);
        }
        Some(line)
    };
    let line = tokio::time::timeout(ADMIT_WITHIN, read)
        .await
        .ok()
        .flatten();
    // Every byte is compared, so the time taken tells nothing of the token.
    line.is_some_and(|line| {
        line.len() == expected.len()
            && line
                .iter()
                .zip(expected.as_bytes())
                .fold(0, |differ, (a, b)| differ | (a ^ b))
                == 0
    })
}

/// The listening socket and the token file; dropping it stops serving and
/// removes both.
pub struct ControlSocket {
    path: PathBuf,
    accept: JoinHandle<()>,
    _token: TokenFile,
}

impl ControlSocket {
    /// Listen at `address`, with `token` in its token file, forwarding each
    /// command line to `commands`.
    pub async fn bind(
        address: &Address,
        token: String,
        clients: Clients,
        greeting: Greeting,
        commands: mpsc::UnboundedSender<String>,
    ) -> Result<Self> {
        // The socket first: a daemon already running keeps its token file.
        let listener = Listener::bind(&address.socket).await?;
        let kept = TokenFile::write(&address.token, &token)?;
        let token: Arc<str> = token.into();
        let accept = tokio::spawn(async move {
            // Connections live in this set, so stopping the accept loop ends them all.
            let mut connections = JoinSet::new();
            while let Ok(stream) = listener.accept().await {
                connections.spawn(serve(
                    stream,
                    Arc::clone(&token),
                    clients.clone(),
                    Arc::clone(&greeting),
                    commands.clone(),
                ));
            }
        });
        Ok(Self {
            path: address.socket.clone(),
            accept,
            _token: kept,
        })
    }
}

impl Drop for ControlSocket {
    fn drop(&mut self) {
        self.accept.abort();
        local_socket::release(&self.path);
    }
}

/// The token in a file only its user reads; dropping it removes the file.
struct TokenFile(PathBuf);

impl TokenFile {
    fn write(path: &Path, token: &str) -> io::Result<Self> {
        // A file left by a daemon that was killed is replaced, never reused.
        match fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        if let Some(parent) = path.parent() {
            private_dir(parent)?;
        }
        private_file()
            .write(true)
            .create_new(true)
            .open(path)?
            .write_all(token.as_bytes())?;
        Ok(Self(path.into()))
    }
}

impl Drop for TokenFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// A daemon a test plays at an endpoint, with a token of its own: it hands
/// out each client once admitted, as the daemon does.
#[cfg(test)]
pub struct FakeListener {
    listener: Listener,
    _token: TokenFile,
}

#[cfg(test)]
impl FakeListener {
    pub async fn bind(address: &Address) -> Self {
        Self {
            listener: Listener::bind(&address.socket).await.unwrap(),
            _token: TokenFile::write(&address.token, "test-token").unwrap(),
        }
    }

    pub async fn accept(&self) -> io::Result<Stream> {
        loop {
            let mut stream = self.listener.accept().await?;
            if admitted(&mut stream, "test-token").await {
                return Ok(stream);
            }
        }
    }
}

async fn serve(
    mut stream: Stream,
    token: Arc<str>,
    clients: Clients,
    greeting: Greeting,
    commands: mpsc::UnboundedSender<String>,
) {
    if !admitted(&mut stream, &token).await {
        return;
    }
    let (reader, mut writer) = local_socket::split(stream);
    let (outgoing, mut queue) = mpsc::channel::<String>(MAX_QUEUED);
    for event in greeting() {
        let _ = outgoing.try_send(format!("{event}\n"));
    }
    let (kept, dropped) = oneshot::channel();
    let answer = outgoing.clone();
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
            // Lines are read in order, so every line before `SYNC` was taken.
            if command == SYNC {
                if answer.try_send(SYNCED.into()).is_err() {
                    break;
                }
            } else if !command.is_empty() && commands.send(command.to_string()).is_err() {
                break;
            }
        }
    };
    // The client is done when it stops reading, stops writing, sends a line
    // past the cap, or falls behind and its queue is dropped or cannot take
    // its `SYNCED`.
    tokio::select! { () = write => {}, () = read => {}, _ = dropped => {} }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use tokio::io::AsyncReadExt;

    use super::*;

    async fn connect(address: &Address) -> Stream {
        super::connect(address).await.unwrap().unwrap()
    }

    async fn bound(
        address: &Address,
        greeting: Vec<Event>,
    ) -> (ControlSocket, Clients, mpsc::UnboundedReceiver<String>) {
        let clients = Clients::default();
        let (commands, received) = mpsc::unbounded_channel();
        let socket = ControlSocket::bind(
            address,
            "secret".into(),
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
        let path = Address::scratch(directory.path());
        let greeting = vec![json!({"type": "snapshot", "kinds": ["meeting"]})];
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
        // The socket file goes with it; a named pipe has none.
        #[cfg(unix)]
        assert!(!path.socket.exists());
    }

    #[tokio::test]
    async fn sync_is_answered_to_its_client_alone_after_the_lines_before_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = Address::scratch(directory.path());
        let (_socket, clients, mut commands) = bound(&path, Vec::new()).await;
        let other = connect(&path).await;
        let stream = connect(&path).await;
        let (reader, mut writer) = local_socket::split(stream);
        let mut lines = BufReader::new(reader).lines();
        writer.write_all(b"session.toggle\nsync\n").await.unwrap();
        assert_eq!(
            lines.next_line().await.unwrap().unwrap(),
            r#"{"type":"synced"}"#
        );
        assert_eq!(commands.try_recv().unwrap(), "session.toggle");
        assert!(commands.try_recv().is_err(), "`sync` is not a command");
        clients.emit(&json!({"type": "note"}));
        let mut other = BufReader::new(other).lines();
        assert_eq!(
            other.next_line().await.unwrap().unwrap(),
            r#"{"type":"note"}"#
        );
    }

    #[tokio::test]
    async fn a_line_past_the_cap_ends_the_connection() {
        let directory = tempfile::tempdir().unwrap();
        let path = Address::scratch(directory.path());
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
        let path = Address::scratch(directory.path());
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
        let path = Address::scratch(directory.path());
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

    #[tokio::test]
    async fn a_client_whose_queue_cannot_take_synced_is_dropped() {
        let directory = tempfile::tempdir().unwrap();
        let path = Address::scratch(directory.path());
        let (_socket, clients, _commands) = bound(&path, Vec::new()).await;
        let mut stalled = connect(&path).await;
        while clients.0.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        let text = "x".repeat(1024);
        for _ in 0..MAX_QUEUED * 2 {
            clients.emit(&json!({"type": "signal", "input": text, "level": 0.5, "speech": true}));
            tokio::task::yield_now().await;
        }
        stalled.write_all(b"sync\n").await.unwrap();
        // A signal keeps a client whose queue is full: only a closed one goes.
        let started = std::time::Instant::now();
        while !clients.0.lock().unwrap().is_empty() {
            assert!(
                started.elapsed() < std::time::Duration::from_secs(5),
                "the daemon closes the connection"
            );
            clients.emit(&json!({"type": "signal", "input": "x", "level": 0.5, "speech": true}));
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    /// Emit an event until the clients that left are dropped; five seconds at most.
    #[cfg(unix)]
    async fn emit_until_alone(clients: &Clients) -> usize {
        let started = std::time::Instant::now();
        loop {
            clients.emit(&json!({"type": "note", "text": "x"}));
            let left = clients.0.lock().unwrap().len();
            if left == 0 || started.elapsed() > std::time::Duration::from_secs(5) {
                return left;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    // A named pipe has no half to shut: these two run where a socket has.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_client_that_stops_writing_is_dropped() {
        let directory = tempfile::tempdir().unwrap();
        let path = Address::scratch(directory.path());
        let (_socket, clients, mut commands) = bound(&path, Vec::new()).await;
        let mut client = connect(&path).await;
        client.write_all(b"one\n").await.unwrap();
        assert_eq!(commands.recv().await.unwrap(), "one");
        // It could still read what the daemon writes.
        let client = client.into_std().unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        assert_eq!(emit_until_alone(&clients).await, 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_client_that_stops_reading_is_dropped() {
        let directory = tempfile::tempdir().unwrap();
        let path = Address::scratch(directory.path());
        let (_socket, clients, mut commands) = bound(&path, Vec::new()).await;
        let mut client = connect(&path).await;
        client.write_all(b"ready\n").await.unwrap();
        assert_eq!(commands.recv().await.unwrap(), "ready");
        // Its writes still arrive; the daemon's writes to it fail.
        let client = client.into_std().unwrap();
        client.shutdown(std::net::Shutdown::Read).unwrap();
        assert_eq!(emit_until_alone(&clients).await, 0);
    }

    /// The connection is closed after `line`, and no command came of it.
    async fn ends_the_connection(line: &[u8], keep_commands: bool) {
        let directory = tempfile::tempdir().unwrap();
        let path = Address::scratch(directory.path());
        let (_socket, _, mut commands) = bound(&path, Vec::new()).await;
        if !keep_commands {
            commands.close();
        }
        let mut client = connect(&path).await;
        client.write_all(line).await.unwrap();
        let mut rest = Vec::new();
        let closed = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.read_to_end(&mut rest),
        )
        .await;
        assert!(closed.is_ok(), "the daemon closes the connection");
        assert!(commands.try_recv().is_err());
    }

    #[tokio::test]
    async fn a_line_that_is_not_utf8_ends_the_connection() {
        ends_the_connection(b"\xff\xfe\n", true).await;
    }

    #[tokio::test]
    async fn a_command_the_daemon_no_longer_takes_ends_the_connection() {
        ends_the_connection(b"action probe\n", false).await;
    }

    #[tokio::test]
    async fn the_token_file_is_the_users_alone_and_goes_with_the_socket() {
        let directory = tempfile::tempdir().unwrap();
        let address = Address::scratch(directory.path());
        fs::write(&address.token, "stale").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&address.token, fs::Permissions::from_mode(0o644)).unwrap();
        }
        let (socket, _, _commands) = bound(&address, Vec::new()).await;
        assert_eq!(fs::read_to_string(&address.token).unwrap(), "secret");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&address.token).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        drop(socket);
        assert!(!address.token.exists());
    }

    #[tokio::test]
    async fn the_token_file_makes_its_directory() {
        let directory = tempfile::tempdir().unwrap();
        let address = Address {
            socket: local_socket::scratch(directory.path()),
            token: directory.path().join("eco").join("eco.token"),
        };
        let _bound = bound(&address, Vec::new()).await;
        assert_eq!(fs::read_to_string(&address.token).unwrap(), "secret");
    }

    #[tokio::test]
    async fn a_second_daemon_leaves_the_running_ones_token() {
        let directory = tempfile::tempdir().unwrap();
        let address = Address::scratch(directory.path());
        let _running = bound(&address, Vec::new()).await;
        let (commands, _) = mpsc::unbounded_channel();
        let second = ControlSocket::bind(
            &address,
            "other".into(),
            Clients::default(),
            Arc::new(Vec::new),
            commands,
        )
        .await;
        assert!(second.is_err());
        assert_eq!(fs::read_to_string(&address.token).unwrap(), "secret");
    }

    /// What a client of a daemon greeting with `hello` gets after it writes
    /// `first` straight to the pipe, and the command the daemon took from it.
    #[cfg(windows)]
    async fn after_writing(first: &[u8]) -> (Vec<u8>, Option<String>) {
        let directory = tempfile::tempdir().unwrap();
        let address = Address::scratch(directory.path());
        let hello = vec![json!({"type": "daemon"})];
        let (_socket, _, mut commands) = bound(&address, hello).await;
        let mut client = local_socket::connect(&address.socket)
            .await
            .unwrap()
            .unwrap();
        client.write_all(first).await.unwrap();
        let mut received = Vec::new();
        let closed = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            client.read_to_end(&mut received),
        )
        .await;
        assert!(closed.is_ok(), "the daemon closes the connection");
        (received, commands.try_recv().ok())
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_client_without_the_token_gets_nothing_and_is_disconnected() {
        assert_eq!(after_writing(b"sessions\n").await, (Vec::new(), None));
        assert_eq!(after_writing(b"").await, (Vec::new(), None));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_client_with_a_wrong_token_gets_nothing_and_is_disconnected() {
        assert_eq!(
            after_writing(b"auth guess\nsessions\n").await,
            (Vec::new(), None)
        );
        assert_eq!(
            after_writing(b"auth secretX\nsessions\n").await,
            (Vec::new(), None)
        );
        assert_eq!(
            after_writing(b"auth secreT\nsessions\n").await,
            (Vec::new(), None)
        );
    }

    #[cfg(windows)]
    #[tokio::test(start_paused = true)]
    async fn a_client_that_says_nothing_is_disconnected() {
        let directory = tempfile::tempdir().unwrap();
        let address = Address::scratch(directory.path());
        let (_socket, clients, _commands) = bound(&address, Vec::new()).await;
        let mut silent = local_socket::connect(&address.socket)
            .await
            .unwrap()
            .unwrap();
        let mut received = Vec::new();
        silent.read_to_end(&mut received).await.unwrap();
        assert!(received.is_empty());
        assert!(clients.0.lock().unwrap().is_empty());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_client_with_the_token_is_greeted_and_heard() {
        let directory = tempfile::tempdir().unwrap();
        let address = Address::scratch(directory.path());
        let hello = vec![json!({"type": "daemon"})];
        let (_socket, _, mut commands) = bound(&address, hello).await;
        let mut client = local_socket::connect(&address.socket)
            .await
            .unwrap()
            .unwrap();
        client
            .write_all(b"auth secret\naction probe\n")
            .await
            .unwrap();
        let mut lines = BufReader::new(client).lines();
        assert_eq!(
            lines.next_line().await.unwrap().unwrap(),
            r#"{"type":"daemon"}"#
        );
        assert_eq!(commands.recv().await.unwrap(), "action probe");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_client_reads_the_token_from_its_file() {
        let directory = tempfile::tempdir().unwrap();
        let address = Address::scratch(directory.path());
        let (_socket, _, mut commands) = bound(&address, Vec::new()).await;
        let mut client = connect(&address).await;
        client.write_all(b"sessions\n").await.unwrap();
        assert_eq!(commands.recv().await.unwrap(), "sessions");
        // With the file gone, as while the daemon starts or stops, nothing is reached.
        fs::remove_file(&address.token).unwrap();
        assert!(super::connect(&address).await.unwrap().is_none());
    }
}
