//! Installing a release of eco over the installed one, the same for `eco
//! update` and the daemon's `update.install` (docs/design.md §13): the
//! release's SHA256SUMS names this platform's package, which is downloaded,
//! checked against its line there and handed to the platform's installer; a
//! package that does not match installs nothing. Also `eco update` and `eco
//! version` themselves.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::adapters::control_socket::Address;
use crate::adapters::releases_github::GitHub;
use crate::cli::{self, Failure};
use crate::ports::{InstallProgress, Installed, Installer, Release, ReleaseError, Releases};
use crate::session::update;

// The download the platforms' installers share; Android has no installer.
#[cfg(not(target_os = "android"))]
pub(crate) mod download;
#[cfg(not(target_os = "android"))]
pub use download::{DOWNLOADS, Scratch, fetch};

/// This eco's version.
const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// This platform's installer: none on Android, where the system installs the
/// app; the daemon says so.
#[cfg(target_os = "android")]
pub fn installer() -> Arc<dyn Installer> {
    Arc::new(crate::adapters::android::NoInstaller)
}

/// This platform's installer: the Arch package through pacman on Linux, the
/// Windows Installer package on Windows.
#[cfg(target_os = "linux")]
pub fn installer() -> Arc<dyn Installer> {
    Arc::new(crate::adapters::installer_linux::Pacman::new(DOWNLOADS))
}

/// This platform's installer: the folder `mise run install:macos` fills.
#[cfg(target_os = "macos")]
pub fn installer() -> Arc<dyn Installer> {
    Arc::new(crate::adapters::installer_macos::Folder::new(DOWNLOADS))
}

/// This platform's installer: the Arch package through pacman on Linux, the
/// Windows Installer package on Windows.
#[cfg(windows)]
pub fn installer() -> Arc<dyn Installer> {
    Arc::new(crate::adapters::installer_windows::Msi::new(DOWNLOADS))
}

/// Delete what an update left in the temporary directory because it was
/// running: on Windows, the copy of eco that installed it; nothing is left
/// behind elsewhere.
#[cfg(unix)]
pub fn tidy() {}

/// Delete what an update left in the temporary directory because it was
/// running: on Windows, the copy of eco that installed it; pacman leaves
/// nothing behind on Linux.
#[cfg(windows)]
pub fn tidy() {
    crate::adapters::installer_windows::tidy(&std::env::temp_dir());
}

/// A release's version: its tag without the `v`.
fn version_of(release: &Release) -> &str {
    release.tag.trim_start_matches('v')
}

/// What `eco update` says on stderr of `stage` of the install of `version`,
/// whether it or the daemon installs it.
pub fn said(stage: &str, version: &str) -> String {
    match stage {
        "verifying" => format!("eco: checking {version} against its SHA256SUMS"),
        stage => format!("eco: {stage} {version}"),
    }
}

/// `eco update` of the user's eco, against GitHub: through the user's daemon,
/// or here with this platform's installer.
pub async fn run(version: Option<String>, check: bool) -> Result<Value, Failure> {
    let releases = GitHub::new().map_err(unavailable)?;
    update(&Address::user(), &releases, &*installer(), version, check).await
}

/// `eco uninstall` of the user's eco: through the user's daemon, or here with
/// this platform's installer.
pub async fn run_uninstall(everything: bool) -> Result<Value, Failure> {
    uninstall(&Address::user(), &*installer(), everything).await
}

/// `eco version` of the user's eco and daemon, against GitHub.
pub async fn run_version() -> Result<Value, Failure> {
    let greeting = cli::greeting(&Address::user()).await.ok().flatten();
    let releases = GitHub::new().map_err(unavailable)?;
    Ok(version(greeting, &releases).await)
}

/// The releases could not be asked, saying why.
fn unavailable(error: ReleaseError) -> Failure {
    Failure::new("update.unavailable", error.0)
}

