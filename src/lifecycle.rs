//! Control the user service and verify the daemon over its socket.

use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::{sleep, timeout};

use crate::adapters::local_socket::{self, Stream};
use crate::paths;

const READY_TIMEOUT: Duration = Duration::from_secs(10);

struct Daemon {
    stream: BufReader<Stream>,
    overlay: bool,
    pid: u32,
    version: String,
}

async fn socket() -> Result<Option<Stream>> {
    socket_at(&paths::socket_path()).await
}

async fn socket_at(path: &Path) -> Result<Option<Stream>> {
    Ok(local_socket::connect(path).await?)
}

async fn connect() -> Result<Option<Daemon>> {
    connect_at(&paths::socket_path()).await
}

async fn connect_at(path: &Path) -> Result<Option<Daemon>> {
    let Some(stream) = socket_at(path).await? else {
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

async fn service(action: &str) -> Result<()> {
    let variables: Vec<_> = [
        "WAYLAND_DISPLAY",
        "HYPRLAND_INSTANCE_SIGNATURE",
        "XDG_CURRENT_DESKTOP",
    ]
    .into_iter()
    .filter_map(|name| std::env::var_os(name).map(|value| (name, value)))
    .collect();
    if !variables.is_empty() {
        let mut import = Command::new("systemctl");
        import.args(["--user", "set-environment"]);
        for (name, value) in variables {
            let mut assignment = std::ffi::OsString::from(name);
            assignment.push("=");
            assignment.push(value);
            import.arg(assignment);
        }
        let output = import
            .output()
            .await
            .context("cannot update the user service environment")?;
        if !output.status.success() {
            bail!(
                "cannot update the user service environment: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
    }
    let output = Command::new("systemctl")
        .args(["--user", action, "eco.service"])
        .output()
        .await
        .context("cannot run systemctl --user")?;
    if !output.status.success() {
        let reason = String::from_utf8_lossy(&output.stderr);
        bail!("cannot {action} eco.service: {}", reason.trim());
    }
    Ok(())
}

async fn ready() -> Result<Daemon> {
    let until = tokio::time::Instant::now() + READY_TIMEOUT;
    loop {
        if let Some(daemon) = connect().await? {
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

pub async fn start(show_window: bool) -> Result<()> {
    let daemon = match connect().await? {
        Some(daemon) => daemon,
        None => {
            service("start").await?;
            ready().await?
        }
    };
    if show_window {
        open(daemon).await
    } else {
        println!("eco: daemon running");
        Ok(())
    }
}

pub async fn stop() -> Result<()> {
    let Some(mut stream) = socket().await? else {
        println!("eco: daemon already stopped");
        return Ok(());
    };
    stream.write_all(b"stop\n").await?;
    let until = tokio::time::Instant::now() + READY_TIMEOUT;
    loop {
        if socket().await?.is_none() {
            println!("eco: daemon stopped");
            return Ok(());
        }
        if tokio::time::Instant::now() >= until {
            bail!("the daemon did not stop within 10 seconds");
        }
        sleep(Duration::from_millis(100)).await;
    }
}

pub async fn restart() -> Result<()> {
    let show_window = if socket().await?.is_some() {
        connect()
            .await
            .ok()
            .flatten()
            .is_none_or(|daemon| daemon.overlay)
    } else {
        true
    };
    stop().await?;
    start(show_window).await
}

pub async fn status(expect_current_exe: bool, window_open: bool) -> Result<()> {
    let Some(_) = socket().await? else {
        println!("eco: daemon stopped");
        std::process::exit(3);
    };
    let daemon = match connect().await {
        Ok(Some(daemon)) => daemon,
        Err(error) if !expect_current_exe => {
            println!("eco: daemon running ({error})");
            return Ok(());
        }
        result => result?.context("the daemon stopped")?,
    };
    if window_open && !daemon.overlay {
        std::process::exit(3);
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn window_open_waits_for_a_daemon_acknowledgement() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("eco.sock");
        let listener = local_socket::Listener::bind(&path).await.unwrap();
        let server = tokio::spawn(async move {
            let mut stream = listener.accept().await.unwrap();
            stream
                .write_all(
                    b"{\"type\":\"daemon\",\"pid\":1,\"version\":\"test\",\"overlay\":false}\n",
                )
                .await
                .unwrap();
            let mut line = String::new();
            BufReader::new(&mut stream)
                .read_line(&mut line)
                .await
                .unwrap();
            assert_eq!(line, "overlay.open\n");
            stream
                .write_all(b"{\"type\":\"overlay_status\",\"open\":true}\n")
                .await
                .unwrap();
        });
        let daemon = connect_at(&path).await.unwrap().unwrap();
        open(daemon).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn incompatible_socket_is_reported() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("eco.sock");
        let listener = local_socket::Listener::bind(&path).await.unwrap();
        let server = tokio::spawn(async move {
            let mut stream = listener.accept().await.unwrap();
            stream.write_all(b"{\"type\":\"session\"}\n").await.unwrap();
        });
        assert!(
            connect_at(&path)
                .await
                .err()
                .unwrap()
                .to_string()
                .contains("incompatible protocol")
        );
        server.await.unwrap();
    }
}
