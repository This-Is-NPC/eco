//! Start, stop and check the daemon over its socket; the caller names the
//! socket and the service manager that starts the daemon.

use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::{sleep, timeout};

use crate::adapters::local_socket::{self, Stream};
use crate::ports::ServiceManager;

const READY_TIMEOUT: Duration = Duration::from_secs(10);

struct Daemon {
    stream: BufReader<Stream>,
    overlay: bool,
    pid: u32,
    version: String,
}

async fn socket(path: &Path) -> Result<Option<Stream>> {
    Ok(local_socket::connect(path).await?)
}

async fn connect(path: &Path) -> Result<Option<Daemon>> {
    let Some(stream) = socket(path).await? else {
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
    }))
}

async fn ready(socket: &Path) -> Result<Daemon> {
    let until = tokio::time::Instant::now() + READY_TIMEOUT;
    loop {
        if let Some(daemon) = connect(socket).await? {
            return Ok(daemon);
        }
        if tokio::time::Instant::now() >= until {
            bail!("eco.service started, but its daemon did not open the socket");
        }
        sleep(Duration::from_millis(100)).await;
    }
}

async fn open(mut daemon: Daemon) -> Result<()> {
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
                println!("eco: window opened");
                return Ok(());
            }
            bail!(
                "eco window failed: {}",
                event["message"].as_str().unwrap_or("unknown error")
            );
        }
    }
}

/// Reach the daemon on `socket`, started through `service` when nothing answers.
pub async fn start(service: &dyn ServiceManager, socket: &Path, show_window: bool) -> Result<()> {
    let daemon = match connect(socket).await? {
        Some(daemon) => daemon,
        None => {
            service.start().await?;
            ready(socket).await?
        }
    };
    if show_window {
        open(daemon).await
    } else {
        println!("eco: daemon running");
        Ok(())
    }
}

pub async fn stop(socket_path: &Path) -> Result<()> {
    let Some(mut stream) = socket(socket_path).await? else {
        println!("eco: daemon already stopped");
        return Ok(());
    };
    stream.write_all(b"stop\n").await?;
    let until = tokio::time::Instant::now() + READY_TIMEOUT;
    loop {
        if socket(socket_path).await?.is_none() {
            println!("eco: daemon stopped");
            return Ok(());
        }
        if tokio::time::Instant::now() >= until {
            bail!("the daemon did not stop within 10 seconds");
        }
        sleep(Duration::from_millis(100)).await;
    }
}

/// Stop the daemon on `socket` and start it again through `service`, with its
/// window open unless the running daemon had it closed.
pub async fn restart(service: &dyn ServiceManager, socket_path: &Path) -> Result<()> {
    let show_window = if socket(socket_path).await?.is_some() {
        connect(socket_path)
            .await
            .ok()
            .flatten()
            .is_none_or(|daemon| daemon.overlay)
    } else {
        true
    };
    stop(socket_path).await?;
    start(service, socket_path, show_window).await
}

