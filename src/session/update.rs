//! Knowing of a newer eco and installing it: the daily check and what it
//! found on disk, the install, and the daemon's restart, which waits while a
//! session is live.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config;
use crate::domain::assistant::Emit;
use crate::domain::events::error;
use crate::ports::{InstallStage, Installed, Installer, Releases};

/// How long a check holds before the daemon asks again.
pub const DAY: Duration = Duration::from_secs(24 * 60 * 60);

const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// The tag of `version`: `0.5.0` and `v0.5.0` are both `v0.5.0`.
pub fn tag(version: &str) -> String {
    format!("v{}", version.trim().trim_start_matches('v'))
}

/// The version a tag names, without its `v`.
fn version(tag: &str) -> &str {
    tag.trim().trim_start_matches('v')
}

/// A version's numbers, its pre-release suffix left out.
fn numbers(version: &str) -> Vec<u64> {
    let release = version.split(['-', '+']).next().unwrap_or_default();
    release
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// Whether the release tagged `tag` is a later version than `current`.
pub fn newer(current: &str, tag: &str) -> bool {
    numbers(version(tag)) > numbers(version(current))
}

/// What the last check that reached GitHub found, as kept in `update.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checked {
    /// When it was made, in seconds since the epoch.
    pub checked_at: i64,
    /// The latest release's version, without its `v`.
    pub latest: Option<String>,
    /// The latest release's page, with its notes.
    #[serde(default)]
    pub notes: String,
}

/// The check kept at `path`; none when it is missing or half written.
fn read(path: &Path) -> Option<Checked> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Restarts the daemon from the eco executable it is given, in a process that
/// outlives it.
pub type Restart = Arc<dyn Fn(&Path) -> io::Result<()> + Send + Sync>;

/// Whether a session is live.
pub type Live = Arc<dyn Fn() -> bool + Send + Sync>;

/// Seconds since the epoch.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

#[derive(Default)]
struct State {
    checked: Option<Checked>,
    /// Why the last check failed; empty when it did not.
    error: String,
    check: bool,
    /// An install runs, or waits for its restart.
    installing: bool,
    /// The version installed whose restart waits for the last live session to end.
    waiting: Option<(String, Installed)>,
}

/// The ports and the state an update needs.
pub struct Parts {
    pub releases: Arc<dyn Releases>,
    pub installer: Arc<dyn Installer>,
    pub restart: Restart,
    pub live: Live,
    pub emit: Emit,
    /// Where the last check is kept.
    pub state: PathBuf,
    pub now: Clock,
}

/// The daemon's updates: one per daemon, shared by its tasks.
#[derive(Clone)]
pub struct Updates {
    parts: Arc<Parts>,
    state: Arc<Mutex<State>>,
}

