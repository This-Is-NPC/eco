//! ElevenLabs Scribe v2 Realtime (`wss://api.elevenlabs.io/v1/speech-to-text/realtime`):
//! PCM frames in, base64 in JSON; a phrase out each time its VAD commits one.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use std::time::Duration;

use futures::stream::BoxStream;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use crate::adapters::websocket::{self, Protocol, Reading};
use crate::ports::{Frame, Heard, Phrase, SAMPLE_RATE, StreamingSpeechToText, TranscriptionError};

/// The models Scribe streams with.
pub const MODELS: [&str; 1] = ["scribe_v2_realtime"];

pub struct ElevenLabsTranscriber {
    base_url: String,
    model: String,
    /// `None` lets Scribe detect the language ("auto").
    language: Option<String>,
    key: Option<String>,
}

impl ElevenLabsTranscriber {
    pub fn new(base_url: &str, model: &str, language: &str, key: Option<String>) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            language: (language != "auto").then(|| language.into()),
            key,
        }
    }

    fn url(&self) -> String {
        let language = self
            .language
            .as_ref()
            .map(|code| format!("&language_code={code}"))
            .unwrap_or_default();
        format!(
            "{}?model_id={}&audio_format=pcm_{SAMPLE_RATE}&commit_strategy=vad\
             &vad_silence_threshold_secs=0.6&include_timestamps=true{language}",
            self.base_url, self.model
        )
    }
}

fn chunk(frame: &[i16], commit: bool) -> Message {
    let bytes: Vec<u8> = frame.iter().flat_map(|s| s.to_le_bytes()).collect();
    let message = json!({
        "message_type": "input_audio_chunk", "audio_base_64": STANDARD.encode(bytes),
        "commit": commit, "sample_rate": SAMPLE_RATE,
    });
    Message::Text(message.to_string().into())
}

/// What a message from Scribe says.
fn read(text: &str) -> Result<Reading, String> {
    let message: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let kind = message["message_type"].as_str().unwrap_or("");
    if let Some(error) = message["error"].as_str() {
        return Err(format!("elevenlabs {kind}: {error}"));
    }
    if kind == "partial_transcript" {
        let words = message["text"].as_str().unwrap_or("").trim();
        return Ok(if words.is_empty() {
            Reading::Nothing
        } else {
            Reading::Partial(words.into())
        });
    }
    if kind != "committed_transcript_with_timestamps" {
        return Ok(Reading::Nothing);
    }
    let text = message["text"].as_str().unwrap_or("").trim();
    let words: Vec<&Value> = message["words"]
        .as_array()
        .map(|words| words.iter().filter(|w| w["type"] == "word").collect())
        .unwrap_or_default();
    let at = |word: Option<&&Value>, key| word.and_then(|w| w[key].as_f64());
    match (at(words.first(), "start"), at(words.last(), "end")) {
        (Some(start), Some(end)) if !text.is_empty() => Ok(Reading::Phrases(vec![Phrase {
            start,
            end,
            text: text.into(),
        }])),
        _ => Ok(Reading::Nothing),
    }
}

impl StreamingSpeechToText for ElevenLabsTranscriber {
    fn transcribe<'a>(
        &'a self,
        frames: BoxStream<'a, Frame>,
    ) -> BoxStream<'a, Result<Heard, TranscriptionError>> {
        let headers = self
            .key
            .iter()
            .map(|key| ("xi-api-key", key.clone()))
            .collect();
        let protocol = Protocol {
            audio: |pcm: &[i16]| chunk(pcm, false),
            // An empty chunk that commits: whatever was heard is transcribed.
            finish: vec![chunk(&[], true)],
            // Scribe keeps the session open; the commit's answer comes quickly.
            quiet: Duration::from_secs(2),
            read,
            request: None,
        };
        websocket::transcribe(self.url(), headers, protocol, frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_transcripts_are_timed_phrases() {
        let committed = json!({
            "message_type": "committed_transcript_with_timestamps", "text": " Bom dia. ",
            "words": [
                {"text": "Bom", "start": 0.5, "end": 0.7, "type": "word"},
                {"text": " ", "start": 0.7, "end": 0.75, "type": "spacing"},
                {"text": "dia.", "start": 0.75, "end": 1.1, "type": "word"},
            ],
        });
        let Reading::Phrases(phrases) = read(&committed.to_string()).unwrap() else {
            panic!("a phrase")
        };
        assert_eq!(
            phrases,
            [Phrase {
                start: 0.5,
                end: 1.1,
                text: "Bom dia.".into()
            }]
        );
        let partial = r#"{"message_type":"partial_transcript","text":" Bom di "}"#;
        assert!(matches!(read(partial).unwrap(), Reading::Partial(words) if words == "Bom di"));
        let quiet = r#"{"message_type":"partial_transcript","text":""}"#;
        assert!(matches!(read(quiet).unwrap(), Reading::Nothing));
        let refused = r#"{"message_type":"auth_error","error":"invalid key"}"#;
        assert_eq!(
            read(refused).err().unwrap(),
            "elevenlabs auth_error: invalid key"
        );
    }

    #[test]
    fn audio_goes_as_base64_pcm() {
        let Message::Text(text) = chunk(&[1, -1], true) else {
            panic!("text")
        };
        let message: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(message["audio_base_64"], "AQD//w==");
        assert_eq!(message["commit"], true);
        let stt = ElevenLabsTranscriber::new("wss://x/realtime", "scribe_v2_realtime", "pt", None);
        assert!(
            stt.url()
                .ends_with("include_timestamps=true&language_code=pt")
        );
    }
}
