//! The platform's audio on Windows and macOS, through cpal: WASAPI endpoints or
//! Core Audio devices. An output is captured in loopback (what it plays: WASAPI
//! loopback, or a Core Audio process tap), and each device's own format becomes
//! the domain's frames through `audio_convert`. Neither offers eco echo
//! cancellation.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{DeviceDescription, DeviceDirection, DeviceId, ErrorKind, SampleFormat};
use futures::FutureExt;
use futures::future::BoxFuture;
use futures::stream::{self, BoxStream, StreamExt};
use tokio::sync::mpsc;

use crate::adapters::audio_convert::{Sample, ToFrames};
use crate::adapters::devices::{DEFAULT_INPUT, DEFAULT_OUTPUT, defaults, listed};
use crate::ports::{
    AudioDevices, AudioError, AudioSource, Device, EchoCancelling, EchoError, FRAME_SAMPLES, Frame,
    SAMPLE_RATE,
};

/// About 8 s of frames between the device's thread and the session.
const QUEUED_FRAMES: usize = 256;

/// About 2 s of frames: a microphone that gives nothing but zeros that long
/// from its opening is one the system keeps from eco, since a real one always
/// hears some noise. macOS hands such silence, and no error, to a process the
/// user has not let hear the microphone.
const REFUSED_AFTER: usize = 2 * SAMPLE_RATE as usize / FRAME_SAMPLES;

/// The audio of a Windows or macOS session.
pub struct Cpal;

impl AudioDevices for Cpal {
    fn list(&self) -> BoxFuture<'static, Vec<Device>> {
        async {
            // A device that cannot be described panics inside cpal: that listing
            // keeps the defaults alone.
            let found = tokio::task::spawn_blocking(endpoints).await;
            listed(found.unwrap_or_default())
        }
        .boxed()
    }

    fn capture(&self, device: &Device) -> Box<dyn AudioSource> {
        Box::new(CpalSource(device.clone()))
    }

    /// Nothing is cancelled: the default microphone as it is.
    fn capture_cancelled(&self) -> Box<dyn AudioSource> {
        Box::new(CpalSource(defaults().swap_remove(0)))
    }

    fn cancels_echo(&self) -> bool {
        false
    }

    fn cancel_echo(&self, _: &str) -> BoxFuture<'static, Result<EchoCancelling, EchoError>> {
        async { Err(EchoError::Unavailable) }.boxed()
    }
}

/// Every active device that is a microphone or an output.
fn endpoints() -> Vec<Device> {
    let Ok(devices) = cpal::default_host().devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|device| endpoint(device.id(), device.description()))
        .collect()
}

/// The device an endpoint is, by its id (a WASAPI endpoint id or a Core Audio
/// UID) and name; none when either cannot be read or it neither captures nor
/// plays. A device that does both is its microphone.
fn endpoint(
    id: Result<DeviceId, cpal::Error>,
    description: Result<DeviceDescription, cpal::Error>,
) -> Option<Device> {
    let description = description.ok()?;
    let kind = match description.direction() {
        DeviceDirection::Input | DeviceDirection::Duplex => "input",
        DeviceDirection::Output => "output",
        _ => return None,
    };
    Some(Device::new(id.ok()?.id(), description.name(), kind))
}

/// What the device's thread hands the session.
enum Heard {
    Frame(Frame),
    /// The stream must be opened again, on what the device is now.
    Reopen,
    /// The device is gone or failed: the source ends.
    Lost(AudioError),
}

/// What a stream error means for the source; none when the stream goes on.
fn heard(kind: ErrorKind, said: String) -> Option<Heard> {
    match kind {
        ErrorKind::DeviceChanged | ErrorKind::Xrun | ErrorKind::RealtimeDenied => None,
        // A default device that changed: open the new default.
        ErrorKind::StreamInvalidated => Some(Heard::Reopen),
        _ => Some(Heard::Lost(AudioError(said))),
    }
}

/// One device captured while its frames are read.
struct CpalSource(Device);

/// Whether a microphone just opened is kept from eco: it has given only zeros
/// for `REFUSED_AFTER` frames. Once it hears anything, it never is.
#[derive(Default)]
struct Refusal {
    silent: usize,
    heard: bool,
}

