//! eco-window's bridge, as the overlay uses it: the daemon connection (lines
//! each way, on Windows `auth <token>` first on every connection, connected
//! again after the daemon goes), the process id, the environment, a local path,
//! a watched file and raising a window, run offscreen against `tests/window/bridge.qml`;
//! on Windows and macOS, a copied text that outlives the window, on the real platform;
//! the overlay's views driven as the daemon drives them, in
//! `tests/window/views.qml`; a long timeline read back and its counts, in
//! `tests/window/counts.qml`; a long timeline read in pages, in
//! `tests/window/timeline.qml`; a line's menu, merge and split, in
//! `tests/window/lines.qml`; the settings window's section keys, in
//! `tests/window/settings_keys.qml`; the toast stack, in
//! `tests/window/toasts.qml`; the bars and tables of the views, pressed
//! as a keyboard and a pointer press them, in `tests/window/bars.qml`; and the
//! eco window closed with the settings open,
//! saying so to the daemon before it exits,
//! in `tests/window/close.qml`; the uninstall dialog, in
//! `tests/window/uninstall.qml`; its views and settings made only while
//! wanted, in `tests/window/lazy.qml`; and the tray icon's states and menu, in
//! `tests/window/tray.qml`. Needs `mise run window:build`.

use std::path::{Path, PathBuf};
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

/// Where `mise run window:build` puts eco-window: on macOS inside its bundle.
#[cfg(target_os = "linux")]
const PROGRAM: &str = "target/window/eco-window";
#[cfg(windows)]
const PROGRAM: &str = "target/window/eco-window.exe";
#[cfg(target_os = "macos")]
const PROGRAM: &str = "target/window.noindex/eco.app/Contents/MacOS/eco";

/// The eco-window `mise run window:build` built.
fn program() -> PathBuf {
    let program = Path::new(ROOT).join(PROGRAM);
    assert!(
        program.is_file(),
        "{} is missing; run `mise run window:build`",
        program.display()
    );
    program
}

/// eco-window running `tests/window/<qml>.qml` offscreen, with the overlay's
/// modules, its runtime and cache in `dir`, and no display of the user's.
fn offscreen(qml: &str, dir: &Path) -> Command {
    let mut window = Command::new(program());
    window
        .arg(Path::new(ROOT).join(format!("tests/window/{qml}.qml")))
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .env("QML_IMPORT_PATH", Path::new(ROOT).join("overlay"))
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_QPA_PLATFORMTHEME", "")
        .env("XDG_RUNTIME_DIR", dir)
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("XDG_STATE_HOME", dir.join("state"));
    // Offscreen Qt on Windows reads fonts from a directory, by default one
    // beside the program that windeployqt leaves empty: the system's instead.
    #[cfg(windows)]
    window.env(
        "QT_QPA_FONTDIR",
        Path::new(&std::env::var_os("WINDIR").expect("WINDIR is set")).join("Fonts"),
    );
    window
}

