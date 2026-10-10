//! The overlay windows, each an `eco-window` process running the QML in
//! `overlay/`, for as long as the session holds them (docs/design.md §3).

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration;

use rustix::process::{Pid, Signal, kill_process};
use tokio::process::{Child, Command};

use crate::paths;
use crate::ports::WindowControl;

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
/// `control` focuses each window as it opens and, while `hidden`, leaves them
/// all out of screen sharing.
pub struct Windows {
    token: String,
    _kept: TokenFile,
    program: PathBuf,
    control: Arc<dyn WindowControl>,
    hidden: bool,
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
    pub fn new(token: String, control: Arc<dyn WindowControl>, hidden: bool) -> io::Result<Self> {
        Self::run(program(), &paths::token_path(), token, control, hidden)
    }

    /// Windows run by `program`, with the token kept at `token_path`.
    fn run(
        program: PathBuf,
        token_path: &Path,
        token: String,
        control: Arc<dyn WindowControl>,
        hidden: bool,
    ) -> io::Result<Self> {
        Ok(Self {
            _kept: TokenFile::write(token_path, &token)?,
            token,
            program,
            control,
            hidden,
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
        let mut child = Command::new(&self.program)
            .arg(shell())
            .env("ECO_WINDOW", number.to_string())
            .env("ECO_SHOW", show.unwrap_or_default())
            .env("ECO_CALL", call.unwrap_or_default())
            .env("ECO_TOKEN", &self.token)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| io::Error::other(format!("{}: {error}", self.program.display())))?;
        tokio::time::sleep(Duration::from_millis(200)).await;
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!("eco-window exited with {status}")));
        }
        let pid = child
            .id()
            .ok_or_else(|| io::Error::other("the eco window has exited"))?;
        // A window Hyprland could not focus is still the user's window.
        if let Err(error) = self.control.focus(pid).await {
            eprintln!("eco: {error}");
        }
        self.last = number;
        self.open.push(Window {
            number,
            child,
            shows: show.unwrap_or_default().to_string(),
        });
        if self.hidden {
            self.share().await;
        }
        Ok(())
    }

    /// Leave every window out of screen sharing, or show them in it again,
    /// when that changes.
    pub async fn hide_from_share(&mut self, hidden: bool) {
        if hidden != self.hidden {
            self.hidden = hidden;
            self.share().await;
        }
    }

    /// Tell `control` which windows are left out of screen sharing.
    async fn share(&self) {
        let pids = self
            .open
            .iter()
            .filter_map(|window| window.child.id())
            .collect();
        if let Err(error) = self.control.hide_from_share(pids, self.hidden).await {
            eprintln!("eco: {error}");
        }
    }

    /// Give the keyboard to the open window `number`.
    pub async fn focus(&self, number: u32) -> io::Result<()> {
        let pid = self
            .open
            .iter()
            .find(|w| w.number == number)
            .and_then(|w| w.child.id())
            .ok_or_else(|| io::Error::other(format!("no eco window {number} is open")))?;
        self.control.focus(pid).await.map_err(io::Error::other)
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
        if self.hidden {
            self.share().await;
        }
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

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    use futures::FutureExt;
    use futures::future::BoxFuture;

    use super::*;
    use crate::adapters::fake_program::fake_program;
    use crate::ports::WindowError;

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

    /// A WindowControl that notes what it is asked.
    #[derive(Default)]
    struct Noted(Mutex<Vec<String>>);

    impl Noted {
        fn take(&self) -> Vec<String> {
            std::mem::take(&mut self.0.lock().unwrap())
        }
    }

    impl WindowControl for Noted {
        fn focus(&self, pid: u32) -> BoxFuture<'static, Result<(), WindowError>> {
            self.0.lock().unwrap().push(format!("focus {pid}"));
            async { Ok(()) }.boxed()
        }

        fn hide_from_share(
            &self,
            pids: Vec<u32>,
            hidden: bool,
        ) -> BoxFuture<'static, Result<(), WindowError>> {
            self.0
                .lock()
                .unwrap()
                .push(format!("hide {pids:?} {hidden}"));
            async { Ok(()) }.boxed()
        }
    }

    /// Windows whose program notes what it was handed in `dir/window-<number>`
    /// and then waits, or exits with 4 at once when told to show `exit`, or
    /// with 2 a moment after opening when told to show `later`; their token
    /// file is in `dir`.
    fn windows(dir: &Path, control: Arc<dyn WindowControl>, hidden: bool) -> Windows {
        let program = fake_program(
            dir,
            "eco-window",
            &format!(
                r#"echo "$1 $ECO_WINDOW $ECO_SHOW $ECO_CALL $ECO_TOKEN" > '{}'/window-$ECO_WINDOW
[ "$ECO_SHOW" = exit ] && exit 4
[ "$ECO_SHOW" = later ] && sleep 0.4 && exit 2
exec sleep 30"#,
                dir.display()
            ),
        );
        Windows::run(
            program,
            &dir.join("eco.token"),
            "secret".into(),
            control,
            hidden,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn a_window_is_handed_its_number_session_call_and_token() {
        let dir = tempfile::tempdir().unwrap();
        let mut windows = windows(dir.path(), Arc::new(Noted::default()), false);
        windows.open(Some("s1"), Some("{}")).await.unwrap();
        windows.open(None, None).await.unwrap();
        assert!(windows.is_open());
        let handed = |number: u32| fs::read_to_string(dir.path().join(format!("window-{number}")));
        assert_eq!(
            handed(1).unwrap(),
            format!("{} 1 s1 {{}} secret\n", shell().display())
        );
        assert_eq!(
            handed(2).unwrap(),
            format!("{} 2   secret\n", shell().display())
        );
        assert_eq!(windows.shown().collect::<Vec<_>>(), ["s1", ""]);
        windows.shows(2, "s2");
        windows.shows(9, "s9");
        assert_eq!(windows.shown().collect::<Vec<_>>(), ["s1", "s2"]);
        windows.close().await;
    }

    #[tokio::test]
    async fn a_window_that_exits_at_once_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let control = Arc::new(Noted::default());
        let mut windows = windows(dir.path(), control.clone(), false);
        let error = windows.open(Some("exit"), None).await.unwrap_err();
        assert_eq!(error.to_string(), "eco-window exited with exit status: 4");
        assert!(!windows.is_open());
        assert!(control.take().is_empty());
    }

    #[tokio::test]
    async fn a_missing_program_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut windows = windows(dir.path(), Arc::new(Noted::default()), false);
        let missing = dir.path().join("gone");
        windows.program = missing.clone();
        let error = windows.open(None, None).await.unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with(&format!("{}: ", missing.display())),
            "{error}"
        );
    }

    #[tokio::test]
    async fn a_window_that_exits_leaves_the_set_and_the_share() {
        let dir = tempfile::tempdir().unwrap();
        let control = Arc::new(Noted::default());
        let mut windows = windows(dir.path(), control.clone(), true);
        windows.open(None, None).await.unwrap();
        windows.open(Some("later"), None).await.unwrap();
        let first = pids(&windows)[0];
        control.take();
        let status = windows.exited().await.unwrap();
        assert_eq!(status.code(), Some(2));
        assert_eq!(windows.newest(), Some(1));
        assert_eq!(control.take(), [format!("hide [{first}] true")]);
        windows.close().await;
    }

    #[tokio::test(start_paused = true)]
    async fn without_a_window_nothing_exits() {
        let dir = tempfile::tempdir().unwrap();
        let mut windows = windows(dir.path(), Arc::new(Noted::default()), false);
        let waited = tokio::time::timeout(Duration::from_secs(60), windows.exited()).await;
        assert!(waited.is_err());
    }

    #[tokio::test]
    async fn focus_reaches_an_open_window_only() {
        let dir = tempfile::tempdir().unwrap();
        let control = Arc::new(Noted::default());
        let mut windows = windows(dir.path(), control.clone(), false);
        windows.open(None, None).await.unwrap();
        let first = pids(&windows)[0];
        control.take();
        windows.focus(1).await.unwrap();
        assert_eq!(control.take(), [format!("focus {first}")]);
        let error = windows.focus(2).await.unwrap_err();
        assert_eq!(error.to_string(), "no eco window 2 is open");
        windows.close().await;
    }

    #[tokio::test]
    async fn closing_terminates_every_window() {
        let dir = tempfile::tempdir().unwrap();
        let mut windows = windows(dir.path(), Arc::new(Noted::default()), false);
        windows.open(None, None).await.unwrap();
        let pid = pids(&windows)[0];
        windows.close().await;
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
    }

    fn pids(windows: &Windows) -> Vec<u32> {
        windows.open.iter().filter_map(|w| w.child.id()).collect()
    }

    #[tokio::test]
    async fn a_window_is_focused_once_it_opens() {
        let dir = tempfile::tempdir().unwrap();
        let control = Arc::new(Noted::default());
        let mut windows = windows(dir.path(), control.clone(), false);
        windows.open(None, None).await.unwrap();
        let first = pids(&windows)[0];
        assert_eq!(control.take(), [format!("focus {first}")]);
        windows.close().await;
    }

    /// A WindowControl that cannot focus.
    struct Unfocusable;

    impl WindowControl for Unfocusable {
        fn focus(&self, _: u32) -> BoxFuture<'static, Result<(), WindowError>> {
            async { Err(WindowError("no compositor".into())) }.boxed()
        }

        fn hide_from_share(
            &self,
            _: Vec<u32>,
            _: bool,
        ) -> BoxFuture<'static, Result<(), WindowError>> {
            async { Ok(()) }.boxed()
        }
    }

    #[tokio::test]
    async fn a_window_that_cannot_be_focused_stays_open() {
        let dir = tempfile::tempdir().unwrap();
        let mut windows = windows(dir.path(), Arc::new(Unfocusable), false);
        windows.open(None, None).await.unwrap();
        assert_eq!(pids(&windows).len(), 1);
        assert_eq!(windows.newest(), Some(1));
        windows.close().await;
    }

    #[tokio::test]
    async fn every_window_is_hidden_from_share_when_it_opens_while_the_setting_is_on() {
        let dir = tempfile::tempdir().unwrap();
        let control = Arc::new(Noted::default());
        let mut windows = windows(dir.path(), control.clone(), true);
        windows.open(None, None).await.unwrap();
        windows.open(None, None).await.unwrap();
        let [first, second] = pids(&windows)[..] else {
            panic!("two windows")
        };
        assert_eq!(
            control.take(),
            [
                format!("focus {first}"),
                format!("hide [{first}] true"),
                format!("focus {second}"),
                format!("hide [{first}, {second}] true"),
            ]
        );
        windows.close().await;
    }

    #[tokio::test]
    async fn the_windows_follow_the_setting_when_it_changes() {
        let dir = tempfile::tempdir().unwrap();
        let control = Arc::new(Noted::default());
        let mut windows = windows(dir.path(), control.clone(), false);
        windows.open(None, None).await.unwrap();
        let first = pids(&windows)[0];
        control.take();
        windows.hide_from_share(true).await;
        windows.hide_from_share(true).await;
        windows.hide_from_share(false).await;
        assert_eq!(
            control.take(),
            [
                format!("hide [{first}] true"),
                format!("hide [{first}] false")
            ]
        );
        windows.close().await;
    }
}
