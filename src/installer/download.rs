//! The download every installer shares: the release's SHA256SUMS names this
//! platform's package, which is fetched and checked against its line there.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt;
use reqwest::header::USER_AGENT;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::adapters::http::Endpoint;
use crate::ports::{InstallError, InstallProgress, InstallStage};

/// Where GitHub serves the files of each release, under its tag.
pub const DOWNLOADS: &str = "https://github.com/This-Is-NPC/eco/releases/download";

/// What every download says it is; nothing of the user is sent.
const AGENT: &str = concat!("eco/", env!("CARGO_PKG_VERSION"));

/// The longest a download may send nothing.
const SILENCE: Duration = Duration::from_secs(30);

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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::adapters::http::testing::serve;

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
}
