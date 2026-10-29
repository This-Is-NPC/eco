//! Start, stop and check the daemon over its socket; the caller names the
//! daemon's address and the service manager that starts the daemon.

#[cfg(windows)]
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::{sleep, timeout};

use crate::adapters::control_socket::{self, Address};
use crate::adapters::local_socket::Stream;
use crate::cli;
use crate::ports::ServiceManager;

const READY_TIMEOUT: Duration = Duration::from_secs(10);

struct Daemon {
    stream: BufReader<Stream>,
    overlay: bool,
    pid: u32,
    version: String,
    /// The executable the daemon says it runs.
    #[cfg(windows)]
    exe: Option<PathBuf>,
}

async fn socket(address: &Address) -> Result<Option<Stream>> {
    Ok(control_socket::connect(address).await?)
}

async fn connect(address: &Address) -> Result<Option<Daemon>> {
    let Some(stream) = socket(address).await? else {
        return Ok(None);
    };
    let mut stream = BufReader::new(stream);
    let mut greeting = String::new();
    let size = timeout(Duration::from_secs(2), stream.read_line(&mut greeting))
        .await
        .context("the eco socket did not answer")??;
    if size == 0 {
        bail!("the eco socket closed before identifying its daemon");
    }
    let status: Value = serde_json::from_str(&greeting).context("invalid eco socket response")?;
    if status["type"] != "daemon" {
        bail!("the eco socket speaks an incompatible protocol; restart the old daemon");
    }
    Ok(Some(Daemon {
        stream,
        overlay: status["overlay"].as_bool().unwrap_or(false),
        pid: u32::try_from(
            status["pid"]
                .as_u64()
                .context("daemon response has no pid")?,
        )
        .context("daemon response has an invalid pid")?,
        version: status["version"]
            .as_str()
            .context("daemon response has no version")?
            .into(),
        #[cfg(windows)]
        exe: status["exe"].as_str().map(PathBuf::from),
    }))
}

/// The daemon once it answers at `address`, or `None` when it has not within
/// `READY_TIMEOUT`.
async fn ready(address: &Address) -> Result<Option<Daemon>> {
    let until = tokio::time::Instant::now() + READY_TIMEOUT;
    loop {
        if let Some(daemon) = connect(address).await? {
            return Ok(Some(daemon));
        }
        if tokio::time::Instant::now() >= until {
            return Ok(None);
        }
        sleep(Duration::from_millis(100)).await;
    }
}

