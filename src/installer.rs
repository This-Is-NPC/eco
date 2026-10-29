//! Installing a release of eco over the installed one, the same for `eco
//! update` and the daemon's `update.install` (docs/design.md §13): the
//! release's SHA256SUMS names this platform's package, which is downloaded,
//! checked against its line there and handed to the platform's installer; a
//! package that does not match installs nothing. Also `eco update` and `eco
//! version` themselves.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use reqwest::header::USER_AGENT;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::adapters::control_socket::Address;
use crate::adapters::http::Endpoint;
use crate::adapters::releases_github::GitHub;
use crate::cli::{self, Failure};
use crate::ports::{
    InstallError, InstallProgress, InstallStage, Installed, Installer, Release, ReleaseError,
    Releases,
};
use crate::session::update;

/// Where GitHub serves the files of each release, under its tag.
pub const DOWNLOADS: &str = "https://github.com/This-Is-NPC/eco/releases/download";

/// This eco's version.
const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// What every download says it is; nothing of the user is sent.
const AGENT: &str = concat!("eco/", env!("CARGO_PKG_VERSION"));

/// The longest a download may send nothing.
const SILENCE: Duration = Duration::from_secs(30);

/// This platform's installer: the Arch package through pacman on Linux, the
/// Windows Installer package on Windows.
#[cfg(unix)]
pub fn installer() -> Arc<dyn Installer> {
    Arc::new(crate::adapters::installer_linux::Pacman::new(DOWNLOADS))
}

/// This platform's installer: the Arch package through pacman on Linux, the
/// Windows Installer package on Windows.
#[cfg(windows)]
pub fn installer() -> Arc<dyn Installer> {
    Arc::new(crate::adapters::installer_windows::Msi::new(DOWNLOADS))
}

/// Delete what an update left in the temporary directory because it was
/// running: on Windows, the copy of eco that installed it; pacman leaves
/// nothing behind on Linux.
#[cfg(unix)]
pub fn tidy() {}

/// Delete what an update left in the temporary directory because it was
/// running: on Windows, the copy of eco that installed it; pacman leaves
/// nothing behind on Linux.
#[cfg(windows)]
pub fn tidy() {
    crate::adapters::installer_windows::tidy(&std::env::temp_dir());
}

/// What the name of a [`Scratch`] directory starts with.
pub const SCRATCH: &str = "eco-update-";

