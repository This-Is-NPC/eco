//! A device's own audio — any rate, any channel count, float or integer
//! samples — turned into the domain's frames: mixed down to mono, resampled to
//! `SAMPLE_RATE` by a windowed-sinc filter, and cut into `FRAME_SAMPLES`, with
//! the remainder of each chunk carried into the next.

use std::f64::consts::PI;

use crate::ports::{FRAME_SAMPLES, Frame, SAMPLE_RATE};

/// A sample a device delivers.
pub trait Sample: Copy {
    /// The sample as a level in [-1, 1].
    fn level(self) -> f32;
}

impl Sample for f32 {
    fn level(self) -> f32 {
        self
    }
}

impl Sample for i16 {
    fn level(self) -> f32 {
        f32::from(self) / 32_768.0
    }
}

impl Sample for i32 {
    fn level(self) -> f32 {
        (f64::from(self) / 2_147_483_648.0) as f32
    }
}

/// Interleaved chunks of one device in, frames out.
pub struct ToFrames {
    channels: usize,
    /// None when the device already runs at `SAMPLE_RATE`.
    resampler: Option<Resampler>,
    mono: Vec<f32>,
    pending: Frame,
}

impl ToFrames {
    pub fn new(rate: u32, channels: u16) -> Self {
        let rate = rate.max(1);
        Self {
            channels: usize::from(channels.max(1)),
            resampler: (rate != SAMPLE_RATE).then(|| Resampler::new(rate, SAMPLE_RATE)),
            mono: Vec::new(),
            pending: Vec::with_capacity(FRAME_SAMPLES),
        }
    }

    /// The frames `interleaved` completes. A trailing sample that does not fill
    /// every channel is ignored: devices deliver whole sample frames.
    pub fn push<S: Sample>(&mut self, interleaved: &[S]) -> Vec<Frame> {
        let scale = 1.0 / self.channels as f32;
        self.mono.clear();
        self.mono.extend(
            interleaved
                .chunks_exact(self.channels)
                .map(|channels| channels.iter().map(|s| s.level()).sum::<f32>() * scale),
        );
        let resampled = match self.resampler.as_mut() {
            Some(resampler) => resampler.push(&self.mono),
            None => std::mem::take(&mut self.mono),
        };
        let mut frames = Vec::new();
        for level in &resampled {
            self.pending.push(to_i16(*level));
            if self.pending.len() == FRAME_SAMPLES {
                frames.push(std::mem::replace(
                    &mut self.pending,
                    Vec::with_capacity(FRAME_SAMPLES),
                ));
            }
        }
        if self.resampler.is_none() {
            self.mono = resampled;
        }
        frames
    }
}