impl Updates {
    /// Updates that check by themselves when `check`, starting from the check
    /// kept on disk.
    pub fn new(parts: Parts, check: bool) -> Self {
        let state = State {
            checked: read(&parts.state),
            check,
            ..State::default()
        };
        Self {
            parts: Arc::new(parts),
            state: Arc::new(Mutex::new(state)),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("not poisoned")
    }

    /// The `update` event: this version, the latest one known and whether it is newer.
    pub fn event(&self) -> Value {
        let state = self.state();
        let checked = state.checked.as_ref();
        let latest = checked.and_then(|checked| checked.latest.clone());
        json!({
            "type": "update",
            "current": CURRENT,
            "newer": latest.as_deref().is_some_and(|latest| newer(CURRENT, latest)),
            "latest": latest,
            "notes": checked.map(|checked| checked.notes.clone()).unwrap_or_default(),
            "checked_at": checked.map(|checked| checked.checked_at),
            "check": state.check,
            "error": state.error,
        })
    }

    /// Turn the daily check on or off; the `update` event when that changed it.
    pub fn set_check(&self, check: bool) {
        let changed = std::mem::replace(&mut self.state().check, check) != check;
        if changed {
            (self.parts.emit)(self.event());
        }
    }

    /// Ask for the latest release and keep what was found; whether the
    /// `update` event, but for when it was checked, changed.
    pub async fn check(&self) -> bool {
        let before = without_time(self.event());
        let found = self.parts.releases.latest().await;
        let mut state = self.state();
        match found {
            Ok(release) => {
                let checked = Checked {
                    checked_at: (self.parts.now)(),
                    latest: Some(version(&release.tag).to_string()),
                    notes: release.html_url,
                };
                let text = serde_json::to_vec(&checked).expect("a check serializes");
                if let Err(failure) = config::write_atomically(&self.parts.state, &text) {
                    eprintln!("eco: {}: {failure}", self.parts.state.display());
                }
                state.checked = Some(checked);
                state.error.clear();
            }
            Err(failure) => state.error = failure.0,
        }
        drop(state);
        without_time(self.event()) != before
    }

    /// Check at most once a day, for as long as it runs: at once when the
    /// last check is a day old, then a day after each.
    pub async fn daily(self) {
        let last = self.state().checked.as_ref().map(|c| c.checked_at);
        let elapsed = last.map_or(i64::MAX, |at| (self.parts.now)().saturating_sub(at));
        let mut wait = DAY.saturating_sub(Duration::from_secs(elapsed.max(0) as u64));
        loop {
            tokio::time::sleep(wait).await;
            if self.check().await {
                (self.parts.emit)(self.event());
            }
            wait = DAY;
        }
    }

    /// The install of `version`, the latest release when none, or `None`, with
    /// `update.busy` said, while another one runs.
    pub fn install(&self, version: Option<String>) -> Option<BoxFuture<'static, ()>> {
        if std::mem::replace(&mut self.state().installing, true) {
            (self.parts.emit)(error(
                "update.busy",
                "an update is already being installed",
                Value::Null,
            ));
            return None;
        }
        let updates = self.clone();
        Some(Box::pin(async move { updates.run_install(version).await }))
    }

    async fn run_install(&self, version: Option<String>) {
        let tag = match version {
            Some(version) => tag(&version),
            None => match self.parts.releases.latest().await {
                Ok(release) => release.tag,
                Err(failure) => return self.failed("", &failure.0),
            },
        };
        let installing = self::version(&tag).to_string();
        let emit = Arc::clone(&self.parts.emit);
        let said = installing.clone();
        let progress = Box::new(move |stage: InstallStage| {
            emit(progress(stage_name(stage), &said, ""));
        });
        match self.parts.installer.install(tag, progress).await {
            Ok(installed) => self.installed(installing, installed),
            Err(failure) => self.failed(&installing, &failure.0),
        }
    }

    fn failed(&self, version: &str, detail: &str) {
        self.state().installing = false;
        (self.parts.emit)(progress("failed", version, detail));
    }

    /// `version` is installed: restart now, or once the last live session ends.
    fn installed(&self, version: String, installed: Installed) {
        let mut state = self.state();
        if (self.parts.live)() {
            (self.parts.emit)(progress("waiting", &version, ""));
            state.waiting = Some((version, installed));
        } else {
            drop(state);
            self.restart_installed(&version, installed);
        }
    }

    /// Restart into the installed version whose restart waits, once no
    /// session is live; called after each command.
    pub fn sessions_changed(&self) {
        let waiting = {
            let mut state = self.state();
            match state.waiting.is_some() && !(self.parts.live)() {
                true => state.waiting.take(),
                false => None,
            }
        };
        if let Some((version, installed)) = waiting {
            self.restart_installed(&version, installed);
        }
    }

    /// Restart into the installed program, or hand a pending package to the
    /// process that installs it once the daemon has stopped.
    fn restart_installed(&self, version: &str, installed: Installed) {
        (self.parts.emit)(progress("restarting", version, ""));
        let restarted = match installed {
            Installed::Done => (self.parts.restart)(&self.parts.installer.program()),
            Installed::Pending(pending) => pending.hand_off(),
        };
        match restarted {
            Ok(()) => (self.parts.emit)(progress("done", version, "")),
            Err(failure) => self.failed(version, &failure.to_string()),
        }
    }