impl Refusal {
    /// Note `frame`; true once the microphone counts as refused.
    fn refused(&mut self, frame: &[i16]) -> bool {
        if self.heard {
            return false;
        }
        if frame.iter().any(|&sample| sample != 0) {
            self.heard = true;
            return false;
        }
        self.silent += 1;
        self.silent >= REFUSED_AFTER
    }
}

/// Why a refused microphone is not heard, and where the user lets eco hear it.
fn refused(device: &Device) -> AudioError {
    AudioError(format!(
        "{}: macOS lets eco hear only silence; allow eco in System Settings › Privacy & Security › Microphone",
        device.label
    ))
}

/// A playing stream and what it hears. `heard` is declared first so it drops
/// first: the stream's thread may wait on a full queue, and dropping the stream
/// waits for that thread.
struct Open {
    heard: mpsc::Receiver<Heard>,
    _stream: cpal::Stream,
    _silence: Option<cpal::Stream>,
    /// On macOS, a microphone's: whether the system keeps it from eco.
    refusal: Option<Refusal>,
}

enum State {
    Closed,
    Open(Box<Open>),
    Ended,
}

impl AudioSource for CpalSource {
    fn frames(&mut self) -> BoxStream<'_, Result<Frame, AudioError>> {
        let device = self.0.clone();
        stream::unfold(State::Closed, move |mut state| {
            let device = device.clone();
            async move {
                loop {
                    state = match state {
                        State::Ended => return None,
                        State::Closed => {
                            let target = device.clone();
                            let opened = tokio::task::spawn_blocking(move || open(&target)).await;
                            match opened {
                                Ok(Ok(open)) => State::Open(Box::new(open)),
                                Ok(Err(failure)) => return Some((Err(failure), State::Ended)),
                                Err(panicked) => {
                                    let failure =
                                        AudioError(format!("{}: {panicked}", device.label));
                                    return Some((Err(failure), State::Ended));
                                }
                            }
                        }
                        State::Open(mut open) => match open.heard.recv().await {
                            Some(Heard::Frame(frame)) => {
                                if open.refusal.as_mut().is_some_and(|r| r.refused(&frame)) {
                                    return Some((Err(refused(&device)), State::Ended));
                                }
                                return Some((Ok(frame), State::Open(open)));
                            }
                            Some(Heard::Reopen) => State::Closed,
                            Some(Heard::Lost(failure)) => {
                                return Some((Err(failure), State::Ended));
                            }
                            None => return None,
                        },
                    };
                }
            }
        })
        .boxed()
    }
}

/// Start capturing `device`, in its own format, as frames.
fn open(device: &Device) -> Result<Open, AudioError> {
    let failed = |error: cpal::Error| AudioError(format!("{}: {error}", device.label));
    let host = cpal::default_host();
    let endpoint = match device.id.as_str() {
        DEFAULT_INPUT => host.default_input_device(),
        DEFAULT_OUTPUT => host.default_output_device(),
        id => host.device_by_id(&DeviceId::new(host.id(), id)),
    }
    .ok_or_else(|| AudioError(format!("{}: not connected", device.label)))?;
    // An output is opened as an input and captured in loopback; on a device that
    // also captures, that would be its microphone instead.
    if device.kind == "output" && endpoint.supports_input() {
        return Err(AudioError(format!(
            "{}: what it plays cannot be captured, it is also a microphone",
            device.label
        )));
    }
    let supported = match device.kind.as_str() {
        "output" => endpoint.default_output_config(),
        _ => endpoint.default_input_config(),
    }
    .map_err(failed)?;
    let (sender, heard) = mpsc::channel(QUEUED_FRAMES);
    let config = supported.config();
    // WASAPI hands a loopback nothing while its output plays nothing; eco's own
    // silent stream on that output keeps silence arriving as frames. A Core Audio
    // tap hands silence on its own.
    let silence = match device.kind.as_str() {
        "output" if cfg!(windows) => {
            let silent = endpoint
                .build_output_stream_raw(
                    config,
                    supported.sample_format(),
                    // cpal fills the buffer with silence before this runs.
                    |_: &mut cpal::Data, _: &_| {},
                    |_| {},
                    None,
                )
                .map_err(failed)?;
            silent.play().map_err(failed)?;
            Some(silent)
        }
        _ => None,
    };
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&endpoint, config, sender),
        SampleFormat::I16 => build::<i16>(&endpoint, config, sender),
        SampleFormat::I32 => build::<i32>(&endpoint, config, sender),
        other => {
            return Err(AudioError(format!(
                "{}: samples as {other} are not supported",
                device.label
            )));
        }
    }
    .map_err(failed)?;
    stream.play().map_err(failed)?;
    Ok(Open {
        heard,
        _stream: stream,
        _silence: silence,
        refusal: (cfg!(target_os = "macos") && device.kind != "output").then(Refusal::default),
    })
}

