//! Silero VAD through tract: a speech probability per frame.

use std::path::Path;
use std::sync::Arc;

use tract_onnx::prelude::*;

use crate::ports::{FRAME_SAMPLES, SAMPLE_RATE};

/// Silero v5+ expects the last 64 samples of the previous window prepended at 16 kHz.
const CONTEXT_SAMPLES: usize = 64;
const WINDOW: usize = CONTEXT_SAMPLES + FRAME_SAMPLES;
const STATE: [usize; 3] = [2, 1, 128];

#[derive(Debug, thiserror::Error)]
#[error("VAD: {0}")]
pub struct VadError(String);

impl From<TractError> for VadError {
    fn from(error: TractError) -> Self {
        Self(format!("{error:#}"))
    }
}

/// The optimised model, shared by every stream; the state is per stream.
#[derive(Clone)]
pub struct SileroModel(Arc<TypedRunnableModel>);

impl SileroModel {
    pub fn load(path: &Path) -> Result<Self, VadError> {
        // Inputs are (input, sr, state); a known sr lets tract resolve the
        // sample-rate branch.
        let plan = tract_onnx::onnx()
            .model_for_path(path)?
            .with_input_fact(1, tensor0(i64::from(SAMPLE_RATE)).into())?
            .into_optimized()?
            .into_runnable()?;
        Ok(Self(plan))
    }
}

/// Speech probability per frame; one instance per audio stream, since it keeps state.
pub struct SileroVad {
    model: SileroModel,
    state: Tensor,
    window: Vec<f32>,
}

impl SileroVad {
    pub fn new(model: SileroModel) -> Self {
        Self {
            model,
            state: Tensor::zero::<f32>(&STATE).expect("fixed shape"),
            window: vec![0.0; WINDOW],
        }
    }

    pub fn probability(&mut self, frame: &[i16]) -> Result<f32, VadError> {
        self.window.copy_within(FRAME_SAMPLES.., 0);
        for (slot, &sample) in self.window[CONTEXT_SAMPLES..].iter_mut().zip(frame) {
            *slot = sample as f32 / 32768.0;
        }
        let input = Tensor::from_shape(&[1, WINDOW], &self.window)?;
        let sr = tensor0(i64::from(SAMPLE_RATE));
        let mut outputs =
            self.model
                .0
                .run(tvec!(input.into(), sr.into(), self.state.clone().into()))?;
        let probability = *outputs[0].try_as_plain_ram()?.to_scalar::<f32>()?;
        self.state = outputs.remove(1).into_tensor();
        Ok(probability)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths;

    /// The same frames give the probabilities the Python daemon computed.
    #[test]
    fn matches_the_python_vad() {
        let model = paths::vad_model();
        let shown = model.display();
        assert!(
            model.exists(),
            "run `mise run setup` first: {shown} is missing"
        );
        let bytes = include_bytes!("../../tests/fixtures/vad-frames.s16");
        let samples: Vec<i16> = bytes
            .chunks_exact(2)
            .map(|p| i16::from_le_bytes([p[0], p[1]]))
            .collect();
        let expected: Vec<f32> =
            serde_json::from_str(include_str!("../../tests/fixtures/vad-python.json")).unwrap();
        let mut vad = SileroVad::new(SileroModel::load(&model).unwrap());
        for (index, (frame, python)) in samples
            .chunks_exact(FRAME_SAMPLES)
            .zip(expected)
            .enumerate()
        {
            let rust = vad.probability(frame).unwrap();
            assert!(
                (rust - python).abs() < 1e-4,
                "frame {index}: rust {rust} python {python}"
            );
        }
    }

    #[test]
    fn a_file_that_is_not_a_model_is_an_error() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "not onnx").unwrap();
        let error = SileroModel::load(file.path()).err().expect("not a model");
        assert!(error.to_string().starts_with("VAD: "), "{error}");
    }
}
