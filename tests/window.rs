//! eco-window's bridge, as the overlay uses it: the daemon connection (lines
//! each way, on Windows `auth <token>` first on every connection, connected
//! again after the daemon goes), the process id, the environment, a local path
//! and a watched file, run offscreen against `tests/window/bridge.qml`. Needs
//! `mise run window:build`.

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

use listener::{Listener, Stream};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// A socket the daemon's way: a Unix socket in a directory of the test's.
#[cfg(unix)]
mod listener {
    use std::path::Path;

    use tokio::net::{UnixListener, UnixStream};

    pub type Stream = UnixStream;

    pub struct Listener(UnixListener, String);

    impl Listener {
        pub fn bind(dir: &Path) -> Self {
            let path = dir.join("test.sock");
            Self(
                UnixListener::bind(&path).unwrap(),
                path.display().to_string(),
            )
        }

        /// What the window is given to connect to.
        pub fn name(&self) -> &str {
            &self.1
        }

        pub async fn accept(&mut self) -> Stream {
            self.0.accept().await.unwrap().0
        }
    }
}

/// A socket the daemon's way: a named pipe of the test's own.
#[cfg(windows)]
mod listener {
    use std::path::Path;

    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

    pub type Stream = NamedPipeServer;

    pub struct Listener(NamedPipeServer, String);

    impl Listener {
        pub fn bind(_: &Path) -> Self {
            let name = format!(r"\\.\pipe\eco-test-{}", uuid::Uuid::new_v4().simple());
            let first = ServerOptions::new()
                .first_pipe_instance(true)
                .create(&name)
                .unwrap();
            Self(first, name)
        }

        /// What the window is given to connect to.
        pub fn name(&self) -> &str {
            &self.1
        }

        /// The instance a client connected to, with the next one waiting.
        pub async fn accept(&mut self) -> Stream {
            self.0.connect().await.unwrap();
            let next = ServerOptions::new().create(&self.1).unwrap();
            std::mem::replace(&mut self.0, next)
        }
    }
}

/// The next connection to `listener`, past its `auth` line on Windows, or a
/// panic once `window` exits or ten seconds pass.
async fn accept(listener: &mut Listener, window: &mut Child) -> BufReader<Stream> {
    let mut stream = tokio::select! {
        stream = listener.accept() => BufReader::new(stream),
        status = window.wait() => panic!("eco-window exited with {}", status.unwrap()),
        _ = tokio::time::sleep(Duration::from_secs(10)) => panic!("no connection"),
    };
    if cfg!(windows) {
        assert_eq!(read(&mut stream).await, "auth secret");
    }
    stream
}

async fn read(stream: &mut BufReader<Stream>) -> String {
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_line(&mut line))
        .await
        .expect("no answer")
        .unwrap();
    line.trim_end().to_string()
}

/// Send `line` and read the answer.
async fn ask(stream: &mut BufReader<Stream>, line: &str) -> Value {
    stream
        .write_all(format!("{line}\n").as_bytes())
        .await
        .unwrap();
    let answer = read(stream).await;
    serde_json::from_str(&answer).unwrap_or_else(|_| panic!("not JSON: {answer:?}"))
}

/// The file: URL of `path`, as a file dialog gives it.
fn url(path: &Path) -> String {
    let path = path.display().to_string().replace('\\', "/");
    let path = path.replace(' ', "%20");
    if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    }
}

#[tokio::test]
async fn the_bridge_carries_lines_reconnects_reads_files_and_exits_cleanly() {
    let program = Path::new(ROOT).join(format!(
        "target/window/eco-window{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert!(
        program.is_file(),
        "{} is missing; run `mise run window:build`",
        program.display()
    );
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a note.txt");
    std::fs::write(&file, "first").unwrap();
    let mut listener = Listener::bind(dir.path());
    let mut window = Command::new(&program)
        .arg(Path::new(ROOT).join("tests/window/bridge.qml"))
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_QPA_PLATFORMTHEME", "")
        .env("XDG_RUNTIME_DIR", dir.path())
        .env("XDG_CACHE_HOME", dir.path().join("cache"))
        .env("ECO_TEST_SOCKET", listener.name())
        .env("ECO_TEST_TOKEN", "secret")
        .env("ECO_TEST_FILE", &file)
        .env("ECO_TEST_URL", url(&file))
        .env("ECO_TEST_VALUE", "olá")
        .stdout(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();

    let mut first = accept(&mut listener, &mut window).await;
    let answer = ask(&mut first, "ping").await;
    assert_eq!(answer["line"], "ping");
    assert_eq!(answer["value"], "olá");
    assert_eq!(answer["text"], "first");
    assert_eq!(answer["pid"], window.id().unwrap());
    assert_eq!(Path::new(answer["path"].as_str().unwrap()), file);

    std::fs::write(&file, "second").unwrap();
    let start = Instant::now();
    while ask(&mut first, "again").await["text"] != "second" {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the file was not read again"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // The daemon goes away; the window connects again on its own.
    drop(first);
    let mut second = accept(&mut listener, &mut window).await;
    assert_eq!(ask(&mut second, "back").await["line"], "back");

    second.write_all(b"quit\n").await.unwrap();
    let status = tokio::time::timeout(Duration::from_secs(10), window.wait())
        .await
        .expect("eco-window did not exit")
        .unwrap();
    assert!(status.success(), "eco-window exited with {status}");
}
