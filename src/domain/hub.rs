//! Each input captured once, its audio handed to every transcriber reading it.

use std::collections::HashMap;

use futures::StreamExt;
use tokio::sync::broadcast::{self, error::RecvError};

use crate::domain::channel::{Captured, CapturedStream};

/// How many frames and segments wait for a slow reader before it skips ahead:
/// over two minutes of audio.
const BACKLOG: usize = 4096;

/// The captures of a pipeline's inputs, each read by any number of transcribers.
#[derive(Default)]
pub struct Hub {
    inputs: HashMap<String, broadcast::Receiver<Captured>>,
}

/// Where one input's capture hands over what it captured; dropping it ends
/// every reader of that input.
pub struct Capture(broadcast::Sender<Captured>);

impl Capture {
    pub fn send(&self, captured: Captured) {
        // No reader is no loss: the audio was only to be measured.
        let _ = self.0.send(captured);
    }
}

impl Hub {
    /// Take `input` in: what its capture sends reaches its readers.
    pub fn input(&mut self, input: &str) -> Capture {
        let (sender, receiver) = broadcast::channel(BACKLOG);
        self.inputs.insert(input.into(), receiver);
        Capture(sender)
    }

    /// What `input` captures from now on, until its capture ends; `None` for an
    /// input the hub does not have.
    pub fn read(&self, input: &str) -> Option<CapturedStream<'static>> {
        let receiver = self.inputs.get(input)?.resubscribe();
        let read = futures::stream::unfold(receiver, |mut receiver| async move {
            loop {
                match receiver.recv().await {
                    Ok(captured) => return Some((captured, receiver)),
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => return None,
                }
            }
        });
        Some(read.boxed())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use futures::stream::{self, BoxStream};

    use super::*;
    use crate::domain::channel::capture_channel;
    use crate::domain::segmenter::SegmenterConfig;
    use crate::ports::{AudioError, AudioSource, FRAME_SAMPLES, Frame};

    /// Speech between silences, and how many times it was opened.
    struct Counted {
        opened: Mutex<usize>,
    }

    impl AudioSource for Counted {
        fn frames(&mut self) -> BoxStream<'_, Result<Frame, AudioError>> {
            *self.opened.lock().unwrap() += 1;
            let audio = [vec![0; 20], vec![5000; 20], vec![0; 30]].concat();
            let frames = audio
                .into_iter()
                .map(|level| Ok(vec![level; FRAME_SAMPLES]));
            stream::iter(frames).boxed()
        }
    }

    fn loud(frame: &[i16]) -> Result<f32, AudioError> {
        Ok(if frame[0] > 1000 { 1.0 } else { 0.0 })
    }

    fn shape(captured: &[Captured]) -> Vec<String> {
        let one = |captured: &Captured| match captured {
            Captured::Frame(index, _) => format!("frame {index}"),
            Captured::Segment(segment, _) => format!("segment at {}", segment.start),
        };
        captured.iter().map(one).collect()
    }

    #[tokio::test]
    async fn one_capture_reaches_every_reader_alike() {
        let mut hub = Hub::default();
        let capture = hub.input("mic");
        let (first, second) = (hub.read("mic").unwrap(), hub.read("mic").unwrap());
        assert!(hub.read("speakers").is_none());
        let mut source = Counted {
            opened: Mutex::new(0),
        };
        let mut probability = loud;
        let config = SegmenterConfig::default();
        let captured = |captured| capture.send(captured);
        capture_channel(&mut source, &mut probability, &|_, _| {}, config, &captured)
            .await
            .unwrap();
        drop(capture);
        let (first, second): (Vec<_>, Vec<_>) = tokio::join!(first.collect(), second.collect());
        assert_eq!(*source.opened.lock().unwrap(), 1);
        assert_eq!(shape(&first), shape(&second));
        assert_eq!(first.len(), 70 + 1);
        assert!(shape(&first).iter().any(|item| item.starts_with("segment")));
    }

    #[tokio::test]
    async fn a_late_reader_hears_from_when_it_came() {
        let mut hub = Hub::default();
        let capture = hub.input("mic");
        capture.send(Captured::Frame(0, vec![0; FRAME_SAMPLES]));
        let late = hub.read("mic").unwrap();
        capture.send(Captured::Frame(1, vec![0; FRAME_SAMPLES]));
        drop(capture);
        assert_eq!(shape(&late.collect::<Vec<_>>().await), ["frame 1"]);
    }
}
