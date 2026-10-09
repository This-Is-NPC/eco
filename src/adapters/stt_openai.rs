//! POST /audio/transcriptions: the LAN whisper.cpp server, Groq, or OpenAI.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::future::BoxFuture;
use reqwest::multipart::{Form, Part};
use serde_json::Value;

use crate::adapters::http::Endpoint;
use crate::ports::{Phrase, SAMPLE_RATE, SpeechToText, Transcript, TranscriptionError};

pub fn wav_bytes(pcm: &[i16]) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut buffer = Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(&mut buffer, spec).expect("writes to memory");
    for &sample in pcm {
        writer.write_sample(sample).expect("writes to memory");
    }
    writer.finalize().expect("writes to memory");
    buffer.into_inner()
}

pub struct OpenAITranscriber {
    endpoint: Endpoint,
    model: String,
    /// `None` lets the server detect the language of each segment ("auto").
    language: Option<String>,
    /// The server refused `verbose_json` once; ask it for plain text from then on.
    plain: AtomicBool,
}

/// Words joined by single spaces: whisper.cpp joins its segments with newlines.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The reply's phrases with their times, or its whole text as one phrase over the clip.
fn phrases(body: &Value, clip: f64) -> Result<Vec<Phrase>, String> {
    let timed = body
        .get("segments")
        .and_then(Value::as_array)
        .map(|segments| {
            segments
                .iter()
                .filter_map(|segment| {
                    let time = |key| segment.get(key).and_then(Value::as_f64);
                    let text = collapse(segment.get("text")?.as_str()?);
                    let start = time("start")?.clamp(0.0, clip);
                    let end = time("end")?.clamp(start, clip);
                    (!text.is_empty()).then_some(Phrase { start, end, text })
                })
                .collect::<Vec<_>>()
        });
    if let Some(timed) = timed.filter(|t| !t.is_empty()) {
        return Ok(timed);
    }
    let text = body
        .get("text")
        .and_then(Value::as_str)
        .ok_or("the reply has no text")?;
    let text = collapse(text);
    Ok(if text.is_empty() {
        Vec::new()
    } else {
        vec![Phrase {
            start: 0.0,
            end: clip,
            text,
        }]
    })
}

impl OpenAITranscriber {
    pub fn new(endpoint: Endpoint, model: &str, language: &str) -> Self {
        let language = (language != "auto").then(|| language.to_string());
        Self {
            endpoint,
            model: model.into(),
            language,
            plain: AtomicBool::new(false),
        }
    }

    async fn request(&self, pcm: &[i16]) -> Result<Vec<Phrase>, String> {
        loop {
            let plain = self.plain.load(Ordering::Relaxed);
            let file = Part::bytes(wav_bytes(pcm))
                .file_name("speech.wav")
                .mime_str("audio/wav")
                .map_err(|e| e.to_string())?;
            let format = if plain { "json" } else { "verbose_json" };
            let mut form = Form::new()
                .text("model", self.model.clone())
                .text("response_format", format);
            if let Some(language) = &self.language {
                form = form.text("language", language.clone());
            }
            let response = self
                .endpoint
                .post("audio/transcriptions")
                .multipart(form.part("file", file))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            // Models without segment timings (gpt-4o-transcribe) refuse verbose_json.
            if !plain && response.status() == reqwest::StatusCode::BAD_REQUEST {
                self.plain.store(true, Ordering::Relaxed);
                continue;
            }
            let body: Value = response
                .error_for_status()
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;
            return phrases(&body, pcm.len() as f64 / f64::from(SAMPLE_RATE));
        }
    }
}

impl SpeechToText for OpenAITranscriber {
    fn transcribe<'a>(
        &'a self,
        pcm: &'a [i16],
    ) -> BoxFuture<'a, Result<Transcript, TranscriptionError>> {
        Box::pin(async move {
            let phrases = self.request(pcm).await.map_err(TranscriptionError)?;
            Ok(Transcript {
                phrases,
                request: None,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::adapters::http::testing::serve_once;

    async fn transcribe(
        status: u16,
        reply: &str,
        language: &str,
    ) -> (Result<Vec<Phrase>, TranscriptionError>, Vec<u8>) {
        let (base, seen) = serve_once(status, reply.as_bytes().to_vec()).await;
        let endpoint = Endpoint::new(&base, None, Duration::from_secs(5)).unwrap();
        let stt = OpenAITranscriber::new(endpoint, "whisper-1", language);
        let result = stt.transcribe(&vec![0; SAMPLE_RATE as usize]).await;
        let result = result.map(|transcript| transcript.phrases);
        let body = seen.lock().unwrap().body.clone();
        (result, body)
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[tokio::test]
    async fn posts_wav_and_returns_text() {
        let (result, body) = transcribe(200, r#"{"text": " olá,\n tudo bem? \n"}"#, "pt").await;
        let whole = Phrase {
            start: 0.0,
            end: 1.0,
            text: "olá, tudo bem?".into(),
        };
        assert_eq!(result.unwrap(), [whole]);
        assert!(contains(&body, b"name=\"language\"\r\n\r\npt"));
        assert!(contains(
            &body,
            b"name=\"response_format\"\r\n\r\nverbose_json"
        ));
        let wav = body.windows(4).position(|w| w == b"RIFF").unwrap();
        let reader = hound::WavReader::new(Cursor::new(&body[wav..])).unwrap();
        assert_eq!(
            (reader.spec().sample_rate, reader.spec().channels),
            (SAMPLE_RATE, 1)
        );
    }

    #[tokio::test]
    async fn wraps_http_errors() {
        let (result, _) = transcribe(503, "busy", "pt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn auto_language_is_left_to_the_server() {
        let (result, body) = transcribe(200, r#"{"text": "hello"}"#, "auto").await;
        assert_eq!(result.unwrap()[0].text, "hello");
        assert!(!contains(&body, b"name=\"language\""));
    }

    #[tokio::test]
    async fn segments_become_timed_phrases_within_the_clip() {
        let reply = r#"{"text": "Oi. Tudo bem?", "segments": [
            {"start": 0.0, "end": 0.4, "text": " Oi."},
            {"start": 0.4, "end": 0.42, "text": "  "},
            {"start": 0.5, "end": 1.7, "text": " Tudo\n bem?"}
        ]}"#;
        let (result, _) = transcribe(200, reply, "pt").await;
        let phrase = |start, end, text: &str| Phrase {
            start,
            end,
            text: text.into(),
        };
        assert_eq!(
            result.unwrap(),
            [phrase(0.0, 0.4, "Oi."), phrase(0.5, 1.0, "Tudo bem?")]
        );
    }

    #[tokio::test]
    async fn a_server_refusing_segments_is_asked_for_text_from_then_on() {
        let (base, _) = serve_once(400, b"verbose_json unsupported".to_vec()).await;
        let endpoint = Endpoint::new(&base, None, Duration::from_secs(5)).unwrap();
        let stt = OpenAITranscriber::new(endpoint, "gpt-4o-transcribe", "pt");
        // The retry finds the one-shot server gone: still an error, but remembered.
        assert!(
            stt.transcribe(&vec![0; SAMPLE_RATE as usize])
                .await
                .is_err()
        );
        assert!(stt.plain.load(Ordering::Relaxed));
    }
}