fn to_i16(level: f32) -> i16 {
    (level * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16
}

/// Rational polyphase resampling: output `n` sits at input position
/// `n * down / up`, and is the input around it weighted by a Blackman-windowed
/// sinc whose cutoff sits below the lower rate's Nyquist, so nothing above it
/// folds back into the band.
struct Resampler {
    up: usize,
    down: usize,
    /// Taps on each side of an output's position.
    half: usize,
    /// One kernel of `2 * half` taps per phase `0..up`, each summing to 1.
    kernels: Vec<f32>,
    /// Input not yet behind every future output; starts with `half - 1` zeros.
    history: Vec<f32>,
    /// Index in `history` of the input sample at or before the next output.
    next: usize,
    /// The next output's position past `next`, in `1 / up` of an input sample.
    phase: usize,
}

impl Resampler {
    fn new(from: u32, to: u32) -> Self {
        let divisor = gcd(from, to);
        let (up, down) = ((to / divisor) as usize, (from / divisor) as usize);
        // The cutoff as a fraction of the input's Nyquist frequency.
        let cutoff = if to < from {
            0.9 * f64::from(to) / f64::from(from)
        } else {
            1.0
        };
        let half = (16.0 / cutoff).ceil() as usize;
        let mut kernels = Vec::with_capacity(up * 2 * half);
        for phase in 0..up {
            let offset = phase as f64 / up as f64;
            let taps: Vec<f64> = (0..2 * half)
                .map(|tap| {
                    let distance = tap as f64 + 1.0 - half as f64 - offset;
                    sinc(cutoff * distance) * blackman(distance / half as f64)
                })
                .collect();
            let sum: f64 = taps.iter().sum();
            kernels.extend(taps.iter().map(|tap| (tap / sum) as f32));
        }
        Self {
            up,
            down,
            half,
            kernels,
            history: vec![0.0; half - 1],
            next: half - 1,
            phase: 0,
        }
    }

    fn push(&mut self, input: &[f32]) -> Vec<f32> {
        self.history.extend_from_slice(input);
        let taps = 2 * self.half;
        let mut out = Vec::with_capacity(input.len() * self.up / self.down + 1);
        while self.next + self.half < self.history.len() {
            let start = self.next + 1 - self.half;
            let window = &self.history[start..start + taps];
            let kernel = &self.kernels[self.phase * taps..(self.phase + 1) * taps];
            out.push(window.iter().zip(kernel).map(|(x, h)| x * h).sum());
            self.phase += self.down;
            self.next += self.phase / self.up;
            self.phase %= self.up;
        }
        let consumed = self.next + 1 - self.half;
        self.history.drain(..consumed);
        self.next -= consumed;
        out
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

/// The Blackman window over `[-1, 1]`, zero outside it.
fn blackman(x: f64) -> f64 {
    if x.abs() >= 1.0 {
        return 0.0;
    }
    let t = PI * (x + 1.0);
    0.42 - 0.5 * t.cos() + 0.08 * (2.0 * t).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f64, seconds: f64, amplitude: f64) -> Vec<f32> {
        let count = (f64::from(rate) * seconds) as usize;
        (0..count)
            .map(|n| (amplitude * (2.0 * PI * hz * n as f64 / f64::from(rate)).sin()) as f32)
            .collect()
    }

    /// Every sample of `frames`, in order.
    fn samples(frames: &[Frame]) -> Vec<i16> {
        frames.concat()
    }

    /// The root mean square of `samples` past the filter's first few milliseconds.
    fn rms(samples: &[i16]) -> f64 {
        let settled = &samples[160..samples.len() - 160];
        let power: f64 = settled.iter().map(|s| f64::from(*s).powi(2)).sum();
        (power / settled.len() as f64).sqrt()
    }

    #[test]
    fn mono_i16_at_16_khz_passes_through_unchanged() {
        let input: Vec<i16> = (0..FRAME_SAMPLES as i16 * 2)
            .map(|n| n * 31 - 16_000)
            .collect();
        let frames = ToFrames::new(16_000, 1).push(&input);
        assert_eq!(frames, [&input[..FRAME_SAMPLES], &input[FRAME_SAMPLES..]]);
    }

    #[test]
    fn channels_are_averaged_into_one() {
        let stereo: Vec<i16> = [1_000, 3_000].repeat(FRAME_SAMPLES);
        assert_eq!(
            ToFrames::new(16_000, 2).push(&stereo),
            [vec![2_000; FRAME_SAMPLES]]
        );
        let six: Vec<f32> = [0.6, 0.0, 0.0, 0.0, 0.0, 0.0].repeat(FRAME_SAMPLES);
        assert_eq!(
            ToFrames::new(16_000, 6).push(&six),
            [vec![3_277; FRAME_SAMPLES]]
        );
    }

    #[test]
    fn floats_and_wide_integers_become_i16_clamped_at_full_scale() {
        let floats = [0.5_f32, -0.5, 1.0, -1.0, 1.5, -1.5, 0.0].repeat(FRAME_SAMPLES);
        let heard = samples(&ToFrames::new(16_000, 1).push(&floats));
        assert_eq!(
            heard[..7],
            [16_384, -16_384, 32_767, -32_768, 32_767, -32_768, 0]
        );
        let wide = [i32::MAX, i32::MIN, 1 << 30, 0].repeat(FRAME_SAMPLES);
        let heard = samples(&ToFrames::new(16_000, 1).push(&wide));
        assert_eq!(heard[..4], [32_767, -32_768, 16_384, 0]);
    }

    #[test]
    fn a_frame_is_never_short_and_the_rest_waits_for_the_next_chunk() {
        let mut to_frames = ToFrames::new(16_000, 1);
        assert!(to_frames.push(&[7_i16; FRAME_SAMPLES - 1]).is_empty());
        assert_eq!(to_frames.push(&[7_i16; 2]), [vec![7; FRAME_SAMPLES]]);
        assert_eq!(to_frames.pending, [7]);
    }

    #[test]
    fn chunks_of_any_size_make_the_frames_one_chunk_makes() {
        for (rate, channels) in [(48_000, 2), (44_100, 2), (44_100, 1), (8_000, 1)] {
            let input = sine(rate, 440.0, 0.5, 0.5).repeat(usize::from(channels));
            let whole = ToFrames::new(rate, channels).push(&input);
            let mut chunked = ToFrames::new(rate, channels);
            let mut frames = Vec::new();
            let sizes = [1, 7, 480, 441, 1_024, 3, 960].map(|size| size * usize::from(channels));
            let mut rest = &input[..];
            for size in sizes.iter().cycle() {
                if rest.is_empty() {
                    break;
                }
                let (chunk, after) = rest.split_at((*size).min(rest.len()));
                frames.extend(chunked.push(chunk));
                rest = after;
            }
            assert_eq!(frames, whole, "{rate} Hz, {channels} channels");
        }
    }

    #[test]
    fn common_rates_become_16_khz_keeping_the_speech_band() {
        for rate in [48_000, 44_100, 96_000, 22_050, 32_000, 8_000] {
            let input = sine(rate, 1_000.0, 2.0, 0.5);
            let heard = samples(&ToFrames::new(rate, 1).push(&input));
            // Two seconds at 16 kHz in whole frames, less the filter's half width
            // still held back.
            assert_eq!(heard.len(), 62 * FRAME_SAMPLES, "{rate}");
            // A 1 kHz sine at half scale keeps its level: 0.5 / √2 of full scale.
            let expected = 0.5 / 2_f64.sqrt() * 32_768.0;
            let level = rms(&heard);
            assert!((level / expected - 1.0).abs() < 0.01, "{rate}: {level}");
            // And its pitch: a zero crossing every 8 samples, two per cycle.
            let crossings = heard
                .windows(2)
                .filter(|w| (w[0] < 0) != (w[1] < 0))
                .count();
            let expected = heard.len() / 8;
            assert!(crossings.abs_diff(expected) <= 2, "{rate}: {crossings}");
        }
    }

    #[test]
    fn sound_above_8_khz_does_not_fold_back_into_the_band() {
        for (rate, hz) in [(48_000, 12_000.0), (44_100, 10_000.0), (48_000, 20_000.0)] {
            let heard = samples(&ToFrames::new(rate, 1).push(&sine(rate, hz, 1.0, 0.5)));
            // At least 60 dB below the 0.5 / √2 it came in at.
            let level = rms(&heard);
            assert!(level < 11.6, "{rate} Hz, {hz} Hz tone: {level}");
        }
    }

    #[test]
    fn silence_in_is_silence_out() {
        let heard = samples(&ToFrames::new(48_000, 2).push(&[0_f32; 48_000]));
        assert_eq!(heard.len(), 15 * FRAME_SAMPLES);
        assert!(heard.iter().all(|s| *s == 0));
    }
}
