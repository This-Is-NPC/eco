//! `eco setup`: download the pinned models.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::adapters::http;
use crate::config;

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
        target: config::vad_model,
    },
    // WeSpeaker CAM++ trained on VoxCeleb (Apache-2.0), for telling speakers apart.
    Model {
        name: "WeSpeaker CAM++",
        url: "https://huggingface.co/Wespeaker/wespeaker-voxceleb-campplus/resolve/main/voxceleb_CAM%2B%2B.onnx",
        sha256: "b50810498b5bcf5773d086f6993d344476bd0c88b566a41e8d801aaf8461efad",
        target: config::speaker_model,
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
    let mut done = Vec::new();
    for model in &MODELS {
        let target = (model.target)();
        if fs::read(&target).is_ok_and(|bytes| digest(&bytes) == model.sha256) {
            done.push(format!("{} is up to date", target.display()));
            continue;
        }
        let bytes = http::client(None)?
            .get(model.url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        let found = digest(&bytes);
        if found != model.sha256 {
            bail!("{} checksum mismatch: {found}", model.name);
        }
        fs::create_dir_all(target.parent().context("model path has a parent")?)?;
        fs::write(&target, &bytes)?;
        done.push(format!("saved {}", target.display()));
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digests_are_lowercase_hex() {
        assert_eq!(
            digest(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
