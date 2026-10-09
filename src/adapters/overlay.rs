//! The Quickshell overlay windows, running for as long as the session holds them.

use std::io;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use rustix::process::{Pid, Signal, kill_process};
use tokio::process::{Child, Command};

/// The QML: installed beside the binary (`<prefix>/share/eco/overlay` for
/// `<prefix>/bin/eco`), or this checkout's when eco runs from it.
fn overlay_dir() -> PathBuf {
    let installed = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.join("share/eco/overlay")))
        .filter(|dir| dir.join("shell.qml").is_file());
    installed.unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/overlay").into())
}

/// The overlay windows, each its own quickshell process: a number, which the
/// window is told, and the session it shows, which it tells back.
#[derive(Default)]
pub struct Windows {
    last: u32,
    open: Vec<Window>,
}

struct Window {
    number: u32,
    child: Child,
    /// The live session the window shows, or empty.
    shows: String,
}

impl Windows {
    pub fn is_open(&self) -> bool {
        !self.open.is_empty()
    }

    /// The live sessions some window shows.
    pub fn shown(&self) -> impl Iterator<Item = &str> {
        self.open.iter().map(|window| window.shows.as_str())
    }

    /// Open another window, focused, that shows `show` when it is a live session.
    pub async fn open(&mut self, show: Option<&str>) -> io::Result<()> {
        let number = self.last + 1;
        let mut child = Command::new("quickshell")
            .arg("--path")
            .arg(overlay_dir())
            .env("ECO_WINDOW", number.to_string())
            .env("ECO_SHOW", show.unwrap_or_default())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;
        tokio::time::sleep(Duration::from_millis(200)).await;
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!("quickshell exited with {status}")));
        }
        activate(&child).await?;
        self.last = number;
        self.open.push(Window {
            number,
            child,
            shows: show.unwrap_or_default().to_string(),
        });
        Ok(())
    }

    /// Note the session the window `number` shows now, or none when empty.
    pub fn shows(&mut self, number: u32, session: &str) {
        if let Some(window) = self.open.iter_mut().find(|w| w.number == number) {
            window.shows = session.to_string();
        }
    }

    /// Resolves when a window exits, which leaves the set; never, without one.
    /// `None` when its status could not be read.
    pub async fn exited(&mut self) -> Option<ExitStatus> {
        if self.open.is_empty() {
            return std::future::pending().await;
        }
        let waits = self.open.iter_mut().map(|w| Box::pin(w.child.wait()));
        let (status, at, _) = futures::future::select_all(waits).await;
        let status = status.ok();
        self.open.remove(at);
        status
    }

    /// Terminate every window and wait for them to exit.
    pub async fn close(self) {
        for mut window in self.open {
            let running = window.child.id().and_then(|id| Pid::from_raw(id as i32));
            if let Some(pid) = running {
                let _ = kill_process(pid, Signal::TERM);
            }
            let _ = window.child.wait().await;
        }
    }
}

/// Give the keyboard to the window of the quickshell process `child`.
async fn activate(child: &Child) -> io::Result<()> {
    let pid = child
        .id()
        .ok_or_else(|| io::Error::other("the eco window has exited"))?;
    let focus = format!("hl.dsp.focus({{ window = \"pid:{pid}\" }})");
    let mut last = String::new();
    for _ in 0..50 {
        let output = Command::new("hyprctl")
            .args(["dispatch", &focus])
            .output()
            .await?;
        if output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "ok" {
            return Ok(());
        }
        last = format!(
            "{}: {}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(io::Error::other(format!(
        "Hyprland could not focus the eco window ({last})"
    )))
}