/// The daemon's own reason for not coming up in its `output`: the last line
/// it starts with `eco: ` or `Error: `, as it exits on an error, with the
/// `Caused by:` lines that follow it, each indented; `None` when there is no
/// such line. Nothing else is taken: the output may hold transcripts.
fn reason(output: &str) -> Option<String> {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect();
    let from = lines
        .iter()
        .rposition(|line| line.starts_with("eco: ") || line.starts_with("Error: "))?;
    let mut said = vec![lines[from]];
    if lines.get(from + 1) == Some(&"Caused by:") {
        said.extend(
            lines[from + 1..]
                .iter()
                .take_while(|line| **line == "Caused by:" || line.starts_with(' ')),
        );
    }
    Some(
        said.iter()
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// Ask `daemon` for its window and return what to say once it is in front:
/// opened, or the one already open brought forward.
async fn open(mut daemon: Daemon) -> Result<&'static str> {
    daemon.stream.get_mut().write_all(b"overlay.open\n").await?;
    let mut line = String::new();
    loop {
        line.clear();
        let size = timeout(READY_TIMEOUT, daemon.stream.read_line(&mut line))
            .await
            .context("timed out waiting for the eco window")??;
        if size == 0 {
            bail!("the daemon closed while opening the eco window");
        }
        let event: Value = serde_json::from_str(&line).context("invalid eco socket response")?;
        if event["type"] == "overlay_status" {
            if event["open"] == true {
                return Ok(match event["raised"] == true {
                    true => "eco: window already open, brought forward",
                    false => "eco: window opened",
                });
            }
            bail!(
                "eco window failed: {}",
                event["message"].as_str().unwrap_or("unknown error")
            );
        }
    }
}

/// Refuse, before anything is stopped or started, when this process is
/// elevated: a daemon of high integrity cannot be reached by the windows and
/// the Start menu entry, which are not.
async fn ensure_unelevated(service: &dyn ServiceManager) -> Result<()> {
    if service.elevated().await {
        bail!(
            "eco runs without administrator rights, so every window can reach it; run this from a terminal that is not elevated"
        );
    }
    Ok(())
}

/// Start the daemon through `service` and wait for it at `address`.
async fn launch(service: &dyn ServiceManager, address: &Address) -> Result<Daemon> {
    let since = SystemTime::now();
    service.start().await?;
    let Some(daemon) = ready(address).await? else {
        let said = service.output(since).await;
        match said.as_deref().and_then(reason) {
            Some(reason) => {
                bail!("the daemon was started, but did not open the socket; it said:\n{reason}")
            }
            None => bail!("the daemon was started, but did not open the socket"),
        }
    };
    Ok(daemon)
}

/// Whether `daemon` is this eco: the same version, running this executable.
/// An executable this process cannot see, the daemon's pid being of another
/// namespace (a sandbox), is not known to be another.
fn is_current(daemon: &Daemon) -> bool {
    daemon.version == env!("CARGO_PKG_VERSION") && runs_this_executable(daemon).unwrap_or(true)
}

/// The daemon that answers at `address`, or `None` when none does. One of
/// another version or executable, left running by an upgrade, is stopped and
/// started again through `service` first, and that is said on stderr: it
/// would launch windows of this version, which it cannot serve.
async fn current(service: &dyn ServiceManager, address: &Address) -> Result<Option<Daemon>> {
    let Some(daemon) = connect(address).await? else {
        return Ok(None);
    };
    if is_current(&daemon) {
        return Ok(Some(daemon));
    }
    ensure_unelevated(service).await?;
    let old = daemon.version;
    drop(daemon.stream);
    halt(address).await?;
    let daemon = launch(service, address).await?;
    eprintln!("eco: restarted the daemon {old} → {}", daemon.version);
    Ok(Some(daemon))
}

/// Reach the daemon at `address`, started through `service` when nothing
/// answers and restarted when it is not this eco ([`current`]).
pub async fn start(
    service: &dyn ServiceManager,
    address: &Address,
    show_window: bool,
) -> Result<()> {
    let daemon = match current(service, address).await? {
        Some(daemon) => daemon,
        None => {
            ensure_unelevated(service).await?;
            launch(service, address).await?
        }
    };
    if show_window {
        println!("{}", open(daemon).await?);
    } else {
        println!("eco: daemon running");
    }
    Ok(())
}

/// Run `request` against the daemon at `address` and return the exit code,
/// as [`cli::run`] does. A shortcut's command is first sent to the [`current`]
/// daemon, which launches its windows.
pub async fn ask(
    service: &dyn ServiceManager,
    address: &Address,
    request: cli::Request,
) -> Result<i32> {
    if matches!(request, cli::Request::Window(_)) {
        current(service, address).await?;
    }
    Ok(cli::run(address, request).await)
}

/// Ask the daemon at `address` to stop and wait until it has; whether one ran.
async fn halt(address: &Address) -> Result<bool> {
    let Some(mut stream) = socket(address).await? else {
        return Ok(false);
    };
    stream.write_all(b"stop\n").await?;
    let until = tokio::time::Instant::now() + READY_TIMEOUT;
    loop {
        if socket(address).await?.is_none() {
            return Ok(true);
        }
        if tokio::time::Instant::now() >= until {
            bail!("the daemon did not stop within 10 seconds");
        }
        sleep(Duration::from_millis(100)).await;
    }
}

pub async fn stop(address: &Address) -> Result<()> {
    match halt(address).await? {
        true => println!("eco: daemon stopped"),
        false => println!("eco: daemon already stopped"),
    }
    Ok(())
}

/// Stop the daemon at `address` and start it again through `service`, with its
/// window open unless the running daemon had it closed.
pub async fn restart(service: &dyn ServiceManager, address: &Address) -> Result<()> {
    ensure_unelevated(service).await?;
    let show_window = if socket(address).await?.is_some() {
        connect(address)
            .await
            .ok()
            .flatten()
            .is_none_or(|daemon| daemon.overlay)
    } else {
        true
    };
    stop(address).await?;
    start(service, address, show_window).await
}

/// Report the daemon at `address` and return the exit status: 0 when it runs,
/// 3 when it is stopped or `window_open` asks for a window it has closed.
pub async fn status(address: &Address, expect_current_exe: bool, window_open: bool) -> Result<i32> {
    let Some(_) = socket(address).await? else {
        println!("eco: daemon stopped");
        return Ok(3);
    };
    let daemon = match connect(address).await {
        Ok(Some(daemon)) => daemon,
        Err(error) if !expect_current_exe => {
            println!("eco: daemon running ({error})");
            return Ok(0);
        }
        result => result?.context("the daemon stopped")?,
    };
    if window_open && !daemon.overlay {
        return Ok(3);
    }
    if expect_current_exe && !runs_this_executable(&daemon)? {
        bail!(
            "daemon pid {} is running a different eco executable",
            daemon.pid
        );
    }
    println!(
        "eco: daemon running (pid {}, version {})",
        daemon.pid, daemon.version
    );
    Ok(0)
}

/// Whether `daemon` runs this very executable file, by device and inode, so
/// an executable replaced since the daemon started is another.
#[cfg(unix)]
fn runs_this_executable(daemon: &Daemon) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let running = std::fs::metadata(format!("/proc/{}/exe", daemon.pid))?;
    let current = std::fs::metadata(std::env::current_exe()?)?;
    Ok((running.dev(), running.ino()) == (current.dev(), current.ino()))
}

