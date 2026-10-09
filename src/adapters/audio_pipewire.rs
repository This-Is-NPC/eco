//! Capture from one PipeWire device through `pw-record`.

use futures::stream::BoxStream;
use tokio::process::Command;

use crate::adapters::pipe;
use crate::adapters::pipewire_devices::{DEFAULT_INPUT, DEFAULT_OUTPUT, Device};
use crate::ports::{AudioError, AudioSource, Frame, SAMPLE_RATE};

const CAPTURE_SINK: [&str; 2] = ["--properties", "{ stream.capture.sink = true }"];

/// Mono s16le at 16 kHz from one device, through a `pw-record` that lives as long
/// as the stream of frames.
pub struct PipeWireSource {
    args: Vec<String>,
}

impl PipeWireSource {
    pub fn new(device: &Device) -> Self {
        let mut args = Vec::new();
        if device.id != DEFAULT_INPUT && device.id != DEFAULT_OUTPUT {
            args.extend(["--target".into(), device.id.clone()]);
        }
        if device.kind == "output" {
            args.extend(CAPTURE_SINK.map(String::from));
        }
        Self { args }
    }
}

impl AudioSource for PipeWireSource {
    fn frames(&mut self) -> BoxStream<'_, Result<Frame, AudioError>> {
        let mut command = Command::new("pw-record");
        command
            .args([
                "--raw",
                &format!("--rate={SAMPLE_RATE}"),
                "--channels=1",
                "--format=s16",
            ])
            .args(&self.args)
            .arg("-");
        pipe::frames(command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::pipewire_devices::defaults;

    #[test]
    fn pw_record_arguments_per_device() {
        let sink: Vec<String> = CAPTURE_SINK.map(String::from).to_vec();
        assert!(PipeWireSource::new(&defaults()[0]).args.is_empty());
        assert_eq!(
            PipeWireSource::new(&Device::new(DEFAULT_OUTPUT, "", "output")).args,
            sink
        );
        assert_eq!(
            PipeWireSource::new(&Device::new("mic", "", "input")).args,
            ["--target", "mic"]
        );
        let mut speaker = vec!["--target".to_string(), "spk".into()];
        speaker.extend(sink);
        assert_eq!(
            PipeWireSource::new(&Device::new("spk", "", "output")).args,
            speaker
        );
    }
}
