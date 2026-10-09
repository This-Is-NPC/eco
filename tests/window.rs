//! eco-window's bridge, as the overlay uses it: the daemon connection (lines
//! each way, connected again after the daemon goes), the environment, the
//! process id and a watched file, run offscreen against
//! `tests/window/bridge.qml`. Needs `mise run window:build`.

use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use serde_json::Value;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// The next connection to `listener`, or a panic once `window` exits or ten
/// seconds pass.
fn accept(listener: &UnixListener, window: &mut Child) -> UnixStream {
    let start = Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                return stream;
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                if let Some(status) = window.try_wait().unwrap() {
                    panic!("eco-window exited with {status}");
                }
                assert!(start.elapsed() < Duration::from_secs(10), "no connection");
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("accept: {error}"),
        }
    }
}

/// Send `line` and read the answer.
fn ask(stream: &mut UnixStream, line: &str) -> Value {
    writeln!(stream, "{line}").unwrap();
    let mut answer = String::new();
    BufReader::new(&*stream).read_line(&mut answer).unwrap();
    serde_json::from_str(&answer).unwrap_or_else(|_| panic!("not JSON: {answer:?}"))
}

#[test]
fn the_bridge_carries_lines_reconnects_reads_files_and_exits_cleanly() {
    let program = Path::new(ROOT).join("target/window/eco-window");
    assert!(
        program.is_file(),
        "{} is missing; run `mise run window:build`",
        program.display()
    );
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("test.sock");
    let file = dir.path().join("note.txt");
    std::fs::write(&file, "first").unwrap();
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut window = Command::new(&program)
        .arg(Path::new(ROOT).join("tests/window/bridge.qml"))
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_QPA_PLATFORMTHEME", "")
        .env("XDG_RUNTIME_DIR", dir.path())
        .env("XDG_CACHE_HOME", dir.path().join("cache"))
        .env("ECO_TEST_SOCKET", &socket)
        .env("ECO_TEST_FILE", &file)
        .env("ECO_TEST_VALUE", "olá")
        .spawn()
        .unwrap();

    let mut first = accept(&listener, &mut window);
    let answer = ask(&mut first, "ping");
    assert_eq!(answer["line"], "ping");
    assert_eq!(answer["value"], "olá");
    assert_eq!(answer["pid"], window.id());
    assert_eq!(answer["text"], "first");

    std::fs::write(&file, "second").unwrap();
    let start = Instant::now();
    while ask(&mut first, "again")["text"] != "second" {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the file was not read again"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    // The daemon goes away; the window connects again on its own.
    drop(first);
    let mut second = accept(&listener, &mut window);
    assert_eq!(ask(&mut second, "back")["line"], "back");

    writeln!(second, "quit").unwrap();
    let status = window.wait().unwrap();
    assert!(status.success(), "eco-window exited with {status}");
}