    /// Remove eco, and with `everything` the user's files too, by handing the
    /// removal to a process that stops the daemon: `uninstall` says
    /// `removing` once it runs, `failed` when it could not start. Refused,
    /// with `uninstall.recording`, while a session is live; `update.busy` while
    /// an install or a removal runs.
    pub fn uninstall(&self, everything: bool) {
        if (self.parts.live)() {
            return (self.parts.emit)(error(
                "uninstall.recording",
                "a session is recording; end it before removing eco",
                Value::Null,
            ));
        }
        if std::mem::replace(&mut self.state().installing, true) {
            return (self.parts.emit)(error(
                "update.busy",
                "an update is already being installed",
                Value::Null,
            ));
        }
        let stage = match self.parts.installer.uninstall(everything).hand_off() {
            Ok(()) => removal("removing", everything, ""),
            Err(failure) => {
                self.state().installing = false;
                removal("failed", everything, &failure.to_string())
            }
        };
        (self.parts.emit)(stage);
    }

    /// Restart the daemon; while a session is live, only when `force`d, else
    /// `daemon.recording` is said.
    pub fn restart(&self, force: bool) {
        if !force && (self.parts.live)() {
            return (self.parts.emit)(error(
                "daemon.recording",
                "a session is recording; restart anyway with force",
                Value::Null,
            ));
        }
        if let Err(failure) = running_exe().and_then(|exe| (self.parts.restart)(&exe)) {
            let detail = failure.to_string();
            (self.parts.emit)(error(
                "daemon.restart_failed",
                format!("cannot restart the daemon: {detail}"),
                json!({"detail": detail}),
            ));
        }
    }
}

/// The executable the daemon runs from.
fn running_exe() -> io::Result<PathBuf> {
    Ok(super::undeleted(std::env::current_exe()?))
}

/// The name the protocol gives `stage`.
pub fn stage_name(stage: InstallStage) -> &'static str {
    match stage {
        InstallStage::Downloading => "downloading",
        InstallStage::Verifying => "verifying",
        InstallStage::Installing => "installing",
    }
}

/// The `update_progress` event of `stage` for `version`.
fn progress(stage: &str, version: &str, detail: &str) -> Value {
    json!({"type": "update_progress", "stage": stage, "version": version, "detail": detail})
}

/// The `uninstall` event of `stage`.
fn removal(stage: &str, everything: bool, detail: &str) -> Value {
    json!({"type": "uninstall", "stage": stage, "everything": everything, "detail": detail})
}

fn without_time(mut event: Value) -> Value {
    event["checked_at"] = Value::Null;
    event
}

#[cfg(test)]
pub(crate) mod testing {
    //! Releases and an installer as fakes, so updates run without a network.

    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures::FutureExt;

    use super::*;
    use crate::ports::{InstallError, InstallProgress, Pending, Release, ReleaseError};

    /// The releases GitHub would list: `latest`, or the network down when none.
    pub struct FakeReleases {
        latest: Option<&'static str>,
        asked: AtomicUsize,
    }

    impl FakeReleases {
        pub fn asked(&self) -> usize {
            self.asked.load(Ordering::SeqCst)
        }
    }

    impl Releases for FakeReleases {
        fn latest(&self) -> BoxFuture<'static, Result<Release, ReleaseError>> {
            self.asked.fetch_add(1, Ordering::SeqCst);
            let found = match self.latest {
                Some(tag) => Ok(Release {
                    tag: tag.into(),
                    html_url: format!("https://github.com/This-Is-NPC/eco/releases/tag/{tag}"),
                }),
                None => Err(ReleaseError(
                    "cannot reach GitHub: connection refused".into(),
                )),
            };
            async move { found }.boxed()
        }