/// `eco update`: with `check`, whether a release newer than this eco is out;
/// else install release `version`, older ones too, or the latest when it is
/// newer. The running daemon at `address` installs it, so it restarts only
/// once no session records; with no daemon, `installer` does it here.
pub async fn update(
    address: &Address,
    releases: &dyn Releases,
    installer: &dyn Installer,
    version: Option<String>,
    check: bool,
) -> Result<Value, Failure> {
    let release = match &version {
        Some(version) => releases.tagged(update::tag(version)).await,
        None => releases.latest().await,
    }
    .map_err(unavailable)?;
    let newer = update::newer(CURRENT, &release.tag);
    if check {
        return Ok(json!({"current": CURRENT, "latest": version_of(&release),
                         "newer": newer, "notes": release.html_url}));
    }
    if version.is_none() && !newer {
        return Ok(
            json!({"current": CURRENT, "version": CURRENT, "installed": false, "restart": "none"}),
        );
    }
    let version = version_of(&release).to_string();
    let restart = match cli::update(address, &version).await? {
        Some(restart) => restart,
        None => {
            let said_of = version.clone();
            let progress: InstallProgress =
                Box::new(move |stage| eprintln!("{}", said(update::stage_name(stage), &said_of)));
            let failed = |error: String| Failure::new("update.failed", error);
            let installed = installer
                .install(release.tag.clone(), progress)
                .await
                .map_err(|error| failed(error.0))?;
            if let Installed::Pending(pending) = installed {
                eprintln!("{}", said("installing", &version));
                pending
                    .hand_off()
                    .map_err(|error| failed(error.to_string()))?;
            }
            "none"
        }
    };
    Ok(json!({"current": CURRENT, "version": version, "installed": true, "restart": restart}))
}

/// `eco uninstall`: remove eco, and with `everything` the user's files too.
/// The running daemon at `address` hands the removal off, so it refuses while
/// a session records; with no daemon, `installer` does it here. What it
/// removed is said once the removal runs; Windows Installer shows the rest.
pub async fn uninstall(
    address: &Address,
    installer: &dyn Installer,
    everything: bool,
) -> Result<Value, Failure> {
    if !cli::uninstall(address, everything).await? {
        installer
            .uninstall(everything)
            .hand_off()
            .map_err(|error| Failure::new("uninstall.failed", error.to_string()))?;
    }
    Ok(json!({"uninstalled": true, "data": if everything { "deleted" } else { "kept" }}))
}

/// `eco version`: this eco's version, the version of the daemon that greeted
/// with `greeting` when one runs, and the newest release: as that daemon last
/// checked it when it says, else asked of `releases`; `null` when unknown.
pub async fn version(greeting: Option<Vec<Value>>, releases: &dyn Releases) -> Value {
    let events = greeting.unwrap_or_default();
    let of_type = |kind: &str| events.iter().find(|event| event["type"] == kind);
    let daemon = of_type("daemon").and_then(|event| event["version"].as_str());
    let latest = match of_type("update") {
        Some(update) => update["latest"].as_str().map(String::from),
        None => releases
            .latest()
            .await
            .ok()
            .map(|release| version_of(&release).to_string()),
    };
    json!({"installed": CURRENT, "daemon": daemon, "latest": latest, "channel": "release"})
}

#[cfg(test)]
pub(crate) mod testing {
    //! Stand-ins for the releases.

    use futures::future::BoxFuture;

    use super::*;

    /// Releases where the latest is `latest`, and every tag is one; none at
    /// all when `latest` is `None`, as with the network down.
    pub struct FakeReleases {
        pub latest: Option<&'static str>,
    }

    fn release(tag: &str) -> Release {
        Release {
            tag: tag.into(),
            html_url: format!("https://github.com/This-Is-NPC/eco/releases/tag/{tag}"),
        }
    }

    impl Releases for FakeReleases {
        fn latest(&self) -> BoxFuture<'static, Result<Release, ReleaseError>> {
            let latest = self.latest;
            Box::pin(async move {
                latest
                    .map(release)
                    .ok_or_else(|| ReleaseError("cannot reach api.github.com".into()))
            })
        }

