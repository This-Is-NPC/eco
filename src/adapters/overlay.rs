//! The overlay windows, each an `eco-window` process running the QML in
//! `overlay/`, for as long as the session holds them (docs/design.md §3).

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use rustix::process::{Pid, Signal, kill_process};
use tokio::process::{Child, Command};

use crate::paths;

/// The window's QML: installed beside the binary
/// (`<prefix>/share/eco/overlay/shell.qml` for `<prefix>/bin/eco`), or this
/// checkout's when eco runs from it.
fn shell() -> PathBuf {
    paths::shipped("overlay/shell.qml")
        .unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/overlay/shell.qml").into())
}

/// The program that runs it: installed beside the binary
/// (`<prefix>/lib/eco/eco-window`), or this checkout's build
/// (`mise run window:build`) when eco runs from it.
fn program() -> PathBuf {
    paths::shipped_program("eco-window")
        .unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/target/window/eco-window").into())
}

/// The overlay windows, each its own eco-window process: a number, which the
/// window is told, and the session it shows, which it tells back. Every window
/// is handed the same token, which tells its commands apart from other clients';
/// a window the daemon did not start reads it from `paths::token_path()`.
pub struct Windows {
    token: String,
    _kept: TokenFile,
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
    pub fn new(token: String) -> io::Result<Self> {
        Ok(Self {
            _kept: TokenFile::write(&paths::token_path(), &token)?,
            token,
            last: 0,
            open: Vec::new(),
        })
    }

    pub fn is_open(&self) -> bool {
        !self.open.is_empty()
    }

    /// The live sessions some window shows.
    pub fn shown(&self) -> impl Iterator<Item = &str> {
        self.open.iter().map(|window| window.shows.as_str())
    }

    /// The window opened last of those still open.
    pub fn newest(&self) -> Option<u32> {
        self.open.last().map(|window| window.number)
    }

    /// Open another window, focused, that shows `show` when it is a live
    /// session and makes `call` (a `window_call`'s JSON) once it meets the daemon.
    pub async fn open(&mut self, show: Option<&str>, call: Option<&str>) -> io::Result<()> {
        let number = self.last + 1;
        let program = program();
        let mut child = Command::new(&program)
            .arg(shell())
            .env("ECO_WINDOW", number.to_string())
            .env("ECO_SHOW", show.unwrap_or_default())
            .env("ECO_CALL", call.unwrap_or_default())
            .env("ECO_TOKEN", &self.token)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| io::Error::other(format!("{}: {error}", program.display())))?;
        tokio::time::sleep(Duration::from_millis(200)).await;
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!("eco-window exited with {status}")));
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

/// The token in a file only its user reads; dropping it removes the file.
struct TokenFile(PathBuf);

impl TokenFile {
    fn write(path: &Path, token: &str) -> io::Result<Self> {
        // A file left by a daemon that was killed is replaced, never reused.
        match fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
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

/// Give the keyboard to the window of the eco-window process `child`.
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

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn the_token_file_is_the_users_alone_and_goes_with_the_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("eco.token");
        fs::write(&path, "stale").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let file = TokenFile::write(&path, "secret").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "secret");
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        drop(file);
        assert!(!path.exists());
    }
}
