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
        let said = [Phrase {
            start: 0.5,
            end: 1.1,
            text: "Bom dia.".into(),
        }];
        assert!(
            matches!(read(&committed.to_string()).unwrap(), Reading::Phrases(phrases) if phrases == said)
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
        let message = chunk(&[1, -1], true);
        assert!(message.is_text());
        let message: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
        assert_eq!(message["audio_base_64"], "AQD//w==");
        assert_eq!(message["commit"], true);
        let stt = ElevenLabsTranscriber::new("wss://x/realtime", "scribe_v2_realtime", "pt", None);
        assert!(
            stt.url()
                .ends_with("include_timestamps=true&language_code=pt")
        );
    }

    #[test]
    fn what_is_not_a_timed_transcript_says_nothing() {
        assert!(read("not json").is_err());
        let started = r#"{"message_type":"session_started","session_id":"s"}"#;
        assert!(matches!(read(started).unwrap(), Reading::Nothing));
        let untimed = r#"{"message_type":"committed_transcript_with_timestamps","text":"Oi"}"#;
        assert!(matches!(read(untimed).unwrap(), Reading::Nothing));
        let blank = json!({
            "message_type": "committed_transcript_with_timestamps", "text": " ",
            "words": [{"text": " ", "start": 0.1, "end": 0.2, "type": "word"}],
        });
        assert!(matches!(
            read(&blank.to_string()).unwrap(),
            Reading::Nothing
        ));
    }

    /// Over a real WebSocket: the key in the handshake, audio as chunks, the
    /// commit last, and what Scribe answers back; Scribe keeps the session
    /// open, so the stream ends once it has been quiet.
    #[tokio::test]
    async fn a_stream_round_trips_through_a_websocket() {
        use futures::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut asked = String::new();
            // tungstenite fixes the callback's error type.
            #[allow(clippy::result_large_err)]
            let seen = |request: &Request, response: Response| {
                let key = request
                    .headers()
                    .get("xi-api-key")
                    .unwrap()
                    .to_str()
                    .unwrap();
                asked = format!("{} {key}", request.uri());
                Ok(response)
            };
            let mut socket = tokio_tungstenite::accept_hdr_async(socket, seen)
                .await
                .unwrap();
            let mut chunks = 0;
            while let Some(Ok(message)) = socket.next().await {
                // Pings carry no JSON.
                let chunk: Value = serde_json::from_slice(&message.into_data()).unwrap_or_default();
                chunks += usize::from(chunk["message_type"] == "input_audio_chunk");
                if chunk["commit"] == true {
                    break;
                }
            }
            let partial = json!({"message_type": "partial_transcript", "text": "Bom"});
            let committed = json!({
                "message_type": "committed_transcript_with_timestamps", "text": "Bom dia.",
                "words": [{"text": "Bom dia.", "start": 0.0, "end": 0.6, "type": "word"}],
            });
            for message in [partial, committed] {
                let text = message.to_string();
                socket.send(Message::Text(text.into())).await.unwrap();
            }
            // Held open, as Scribe does, until the client leaves.
            while let Some(Ok(_)) = socket.next().await {}
            (asked, chunks)
        });
        let stt = ElevenLabsTranscriber::new(
            &format!("ws://{address}/v1/speech-to-text/realtime"),
            "scribe_v2_realtime",
            "auto",
            Some("xi-key".into()),
        );
        let frames = futures::stream::iter(vec![vec![0i16; 512]]).boxed();
        let heard: Vec<Heard> = StreamingSpeechToText::transcribe(&stt, frames)
            .map(Result::unwrap)
            .collect()
            .await;
        assert_eq!(
            heard,
            [
                Heard::Partial("Bom".into()),
                Heard::Phrase(Phrase {
                    start: 0.0,
                    end: 0.6,
                    text: "Bom dia.".into()
                }),
            ]
        );
        let (asked, chunks) = server.await.unwrap();
        assert!(asked.ends_with("include_timestamps=true xi-key"), "{asked}");
        assert_eq!(chunks, 2);
    }
}
