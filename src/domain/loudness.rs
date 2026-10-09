//! How loud a frame is, for the input traces.

/// Soft log curve, so speech at normal volume reads clearly without clipping.
const CURVE: f64 = 1000.0;

/// How loud an int16 frame is, 0..1 on a log scale that keeps quiet speech visible.
pub fn loudness(frame: &[i16]) -> f64 {
    let mean_square = frame
        .iter()
        .map(|&s| (s as f64 / 32768.0).powi(2))
        .sum::<f64>()
        / frame.len().max(1) as f64;
    let level = (CURVE * mean_square.sqrt()).ln_1p() / CURVE.ln_1p();
    (level * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::FRAME_SAMPLES;

    #[test]
    fn silence_is_zero_and_full_scale_is_one() {
        assert_eq!(loudness(&[0; FRAME_SAMPLES]), 0.0);
        assert!(loudness(&[i16::MAX; FRAME_SAMPLES]) > 0.99);
    }

    #[test]
    fn quiet_speech_still_reads() {
        let level = loudness(&[330; FRAME_SAMPLES]); // about -40 dBFS
        assert!((0.3..0.4).contains(&level), "{level}");
    }
}
