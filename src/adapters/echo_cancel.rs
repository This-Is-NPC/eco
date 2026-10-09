//! PipeWire's echo cancellation (WebRTC) on one microphone, loaded into a
//! `pw-cli` that lives exactly as long as this value: dropping it removes the
//! module and its nodes. The reference is what the default output plays
//! (`monitor.mode`), so no application has to play through a new device.

use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};

use crate::adapters::pipewire_devices::DEFAULT_INPUT;
use crate::ports::AudioError;

/// Nodes eco creates for itself; never offered as devices.
pub const NODE_PREFIX: &str = "eco.aec.";

/// The echo-cancelled microphone this process captures from.
pub fn source_node() -> String {
    format!("{NODE_PREFIX}source.{}", std::process::id())
}

/// The module running; its `pw-cli` (and the stdin that keeps it reading) go with it.
pub struct EchoCancel {
    _process: Child,
    _input: ChildStdin,
}

fn arguments(mic: &str) -> String {
    let pid = std::process::id();
    let target = if mic == DEFAULT_INPUT {
        String::new()
    } else {
        format!(" target.object = \"{mic}\"")
    };
    format!(
        "{{ library.name = aec/libspa-aec-webrtc monitor.mode = true \
         capture.props = {{ node.name = \"{NODE_PREFIX}capture.{pid}\" node.passive = true{target} }} \
         source.props = {{ node.name = \"{}\" node.description = \"eco: echo cancelled\" }} \
         playback.props = {{ node.name = \"{NODE_PREFIX}playback.{pid}\" }} }}",
        source_node()
    )
}

async fn source_exists() -> bool {
    let listed = Command::new("pw-cli")
        .args(["ls", "Node"])
        .stderr(Stdio::null())
        .output()
        .await;
    listed.is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains(&source_node()))
}

/// Cancel, on `mic` (a node name or the default input), what the default
/// output plays; ready once its source exists.
pub async fn start(mic: &str) -> Result<EchoCancel, AudioError> {
    let fail = |detail: String| AudioError(format!("echo cancellation: {detail}"));
    let mut process = Command::new("pw-cli")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| fail(e.to_string()))?;
    let mut input = process.stdin.take().expect("piped");
    let line = format!(
        "load-module libpipewire-module-echo-cancel {}\n",
        arguments(mic)
    );
    input
        .write_all(line.as_bytes())
        .await
        .map_err(|e| fail(e.to_string()))?;
    for _ in 0..30 {
        if source_exists().await {
            return Ok(EchoCancel {
                _process: process,
                _input: input,
            });
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(fail("PipeWire did not create its source".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_module_is_aimed_at_the_microphone() {
        let pid = std::process::id();
        let chosen = arguments("alsa_input.usb-mic");
        assert!(chosen.contains("monitor.mode = true"));
        assert!(chosen.contains("target.object = \"alsa_input.usb-mic\""));
        assert!(chosen.contains(&format!("node.name = \"eco.aec.source.{pid}\"")));
        assert!(!arguments(DEFAULT_INPUT).contains("target.object"));
    }
}
