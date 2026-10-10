//! `eco setup`: download the pinned models and pick the desktop that loads
//! eco's rules.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use reqwest::Client;
use sha2::{Digest, Sha256};

use crate::adapters::desktop_hyprland::Hyprland;
use crate::adapters::http;
use crate::config;
use crate::paths;
use crate::ports::DesktopIntegration;

/// A model file eco downloads, pinned by checksum.
struct Model {
    name: &'static str,
    url: &'static str,
    sha256: &'static str,
    target: fn() -> PathBuf,
}

const MODELS: [Model; 2] = [
    // The export with declared input shapes and no nested sample-rate branches:
    // tract cannot type the branches of the default `silero_vad.onnx`.
    Model {
        name: "Silero VAD",
        url: "https://github.com/snakers4/silero-vad/raw/v6.2.3/src/silero_vad/data/silero_vad_op18_ifless.onnx",
        sha256: "7671cd04b004e9076da0d4a7b1a5aec36adf161c39230c1cb94a4fd5db6bbd28",
        target: paths::vad_model,
    },
    // WeSpeaker CAM++ trained on VoxCeleb (Apache-2.0), for telling speakers apart.
    Model {
        name: "WeSpeaker CAM++",
        url: "https://huggingface.co/Wespeaker/wespeaker-voxceleb-campplus/resolve/main/voxceleb_CAM%2B%2B.onnx",
        sha256: "b50810498b5bcf5773d086f6993d344476bd0c88b566a41e8d801aaf8461efad",
        target: paths::speaker_model,
    },
];

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Download each model unless the one in place already has the pinned checksum.
pub async fn run() -> Result<Vec<String>> {
    let client = http::client(None)?;
    let mut done = Vec::new();
    for model in &MODELS {
        let target = (model.target)();
        done.push(fetch(&client, model.name, model.sha256, model.url, &target).await?);
    }
    Ok(done)
}

/// Download `url` to `target` unless `target` already has `sha256`; nothing is
/// written when the download fails or has another checksum, and a crash while
/// writing leaves the old file or none, never half a model.
async fn fetch(
    client: &Client,
    name: &str,
    sha256: &str,
    url: &str,
    target: &Path,
) -> Result<String> {
    if fs::read(target).is_ok_and(|bytes| digest(&bytes) == sha256) {
        return Ok(format!("{} is up to date", target.display()));
    }
    let bytes = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let found = digest(&bytes);
    if found != sha256 {
        bail!("{name} checksum mismatch: {found}");
    }
    fs::create_dir_all(target.parent().context("model path has a parent")?)?;
    config::replace_file(target, &bytes, 0o644)?;
    Ok(format!("saved {}", target.display()))
}

/// The desktop `eco setup` loads eco's rules into.
pub fn desktop() -> impl DesktopIntegration {
    Hyprland::new(paths::hypr_bindings(), paths::shipped("hypr/eco.lua"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::http::testing::serve_once;

    /// The SHA-256 of `abc`.
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    async fn fetch_from(status: u16, body: &[u8], target: &Path) -> (Result<String>, String) {
        let (base, seen) = serve_once(status, body.to_vec()).await;
        let client = http::client(None).unwrap();
        let done = fetch(
            &client,
            "Test model",
            ABC,
            &format!("{base}/model.onnx"),
            target,
        )
        .await;
        let head = seen.lock().unwrap().head.clone();
        (done, head)
    }

    #[test]
    fn digests_are_lowercase_hex() {
        assert_eq!(
            digest(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[tokio::test]
    async fn a_missing_model_is_downloaded_into_a_new_directory() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("models/model.onnx");
        let (done, head) = fetch_from(200, b"abc", &target).await;
        assert_eq!(done.unwrap(), format!("saved {}", target.display()));
        assert!(head.starts_with("GET /v1/model.onnx "), "{head}");
        assert_eq!(fs::read(&target).unwrap(), b"abc");
    }

    #[tokio::test]
    async fn a_model_with_the_pinned_checksum_is_not_fetched_again() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("model.onnx");
        fs::write(&target, b"abc").unwrap();
        let (done, head) = fetch_from(200, b"abc", &target).await;
        assert_eq!(done.unwrap(), format!("{} is up to date", target.display()));
        assert!(head.is_empty(), "requested {head}");
    }

    #[tokio::test]
    async fn a_model_with_another_checksum_is_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("model.onnx");
        fs::write(&target, b"old").unwrap();
        let (done, head) = fetch_from(200, b"abc", &target).await;
        assert_eq!(done.unwrap(), format!("saved {}", target.display()));
        assert!(!head.is_empty());
        assert_eq!(fs::read(&target).unwrap(), b"abc");
    }

    #[tokio::test]
    async fn a_download_with_the_wrong_checksum_writes_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing.onnx");
        let (done, _) = fetch_from(200, b"evil", &missing).await;
        let error = done.unwrap_err().to_string();
        assert!(
            error.starts_with("Test model checksum mismatch: "),
            "{error}"
        );
        assert!(!missing.exists());

        let kept = directory.path().join("kept.onnx");
        fs::write(&kept, b"old").unwrap();
        let (done, _) = fetch_from(200, b"evil", &kept).await;
        assert!(done.is_err());
        assert_eq!(fs::read(&kept).unwrap(), b"old");
    }

    #[tokio::test]
    async fn an_http_error_is_reported_and_writes_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("model.onnx");
        for status in [404, 500] {
            let (done, _) = fetch_from(status, b"abc", &target).await;
            let error = done.unwrap_err().to_string();
            assert!(error.contains(&status.to_string()), "{error}");
            assert!(!target.exists());
        }
    }
}