#[tokio::test]
async fn the_overlay_views_pass_their_checks() {
    let dir = tempfile::tempdir().unwrap();
    let run = offscreen("views", dir.path())
        .env("ECO_SOCKET", dir.path().join("no-daemon"))
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(Duration::from_secs(30), run)
        .await
        .expect("the views did not finish")
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Opening General refreshes a stale update but leaves a fresh one alone, and
/// its version row gives the result's age.
#[tokio::test]
async fn general_refreshes_stale_updates_and_shows_their_age() {
    let dir = tempfile::tempdir().unwrap();
    let mut listener = Listener::bind(dir.path());
    let mut window = offscreen("updates", dir.path())
        .env("ECO_SOCKET", listener.name())
        .env("ECO_TOKEN", "secret")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut daemon = accept(&mut listener, &mut window).await;
    let mut stderr = window.stderr.take().unwrap();
    let mut said = String::new();
    let mut sent = Vec::new();
    let status = tokio::time::timeout(Duration::from_secs(30), async {
        let lines = async {
            let mut line = String::new();
            while daemon.read_line(&mut line).await.unwrap_or(0) > 0 {
                sent.push(line.trim_end().to_string());
                line.clear();
            }
        };
        let errors = tokio::io::AsyncReadExt::read_to_string(&mut stderr, &mut said);
        let (_, read) = tokio::join!(lines, errors);
        read.unwrap();
        window.wait().await.unwrap()
    })
    .await
    .expect("the update checks did not finish");
    assert!(status.success(), "{said}");
    assert_eq!(
        sent.iter()
            .filter(|line| line.as_str() == "update.check")
            .count(),
        1,
        "{sent:?}"
    );
}

/// UNINSTALL in General asks first and sends `uninstall` with the choice only
/// once confirmed.
#[tokio::test]
async fn uninstalling_asks_first_and_sends_the_choice_once_confirmed() {
    let dir = tempfile::tempdir().unwrap();
    let mut listener = Listener::bind(dir.path());
    let mut window = offscreen("uninstall", dir.path())
        .env("ECO_SOCKET", listener.name())
        .env("ECO_TOKEN", "secret")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut daemon = accept(&mut listener, &mut window).await;
    let mut stderr = window.stderr.take().unwrap();
    let mut said = String::new();
    let mut sent = Vec::new();
    let status = tokio::time::timeout(Duration::from_secs(30), async {
        let lines = async {
            let mut line = String::new();
            while daemon.read_line(&mut line).await.unwrap_or(0) > 0 {
                sent.push(line.trim_end().to_string());
                line.clear();
            }
        };
        let errors = tokio::io::AsyncReadExt::read_to_string(&mut stderr, &mut said);
        let (_, read) = tokio::join!(lines, errors);
        read.unwrap();
        window.wait().await.unwrap()
    })
    .await
    .expect("the uninstall checks did not finish");
    assert!(status.success(), "{said}");
    let removals: Vec<&String> = sent
        .iter()
        .filter(|line| line.starts_with("uninstall"))
        .collect();
    assert_eq!(removals, [r#"uninstall {"everything":true}"#], "{sent:?}");
}

/// A 6000-line session read back loads and takes its translations within a
/// time limit, and its counts are the daemon's after a line is added, edited or
/// removed and after another session is shown.
#[tokio::test]
async fn a_long_timeline_loads_in_time_and_counts_what_the_daemon_says() {
    let dir = tempfile::tempdir().unwrap();
    let run = offscreen("counts", dir.path())
        .env("ECO_SOCKET", dir.path().join("no-daemon"))
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(Duration::from_secs(60), run)
        .await
        .expect("the counts did not finish")
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A long session's timeline in pages: an older page goes in above and the
/// entry read stays put, the live timeline keeps its cap while it follows the
/// newest, a found session opens around the time found, and a page not asked
/// for is not put in.
#[tokio::test]
async fn the_timeline_reads_a_long_session_in_pages() {
    let dir = tempfile::tempdir().unwrap();
    let run = offscreen("timeline", dir.path())
        .env("ECO_SOCKET", dir.path().join("no-daemon"))
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(Duration::from_secs(30), run)
        .await
        .expect("the timeline did not finish")
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The eco window's views and its settings are made only while wanted: a view
/// when first shown, the settings each time they open, let go when they close.
#[tokio::test]
async fn views_and_settings_are_made_when_shown() {
    let dir = tempfile::tempdir().unwrap();
    let run = offscreen("lazy", dir.path())
        .env("ECO_SOCKET", dir.path().join("no-daemon"))
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(Duration::from_secs(30), run)
        .await
        .expect("the views did not finish")
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Presets, live system changes, custom colors and the appearance controls.
#[tokio::test]
async fn appearance_presets_and_custom_colors_follow_the_saved_config() {
    let dir = tempfile::tempdir().unwrap();
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        offscreen("appearance", dir.path())
            .env("ECO_SOCKET", dir.path().join("no-daemon"))
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("the appearance checks did not finish")
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The settings window's section keys, pressed as a keyboard presses them:
/// a section's number, bare or with Ctrl, a field keeping its digits, and the
/// rail unfolding under the keyboard.
#[tokio::test]
async fn the_settings_keys_choose_sections() {
    let dir = tempfile::tempdir().unwrap();
    let run = offscreen("settings_keys", dir.path())
        .env("ECO_SOCKET", dir.path().join("no-daemon"))
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(Duration::from_secs(30), run)
        .await
        .expect("the keys did not finish")
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The toast stack: three at most, the pinned one kept, information closing
/// after its time unless held by the pointer or the keyboard, errors staying,
/// Esc and Tab, and a notice replacing one of its topic or text.
#[tokio::test]
async fn the_toasts_come_stay_and_go_as_they_should() {
    let dir = tempfile::tempdir().unwrap();
    let run = offscreen("toasts", dir.path())
        .env("ECO_SOCKET", dir.path().join("no-daemon"))
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(Duration::from_secs(30), run)
        .await
        .expect("the toasts did not finish")
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The bars and tables of the views, pressed as a keyboard and a pointer
/// press them, with a daemon socket that answers nothing: a row's own act, the
/// guarded command's hairline, a bar's readout, the sessions' NEW SESSION and
/// N, and the details' close.
#[tokio::test]
async fn the_bars_and_tables_answer_their_keys() {
    let dir = tempfile::tempdir().unwrap();
    let mut listener = Listener::bind(dir.path());
    let mut window = offscreen("bars", dir.path())
        .env("ECO_SOCKET", listener.name())
        .env("ECO_TOKEN", "secret")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let _daemon = accept(&mut listener, &mut window).await;
    let mut stderr = window.stderr.take().unwrap();
    let mut said = String::new();
    let status = tokio::time::timeout(Duration::from_secs(30), async {
        tokio::io::AsyncReadExt::read_to_string(&mut stderr, &mut said)
            .await
            .unwrap();
        window.wait().await.unwrap()
    })
    .await
    .expect("the bars did not finish");
    assert!(status.success(), "{said}");
}

/// A line's menu, opened by a click, by its "more" button and by the Menu
/// key; a merge picked in it, given up or done on the next line clicked; and a
/// split at the cursor, counted in characters: the window's own checks pass,
/// and it sends the merge and the two splits, and no other change to a line.
#[tokio::test]
async fn a_lines_menu_merges_and_splits_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut listener = Listener::bind(dir.path());
    let mut window = offscreen("lines", dir.path())
        .env("ECO_SOCKET", listener.name())
        .env("ECO_TOKEN", "secret")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut daemon = accept(&mut listener, &mut window).await;
    let mut stderr = window.stderr.take().unwrap();
    let mut said = String::new();
    let mut sent = Vec::new();
    let status = tokio::time::timeout(Duration::from_secs(30), async {
        let lines = async {
            let mut line = String::new();
            while daemon.read_line(&mut line).await.unwrap_or(0) > 0 {
                sent.push(line.trim_end().to_string());
                line.clear();
            }
        };
        let errors = tokio::io::AsyncReadExt::read_to_string(&mut stderr, &mut said);
        let (_, read) = tokio::join!(lines, errors);
        read.unwrap();
        window.wait().await.unwrap()
    })
    .await
    .expect("the lines did not finish");
    assert!(status.success(), "{said}");
    let changes: Vec<&str> = sent
        .iter()
        .map(String::as_str)
        .filter(|line| line.starts_with("session.line."))
        .collect();
    assert_eq!(
        changes,
        [
            r#"session.line.merge {"id":"s1","into":{"who":"Eles","at":100},"from":{"who":"Eu","at":101}}"#,
            r#"session.line.split {"id":"s1","who":"Eles","at":102,"offset":6}"#,
            r#"session.line.split {"id":"s1","who":"Eles","at":103,"offset":4}"#,
        ]
    );
}

/// The eco window says whether it is active once the daemon greets it; closing
/// it tells the daemon and ends its process, even with
/// the settings open and unsaved: Qt's quit asks every window to close, and
/// the settings refuse.
#[tokio::test]
async fn closing_the_window_tells_the_daemon_and_ends_the_process() {
    let dir = tempfile::tempdir().unwrap();
    let mut listener = Listener::bind(dir.path());
    let mut window = offscreen("close", dir.path())
        .env("ECO_SOCKET", listener.name())
        .env("ECO_TOKEN", "secret")
        .env("ECO_WINDOW", "3")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut daemon = accept(&mut listener, &mut window).await;
    daemon
        .write_all(
            b"{\"type\":\"snapshot\",\"language\":\"en\",\"ui_language\":\"auto\",\"live\":[]}\n",
        )
        .await
        .unwrap();
    let mut stderr = BufReader::new(window.stderr.take().unwrap()).lines();
    let closing = async {
        while let Some(line) = stderr.next_line().await.unwrap() {
            if line.contains("closing") {
                return;
            }
        }
    };
    tokio::select! {
        () = closing => {}
        status = window.wait() => panic!("eco-window exited with {} before closing", status.unwrap()),
        _ = tokio::time::sleep(Duration::from_secs(30)) => panic!("the window never closed"),
    }
    let mut active = false;
    loop {
        let line = read(&mut daemon).await;
        assert!(!line.is_empty(), "the window never said it closed");
        active |= line.starts_with(r#"window.active {"window":3,"active":"#);
        if line == r#"window.closed {"window":3}"# {
            break;
        }
    }
    assert!(active, "the window never said whether it is active");
    let status = tokio::time::timeout(Duration::from_secs(2), window.wait())
        .await
        .expect("eco-window still runs 2 s after its window closed")
        .unwrap();
    assert!(status.success(), "eco-window exited with {status}");
}

/// The tray icon leaves out the events it draws nothing from, follows the
/// sessions, and its click and menu send what `eco window` and `eco stop`
/// send: a context click nothing, a click and Open `focus`, then New session,
/// Settings and Stop eco.
#[tokio::test]
async fn the_tray_shows_the_state_and_its_menu_sends_the_cli_lines() {
    let dir = tempfile::tempdir().unwrap();
    let mut listener = Listener::bind(dir.path());
    let mut tray = offscreen("tray", dir.path())
        .env("ECO_SOCKET", listener.name())
        .env("ECO_TOKEN", "secret")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut daemon = accept(&mut listener, &mut tray).await;
    // First it leaves out what it draws nothing from, keeping the sessions and errors.
    let events = read(&mut daemon).await;
    let except: Value = serde_json::from_str(events.strip_prefix("events ").unwrap()).unwrap();
    let except: Vec<&str> = except["except"]
        .as_array()
        .unwrap()
        .iter()
        .map(|kind| kind.as_str().unwrap())
        .collect();
    for left in ["signal", "transcript_partial", "suggestion_delta"] {
        assert!(except.contains(&left), "{left} still sent");
    }
    for kept in ["session", "error", "snapshot"] {
        assert!(!except.contains(&kept), "{kept} left out");
    }
    let mut lines = Vec::new();
    for _ in 0..5 {
        lines.push(read(&mut daemon).await);
    }
    assert_eq!(
        lines,
        [
            r#"window.call {"call":"focus"}"#,
            r#"window.call {"call":"focus"}"#,
            r#"window.call {"call":"new_session"}"#,
            r#"window.call {"call":"config"}"#,
            "stop",
        ]
    );
    let output = tokio::time::timeout(Duration::from_secs(10), tray.wait_with_output())
        .await
        .expect("the tray did not exit")
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// eco-window running `tests/window/bridge.qml` on the Qt `platform`,
/// connecting to `listener`, its runtime in `dir` and its watched file `file`.
fn bridge(platform: &str, dir: &Path, listener: &Listener, file: &Path) -> Child {
    offscreen("bridge", dir)
        .env("QT_QPA_PLATFORM", platform)
        .env("ECO_TEST_SOCKET", listener.name())
        .env("ECO_TEST_TOKEN", "secret")
        .env("ECO_TEST_FILE", file)
        .env("ECO_TEST_URL", url(file))
        .env("ECO_TEST_VALUE", "olá")
        .stdout(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap()
}

#[tokio::test]
async fn the_bridge_carries_lines_reconnects_reads_files_and_exits_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a note.txt");
    std::fs::write(&file, "first").unwrap();
    let mut listener = Listener::bind(dir.path());
    let mut window = bridge("offscreen", dir.path(), &listener, &file);

    let mut first = accept(&mut listener, &mut window).await;
    let answer = ask(&mut first, "ping").await;
    assert_eq!(answer["line"], "ping");
    assert_eq!(answer["value"], "olá");
    assert_eq!(answer["text"], "first");
    assert_eq!(answer["pid"], window.id().unwrap());
    assert_eq!(Path::new(answer["path"].as_str().unwrap()), file);
    // No offscreen window is in front of Windows, whatever Qt calls active; on
    // macOS the application is in front once Qt has activated its window; on
    // Linux the bridge leaves it to Qt.
    #[cfg(not(target_os = "macos"))]
    assert_eq!(answer["foreground"], cfg!(unix));
    #[cfg(target_os = "macos")]
    {
        let start = Instant::now();
        while ask(&mut first, "active").await["foreground"] != true {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "the application never became active"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    assert!(answer["system_dark"].is_boolean());
    // On macOS the fixed-width family is Cocoa's, read on the real platform below.
    #[cfg(not(target_os = "macos"))]
    assert_eq!(answer["monospace"], "monospace");
    #[cfg(target_os = "linux")]
    assert_eq!(
        Path::new(answer["system_theme_directory"].as_str().unwrap()),
        dir.path().join("state/omarchy/current")
    );
    #[cfg(not(target_os = "linux"))]
    assert_eq!(answer["system_theme_directory"], "");
    assert_eq!(ask(&mut first, "raise").await["line"], "raise");

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

/// The real Qt platform, and the system's own reading of its clipboard.
#[cfg(windows)]
fn clipboard() -> (&'static str, std::process::Command) {
    let mut read = std::process::Command::new("powershell");
    read.args([
        "-NoProfile",
        "-Command",
        "[Console]::OutputEncoding = [Text.Encoding]::UTF8; Get-Clipboard",
    ]);
    ("windows", read)
}

/// The real Qt platform, and the system's own reading of its clipboard.
#[cfg(target_os = "macos")]
fn clipboard() -> (&'static str, std::process::Command) {
    let mut read = std::process::Command::new("pbpaste");
    read.env("LANG", "en_US.UTF-8");
    ("cocoa", read)
}

/// A text copied on Windows or macOS stays on the clipboard once the window is
/// killed, as `eco stop` kills it, and on macOS the text is in the system's
/// fixed-width font. Offscreen, Qt keeps a clipboard and fonts of its own.
#[cfg(any(windows, target_os = "macos"))]
#[tokio::test]
async fn a_copied_text_outlives_the_window() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("note.txt");
    std::fs::write(&file, "").unwrap();
    let mut listener = Listener::bind(dir.path());
    let (platform, mut read) = clipboard();
    let mut window = bridge(platform, dir.path(), &listener, &file);
    let mut link = accept(&mut listener, &mut window).await;
    let answer = ask(&mut link, "copy").await;
    assert_eq!(answer["line"], "copy");
    // macOS has no "monospace" family: the system's fixed-width one stands in.
    #[cfg(target_os = "macos")]
    assert_ne!(answer["monospace"], "monospace");
    window.kill().await.unwrap();
    let read = read.output().unwrap();
    assert_eq!(String::from_utf8_lossy(&read.stdout).trim(), "olá");
}