/// A directory of its own under `parent`, deleted with what it holds when dropped.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(parent: &Path) -> Result<Self, InstallError> {
        let path = parent.join(format!("{SCRATCH}{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&path)
            .map_err(|error| InstallError(format!("cannot create {}: {error}", path.display())))?;
        Ok(Self(path))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// The directory, which stays where it is.
    #[cfg(windows)]
    pub fn keep(self) -> PathBuf {
        std::mem::ManuallyDrop::new(self).0.clone()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The checksum and name of the first line of `sums` that names a file
/// `is_package` takes: 64 hex digits, spaces, an optional `*`, the name. A
/// name with a path separator is never taken.
fn package_line(sums: &str, is_package: fn(&str) -> bool) -> Option<([u8; 32], String)> {
    sums.lines().find_map(|line| {
        let (digest, rest) = line.trim_end_matches('\r').split_once(' ')?;
        let name = rest.trim_start_matches(' ');
        let name = name.strip_prefix('*').unwrap_or(name);
        if name.contains(['/', '\\']) || !is_package(name) {
            return None;
        }
        Some((checksum(digest)?, name.to_string()))
    })
}

/// The 32 bytes the 64 hex digits `digest` write.
pub fn checksum(digest: &str) -> Option<[u8; 32]> {
    if digest.len() != 64 || !digest.is_ascii() {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (at, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&digest[at * 2..at * 2 + 2], 16).ok()?;
    }
    Some(bytes)
}

/// `path` of `endpoint`, or why it could not be had.
async fn get(endpoint: &Endpoint, path: &str) -> Result<reqwest::Response, InstallError> {
    let failed = |why: String| InstallError(format!("could not download {path}: {why}"));
    let reply = endpoint
        .get(path)
        .header(USER_AGENT, AGENT)
        .send()
        .await
        .map_err(|error| failed(endpoint.failure(&error, Duration::ZERO)))?;
    if !reply.status().is_success() {
        return Err(failed(reply.status().to_string()));
    }
    Ok(reply)
}

/// A package downloaded and checked against the release's SHA256SUMS.
#[derive(Debug)]
pub struct Fetched {
    pub path: PathBuf,
    /// Its SHA-256, which SHA256SUMS names.
    #[cfg_attr(
        all(unix, not(test)),
        expect(
            dead_code,
            reason = "only the Windows installer checks the package again before it installs"
        )
    )]
    pub sha256: [u8; 32],
}

/// Download release `tag`'s package — the file of the release's SHA256SUMS
/// whose name `is_package` takes — from `downloads` into `into`, checked
/// against its line there, telling `progress` each stage. A package that does
/// not match is deleted, and nothing may install it.
pub async fn fetch(
    downloads: &str,
    tag: &str,
    is_package: fn(&str) -> bool,
    into: &Path,
    progress: &InstallProgress,
) -> Result<Fetched, InstallError> {
    let endpoint =
        Endpoint::new(downloads, None, SILENCE).map_err(|error| InstallError(error.0))?;
    progress(InstallStage::Downloading);
    let sums = get(&endpoint, &format!("{tag}/SHA256SUMS"))
        .await?
        .text()
        .await
        .map_err(|error| InstallError(format!("could not read {tag}/SHA256SUMS: {error}")))?;
    let (expected, name) = package_line(&sums, is_package).ok_or_else(|| {
        InstallError(format!(
            "the SHA256SUMS of {tag} names no package for this system"
        ))
    })?;
    let path = into.join(&name);
    let found = match download(&endpoint, &format!("{tag}/{name}"), &path).await {
        Ok(found) => found,
        Err(error) => {
            let _ = tokio::fs::remove_file(&path).await;
            return Err(error);
        }
    };
    progress(InstallStage::Verifying);
    if found != expected {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(InstallError(format!(
            "{name} does not match the release's SHA256SUMS; nothing was installed"
        )));
    }
    Ok(Fetched {
        path,
        sha256: expected,
    })
}

/// Write `path` of `endpoint` to `file` as it arrives; its SHA-256.
async fn download(endpoint: &Endpoint, path: &str, file: &Path) -> Result<[u8; 32], InstallError> {
    let reply = get(endpoint, path).await?;
    let failed = |why: String| InstallError(format!("could not download {path}: {why}"));
    let mut out = tokio::fs::File::create(file)
        .await
        .map_err(|error| failed(error.to_string()))?;
    let mut hash = Sha256::new();
    let mut body = reply.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|error| failed(endpoint.failure(&error, Duration::ZERO)))?;
        hash.update(&chunk);
        out.write_all(&chunk)
            .await
            .map_err(|error| failed(error.to_string()))?;
    }
    out.flush()
        .await
        .map_err(|error| failed(error.to_string()))?;
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&hash.finalize());
    Ok(digest)
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
    use std::sync::{Arc, Mutex};

    use super::testing::FakeReleases;
    use super::*;
    use crate::adapters::http::testing::serve;
    use crate::session::update::testing::FakeInstaller;

    /// The SHA-256 of `abc`.
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn linux_package(name: &str) -> bool {
        name.starts_with("eco-") && name.ends_with(".pkg.tar.zst")
    }

    /// A progress that keeps the stages it is told.
    fn recorded() -> (InstallProgress, Arc<Mutex<Vec<InstallStage>>>) {
        let stages = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&stages);
        (
            Box::new(move |stage| kept.lock().unwrap().push(stage)),
            stages,
        )
    }

    #[test]
    fn the_package_line_is_the_first_whose_name_the_platform_takes() {
        let sums = format!(
            "{ABC}  eco-0.5.0-windows-x86_64.zip\n{ABC} *eco-0.5.0-1-x86_64.pkg.tar.zst\r\n{ABC}  eco-0.5.1-1-x86_64.pkg.tar.zst\n"
        );
        let (digest, name) = package_line(&sums, linux_package).unwrap();
        assert_eq!(name, "eco-0.5.0-1-x86_64.pkg.tar.zst");
        assert_eq!(Some(digest), checksum(ABC));
        assert_eq!(digest[..2], [0xba, 0x78]);
    }

    #[test]
    fn a_line_with_a_bad_digest_or_a_path_is_never_the_package() {
        let short = "abc  eco-0.5.0-1-x86_64.pkg.tar.zst";
        let odd = format!("{}  eco-0.5.0-1-x86_64.pkg.tar.zst", "zz".repeat(32));
        let path = format!("{ABC}  ../eco-0.5.0-1-x86_64.pkg.tar.zst");
        for sums in [short.to_string(), odd, path] {
            assert_eq!(package_line(&sums, linux_package), None, "{sums}");
        }
    }

    #[tokio::test]
    async fn a_package_that_matches_is_kept_after_each_stage() {
        let sums = format!("{ABC}  eco-0.5.0-1-x86_64.pkg.tar.zst\n").into_bytes();
        let (base, seen) = serve(vec![(200, None, sums), (200, None, b"abc".to_vec())]).await;
        let directory = tempfile::tempdir().unwrap();
        let (progress, stages) = recorded();
        let fetched = fetch(&base, "v0.5.0", linux_package, directory.path(), &progress)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&fetched.path).unwrap(), b"abc");
        assert_eq!(Some(fetched.sha256), checksum(ABC));
        assert_eq!(
            *stages.lock().unwrap(),
            [InstallStage::Downloading, InstallStage::Verifying]
        );
        let head = seen.lock().unwrap().head.clone();
        assert!(
            head.starts_with("GET /v1/v0.5.0/eco-0.5.0-1-x86_64.pkg.tar.zst "),
            "{head}"
        );
        assert!(head.contains(&format!("user-agent: {AGENT}")), "{head}");
    }

    #[tokio::test]
    async fn a_package_that_does_not_match_is_deleted() {
        let sums = format!("{ABC}  eco-0.5.0-1-x86_64.pkg.tar.zst\n").into_bytes();
        let (base, _) = serve(vec![(200, None, sums), (200, None, b"evil".to_vec())]).await;
        let directory = tempfile::tempdir().unwrap();
        let (progress, _) = recorded();
        let error = fetch(&base, "v0.5.0", linux_package, directory.path(), &progress)
            .await
            .unwrap_err();
        assert_eq!(
            error.0,
            "eco-0.5.0-1-x86_64.pkg.tar.zst does not match the release's SHA256SUMS; nothing was installed"
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn a_release_without_this_systems_package_or_its_sums_is_refused() {
        let sums = format!("{ABC}  eco-0.5.0-windows-x86_64.zip\n").into_bytes();
        let (base, _) = serve(vec![(200, None, sums)]).await;
        let directory = tempfile::tempdir().unwrap();
        let (progress, _) = recorded();
        let error = fetch(&base, "v0.5.0", linux_package, directory.path(), &progress)
            .await
            .unwrap_err();
        assert_eq!(
            error.0,
            "the SHA256SUMS of v0.5.0 names no package for this system"
        );

        let (base, _) = serve(vec![(404, None, Vec::new())]).await;
        let error = fetch(&base, "v9.9.9", linux_package, directory.path(), &progress)
            .await
            .unwrap_err();
        assert!(
            error
                .0
                .starts_with("could not download v9.9.9/SHA256SUMS: 404"),
            "{}",
            error.0
        );
    }

    #[test]
    fn each_stage_is_said_with_the_version() {
        assert_eq!(said("downloading", "0.5.0"), "eco: downloading 0.5.0");
        assert_eq!(
            said("verifying", "0.5.0"),
            "eco: checking 0.5.0 against its SHA256SUMS"
        );
        assert_eq!(said("installing", "0.5.0"), "eco: installing 0.5.0");
    }

    #[tokio::test]
    async fn a_package_that_cannot_be_downloaded_leaves_nothing() {
        let sums = format!("{ABC}  eco-0.5.0-1-x86_64.pkg.tar.zst\n").into_bytes();
        let (base, _) = serve(vec![(200, None, sums), (404, None, Vec::new())]).await;
        let directory = tempfile::tempdir().unwrap();
        let (progress, stages) = recorded();
        let error = fetch(&base, "v0.5.0", linux_package, directory.path(), &progress)
            .await
            .unwrap_err();
        assert!(
            error
                .0
                .starts_with("could not download v0.5.0/eco-0.5.0-1-x86_64.pkg.tar.zst: 404"),
            "{}",
            error.0
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        assert_eq!(*stages.lock().unwrap(), [InstallStage::Downloading]);
    }

    #[test]
    fn a_scratch_directory_goes_with_what_it_holds() {
        let directory = tempfile::tempdir().unwrap();
        let scratch = Scratch::new(directory.path()).unwrap();
        let path = scratch.path().to_path_buf();
        std::fs::write(path.join("package"), b"abc").unwrap();
        drop(scratch);
        assert!(!path.exists());
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