/// Report the daemon on `socket` and return the exit status: 0 when it runs,
/// 3 when it is stopped or `window_open` asks for a window it has closed.
pub async fn status(
    socket_path: &Path,
    expect_current_exe: bool,
    window_open: bool,
) -> Result<i32> {
    let Some(_) = socket(socket_path).await? else {
        println!("eco: daemon stopped");
        return Ok(3);
    };
    let daemon = match connect(socket_path).await {
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
    if expect_current_exe {
        let running = std::fs::metadata(format!("/proc/{}/exe", daemon.pid))?;
        let current = std::fs::metadata(std::env::current_exe()?)?;
        if (running.dev(), running.ino()) != (current.dev(), current.ino()) {
            bail!(
                "daemon pid {} is running a different eco executable",
                daemon.pid
            );
        }
    }
    println!(
        "eco: daemon running (pid {}, version {})",
        daemon.pid, daemon.version
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn socket_in(directory: &tempfile::TempDir) -> PathBuf {
        directory.path().join("eco.sock")
    }

    fn greeting(pid: u32, overlay: bool) -> String {
        format!(
            "{{\"type\":\"daemon\",\"pid\":{pid},\"version\":\"test\",\"overlay\":{overlay}}}\n"
        )
    }

    /// Whether the fake daemon goes away on `stop`.
    #[derive(Clone, Copy, PartialEq)]
    enum OnStop {
        Exit,
        Ignore,
    }

    /// A daemon on `socket` that greets every client as `pid`, opens its
    /// window on `overlay.open` and goes away on `stop` unless it ignores it.
    async fn fake_daemon(socket: &Path, pid: u32, overlay: bool, on_stop: OnStop) {
        let listener = local_socket::Listener::bind(socket).await.unwrap();
        tokio::spawn(async move {
            while let Ok(stream) = listener.accept().await {
                let mut stream = BufReader::new(stream);
                let _ = stream
                    .get_mut()
                    .write_all(greeting(pid, overlay).as_bytes())
                    .await;
                let mut line = String::new();
                while matches!(stream.read_line(&mut line).await, Ok(1..)) {
                    if line == "stop\n" && on_stop == OnStop::Exit {
                        return;
                    }
                    if line == "overlay.open\n" {
                        let _ = stream
                            .get_mut()
                            .write_all(b"{\"type\":\"overlay_status\",\"open\":true}\n")
                            .await;
                    }
                    line.clear();
                }
            }
        });
    }

    /// A socket on `path` that writes `reply` to every client and hangs up.
    async fn replying(path: &Path, reply: &'static [u8]) {
        let listener = local_socket::Listener::bind(path).await.unwrap();
        tokio::spawn(async move {
            while let Ok(mut stream) = listener.accept().await {
                let _ = stream.write_all(reply).await;
            }
        });
    }

    /// A service manager whose daemon comes up on `socket` with its window closed.
    struct FakeService {
        socket: PathBuf,
        starts: AtomicUsize,
    }

    impl ServiceManager for FakeService {
        fn start(&self) -> futures::future::BoxFuture<'_, Result<()>> {
            Box::pin(async {
                self.starts.fetch_add(1, Ordering::SeqCst);
                fake_daemon(&self.socket, 1, false, OnStop::Exit).await;
                Ok(())
            })
        }
    }

    fn fake_service(directory: &tempfile::TempDir) -> FakeService {
        FakeService {
            socket: socket_in(directory),
            starts: 0.into(),
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
        let listener = local_socket::Listener::bind(&path).await.unwrap();
        tokio::spawn(async move {
            let stream = listener.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            let hello = greeting(1, false);
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
        let listener = local_socket::Listener::bind(&path).await.unwrap();
        let server = tokio::spawn(async move {
            let mut stream = listener.accept().await.unwrap();
            stream
                .write_all(greeting(1, false).as_bytes())
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
        open(daemon).await.unwrap();
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
        fake_daemon(&service.socket, 1, false, OnStop::Exit).await;
        start(&service, &service.socket, false).await.unwrap();
        assert_eq!(service.starts.into_inner(), 0);
    }

    #[tokio::test]
    async fn a_service_that_fails_to_start_is_reported() {
        struct Broken;
        impl ServiceManager for Broken {
            fn start(&self) -> futures::future::BoxFuture<'_, Result<()>> {
                Box::pin(async { bail!("cannot start eco.service: no user manager") })
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

    #[tokio::test(start_paused = true)]
    async fn a_service_whose_daemon_never_listens_times_out() {
        struct Silent;
        impl ServiceManager for Silent {
            fn start(&self) -> futures::future::BoxFuture<'_, Result<()>> {
                Box::pin(async { Ok(()) })
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let error = start(&Silent, &socket_in(&directory), false)
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "eco.service started, but its daemon did not open the socket"
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
        let _listener = local_socket::Listener::bind(&path).await.unwrap();
        assert_eq!(
            connect(&path).await.err().unwrap().to_string(),
            "the eco socket did not answer"
        );
    }

    #[tokio::test]
    async fn stop_stops_a_running_daemon() {
        let directory = tempfile::tempdir().unwrap();
        let path = socket_in(&directory);
        fake_daemon(&path, 1, false, OnStop::Exit).await;
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
        fake_daemon(&path, 1, false, OnStop::Ignore).await;
        assert_eq!(
            stop(&path).await.unwrap_err().to_string(),
            "the daemon did not stop within 10 seconds"
        );
    }

    #[tokio::test]
    async fn restart_keeps_a_closed_window_closed() {
        let directory = tempfile::tempdir().unwrap();
        let service = fake_service(&directory);
        fake_daemon(&service.socket, 2, false, OnStop::Exit).await;
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
        fake_daemon(&path, 1, true, OnStop::Exit).await;
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
        fake_daemon(&path, 1, false, OnStop::Exit).await;
        assert_eq!(status(&path, false, true).await.unwrap(), 3);
        assert_eq!(status(&path, false, false).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn status_accepts_a_daemon_running_this_executable() {
        let directory = tempfile::tempdir().unwrap();
        let path = socket_in(&directory);
        fake_daemon(&path, std::process::id(), false, OnStop::Exit).await;
        assert_eq!(status(&path, true, false).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn status_rejects_a_daemon_running_another_executable() {
        let directory = tempfile::tempdir().unwrap();
        let path = socket_in(&directory);
        let mut other = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        // Until it execs, the child still runs this test's executable.
        let exe = format!("/proc/{}/exe", other.id());
        while std::fs::read_link(&exe).ok() == std::env::current_exe().ok() {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        fake_daemon(&path, other.id(), false, OnStop::Exit).await;
        let error = status(&path, true, false).await.unwrap_err();
        other.kill().unwrap();
        other.wait().unwrap();
        assert_eq!(
            error.to_string(),
            format!(
                "daemon pid {} is running a different eco executable",
                other.id()
            )
        );
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