/// Whether `daemon` runs this very executable, by the path it says it runs:
/// Windows lets no one overwrite or delete an executable while it runs.
#[cfg(windows)]
fn runs_this_executable(daemon: &Daemon) -> Result<bool> {
    Ok(daemon.exe.as_deref() == Some(std::env::current_exe()?.as_path()))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::adapters::control_socket::FakeListener;

    fn socket_in(directory: &tempfile::TempDir) -> Address {
        Address::scratch(directory.path())
    }

    /// This eco's version, which a current daemon has.
    const VERSION: &str = env!("CARGO_PKG_VERSION");

    /// A daemon's greeting as `pid` of `version`, which runs this test's
    /// executable when it is this process.
    fn greeting(pid: u32, version: &str, overlay: bool) -> String {
        let exe = if pid == std::process::id() {
            std::env::current_exe().unwrap()
        } else {
            PathBuf::from("/elsewhere/eco")
        };
        let greeting = serde_json::json!({"type": "daemon", "pid": pid, "version": version,
                                          "exe": exe, "overlay": overlay});
        format!("{greeting}\n")
    }

    /// Whether the fake daemon goes away on `stop`.
    #[derive(Clone, Copy, PartialEq)]
    enum OnStop {
        Exit,
        Ignore,
    }

    /// A daemon on `socket` that greets every client as `pid` of `version`,
    /// opens its window on `overlay.open`, answers `sync` and goes away on
    /// `stop` unless it ignores it.
    /// Like the real one, it serves each client on its own while it listens,
    /// and its clients go with it.
    async fn fake_daemon(
        socket: &Address,
        pid: u32,
        version: &'static str,
        overlay: bool,
        on_stop: OnStop,
    ) {
        let listener = FakeListener::bind(socket).await;
        tokio::spawn(async move {
            let (stopped, mut stop) = tokio::sync::mpsc::channel::<()>(1);
            let mut clients = tokio::task::JoinSet::new();
            loop {
                let stream = tokio::select! {
                    accepted = listener.accept() => accepted.unwrap(),
                    _ = stop.recv() => break,
                };
                let stopped = stopped.clone();
                clients.spawn(async move {
                    let mut stream = BufReader::new(stream);
                    let _ = stream
                        .get_mut()
                        .write_all(greeting(pid, version, overlay).as_bytes())
                        .await;
                    let mut line = String::new();
                    while matches!(stream.read_line(&mut line).await, Ok(1..)) {
                        if line == "stop\n" && on_stop == OnStop::Exit {
                            let _ = stopped.send(()).await;
                            return;
                        }
                        if line == "sync\n" {
                            let _ = stream.get_mut().write_all(b"{\"type\":\"synced\"}\n").await;
                        }
                        if line == "overlay.open\n" {
                            let _ = stream
                                .get_mut()
                                .write_all(b"{\"type\":\"overlay_status\",\"open\":true}\n")
                                .await;
                        }
                        line.clear();
                    }
                });
            }
        });
    }

    /// A socket on `path` that writes `reply` to every client and hangs up.
    async fn replying(path: &Address, reply: &'static [u8]) {
        let listener = FakeListener::bind(path).await;
        tokio::spawn(async move {
            while let Ok(mut stream) = listener.accept().await {
                let _ = stream.write_all(reply).await;
            }
        });
    }

    /// A service manager whose daemon comes up on `socket` with its window
    /// closed, run by an `elevated` process or not.
    struct FakeService {
        socket: Address,
        starts: AtomicUsize,
        elevated: bool,
    }

    impl ServiceManager for FakeService {
        fn start(&self) -> futures::future::BoxFuture<'_, Result<()>> {
            Box::pin(async {
                self.starts.fetch_add(1, Ordering::SeqCst);
                fake_daemon(&self.socket, 1, VERSION, false, OnStop::Exit).await;
                Ok(())
            })
        }

        fn elevated(&self) -> futures::future::BoxFuture<'_, bool> {
            Box::pin(async { self.elevated })
        }

        fn output(&self, _: SystemTime) -> futures::future::BoxFuture<'_, Option<String>> {
            Box::pin(async { None })
        }
    }

    fn fake_service(directory: &tempfile::TempDir) -> FakeService {
        FakeService {
            socket: socket_in(directory),
            starts: 0.into(),
            elevated: false,
        }
    }

    fn elevated_service(directory: &tempfile::TempDir) -> FakeService {
        FakeService {
            elevated: true,
            ..fake_service(directory)
        }
    }

    async fn connect_error(reply: &'static [u8]) -> String {
        let directory = tempfile::tempdir().unwrap();
        let path = socket_in(&directory);
        replying(&path, reply).await;
        format!("{:#}", connect(&path).await.err().unwrap())
    }

    /// The error of opening the window of a daemon that answers `overlay.open`
    /// with `reply` and then hangs up, or never answers when `reply` is `None`.
    async fn open_error(reply: Option<&'static [u8]>) -> String {
        let directory = tempfile::tempdir().unwrap();
        let path = socket_in(&directory);
        let listener = FakeListener::bind(&path).await;
        tokio::spawn(async move {
            let stream = listener.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            let hello = greeting(1, VERSION, false);
            stream.get_mut().write_all(hello.as_bytes()).await.unwrap();
            let mut line = String::new();
            stream.read_line(&mut line).await.unwrap();
            match reply {
                Some(reply) => stream.get_mut().write_all(reply).await.unwrap(),
                None => std::future::pending().await,
            }
        });
        let daemon = connect(&path).await.unwrap().unwrap();
        format!("{:#}", open(daemon).await.unwrap_err())
    }

    #[tokio::test]
    async fn window_open_waits_for_a_daemon_acknowledgement() {
        let directory = tempfile::tempdir().unwrap();
        let path = socket_in(&directory);
        let listener = FakeListener::bind(&path).await;
        let server = tokio::spawn(async move {
            let mut stream = listener.accept().await.unwrap();
            stream
                .write_all(greeting(1, VERSION, false).as_bytes())
                .await
                .unwrap();
            let mut line = String::new();
            BufReader::new(&mut stream)
                .read_line(&mut line)
                .await
                .unwrap();
            assert_eq!(line, "overlay.open\n");
            stream
                .write_all(b"{\"type\":\"session\"}\n{\"type\":\"overlay_status\",\"open\":true}\n")
                .await
                .unwrap();
        });
        let daemon = connect(&path).await.unwrap().unwrap();
        assert_eq!(open(daemon).await.unwrap(), "eco: window opened");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_window_already_open_is_said_to_be_brought_forward() {
        let directory = tempfile::tempdir().unwrap();
        let path = socket_in(&directory);
        let listener = FakeListener::bind(&path).await;
        let server = tokio::spawn(async move {
            let mut stream = listener.accept().await.unwrap();
            stream
                .write_all(greeting(1, VERSION, true).as_bytes())
                .await
                .unwrap();
            let mut line = String::new();
            BufReader::new(&mut stream)
                .read_line(&mut line)
                .await
                .unwrap();
            stream
                .write_all(b"{\"type\":\"overlay_status\",\"open\":true,\"raised\":true}\n")
                .await
                .unwrap();
        });
        let daemon = connect(&path).await.unwrap().unwrap();
        assert_eq!(
            open(daemon).await.unwrap(),
            "eco: window already open, brought forward"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_window_that_fails_to_open_is_reported() {
        assert_eq!(
            open_error(Some(
                b"{\"type\":\"overlay_status\",\"open\":false,\"message\":\"no display\"}\n"
            ))
            .await,
            "eco window failed: no display"
        );
        assert_eq!(
            open_error(Some(b"{\"type\":\"overlay_status\",\"open\":false}\n")).await,
            "eco window failed: unknown error"
        );
    }

    #[tokio::test]
    async fn a_daemon_that_breaks_while_opening_the_window_is_reported() {
        assert!(
            open_error(Some(b"garbage\n"))
                .await
                .starts_with("invalid eco socket response")
        );
        assert_eq!(
            open_error(Some(b"")).await,
            "the daemon closed while opening the eco window"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_window_that_never_answers_times_out() {
        assert!(
            open_error(None)
                .await
                .starts_with("timed out waiting for the eco window")
        );
    }

    #[tokio::test]
    async fn start_starts_the_service_when_nothing_answers() {
        let directory = tempfile::tempdir().unwrap();
        let service = fake_service(&directory);
        start(&service, &service.socket, false).await.unwrap();
        assert_eq!(service.starts.into_inner(), 1);
    }

    #[tokio::test]
    async fn start_opens_the_window_of_the_daemon_it_started() {
        let directory = tempfile::tempdir().unwrap();
        let service = fake_service(&directory);
        start(&service, &service.socket, true).await.unwrap();
        assert_eq!(service.starts.into_inner(), 1);
    }

    #[tokio::test]
    async fn start_leaves_the_service_alone_when_the_daemon_answers() {
        let directory = tempfile::tempdir().unwrap();
        let service = fake_service(&directory);
        fake_daemon(
            &service.socket,
            std::process::id(),
            VERSION,
            false,
            OnStop::Exit,
        )
        .await;
        start(&service, &service.socket, true).await.unwrap();
        assert_eq!(service.starts.into_inner(), 0);
    }

    #[tokio::test]
    async fn start_restarts_a_daemon_of_another_version_and_opens_its_window() {
        let directory = tempfile::tempdir().unwrap();
        let service = fake_service(&directory);
        fake_daemon(
            &service.socket,
            std::process::id(),
            "0.0.1",
            false,
            OnStop::Exit,
        )
        .await;
        start(&service, &service.socket, true).await.unwrap();
        assert_eq!(service.starts.into_inner(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_daemon_of_another_version_that_ignores_stop_is_reported() {
        let directory = tempfile::tempdir().unwrap();
        let service = fake_service(&directory);
        fake_daemon(
            &service.socket,
            std::process::id(),
            "0.0.1",
            false,
            OnStop::Ignore,
        )
        .await;
        let error = start(&service, &service.socket, true).await.unwrap_err();
        assert_eq!(
            error.to_string(),
            "the daemon did not stop within 10 seconds"
        );
        assert_eq!(service.starts.into_inner(), 0);
    }

    /// `eco window toggle` against a daemon of `version` on `service`'s
    /// socket: its exit code, and how often the service started one.
    async fn window_toggle(version: &'static str) -> (i32, usize) {
        let directory = tempfile::tempdir().unwrap();
        let service = fake_service(&directory);
        fake_daemon(
            &service.socket,
            std::process::id(),
            version,
            true,
            OnStop::Exit,
        )
        .await;
        let request = cli::Request::Window(cli::Window::Toggle);
        let code = ask(&service, &service.socket, request).await.unwrap();
        (code, service.starts.into_inner())
    }

    #[tokio::test]
    async fn a_shortcut_restarts_a_daemon_of_another_version_first() {
        assert_eq!(window_toggle("0.0.1").await, (0, 1));
    }

    #[tokio::test]
    async fn a_shortcut_leaves_a_current_daemon_alone() {
        assert_eq!(window_toggle(VERSION).await, (0, 0));
    }

    #[tokio::test]
    async fn a_shortcut_with_no_daemon_starts_none() {
        let directory = tempfile::tempdir().unwrap();
        let service = fake_service(&directory);
        let request = cli::Request::Window(cli::Window::Toggle);
        assert_eq!(ask(&service, &service.socket, request).await.unwrap(), 1);
        assert_eq!(service.starts.into_inner(), 0);
    }

    #[tokio::test]
    async fn only_a_shortcut_restarts_a_daemon_of_another_version() {
        let directory = tempfile::tempdir().unwrap();
        let service = fake_service(&directory);
        fake_daemon(
            &service.socket,
            std::process::id(),
            "0.0.1",
            false,
            OnStop::Exit,
        )
        .await;
        // Refused before it is sent, so the daemon need not answer it.
        let request = cli::Request::People(cli::People::Forget { id: "a\nb".into() });
        assert_eq!(ask(&service, &service.socket, request).await.unwrap(), 1);
        assert_eq!(service.starts.load(Ordering::SeqCst), 0);
        assert_eq!(status(&service.socket, false, false).await.unwrap(), 0);
        assert_eq!(service.starts.into_inner(), 0);
    }

    #[tokio::test]
    async fn a_service_that_fails_to_start_is_reported() {
        struct Broken;
        impl ServiceManager for Broken {
            fn start(&self) -> futures::future::BoxFuture<'_, Result<()>> {
                Box::pin(async { bail!("cannot start eco.service: no user manager") })
            }

            fn output(&self, _: SystemTime) -> futures::future::BoxFuture<'_, Option<String>> {
                Box::pin(async { None })
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let error = start(&Broken, &socket_in(&directory), false)
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "cannot start eco.service: no user manager"
        );
    }

    const REFUSAL: &str = "eco runs without administrator rights, so every window can reach it; run this from a terminal that is not elevated";

    #[tokio::test]
    async fn an_elevated_start_starts_no_daemon() {
        let directory = tempfile::tempdir().unwrap();
        let service = elevated_service(&directory);
        let error = start(&service, &service.socket, false).await.unwrap_err();
        assert_eq!(error.to_string(), REFUSAL);
        assert_eq!(service.starts.into_inner(), 0);
    }

    #[tokio::test]
    async fn an_elevated_restart_stops_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let service = elevated_service(&directory);
        fake_daemon(&service.socket, 1, VERSION, true, OnStop::Exit).await;
        let error = restart(&service, &service.socket).await.unwrap_err();
        assert_eq!(error.to_string(), REFUSAL);
        assert_eq!(status(&service.socket, false, false).await.unwrap(), 0);
        assert_eq!(service.starts.into_inner(), 0);
    }

    #[tokio::test]
    async fn an_elevated_start_keeps_an_old_daemon_running() {
        let directory = tempfile::tempdir().unwrap();
        let service = elevated_service(&directory);
        fake_daemon(&service.socket, 1, "0.0.1", true, OnStop::Exit).await;
        let error = start(&service, &service.socket, false).await.unwrap_err();
        assert_eq!(error.to_string(), REFUSAL);
        assert_eq!(status(&service.socket, false, false).await.unwrap(), 0);
        assert_eq!(service.starts.into_inner(), 0);
    }

    /// A service whose daemon never listens and wrote `output`.
    struct Silent {
        output: Option<&'static str>,
        asked_since: std::sync::Mutex<Option<SystemTime>>,
    }

    impl ServiceManager for Silent {
        fn start(&self) -> futures::future::BoxFuture<'_, Result<()>> {
            Box::pin(async { Ok(()) })
        }

        fn output(&self, since: SystemTime) -> futures::future::BoxFuture<'_, Option<String>> {
            *self.asked_since.lock().unwrap() = Some(since);
            Box::pin(async { self.output.map(String::from) })
        }
    }

    /// The error of starting a daemon that never listens and wrote `output`.
    async fn silent_error(output: Option<&'static str>) -> String {
        let directory = tempfile::tempdir().unwrap();
        let service = Silent {
            output,
            asked_since: Default::default(),
        };
        let before = SystemTime::now();
        let error = start(&service, &socket_in(&directory), false)
            .await
            .unwrap_err();
        let since = service.asked_since.into_inner().unwrap().unwrap();
        assert!(since >= before && since <= SystemTime::now());
        error.to_string()
    }

    #[tokio::test(start_paused = true)]
    async fn a_service_whose_daemon_never_listens_times_out() {
        assert_eq!(
            silent_error(None).await,
            "the daemon was started, but did not open the socket"
        );
        assert_eq!(
            silent_error(Some("\n  \n")).await,
            "the daemon was started, but did not open the socket"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_daemon_that_never_listens_is_reported_with_what_it_said() {
        assert_eq!(
            silent_error(Some(
                "eco: missing /m/silero_vad.onnx; run `eco setup`\r\n\
                 eco: missing /m/silero_vad.onnx; run `eco setup`\r\n"
            ))
            .await,
            "the daemon was started, but did not open the socket; it said:\n\
             \x20 eco: missing /m/silero_vad.onnx; run `eco setup`"
        );
        assert_eq!(
            silent_error(Some(
                "eco: warning\nError: cannot bind\n\nCaused by:\n    in use\n"
            ))
            .await,
            "the daemon was started, but did not open the socket; it said:\n\
             \x20 Error: cannot bind\n  Caused by:\n      in use"
        );
        assert_eq!(
            silent_error(Some(
                "Error: no device\n[Them] a transcript\n    an answer\n"
            ))
            .await,
            "the daemon was started, but did not open the socket; it said:\n\
             \x20 Error: no device"
        );
        assert_eq!(
            silent_error(Some("[Them] a transcript\n**Answer:** yes\n")).await,
            "the daemon was started, but did not open the socket"
        );
    }

    #[tokio::test]
    async fn a_socket_that_is_not_a_daemon_is_reported() {
        assert!(
            connect_error(b"{\"type\":\"session\"}\n")
                .await
                .contains("incompatible protocol")
        );
        assert_eq!(
            connect_error(b"").await,
            "the eco socket closed before identifying its daemon"
        );
        assert!(
            connect_error(b"garbage\n")
                .await
                .starts_with("invalid eco socket response")
        );
        assert_eq!(
            connect_error(b"{\"type\":\"daemon\",\"version\":\"test\"}\n").await,
            "daemon response has no pid"
        );
        assert!(
            connect_error(b"{\"type\":\"daemon\",\"pid\":4294967296,\"version\":\"test\"}\n")
                .await
                .starts_with("daemon response has an invalid pid")
        );
        assert_eq!(
            connect_error(b"{\"type\":\"daemon\",\"pid\":1}\n").await,
            "daemon response has no version"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_socket_that_never_greets_times_out() {
        let directory = tempfile::tempdir().unwrap();
        let path = socket_in(&directory);
        let _listener = FakeListener::bind(&path).await;
        assert_eq!(
            connect(&path).await.err().unwrap().to_string(),
            "the eco socket did not answer"
        );
    }

    #[tokio::test]
    async fn stop_stops_a_running_daemon() {
        let directory = tempfile::tempdir().unwrap();
        let path = socket_in(&directory);
        fake_daemon(&path, 1, VERSION, false, OnStop::Exit).await;
        stop(&path).await.unwrap();
        assert!(socket(&path).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn stop_of_a_stopped_daemon_succeeds() {
        let directory = tempfile::tempdir().unwrap();
        stop(&socket_in(&directory)).await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn a_daemon_that_ignores_stop_is_reported() {
        let directory = tempfile::tempdir().unwrap();
        let path = socket_in(&directory);
        fake_daemon(&path, 1, VERSION, false, OnStop::Ignore).await;
        assert_eq!(
            stop(&path).await.unwrap_err().to_string(),
            "the daemon did not stop within 10 seconds"
        );
    }

    #[tokio::test]
    async fn restart_keeps_a_closed_window_closed() {
        let directory = tempfile::tempdir().unwrap();
        let service = fake_service(&directory);
        fake_daemon(&service.socket, 2, VERSION, false, OnStop::Exit).await;
        restart(&service, &service.socket).await.unwrap();
        assert_eq!(service.starts.load(Ordering::SeqCst), 1);
        assert_eq!(status(&service.socket, false, true).await.unwrap(), 3);
    }

    #[tokio::test]
    async fn restart_of_a_stopped_daemon_starts_it_with_its_window() {
        let directory = tempfile::tempdir().unwrap();
        let service = fake_service(&directory);
        restart(&service, &service.socket).await.unwrap();
        assert_eq!(service.starts.into_inner(), 1);
    }

    #[tokio::test]
    async fn status_of_a_running_daemon_exits_0() {
        let directory = tempfile::tempdir().unwrap();
        let path = socket_in(&directory);
        fake_daemon(&path, 1, VERSION, true, OnStop::Exit).await;
        assert_eq!(status(&path, false, false).await.unwrap(), 0);
        assert_eq!(status(&path, false, true).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn status_of_a_stopped_daemon_exits_3() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(
            status(&socket_in(&directory), false, false).await.unwrap(),
            3
        );
    }

    #[tokio::test]
    async fn status_exits_3_when_the_window_must_be_open_and_is_not() {
        let directory = tempfile::tempdir().unwrap();
        let path = socket_in(&directory);
        fake_daemon(&path, 1, VERSION, false, OnStop::Exit).await;
        assert_eq!(status(&path, false, true).await.unwrap(), 3);
        assert_eq!(status(&path, false, false).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn status_accepts_a_daemon_running_this_executable() {
        let directory = tempfile::tempdir().unwrap();
        let path = socket_in(&directory);
        fake_daemon(&path, std::process::id(), VERSION, false, OnStop::Exit).await;
        assert_eq!(status(&path, true, false).await.unwrap(), 0);
    }

    /// A process running another executable than this test's, as the pid a
    /// fake daemon greets with, ended when it is dropped.
    struct Elsewhere {
        pid: u32,
        #[cfg(unix)]
        child: std::process::Child,
    }

    impl Drop for Elsewhere {
        fn drop(&mut self) {
            #[cfg(unix)]
            {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
    }

    /// On Windows a fake daemon greets with an executable path of its own
    /// for any pid but this one; on Linux the executable is the pid's.
    async fn elsewhere() -> Elsewhere {
        #[cfg(unix)]
        {
            let child = std::process::Command::new("sleep")
                .arg("30")
                .spawn()
                .unwrap();
            // Until it execs, the child still runs this test's executable.
            let exe = format!("/proc/{}/exe", child.id());
            while std::fs::read_link(&exe).ok() == std::env::current_exe().ok() {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            Elsewhere {
                pid: child.id(),
                child,
            }
        }
        #[cfg(windows)]
        Elsewhere { pid: 1 }
    }

    #[tokio::test]
    async fn status_rejects_a_daemon_running_another_executable() {
        let directory = tempfile::tempdir().unwrap();
        let path = socket_in(&directory);
        let other = elsewhere().await;
        fake_daemon(&path, other.pid, VERSION, false, OnStop::Exit).await;
        let error = status(&path, true, false).await.unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "daemon pid {} is running a different eco executable",
                other.pid
            )
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn start_keeps_a_daemon_whose_executable_it_cannot_see() {
        let directory = tempfile::tempdir().unwrap();
        let service = fake_service(&directory);
        // Above the largest pid Linux hands out (`pid_max` is at most 2^22).
        fake_daemon(&service.socket, 1 << 23, VERSION, false, OnStop::Exit).await;
        start(&service, &service.socket, false).await.unwrap();
        assert_eq!(service.starts.into_inner(), 0);
    }

    #[tokio::test]
    async fn start_restarts_a_daemon_running_another_executable() {
        let directory = tempfile::tempdir().unwrap();
        let service = fake_service(&directory);
        let other = elsewhere().await;
        fake_daemon(&service.socket, other.pid, VERSION, false, OnStop::Exit).await;
        start(&service, &service.socket, false).await.unwrap();
        assert_eq!(service.starts.into_inner(), 1);
    }

    #[tokio::test]
    async fn status_of_a_socket_that_is_not_a_daemon() {
        let directory = tempfile::tempdir().unwrap();
        let path = socket_in(&directory);
        replying(&path, b"garbage\n").await;
        assert_eq!(status(&path, false, false).await.unwrap(), 0);
        assert!(
            status(&path, true, false)
                .await
                .unwrap_err()
                .to_string()
                .starts_with("invalid eco socket response")
        );
    }
}
