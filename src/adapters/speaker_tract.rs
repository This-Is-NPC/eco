//! WeSpeaker speaker embeddings (CAM++ or ResNet34) through tract, over Kaldi
//! fbank features with the mean of each clip removed.

use std::path::Path;
use std::sync::Arc;

use tract_onnx::prelude::*;

use crate::adapters::fbank::{Fbank, MEL_BINS};
use crate::ports::{EmbeddingError, SpeakerEmbedder};

impl From<TractError> for EmbeddingError {
    fn from(error: TractError) -> Self {
        Self(format!("{error:#}"))
    }
}

/// One optimised plan for every clip length; `embed` may run from several threads.
pub struct TractEmbedder {
    plan: Arc<TypedRunnableModel>,
    fbank: Fbank,
}

impl TractEmbedder {
    pub fn load(path: &Path) -> Result<Self, EmbeddingError> {
        let mut model = tract_onnx::onnx().model_for_path(path)?;
        let frames = model.symbols.sym("T");
        model.set_input_fact(
            0,
            f32::fact([TDim::from(1), frames.into(), TDim::from(MEL_BINS)]).into(),
        )?;
        let plan = model.into_optimized()?.into_runnable()?;
        Ok(Self {
            plan,
            fbank: Fbank::new(),
        })
    }
}

impl SpeakerEmbedder for TractEmbedder {
    fn embed(&self, pcm: &[i16]) -> Result<Vec<f32>, EmbeddingError> {
        let rows = self.fbank.features(pcm);
        if rows.is_empty() {
            return Err(EmbeddingError("the clip is shorter than a frame".into()));
        }
        let mut mean = [0f32; MEL_BINS];
        for row in &rows {
            mean.iter_mut().zip(row).for_each(|(m, e)| *m += e);
        }
        mean.iter_mut().for_each(|m| *m /= rows.len() as f32);
        let features: Vec<f32> = rows
            .iter()
            .flat_map(|row| row.iter().zip(&mean).map(|(e, m)| e - m))
            .collect();
        let input = Tensor::from_shape(&[1, rows.len(), MEL_BINS], &features)?;
        let outputs = self.plan.run(tvec!(input.into()))?;
        let embedding = outputs[0].try_as_plain_ram()?.as_slice::<f32>()?.to_vec();
        Ok(embedding)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::paths;

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let norm = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (norm(a) * norm(b))
    }

    fn floats(value: &Value) -> Vec<f32> {
        let list = value.as_array().expect("a list");
        list.iter()
            .map(|x| x.as_f64().expect("a number") as f32)
            .collect()
    }

    /// Three AMI clips (ES2004a, CC BY 4.0) give the features and embeddings of
    /// the WeSpeaker pipeline (kaldi-native-fbank and ONNX Runtime).
    #[test]
    fn matches_the_wespeaker_pipeline() {
        let model = paths::speaker_model();
        assert!(
            model.exists(),
            "run `mise run setup` first: {} is missing",
            model.display()
        );
        let bytes = include_bytes!("../../tests/fixtures/speaker-clips.s16");
        let samples: Vec<i16> = bytes
            .chunks_exact(2)
            .map(|p| i16::from_le_bytes([p[0], p[1]]))
            .collect();
        let reference: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/speaker-reference.json"))
                .unwrap();
        let clips: Vec<&[i16]> = samples.chunks(samples.len() / 3).collect();

        let rows = Fbank::new().features(clips[0]);
        for (index, expected) in reference["fbank"].as_array().unwrap().iter().enumerate() {
            let worst = rows[index]
                .iter()
                .zip(floats(expected))
                .map(|(rust, python)| (rust - python).abs())
                .fold(0.0, f32::max);
            assert!(worst < 1e-3, "frame {index}: off by {worst}");
        }

        let embedder = TractEmbedder::load(&model).unwrap();
        let embeddings: Vec<Vec<f32>> = clips.iter().map(|c| embedder.embed(c).unwrap()).collect();
        for (index, expected) in reference["embeddings"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let similarity = cosine(&embeddings[index], &floats(expected));
            assert!(similarity > 0.999, "clip {index}: cosine {similarity}");
        }
        // Three people: each clip is further from the others than from itself.
        assert!(cosine(&embeddings[0], &embeddings[1]) < 0.5);
        assert!(cosine(&embeddings[1], &embeddings[2]) < 0.5);
    }
}