        fn tagged(&self, tag: String) -> BoxFuture<'static, Result<Release, ReleaseError>> {
            let up = self.latest.is_some();
            Box::pin(async move {
                up.then(|| release(&tag))
                    .ok_or_else(|| ReleaseError("cannot reach api.github.com".into()))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::io::AsyncWriteExt;

    use super::testing::FakeReleases;
    use super::*;
    use crate::session::update::testing::FakeInstaller;

    #[test]
    fn each_stage_is_said_with_the_version() {
        assert_eq!(said("downloading", "0.5.0"), "eco: downloading 0.5.0");
        assert_eq!(
            said("verifying", "0.5.0"),
            "eco: checking 0.5.0 against its SHA256SUMS"
        );
        assert_eq!(said("installing", "0.5.0"), "eco: installing 0.5.0");
    }

    /// No daemon answers here.
    fn nowhere(directory: &tempfile::TempDir) -> Address {
        Address::scratch(directory.path())
    }

    #[tokio::test]
    async fn check_says_whether_a_newer_release_is_out() {
        let directory = tempfile::tempdir().unwrap();
        let installer = FakeInstaller::default();
        let newer = FakeReleases {
            latest: Some("v999.0.0"),
        };
        let said = update(&nowhere(&directory), &newer, &installer, None, true)
            .await
            .unwrap();
        assert_eq!(
            said,
            json!({"current": CURRENT, "latest": "999.0.0", "newer": true,
                   "notes": "https://github.com/This-Is-NPC/eco/releases/tag/v999.0.0"})
        );
        let same = FakeReleases {
            latest: Some(concat!("v", env!("CARGO_PKG_VERSION"))),
        };
        let said = update(&nowhere(&directory), &same, &installer, None, true)
            .await
            .unwrap();
        assert_eq!(said["newer"], false);
        assert!(installer.installed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn with_the_network_down_nothing_is_checked_or_installed() {
        let directory = tempfile::tempdir().unwrap();
        let installer = FakeInstaller::default();
        let down = FakeReleases { latest: None };
        for check in [true, false] {
            let failure = update(&nowhere(&directory), &down, &installer, None, check)
                .await
                .unwrap_err();
            assert_eq!(
                failure,
                Failure::new("update.unavailable", "cannot reach api.github.com")
            );
        }
        assert!(installer.installed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn without_a_daemon_the_newer_release_is_installed_here() {
        let directory = tempfile::tempdir().unwrap();
        let installer = FakeInstaller::default();
        let newer = FakeReleases {
            latest: Some("v999.0.0"),
        };
        let said = update(&nowhere(&directory), &newer, &installer, None, false)
            .await
            .unwrap();
        assert_eq!(
            said,
            json!({"current": CURRENT, "version": "999.0.0", "installed": true, "restart": "none"})
        );
        assert_eq!(*installer.installed.lock().unwrap(), ["v999.0.0"]);
    }

    #[tokio::test]
    async fn a_running_daemon_installs_and_says_when_it_restarts() {
        use tokio::io::{AsyncBufReadExt, BufReader};

        use crate::adapters::{control_socket, local_socket};

        let directory = tempfile::tempdir().unwrap();
        let address = Address::scratch(directory.path());
        let listener = control_socket::FakeListener::bind(&address).await;
        tokio::spawn(async move {
            let stream = listener.accept().await.unwrap();
            let (reader, mut writer) = local_socket::split(stream);
            let mut lines = BufReader::new(reader).lines();
            let line = lines.next_line().await.unwrap().unwrap();
            assert_eq!(line, r#"update.install {"version":"999.0.0"}"#);
            let waiting = json!({"type": "update_progress", "stage": "waiting",
                                 "version": "999.0.0", "detail": ""});
            writer
                .write_all(format!("{waiting}\n").as_bytes())
                .await
                .unwrap();
            let _ = lines.next_line().await;
        });
        let installer = FakeInstaller::default();
        let newer = FakeReleases {
            latest: Some("v999.0.0"),
        };
        let said = update(&address, &newer, &installer, None, false)
            .await
            .unwrap();
        assert_eq!(said["restart"], "when_session_ends");
        assert!(installer.installed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_same_version_is_not_installed_again_unless_named() {
        let directory = tempfile::tempdir().unwrap();
        let installer = FakeInstaller::default();
        let same = FakeReleases {
            latest: Some(concat!("v", env!("CARGO_PKG_VERSION"))),
        };
        let said = update(&nowhere(&directory), &same, &installer, None, false)
            .await
            .unwrap();
        assert_eq!(said["installed"], false);
        assert!(installer.installed.lock().unwrap().is_empty());
        // An older release, named, is installed.
        let said = update(
            &nowhere(&directory),
            &same,
            &installer,
            Some("0.0.1".into()),
            false,
        )
        .await
        .unwrap();
        assert_eq!(said["version"], "0.0.1");
        assert_eq!(*installer.installed.lock().unwrap(), ["v0.0.1"]);
    }

    #[tokio::test]
    async fn without_a_daemon_a_pending_install_is_handed_off_here() {
        let directory = tempfile::tempdir().unwrap();
        let handed_off = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let installer = FakeInstaller {
            pending: Some(Arc::clone(&handed_off)),
            ..FakeInstaller::default()
        };
        let newer = FakeReleases {
            latest: Some("v999.0.0"),
        };
        let said = update(&nowhere(&directory), &newer, &installer, None, false)
            .await
            .unwrap();
        assert_eq!(said["installed"], true);
        assert_eq!(handed_off.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_failed_install_is_an_update_failure() {
        let directory = tempfile::tempdir().unwrap();
        let installer = FakeInstaller {
            fails: Some("pkexec is missing"),
            ..FakeInstaller::default()
        };
        let newer = FakeReleases {
            latest: Some("v999.0.0"),
        };
        let failure = update(&nowhere(&directory), &newer, &installer, None, false)
            .await
            .unwrap_err();
        assert_eq!(failure, Failure::new("update.failed", "pkexec is missing"));
    }

    #[tokio::test]
    async fn without_a_daemon_the_removal_is_handed_off_here() {
        let directory = tempfile::tempdir().unwrap();
        let installer = FakeInstaller::default();
        let said = uninstall(&nowhere(&directory), &installer, false)
            .await
            .unwrap();
        assert_eq!(said, json!({"uninstalled": true, "data": "kept"}));
        let said = uninstall(&nowhere(&directory), &installer, true)
            .await
            .unwrap();
        assert_eq!(said, json!({"uninstalled": true, "data": "deleted"}));
        assert_eq!(*installer.uninstalled.lock().unwrap(), [false, true]);
    }

    #[tokio::test]
    async fn a_removal_that_cannot_be_handed_off_is_a_failure() {
        let directory = tempfile::tempdir().unwrap();
        let installer = FakeInstaller {
            uninstall_fails: Some("no systemd-run"),
            ..FakeInstaller::default()
        };
        let failure = uninstall(&nowhere(&directory), &installer, true)
            .await
            .unwrap_err();
        assert_eq!(failure, Failure::new("uninstall.failed", "no systemd-run"));
    }

    #[tokio::test]
    async fn a_running_daemon_removes_and_the_installer_is_left_alone() {
        use tokio::io::{AsyncBufReadExt, BufReader};

        use crate::adapters::{control_socket, local_socket};

        let directory = tempfile::tempdir().unwrap();
        let address = Address::scratch(directory.path());
        let listener = control_socket::FakeListener::bind(&address).await;
        tokio::spawn(async move {
            let stream = listener.accept().await.unwrap();
            let (reader, mut writer) = local_socket::split(stream);
            let mut lines = BufReader::new(reader).lines();
            let line = lines.next_line().await.unwrap().unwrap();
            assert_eq!(line, r#"uninstall {"everything":true}"#);
            let removing = json!({"type": "uninstall", "stage": "removing",
                                  "everything": true, "detail": ""});
            writer
                .write_all(format!("{removing}\n").as_bytes())
                .await
                .unwrap();
            let _ = lines.next_line().await;
        });
        let installer = FakeInstaller::default();
        let said = uninstall(&address, &installer, true).await.unwrap();
        assert_eq!(said["data"], "deleted");
        assert!(installer.uninstalled.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn version_asks_the_daemon_first_and_the_releases_otherwise() {
        let releases = FakeReleases {
            latest: Some("v999.0.0"),
        };
        let greeting = vec![
            json!({"type": "daemon", "version": "0.3.0"}),
            json!({"type": "snapshot"}),
            json!({"type": "update", "latest": "0.5.0"}),
        ];
        assert_eq!(
            version(Some(greeting), &releases).await,
            json!({"installed": CURRENT, "daemon": "0.3.0", "latest": "0.5.0", "channel": "release"})
        );
        assert_eq!(
            version(None, &releases).await,
            json!({"installed": CURRENT, "daemon": null, "latest": "999.0.0", "channel": "release"})
        );
        // A daemon that never checked says so; an older one without updates is asked around.
        let unchecked = vec![
            json!({"type": "daemon", "version": CURRENT}),
            json!({"type": "update", "latest": null}),
        ];
        assert_eq!(
            version(Some(unchecked), &releases).await["latest"],
            json!(null)
        );
        let older = vec![json!({"type": "daemon", "version": "0.3.0"})];
        assert_eq!(version(Some(older), &releases).await["latest"], "999.0.0");
        let down = FakeReleases { latest: None };
        assert_eq!(version(None, &down).await["latest"], json!(null));
    }
}
