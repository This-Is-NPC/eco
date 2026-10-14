//! The overlay windows, each an `eco-window` process running the QML in
//! `overlay/`, for as long as the session holds them, and the tray icon, one
//! more such process while `[ui] tray` is on (docs/design.md §3).

use std::io;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration;

use tokio::process::{Child, Command};
use tokio::time::Instant;

use crate::paths;
use crate::ports::WindowControl;

/// The overlay's entry `name` (`shell.qml`, the window; `tray.qml`, the tray
/// icon): installed beside the binary (`<prefix>/share/eco/overlay/<name>` for
/// `<prefix>/bin/eco`), or this checkout's when eco runs from it.
fn entry(name: &str) -> PathBuf {
    paths::shipped(&format!("overlay/{name}"))
        .unwrap_or_else(|| Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/overlay")).join(name))
}

/// The interface texts of the language pack `code` (`pt-BR`), by key; none
/// when the pack is not there or cannot be read.
pub fn language_pack(code: &str) -> serde_json::Map<String, serde_json::Value> {
    let path = entry(&format!("Eco/Core/i18n/{code}.json"));
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// The program that runs it: installed beside the binary
/// (`<prefix>/lib/eco/eco-window`, `eco-window.exe` on Windows), or this
/// checkout's build (`mise run window:build`) when eco runs from it.
pub fn program() -> PathBuf {
    let name = format!("eco-window{}", std::env::consts::EXE_SUFFIX);
    paths::shipped_program(&name).unwrap_or_else(|| {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/target/window")).join(name)
    })
}

/// The overlay windows, each its own eco-window process: a number, which the
/// window is told, and the session it shows, which it tells back. Every window
/// is told the daemon's socket and handed the same token, which tells its commands apart from other clients';
/// a window the daemon did not start reads it from `paths::token_path()`,
/// which the control socket writes.
/// `control` focuses each window as it opens and, while `hidden`, leaves them
/// all out of screen sharing. The tray icon is not one of the windows: it is
/// neither numbered, focused nor hidden, and a window call never goes to it.
pub struct Windows {
    launch: Launch,
    control: Arc<dyn WindowControl>,
    hidden: bool,
    last: u32,
    open: Vec<Window>,
    tray: Option<Tray>,
}

/// What runs every eco-window process: the program, the daemon's socket and
/// the token of the run.
struct Launch {
    program: PathBuf,
    socket: PathBuf,
    token: String,
}

impl Launch {
    /// The program running the overlay's entry `name`, with `env` set besides
    /// the socket and the token.
    fn spawn(&self, name: &str, env: &[(&str, &str)]) -> io::Result<Child> {
        Command::new(&self.program)
            .arg(entry(name))
            .envs(env.iter().copied())
            .env("ECO_SOCKET", &self.socket)
            .env("ECO_TOKEN", &self.token)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| io::Error::other(format!("{}: {error}", self.program.display())))
    }
}

/// The tray icon's process while `[ui] tray` is on: started again, after a
/// pause that doubles with each early end, whenever it exits or cannot start.
struct Tray {
    /// The process while it runs, and when it started.
    running: Option<(Child, Instant)>,
    /// How many times in a row it ended within `STEADY` of starting.
    failures: u32,
    /// When it starts again, while it does not run.
    again: Instant,
}

/// The first pause before the tray starts again, and the longest.
const RETRY: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(60);
/// A tray that ran this long ended on its own, not as a failure to start.
const STEADY: Duration = Duration::from_secs(60);

impl Tray {
    fn start(launch: &Launch) -> Self {
        let mut tray = Self {
            running: None,
            failures: 0,
            again: Instant::now(),
        };
        tray.run(launch);
        tray
    }

    /// Run the process, or wait to try again when it cannot start.
    fn run(&mut self, launch: &Launch) {
        match launch.spawn("tray.qml", &[]) {
            Ok(child) => self.running = Some((child, Instant::now())),
            Err(error) => self.retry(&format!("could not start ({error})")),
        }
    }

    /// Start again after the next pause, saying why.
    fn retry(&mut self, why: &str) {
        let pause = RETRY
            .saturating_mul(2u32.saturating_pow(self.failures))
            .min(RETRY_MAX);
        eprintln!(
            "eco: the tray icon {why}; it starts again in {} s",
            pause.as_secs()
        );
        self.failures += 1;
        self.again = Instant::now() + pause;
    }

    /// Resolves once the process exited, or once it was started again.
    async fn keep(&mut self, launch: &Launch) {
        match &mut self.running {
            Some((child, since)) => {
                let status = child.wait().await.map(|status| status.to_string());
                if since.elapsed() >= STEADY {
                    self.failures = 0;
                }
                self.running = None;
                let status = status.unwrap_or_else(|error| error.to_string());
                self.retry(&format!("exited ({status})"));
            }
            None => {
                tokio::time::sleep_until(self.again).await;
                self.run(launch);
            }
        }
    }

    /// End the process, and wait for it to exit.
    async fn end(self) {
        if let Some((mut child, _)) = self.running {
            terminate(&mut child);
            let _ = child.wait().await;
        }
    }
}

struct Window {
    number: u32,
    child: Child,
    /// The live session the window shows, or empty.
    shows: String,
}

impl Windows {
    pub fn new(
        socket: PathBuf,
        token: String,
        control: Arc<dyn WindowControl>,
        hidden: bool,
    ) -> Self {
        Self::run(program(), socket, token, control, hidden)
    }

    /// Windows run by `program`.
    fn run(
        program: PathBuf,
        socket: PathBuf,
        token: String,
        control: Arc<dyn WindowControl>,
        hidden: bool,
    ) -> Self {
        Self {
            launch: Launch {
                program,
                socket,
                token,
            },
            control,
            hidden,
            last: 0,
            open: Vec::new(),
            tray: None,
        }
    }

    pub fn is_open(&self) -> bool {
        !self.open.is_empty()
    }

    /// The live sessions some window shows.
    pub fn shown(&self) -> impl Iterator<Item = &str> {
        self.open.iter().map(|window| window.shows.as_str())
    }

    /// The numbers of the windows open, the oldest first.
    pub fn numbers(&self) -> impl Iterator<Item = u32> {
        self.open.iter().map(|window| window.number)
    }

    /// The window opened last of those still open.
    pub fn newest(&self) -> Option<u32> {
        self.numbers().last()
    }

    /// Open another window, focused, that shows `show` when it is a live
    /// session and makes `call` (a `window_call`'s JSON) once it meets the daemon.
    pub async fn open(&mut self, show: Option<&str>, call: Option<&str>) -> io::Result<()> {
        let number = self.last + 1;
        let mut child = self.launch.spawn(
            "shell.qml",
            &[
                ("ECO_WINDOW", &number.to_string()),
                ("ECO_SHOW", show.unwrap_or_default()),
                ("ECO_CALL", call.unwrap_or_default()),
            ],
        )?;
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

    /// The window `number` said it closes: it leaves the set at once, and its
    /// process, when it still runs `LINGER` later, is ended.
    pub fn closed(&mut self, number: u32) {
        let Some(at) = self.open.iter().position(|w| w.number == number) else {
            return;
        };
        let mut window = self.open.remove(at);
        tokio::spawn(async move {
            if tokio::time::timeout(LINGER, window.child.wait())
                .await
                .is_err()
            {
                let _ = window.child.kill().await;
            }
        });
    }

    /// Run the tray icon while `on`, or end it; nothing changes when it
    /// already is as asked.
    pub async fn tray(&mut self, on: bool) {
        if on && self.tray.is_none() {
            self.tray = Some(Tray::start(&self.launch));
        } else if !on && let Some(tray) = self.tray.take() {
            tray.end().await;
        }
    }

    /// Resolves when a window exits, which leaves the set; never, without one.
    /// `None` when its status could not be read. Meanwhile it starts the tray
    /// icon again when that exits.
    pub async fn exited(&mut self) -> Option<ExitStatus> {
        loop {
            let exited = {
                let open = &mut self.open;
                let windows = async move {
                    if open.is_empty() {
                        return std::future::pending().await;
                    }
                    let waits = open.iter_mut().map(|w| Box::pin(w.child.wait()));
                    let (status, at, _) = futures::future::select_all(waits).await;
                    (status.ok(), at)
                };
                let launch = &self.launch;
                let tray = async {
                    match self.tray.as_mut() {
                        Some(tray) => tray.keep(launch).await,
                        None => std::future::pending().await,
                    }
                };
                tokio::select! {
                    exited = windows => Some(exited),
                    () = tray => None,
                }
            };
            if let Some((status, at)) = exited {
                self.open.remove(at);
                if self.hidden {
                    self.share().await;
                }
                return status;
            }
        }
    }

    /// Terminate every window and the tray icon, and wait for them to exit.
    pub async fn close(self) {
        for mut window in self.open {
            terminate(&mut window.child);
            let _ = window.child.wait().await;
        }
        if let Some(tray) = self.tray {
            tray.end().await;
        }
    }
}

/// How long a window that said it closes may take to exit before it is ended.
const LINGER: Duration = Duration::from_secs(2);

/// Ask `child` to exit: SIGTERM, so the window closes cleanly.
#[cfg(unix)]
fn terminate(child: &mut Child) {
    use rustix::process::{Pid, Signal, kill_process};
    if let Some(pid) = child.id().and_then(|id| Pid::from_raw(id as i32)) {
        let _ = kill_process(pid, Signal::TERM);
    }
}

/// Ask `child` to exit: Windows has no signal a window process hears, so it is
/// terminated.
#[cfg(windows)]
fn terminate(child: &mut Child) {
    let _ = child.start_kill();
}

#[cfg(test)]
mod packs {
    use super::*;

    #[test]
    fn a_language_pack_is_read_by_its_code_and_one_missing_is_empty() {
        let english = language_pack("en-US");
        assert_eq!(english["notify.answer"], "{action} · answer ready");
        assert_eq!(language_pack("pt-BR")["_locale"], "pt_BR");
        assert!(language_pack("xx-XX").is_empty());
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_window_process_that_is_terminated_exits() {
        let mut child = Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        terminate(&mut child);
        let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(!status.success());
    }
}

/// The windows, each run by a stand-in shell script.
#[cfg(all(test, unix))]
pub(crate) mod programs {
    use std::fs;
    use std::path::Path;
    use std::sync::Mutex;

    use futures::FutureExt;
    use futures::future::BoxFuture;

    use super::*;
    use crate::adapters::fake_program::fake_program;
    use crate::ports::WindowError;

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
    /// with 2 a moment after opening when told to show `later`. Run as the
    /// tray, it adds a line to `dir/tray` instead, and exits with 3 at once
    /// when `dir/tray-dies` exists, which it removes.
    pub(crate) fn windows(dir: &Path, control: Arc<dyn WindowControl>, hidden: bool) -> Windows {
        let program = fake_program(
            dir,
            "eco-window",
            &format!(
                r#"cd '{}'
if [ "${{1##*/}}" = tray.qml ]; then
  echo "$1 [$ECO_WINDOW] $ECO_SOCKET $ECO_TOKEN" >> tray
  [ -e tray-dies ] && rm tray-dies && exit 3
  exec sleep 30
fi
echo "$1 $ECO_WINDOW $ECO_SHOW $ECO_CALL $ECO_SOCKET $ECO_TOKEN" > window-$ECO_WINDOW
[ "$ECO_SHOW" = exit ] && exit 4
[ "$ECO_SHOW" = later ] && sleep 0.4 && exit 2
exec sleep 30"#,
                dir.display()
            ),
        );
        Windows::run(
            program,
            dir.join("eco.sock"),
            "secret".into(),
            control,
            hidden,
        )
    }

    /// The tray's process, while it runs.
    fn tray_pid(windows: &Windows) -> Option<u32> {
        let (child, _) = windows.tray.as_ref()?.running.as_ref()?;
        child.id()
    }

    /// The lines the tray program wrote, once there are `count`, while
    /// `windows` waits for its windows; `exited` never resolves meanwhile.
    async fn tray_starts(dir: &Path, windows: &mut Windows, count: usize) -> Vec<String> {
        let start = std::time::Instant::now();
        loop {
            let lines: Vec<String> = fs::read_to_string(dir.join("tray"))
                .unwrap_or_default()
                .lines()
                .map(String::from)
                .collect();
            if lines.len() >= count {
                return lines;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "the tray started {} times",
                lines.len()
            );
            let waited = tokio::time::timeout(Duration::from_millis(50), windows.exited()).await;
            assert!(waited.is_err(), "a window exited");
        }
    }

    #[tokio::test]
    async fn the_tray_runs_while_on_and_is_no_window() {
        let dir = tempfile::tempdir().unwrap();
        let control = Arc::new(Noted::default());
        let mut windows = windows(dir.path(), control.clone(), true);
        windows.tray(false).await;
        assert!(windows.tray.is_none());
        windows.tray(true).await;
        windows.tray(true).await;
        let started = tray_starts(dir.path(), &mut windows, 1).await;
        let socket = dir.path().join("eco.sock");
        assert_eq!(
            started,
            [format!(
                "{} [] {} secret",
                entry("tray.qml").display(),
                socket.display()
            )]
        );
        let pid = tray_pid(&windows).unwrap();
        assert!(!windows.is_open());
        assert_eq!(windows.newest(), None);
        assert!(control.take().is_empty());
        // The first window is still window 1, and the only one shared or focused.
        windows.open(None, None).await.unwrap();
        assert_eq!(windows.newest(), Some(1));
        let window = pids(&windows)[0];
        assert_eq!(
            control.take(),
            [format!("focus {window}"), format!("hide [{window}] true")]
        );
        windows.tray(false).await;
        assert!(ended(pid));
        assert!(windows.tray.is_none());
        assert!(!ended(window));
        windows.close().await;
    }

    #[tokio::test]
    async fn a_tray_that_exits_starts_again() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("tray-dies"), "").unwrap();
        let mut windows = windows(dir.path(), Arc::new(Noted::default()), false);
        windows.tray(true).await;
        tray_starts(dir.path(), &mut windows, 2).await;
        let pid = tray_pid(&windows).unwrap();
        windows.close().await;
        assert!(ended(pid));
    }

    #[tokio::test]
    async fn a_tray_that_cannot_start_is_tried_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut windows = windows(dir.path(), Arc::new(Noted::default()), false);
        let program = std::mem::replace(&mut windows.launch.program, dir.path().join("gone"));
        windows.tray(true).await;
        assert_eq!(tray_pid(&windows), None);
        windows.launch.program = program;
        tray_starts(dir.path(), &mut windows, 1).await;
        windows.close().await;
    }

    #[tokio::test]
    async fn a_window_is_handed_its_number_session_call_socket_and_token() {
        let dir = tempfile::tempdir().unwrap();
        let mut windows = windows(dir.path(), Arc::new(Noted::default()), false);
        windows.open(Some("s1"), Some("{}")).await.unwrap();
        windows.open(None, None).await.unwrap();
        assert!(windows.is_open());
        let handed = |number: u32| fs::read_to_string(dir.path().join(format!("window-{number}")));
        let socket = dir.path().join("eco.sock");
        assert_eq!(
            handed(1).unwrap(),
            format!(
                "{} 1 s1 {{}} {} secret\n",
                entry("shell.qml").display(),
                socket.display()
            )
        );
        assert_eq!(
            handed(2).unwrap(),
            format!(
                "{} 2   {} secret\n",
                entry("shell.qml").display(),
                socket.display()
            )
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
        windows.launch.program = missing.clone();
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

    /// Whether the process `pid` is gone or only waits to be reaped.
    fn ended(pid: u32) -> bool {
        fs::read_to_string(format!("/proc/{pid}/stat")).map_or(true, |stat| {
            stat.rsplit(')')
                .next()
                .unwrap()
                .trim_start()
                .starts_with('Z')
        })
    }

    #[tokio::test(start_paused = true)]
    async fn a_window_that_says_it_closed_and_stays_is_ended() {
        let dir = tempfile::tempdir().unwrap();
        let mut windows = windows(dir.path(), Arc::new(Noted::default()), false);
        windows.open(None, None).await.unwrap();
        windows.open(None, None).await.unwrap();
        let [first, second]: [u32; 2] = pids(&windows).try_into().unwrap();
        assert_eq!(windows.numbers().collect::<Vec<_>>(), [1, 2]);
        windows.closed(1);
        windows.closed(7);
        assert_eq!(pids(&windows), [second]);
        assert_eq!(windows.newest(), Some(2));
        assert_eq!(windows.numbers().collect::<Vec<_>>(), [2]);
        tokio::time::sleep(LINGER - Duration::from_millis(100)).await;
        assert!(!ended(first));
        tokio::time::sleep(Duration::from_millis(200)).await;
        let start = std::time::Instant::now();
        while !ended(first) {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "window {first} still runs"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!ended(second));
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

    /// A WindowControl that can neither focus nor hide from share.
    struct Refusing;

    impl WindowControl for Refusing {
        fn focus(&self, _: u32) -> BoxFuture<'static, Result<(), WindowError>> {
            async { Err(WindowError("no compositor".into())) }.boxed()
        }

        fn hide_from_share(
            &self,
            _: Vec<u32>,
            _: bool,
        ) -> BoxFuture<'static, Result<(), WindowError>> {
            async { Err(WindowError("no compositor".into())) }.boxed()
        }
    }

    #[tokio::test]
    async fn a_window_the_compositor_refuses_stays_open() {
        let dir = tempfile::tempdir().unwrap();
        let mut windows = windows(dir.path(), Arc::new(Refusing), true);
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
        let [first, second]: [u32; 2] = pids(&windows).try_into().unwrap();
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