fn build<T: cpal::SizedSample + Sample>(
    endpoint: &cpal::Device,
    config: cpal::StreamConfig,
    sender: mpsc::Sender<Heard>,
) -> Result<cpal::Stream, cpal::Error> {
    let mut to_frames = ToFrames::new(config.sample_rate, config.channels);
    let errors = sender.clone();
    endpoint.build_input_stream(
        config,
        move |data: &[T], _: &_| {
            for frame in to_frames.push(data) {
                // A session this far behind loses new audio; the device's thread never waits.
                let _ = sender.try_send(Heard::Frame(frame));
            }
        },
        move |error: cpal::Error| {
            if let Some(said) = heard(error.kind(), error.to_string()) {
                let _ = errors.blocking_send(said);
            }
        },
        None,
    )
}

#[cfg(test)]
mod tests {
    use cpal::DeviceDescriptionBuilder;

    use super::*;

    fn described(name: &str, direction: DeviceDirection) -> Result<DeviceDescription, cpal::Error> {
        Ok(DeviceDescriptionBuilder::new(name)
            .direction(direction)
            .build())
    }

    fn id(endpoint: &str) -> Result<DeviceId, cpal::Error> {
        Ok(DeviceId::new(cpal::default_host().id(), endpoint))
    }

    #[test]
    fn endpoints_are_devices_by_endpoint_id_and_friendly_name() {
        let mic = "{0.0.1.00000000}.{6a1c}";
        assert_eq!(
            endpoint(
                id(mic),
                described("Microphone (USB)", DeviceDirection::Input)
            ),
            Some(Device::new(mic, "Microphone (USB)", "input"))
        );
        let speakers = "{0.0.0.00000000}.{9f2e}";
        assert_eq!(
            endpoint(id(speakers), described("Speakers", DeviceDirection::Output)),
            Some(Device::new(speakers, "Speakers", "output"))
        );
        let headset = "AppleUSBAudioEngine:Headset:1";
        assert_eq!(
            endpoint(id(headset), described("Headset", DeviceDirection::Duplex)),
            Some(Device::new(headset, "Headset", "input"))
        );
        assert_eq!(
            endpoint(id(mic), described("?", DeviceDirection::Unknown)),
            None
        );
        let gone = || cpal::Error::from(ErrorKind::DeviceNotAvailable);
        assert_eq!(
            endpoint(Err(gone()), described("Mic", DeviceDirection::Input)),
            None
        );
        assert_eq!(endpoint(id(mic), Err(gone())), None);
    }

    #[test]
    fn a_changed_default_reopens_and_a_lost_device_ends_the_source() {
        let said = || "said".to_string();
        for kind in [
            ErrorKind::DeviceChanged,
            ErrorKind::Xrun,
            ErrorKind::RealtimeDenied,
        ] {
            assert!(heard(kind, said()).is_none(), "{kind:?}");
        }
        assert!(matches!(
            heard(ErrorKind::StreamInvalidated, said()),
            Some(Heard::Reopen)
        ));
        for kind in [ErrorKind::DeviceNotAvailable, ErrorKind::BackendError] {
            let Some(Heard::Lost(AudioError(lost))) = heard(kind, said()) else {
                panic!("{kind:?} goes on");
            };
            assert_eq!(lost, "said");
        }
    }