        fn tagged(&self, _: String) -> BoxFuture<'static, Result<Release, ReleaseError>> {
            unreachable!("the daemon names the tag it installs")
        }
    }

    /// Goes through every stage, then fails with `fails` or installs: the tags
    /// it installed are kept. With `pending`, the install is left to a
    /// hand-off that counts into it.
    #[derive(Default)]
    pub struct FakeInstaller {
        pub fails: Option<&'static str>,
        pub pending: Option<Arc<AtomicUsize>>,
        pub installed: Mutex<Vec<String>>,
        /// Whether `everything` was asked at each removal handed off.
        pub uninstalled: Arc<Mutex<Vec<bool>>>,
        /// Why a removal cannot be handed off.
        pub uninstall_fails: Option<&'static str>,
        /// Why the package cannot be removed.
        pub remove_fails: Option<&'static str>,
    }

    /// A pending removal that keeps what it was asked.
    #[derive(Debug)]
    struct RemovalHandOff {
        everything: bool,
        fails: Option<&'static str>,
        asked: Arc<Mutex<Vec<bool>>>,
    }

    impl Pending for RemovalHandOff {
        fn hand_off(&self) -> io::Result<()> {
            match self.fails {
                Some(why) => Err(io::Error::other(why)),
                None => {
                    self.asked.lock().unwrap().push(self.everything);
                    Ok(())
                }
            }
        }
    }

    /// A pending install that counts its hand-offs.
    #[derive(Debug)]
    struct CountedHandOff(Arc<AtomicUsize>);

    impl Pending for CountedHandOff {
        fn hand_off(&self) -> io::Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    impl Installer for FakeInstaller {
        fn program(&self) -> PathBuf {
            PathBuf::from(INSTALLED)
        }

        fn install(
            &self,
            tag: String,
            progress: InstallProgress,
        ) -> BoxFuture<'static, Result<Installed, InstallError>> {
            progress(InstallStage::Downloading);
            progress(InstallStage::Verifying);
            let result = match self.fails {
                Some(why) => Err(InstallError(why.into())),
                None => {
                    progress(InstallStage::Installing);
                    self.installed.lock().unwrap().push(tag);
                    Ok(match &self.pending {
                        Some(handed_off) => {
                            Installed::Pending(Box::new(CountedHandOff(Arc::clone(handed_off))))
                        }
                        None => Installed::Done,
                    })
                }
            };
            async move { result }.boxed()
        }

        fn uninstall(&self, everything: bool) -> Box<dyn Pending> {
            Box::new(RemovalHandOff {
                everything,
                fails: self.uninstall_fails,
                asked: Arc::clone(&self.uninstalled),
            })
        }

        fn remove(&self) -> BoxFuture<'static, Result<(), InstallError>> {
            let result = match self.remove_fails {
                Some(why) => Err(InstallError(why.into())),
                None => Ok(()),
            };
            async move { result }.boxed()
        }
    }

    /// The fakes behind a test's updates, and how often it restarted.
    pub struct Fakes {
        pub releases: Arc<FakeReleases>,
        pub installer: Arc<FakeInstaller>,
        pub restarts: Arc<AtomicUsize>,
        /// The executables each restart was given.
        pub restarted: Arc<Mutex<Vec<PathBuf>>>,
    }

    impl Fakes {
        pub fn restarts(&self) -> usize {
            self.restarts.load(Ordering::SeqCst)
        }
    }

    /// The executable the fake installer installs.
    pub const INSTALLED: &str = "/installed/eco";

    /// The epoch second the tests start at.
    pub const START: i64 = 1_800_000_000;

    /// Updates that find `latest`, install with `installer` and keep their
    /// check at `state`; their clock starts at [`START`] and runs with tokio's.
    pub fn updates(
        latest: Option<&'static str>,
        installer: FakeInstaller,
        live: Live,
        emit: Emit,
        state: PathBuf,
    ) -> (Updates, Fakes) {
        let releases = Arc::new(FakeReleases {
            latest,
            asked: AtomicUsize::new(0),
        });
        let installer = Arc::new(installer);
        let restarts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&restarts);
        let restarted = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&restarted);
        let started = tokio::time::Instant::now();
        let parts = Parts {
            releases: releases.clone(),
            installer: installer.clone(),
            restart: Arc::new(move |exe| {
                counted.fetch_add(1, Ordering::SeqCst);
                kept.lock().unwrap().push(exe.to_path_buf());
                Ok(())
            }),
            live,
            emit,
            state,
            now: Arc::new(move || START + started.elapsed().as_secs() as i64),
        };
        let fakes = Fakes {
            releases,
            installer,
            restarts,
            restarted,
        };
        (Updates::new(parts, true), fakes)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::testing::{FakeInstaller, Fakes, START, updates};
    use super::*;

    struct Fixture {
        updates: Updates,
        fakes: Fakes,
        events: Arc<Mutex<Vec<Value>>>,
        live: Arc<AtomicBool>,
        dir: tempfile::TempDir,
    }

    fn fixture(
        latest: Option<&'static str>,
        installer: FakeInstaller,
        kept: Option<i64>,
    ) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state").join("update.json");
        if let Some(at) = kept {
            let checked = json!({"checked_at": at, "latest": "0.0.1", "notes": "old"});
            config::write_atomically(&state, checked.to_string().as_bytes()).unwrap();
        }
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let live = Arc::new(AtomicBool::new(false));
        let recording = Arc::clone(&live);
        let (updates, fakes) = updates(
            latest,
            installer,
            Arc::new(move || recording.load(Ordering::SeqCst)),
            Arc::new(move |event| sink.lock().unwrap().push(event)),
            state,
        );
        Fixture {
            updates,
            fakes,
            events,
            live,
            dir,
        }
    }

    impl Fixture {
        fn stages(&self) -> Vec<String> {
            let events = self.events.lock().unwrap();
            events
                .iter()
                .filter(|event| event["type"] == "update_progress")
                .map(|event| event["stage"].as_str().unwrap().to_string())
                .collect()
        }

        fn restarts(&self) -> usize {
            self.fakes.restarts()
        }

        fn kept(&self) -> Option<Checked> {
            read(&self.dir.path().join("state").join("update.json"))
        }
    }

    #[test]
    fn versions_compare_by_their_numbers() {
        assert!(newer("0.4.0", "v0.5.0"));
        assert!(newer("0.4.9", "v0.4.10"));
        assert!(newer("0.4.0", "1.0.0"));
        assert!(!newer("0.4.0", "v0.4.0"));
        assert!(!newer("0.5.0", "v0.4.0"));
        assert!(!newer("0.4.0", "v0.4.0-rc1"));
        assert_eq!(tag("0.5.0"), "v0.5.0");
        assert_eq!(tag(" v0.5.0"), "v0.5.0");
    }

    #[tokio::test]
    async fn a_newer_release_is_said_and_kept() {
        let f = fixture(Some("v99.0.0"), FakeInstaller::default(), None);
        assert!(f.updates.check().await);
        let event = f.updates.event();
        assert_eq!(event["current"], CURRENT);
        assert_eq!(event["latest"], "99.0.0");
        assert_eq!(event["newer"], true);
        assert_eq!(
            event["notes"],
            "https://github.com/This-Is-NPC/eco/releases/tag/v99.0.0"
        );
        assert_eq!(event["checked_at"], START);
        assert_eq!(event["error"], "");
        let kept = f.kept().unwrap();
        assert_eq!(
            (kept.checked_at, kept.latest.as_deref()),
            (START, Some("99.0.0"))
        );
    }

    #[tokio::test]
    async fn the_same_version_is_not_newer() {
        let current: &'static str = Box::leak(tag(CURRENT).into_boxed_str());
        let f = fixture(Some(current), FakeInstaller::default(), None);
        f.updates.check().await;
        let event = f.updates.event();
        assert_eq!(event["latest"], CURRENT);
        assert_eq!(event["newer"], false);
    }

    #[tokio::test]
    async fn a_network_that_is_down_says_why_and_keeps_the_last_check() {
        let f = fixture(None, FakeInstaller::default(), Some(START - 60));
        assert!(f.updates.check().await);
        let event = f.updates.event();
        assert_eq!(event["error"], "cannot reach GitHub: connection refused");
        assert_eq!(event["latest"], "0.0.1");
        assert_eq!(event["checked_at"], START - 60);
        assert_eq!(f.kept().unwrap().checked_at, START - 60);
    }

    #[tokio::test]
    async fn a_half_written_state_file_is_no_check() {
        let f = fixture(None, FakeInstaller::default(), None);
        let state = f.dir.path().join("state").join("update.json");
        config::write_atomically(&state, br#"{"checked_at": 1"#).unwrap();
        assert_eq!(read(&state), None);
    }

    #[tokio::test(start_paused = true)]
    async fn a_day_old_check_is_made_at_once_then_daily() {
        let f = fixture(
            Some("v99.0.0"),
            FakeInstaller::default(),
            Some(START - 25 * 3600),
        );
        let daily = tokio::spawn(f.updates.clone().daily());
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(f.fakes.releases.asked(), 1);
        assert_eq!(f.events.lock().unwrap()[0]["latest"], "99.0.0");
        tokio::time::sleep(DAY - Duration::from_secs(2)).await;
        assert_eq!(f.fakes.releases.asked(), 1);
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(f.fakes.releases.asked(), 2);
        // Nothing changed but the time it was checked: nothing more is said.
        assert_eq!(f.events.lock().unwrap().len(), 1);
        daily.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_recent_check_waits_out_its_day() {
        let f = fixture(
            Some("v99.0.0"),
            FakeInstaller::default(),
            Some(START - 3600),
        );
        let daily = tokio::spawn(f.updates.clone().daily());
        tokio::time::sleep(Duration::from_secs(23 * 3600 - 1)).await;
        assert_eq!(f.fakes.releases.asked(), 0);
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(f.fakes.releases.asked(), 1);
        daily.abort();
    }

    #[tokio::test]
    async fn turning_the_check_off_is_said() {
        let f = fixture(None, FakeInstaller::default(), None);
        f.updates.set_check(true);
        assert!(f.events.lock().unwrap().is_empty());
        f.updates.set_check(false);
        assert_eq!(f.events.lock().unwrap()[0]["check"], false);
    }

    #[tokio::test]
    async fn an_install_goes_through_its_stages_and_restarts() {
        let f = fixture(Some("v99.0.0"), FakeInstaller::default(), None);
        f.updates.install(None).unwrap().await;
        assert_eq!(*f.fakes.installer.installed.lock().unwrap(), ["v99.0.0"]);
        assert_eq!(
            f.stages(),
            [
                "downloading",
                "verifying",
                "installing",
                "restarting",
                "done"
            ]
        );
        assert_eq!(f.events.lock().unwrap()[0]["version"], "99.0.0");
        assert_eq!(f.restarts(), 1);
    }

    #[tokio::test]
    async fn a_named_version_is_installed_even_when_older() {
        let f = fixture(None, FakeInstaller::default(), None);
        f.updates.install(Some("0.1.0".into())).unwrap().await;
        assert_eq!(*f.fakes.installer.installed.lock().unwrap(), ["v0.1.0"]);
        assert_eq!(f.fakes.releases.asked(), 0);
    }

    #[tokio::test]
    async fn a_checksum_mismatch_installs_nothing_and_does_not_restart() {
        let installer = FakeInstaller {
            fails: Some("eco-99.0.0.pkg.tar.zst does not match SHA256SUMS; nothing was installed"),
            ..FakeInstaller::default()
        };
        let f = fixture(Some("v99.0.0"), installer, None);
        f.updates.install(None).unwrap().await;
        assert_eq!(f.stages(), ["downloading", "verifying", "failed"]);
        let events = f.events.lock().unwrap().clone();
        assert_eq!(
            events.last().unwrap()["detail"],
            "eco-99.0.0.pkg.tar.zst does not match SHA256SUMS; nothing was installed"
        );
        assert_eq!(f.restarts(), 0);
        // A failed install lets the next one run.
        assert!(f.updates.install(None).is_some());
    }

    #[tokio::test]
    async fn without_the_network_the_latest_cannot_be_installed() {
        let f = fixture(None, FakeInstaller::default(), None);
        f.updates.install(None).unwrap().await;
        assert_eq!(f.stages(), ["failed"]);
        assert!(f.fakes.installer.installed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_second_install_while_one_runs_is_refused() {
        let f = fixture(Some("v99.0.0"), FakeInstaller::default(), None);
        let first = f.updates.install(None).unwrap();
        assert!(f.updates.install(None).is_none());
        assert_eq!(f.events.lock().unwrap()[0]["code"], "update.busy");
        first.await;
    }

    #[tokio::test]
    async fn a_live_session_defers_the_restart_until_it_ends() {
        let f = fixture(Some("v99.0.0"), FakeInstaller::default(), None);
        f.live.store(true, Ordering::SeqCst);
        f.updates.install(None).unwrap().await;
        assert_eq!(
            f.stages(),
            ["downloading", "verifying", "installing", "waiting"]
        );
        f.updates.sessions_changed();
        assert_eq!(f.restarts(), 0);
        f.live.store(false, Ordering::SeqCst);
        f.updates.sessions_changed();
        assert_eq!(f.restarts(), 1);
        assert_eq!(
            f.stages(),
            [
                "downloading",
                "verifying",
                "installing",
                "waiting",
                "restarting",
                "done"
            ]
        );
        // Restarted once only.
        f.updates.sessions_changed();
        assert_eq!(f.restarts(), 1);
    }

    #[tokio::test]
    async fn a_pending_install_is_handed_off_instead_of_restarting() {
        let handed_off = Arc::new(AtomicUsize::new(0));
        let installer = FakeInstaller {
            pending: Some(Arc::clone(&handed_off)),
            ..FakeInstaller::default()
        };
        let f = fixture(Some("v99.0.0"), installer, None);
        f.updates.install(None).unwrap().await;
        assert_eq!(handed_off.load(Ordering::SeqCst), 1);
        assert_eq!(f.restarts(), 0);
        assert_eq!(
            f.stages(),
            [
                "downloading",
                "verifying",
                "installing",
                "restarting",
                "done"
            ]
        );
    }

    #[tokio::test]
    async fn a_pending_install_waits_for_the_live_session_then_hands_off_once() {
        let handed_off = Arc::new(AtomicUsize::new(0));
        let installer = FakeInstaller {
            pending: Some(Arc::clone(&handed_off)),
            ..FakeInstaller::default()
        };
        let f = fixture(Some("v99.0.0"), installer, None);
        f.live.store(true, Ordering::SeqCst);
        f.updates.install(None).unwrap().await;
        f.updates.sessions_changed();
        assert_eq!(handed_off.load(Ordering::SeqCst), 0);
        assert_eq!(f.stages().last().unwrap(), "waiting");
        f.live.store(false, Ordering::SeqCst);
        f.updates.sessions_changed();
        f.updates.sessions_changed();
        assert_eq!(handed_off.load(Ordering::SeqCst), 1);
        assert_eq!(f.restarts(), 0);
        assert_eq!(f.stages()[4..], ["restarting", "done"]);
    }

    fn removals(f: &Fixture) -> Vec<Value> {
        let events = f.events.lock().unwrap();
        events.clone()
    }

    #[tokio::test]
    async fn removing_eco_hands_the_removal_off_and_says_so() {
        for everything in [false, true] {
            let f = fixture(None, FakeInstaller::default(), None);
            f.updates.uninstall(everything);
            assert_eq!(removals(&f), [removal("removing", everything, "")]);
            assert_eq!(*f.fakes.installer.uninstalled.lock().unwrap(), [everything]);
        }
    }

    #[tokio::test]
    async fn a_recording_session_refuses_the_removal() {
        let f = fixture(None, FakeInstaller::default(), None);
        f.live.store(true, Ordering::SeqCst);
        f.updates.uninstall(true);
        assert_eq!(f.events.lock().unwrap()[0]["code"], "uninstall.recording");
        assert!(f.fakes.installer.uninstalled.lock().unwrap().is_empty());
        // Nothing was started: once the session ends, the removal can go.
        f.live.store(false, Ordering::SeqCst);
        f.updates.uninstall(true);
        assert_eq!(f.fakes.installer.uninstalled.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_removal_cannot_start_while_an_install_or_a_removal_runs() {
        let f = fixture(Some("v99.0.0"), FakeInstaller::default(), None);
        let install = f.updates.install(None).unwrap();
        f.updates.uninstall(false);
        assert_eq!(f.events.lock().unwrap()[0]["code"], "update.busy");
        install.await;
        let f = fixture(None, FakeInstaller::default(), None);
        f.updates.uninstall(false);
        f.updates.uninstall(false);
        assert_eq!(f.events.lock().unwrap()[1]["code"], "update.busy");
        assert_eq!(f.fakes.installer.uninstalled.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_removal_that_cannot_start_says_why_and_can_be_asked_again() {
        let installer = FakeInstaller {
            uninstall_fails: Some("no systemd-run"),
            ..FakeInstaller::default()
        };
        let f = fixture(None, installer, None);
        f.updates.uninstall(true);
        f.updates.uninstall(true);
        let said = removals(&f);
        assert_eq!(said[0], removal("failed", true, "no systemd-run"));
        assert_eq!(said[1], removal("failed", true, "no systemd-run"));
    }

    #[tokio::test]
    async fn a_restart_that_cannot_start_fails_the_install() {
        let mut f = fixture(Some("v99.0.0"), FakeInstaller::default(), None);
        let parts = Arc::get_mut(&mut f.updates.parts).unwrap();
        parts.restart = Arc::new(|_| Err(io::Error::other("no systemd-run")));
        f.updates.install(None).unwrap().await;
        let events = f.events.lock().unwrap().clone();
        assert_eq!(events.last().unwrap()["stage"], "failed");
        assert_eq!(events.last().unwrap()["detail"], "no systemd-run");
    }

    #[tokio::test]
    async fn a_check_that_cannot_be_kept_is_still_said() {
        let mut f = fixture(Some("v99.0.0"), FakeInstaller::default(), None);
        // The state's directory is a file: nothing can be written under it.
        let blocked = f.dir.path().join("blocked");
        std::fs::write(&blocked, b"").unwrap();
        Arc::get_mut(&mut f.updates.parts).unwrap().state = blocked.join("update.json");
        assert!(f.updates.check().await);
        assert_eq!(f.updates.event()["latest"], "99.0.0");
        assert_eq!(read(&blocked.join("update.json")), None);
    }

    #[tokio::test]
    async fn a_restart_that_cannot_start_is_said() {
        let mut f = fixture(None, FakeInstaller::default(), None);
        let parts = Arc::get_mut(&mut f.updates.parts).unwrap();
        parts.restart = Arc::new(|_| Err(io::Error::other("no systemd-run")));
        f.updates.restart(false);
        let events = f.events.lock().unwrap().clone();
        assert_eq!(events[0]["code"], "daemon.restart_failed");
        assert_eq!(events[0]["params"]["detail"], "no systemd-run");
    }

    #[tokio::test]
    async fn restarting_while_recording_asks_for_force() {
        let f = fixture(None, FakeInstaller::default(), None);
        f.live.store(true, Ordering::SeqCst);
        f.updates.restart(false);
        assert_eq!(f.events.lock().unwrap()[0]["code"], "daemon.recording");
        assert_eq!(f.restarts(), 0);
        f.updates.restart(true);
        assert_eq!(f.restarts(), 1);
        f.live.store(false, Ordering::SeqCst);
        f.updates.restart(false);
        assert_eq!(f.restarts(), 2);
    }

    #[tokio::test]
    async fn an_install_restarts_into_the_installers_program() {
        let f = fixture(Some("v99.0.0"), FakeInstaller::default(), None);
        f.updates.install(None).unwrap().await;
        let restarted = f.fakes.restarted.lock().unwrap();
        assert_eq!(*restarted, [PathBuf::from(testing::INSTALLED)]);
    }

    #[tokio::test]
    async fn a_plain_restart_runs_the_running_executable() {
        let f = fixture(None, FakeInstaller::default(), None);
        f.updates.restart(false);
        let restarted = f.fakes.restarted.lock().unwrap();
        assert_eq!(*restarted, [std::env::current_exe().unwrap()]);
    }
}
