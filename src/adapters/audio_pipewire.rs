//! The platform's audio on PipeWire: devices from `pw-dump`, capture through
//! `pw-record`, echo cancellation through `pw-cli`.

use futures::FutureExt;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use tokio::process::Command;

use crate::adapters::pipewire_devices::{DEFAULT_INPUT, DEFAULT_OUTPUT, list_devices};
use crate::adapters::{echo_cancel, pipe};
use crate::ports::{
    AudioDevices, AudioError, AudioSource, Device, EchoCancelling, Frame, SAMPLE_RATE,
};

/// The audio of a PipeWire session.
pub struct PipeWire;

impl AudioDevices for PipeWire {
    fn list(&self) -> BoxFuture<'static, Vec<Device>> {
        list_devices().boxed()
    }

    fn capture(&self, device: &Device) -> Box<dyn AudioSource> {
        Box::new(PipeWireSource::new(device))
    }

    fn capture_cancelled(&self) -> Box<dyn AudioSource> {
        let node = Device::new(&echo_cancel::source_node(), "eco", "input");
        Box::new(PipeWireSource::new(&node))
    }

    fn cancel_echo(&self, mic: &str) -> BoxFuture<'static, Result<EchoCancelling, AudioError>> {
        let mic = mic.to_owned();
        async move {
            let module = echo_cancel::start(&mic).await?;
            Ok(Box::new(module) as EchoCancelling)
        }
        .boxed()
    }
}

const CAPTURE_SINK: [&str; 2] = ["--properties", "{ stream.capture.sink = true }"];

/// Mono s16le at 16 kHz from one device, through a `pw-record` that lives as long
/// as the stream of frames.
struct PipeWireSource {
    args: Vec<String>,
}

impl PipeWireSource {
    fn new(device: &Device) -> Self {
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
