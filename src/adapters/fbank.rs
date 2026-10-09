//! Kaldi log mel filterbank features (`compute-fbank-feats` defaults with 80 bins,
//! no dither), the input speaker embedding models are trained on.

use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

use crate::ports::SAMPLE_RATE;

pub const MEL_BINS: usize = 80;
/// 25 ms frames every 10 ms at 16 kHz.
const FRAME_LENGTH: usize = 400;
const FRAME_SHIFT: usize = 160;
const FFT_SIZE: usize = 512;
const PREEMPHASIS: f32 = 0.97;
const LOW_HZ: f32 = 20.0;

fn mel(hz: f32) -> f32 {
    1127.0 * (1.0 + hz / 700.0).ln()
}

/// The precomputed window, mel weights and FFT plan; one serves every call.
pub struct Fbank {
    window: Vec<f32>,
    /// Per mel bin: the first FFT bin it covers and its weights from there.
    banks: Vec<(usize, Vec<f32>)>,
    fft: Arc<dyn Fft<f32>>,
}

impl Fbank {
    pub fn new() -> Self {
        let last = (FRAME_LENGTH - 1) as f32;
        let window = (0..FRAME_LENGTH)
            .map(|i| 0.54 - 0.46 * (2.0 * std::f32::consts::PI * i as f32 / last).cos())
            .collect();
        let (low, high) = (mel(LOW_HZ), mel(SAMPLE_RATE as f32 / 2.0));
        let step = (high - low) / (MEL_BINS + 1) as f32;
        let bin_hz = SAMPLE_RATE as f32 / FFT_SIZE as f32;
        let banks = (0..MEL_BINS)
            .map(|bin| {
                let left = low + bin as f32 * step;
                let (center, right) = (left + step, left + 2.0 * step);
                let weights: Vec<(usize, f32)> = (0..FFT_SIZE / 2)
                    .filter_map(|i| {
                        let m = mel(bin_hz * i as f32);
                        let weight = if m <= left || m >= right {
                            return None;
                        } else if m <= center {
                            (m - left) / (center - left)
                        } else {
                            (right - m) / (right - center)
                        };
                        Some((i, weight))
                    })
                    .collect();
                let first = weights.first().map_or(0, |w| w.0);
                (first, weights.into_iter().map(|w| w.1).collect())
            })
            .collect();
        Self {
            window,
            banks,
            fft: FftPlanner::new().plan_fft_forward(FFT_SIZE),
        }
    }

    /// One row of `MEL_BINS` log energies per complete frame of `pcm`.
    pub fn features(&self, pcm: &[i16]) -> Vec<[f32; MEL_BINS]> {
        if pcm.len() < FRAME_LENGTH {
            return Vec::new();
        }
        let frames = 1 + (pcm.len() - FRAME_LENGTH) / FRAME_SHIFT;
        let mut buffer = vec![Complex32::default(); FFT_SIZE];
        let mut scratch = vec![Complex32::default(); self.fft.get_inplace_scratch_len()];
        let mut frame = [0f32; FRAME_LENGTH];
        let mut power = [0f32; FFT_SIZE / 2];
        (0..frames)
            .map(|index| {
                let start = index * FRAME_SHIFT;
                for (slot, &sample) in frame.iter_mut().zip(&pcm[start..start + FRAME_LENGTH]) {
                    *slot = f32::from(sample);
                }
                let mean = frame.iter().sum::<f32>() / FRAME_LENGTH as f32;
                frame.iter_mut().for_each(|s| *s -= mean);
                for i in (1..FRAME_LENGTH).rev() {
                    frame[i] -= PREEMPHASIS * frame[i - 1];
                }
                frame[0] *= 1.0 - PREEMPHASIS;
                for (i, slot) in buffer.iter_mut().enumerate() {
                    let sample = frame.get(i).map_or(0.0, |s| s * self.window[i]);
                    *slot = Complex32::new(sample, 0.0);
                }
                self.fft.process_with_scratch(&mut buffer, &mut scratch);
                for (energy, value) in power.iter_mut().zip(&buffer) {
                    *energy = value.norm_sqr();
                }
                let mut row = [0f32; MEL_BINS];
                for (energy, (first, weights)) in row.iter_mut().zip(&self.banks) {
                    let sum: f32 = weights
                        .iter()
                        .zip(&power[*first..])
                        .map(|(w, p)| w * p)
                        .sum();
                    *energy = sum.max(f32::EPSILON).ln();
                }
                row
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_follow_kaldi_snip_edges() {
        let fbank = Fbank::new();
        assert!(fbank.features(&[0; FRAME_LENGTH - 1]).is_empty());
        assert_eq!(fbank.features(&[0; FRAME_LENGTH]).len(), 1);
        assert_eq!(fbank.features(&[0; 16_000]).len(), 98);
    }

    #[test]
    fn silence_floors_at_epsilon() {
        let rows = Fbank::new().features(&[0; FRAME_LENGTH]);
        assert!(rows[0].iter().all(|&e| e == f32::EPSILON.ln()));
    }

    #[test]
    fn a_tone_peaks_in_its_mel_bin() {
        let tone: Vec<i16> = (0..FRAME_LENGTH * 4)
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE as f32;
                (8000.0 * (2.0 * std::f32::consts::PI * 1000.0 * t).sin()) as i16
            })
            .collect();
        let rows = Fbank::new().features(&tone);
        let peak = (0..MEL_BINS)
            .max_by(|&a, &b| rows[0][a].total_cmp(&rows[0][b]))
            .unwrap();
        let (low, high) = (mel(LOW_HZ), mel(8000.0));
        let center = |bin: usize| low + (bin + 1) as f32 * (high - low) / (MEL_BINS + 1) as f32;
        let nearest = (0..MEL_BINS)
            .min_by(|&a, &b| {
                (center(a) - mel(1000.0))
                    .abs()
                    .total_cmp(&(center(b) - mel(1000.0)).abs())
            })
            .unwrap();
        assert!(
            peak.abs_diff(nearest) <= 1,
            "peak {peak}, 1 kHz is bin {nearest}"
        );
    }
}