    #[tokio::test]
    async fn a_device_that_is_not_there_ends_the_source_with_why() {
        let gone = Device::new("{0.0.1.00000000}.{gone}", "Old headset", "input");
        let mut source = Cpal.capture(&gone);
        let frames: Vec<_> = source.frames().collect().await;
        let [Err(AudioError(said))] = &frames[..] else {
            panic!("{frames:?}");
        };
        assert_eq!(said, "Old headset: not connected");
    }

    #[test]
    fn a_microphone_that_gives_only_zeros_from_its_opening_is_refused() {
        let silent = vec![0; FRAME_SAMPLES];
        let mut refusal = Refusal::default();
        for _ in 1..REFUSED_AFTER {
            assert!(!refusal.refused(&silent));
        }
        assert!(refusal.refused(&silent));
    }

    #[test]
    fn a_microphone_that_heard_anything_is_never_refused() {
        let silent = vec![0; FRAME_SAMPLES];
        let mut noise = silent.clone();
        noise[100] = -3;
        let mut refusal = Refusal::default();
        assert!(!refusal.refused(&silent));
        assert!(!refusal.refused(&noise));
        for _ in 0..REFUSED_AFTER * 2 {
            assert!(!refusal.refused(&silent));
        }
    }

    #[test]
    fn a_refused_microphone_says_where_to_allow_it() {
        let said = refused(&Device::new(
            "BuiltInMicrophoneDevice",
            "MacBook Microphone",
            "input",
        ))
        .0;
        assert_eq!(
            said,
            "MacBook Microphone: macOS lets eco hear only silence; allow eco in System Settings › Privacy & Security › Microphone"
        );
    }

    #[tokio::test]
    async fn the_listing_starts_with_the_system_defaults() {
        assert_eq!(Cpal.list().await[..2], defaults()[..]);
    }

    #[tokio::test]
    async fn echo_cancellation_is_not_available() {
        assert!(!Cpal.cancels_echo());
        assert!(matches!(
            Cpal.cancel_echo(DEFAULT_INPUT).await,
            Err(EchoError::Unavailable)
        ));
    }

    /// Run by hand on a Windows machine: prints what eco lists, then what one
    /// second of each device gives: the defaults first with the default output
    /// silent, then every device while a tone plays on the default output.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "reads this machine's audio endpoints and captures from them"]
    async fn print_this_machines_devices_and_a_second_of_each() {
        let devices = Cpal.list().await;
        for device in &devices {
            println!("{:<6} {:<50} {}", device.kind, device.label, device.id);
        }
        // A tone on the default output while it is captured.
        let output = cpal::default_host().default_output_device().unwrap();
        let config = output.default_output_config().unwrap().config();
        let (rate, channels) = (config.sample_rate as f32, usize::from(config.channels));
        let mut n = 0_f32;
        let tone = output
            .build_output_stream(
                config,
                move |data: &mut [f32], _: &_| {
                    for frame in data.chunks_mut(channels) {
                        n += 1.0;
                        frame.fill((n * 440.0 * std::f32::consts::TAU / rate).sin() * 0.2);
                    }
                },
                |error| println!("tone: {error}"),
                None,
            )
            .unwrap();
        let silent = devices[..2].iter().map(|device| (device, false));
        let playing = devices.iter().map(|device| (device, true));
        for (device, playing) in silent.chain(playing) {
            if playing {
                tone.play().unwrap();
            }
            let mut source = Cpal.capture(device);
            let heard: Vec<_> = source
                .frames()
                .take_until(tokio::time::sleep(std::time::Duration::from_secs(1)))
                .collect()
                .await;
            let frames = heard.iter().filter(|frame| frame.is_ok()).count();
            let peak = heard
                .iter()
                .flatten()
                .flatten()
                .map(|s| s.unsigned_abs())
                .max();
            let failed: Vec<_> = heard
                .iter()
                .filter_map(|frame| frame.as_ref().err())
                .collect();
            let id = &device.id;
            println!("{id} (tone {playing}): {frames} frames, peak {peak:?}, {failed:?}");
        }
    }
}
