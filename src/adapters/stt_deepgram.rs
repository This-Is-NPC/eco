//! Deepgram (`wss://api.deepgram.com/v1/listen`): live, PCM frames in and a
//! phrase out each time Deepgram ends an utterance; for a file, each segment
//! posted to the same path over HTTPS, which runs faster than the audio. What
//! a stream cost comes from Deepgram's management API, by the request's id.

use std::sync::Arc;
use std::time::Duration;

use futures::future::BoxFuture;
use futures::stream::BoxStream;
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;

use crate::adapters::http;
use crate::adapters::stt_openai::wav_bytes;
use crate::adapters::websocket::{self, Protocol, Reading};
use crate::ports::{
    BillingError, Frame, Heard, Phrase, SAMPLE_RATE, SpeechToText, StreamingSpeechToText,
    Transcript, TranscriptionBilling, TranscriptionError,
};

pub struct DeepgramTranscriber {
    base_url: String,
    model: String,
    /// `None` lets Deepgram detect the language ("auto").
    language: Option<String>,
    key: Option<String>,
}

impl DeepgramTranscriber {
    pub fn new(base_url: &str, model: &str, language: &str, key: Option<String>) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            language: (language != "auto").then(|| language.into()),
            key,
        }
    }

    fn query(&self) -> String {
        let language = self.language.as_deref().unwrap_or("multi");
        format!(
            "model={}&language={language}&punctuate=true&smart_format=true",
            self.model
        )
    }

    fn url(&self) -> String {
        // Silence of 300 ms ends a phrase; 1 s ends it even when the words keep
        // coming in interim results.
        format!(
            "{}?{}&encoding=linear16&sample_rate={SAMPLE_RATE}&channels=1\
             &interim_results=true&endpointing=300&utterance_end_ms=1000",
            self.base_url,
            self.query()
        )
    }

    /// The same path over HTTPS, for recorded audio.
    fn files_url(&self) -> String {
        let https = self
            .base_url
            .replacen("wss://", "https://", 1)
            .replacen("ws://", "http://", 1);
        format!("{https}?{}&utterances=true", self.query())
    }

    async fn post(&self, pcm: &[i16]) -> Result<Transcript, String> {
        let client =
            http::client(Some(std::time::Duration::from_secs(60))).map_err(|e| e.to_string())?;
        let mut request = client
            .post(self.files_url())
            .header("Content-Type", "audio/wav")
            .body(wav_bytes(pcm));
        if let Some(key) = &self.key {
            request = request.header("Authorization", format!("Token {key}"));
        }
        let reply: Value = request
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        Ok(transcript(&reply))
    }
}

/// Deepgram's API root beside `base_url`, its `/listen` path left off.
fn api(base_url: &str) -> String {
    let https = base_url
        .replacen("wss://", "https://", 1)
        .replacen("ws://", "http://", 1);
    https.trim_end_matches("/listen").into()
}

/// The models Deepgram streams with, from `GET /v1/models` beside `base_url`.
pub async fn models(base_url: &str, key: Option<String>) -> Result<Vec<String>, String> {
    let url = format!("{}/models", api(base_url));
    let client =
        http::client(Some(std::time::Duration::from_secs(10))).map_err(|e| e.to_string())?;
    let mut request = client.get(url);
    if let Some(key) = key {
        request = request.header("Authorization", format!("Token {key}"));
    }
    let reply: Value = request
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(streaming_models(&reply))
}

fn streaming_models(reply: &Value) -> Vec<String> {
    let mut names: Vec<String> = reply["stt"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|model| model["streaming"] == true)
        .filter_map(|model| model["canonical_name"].as_str().map(String::from))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// A recorded-audio reply: its utterances as phrases, and the request Deepgram
/// billed it under.
fn transcript(reply: &Value) -> Transcript {
    let utterances = reply["results"]["utterances"].as_array();
    let phrases = utterances
        .into_iter()
        .flatten()
        .filter_map(|u| {
            let text = u["transcript"].as_str()?.trim();
            (!text.is_empty()).then(|| Phrase {
                start: u["start"].as_f64().unwrap_or(0.0),
                end: u["end"].as_f64().unwrap_or(0.0),
                text: text.into(),
            })
        })
        .collect();
    let request = reply["metadata"]["request_id"].as_str().map(String::from);
    Transcript { phrases, request }
}

impl SpeechToText for DeepgramTranscriber {
    fn transcribe<'a>(
        &'a self,
        pcm: &'a [i16],
    ) -> BoxFuture<'a, Result<Transcript, TranscriptionError>> {
        Box::pin(async move { self.post(pcm).await.map_err(TranscriptionError) })
    }
}

/// What Deepgram charged for a request, from its management API: the key's
/// project (`GET /v1/projects`, its first), then the request's
/// `response.details.usd`. A key without the `usage:read` scope cannot tell.
#[derive(Clone)]
pub struct DeepgramBilling {
    api: String,
    key: String,
    project: Arc<tokio::sync::OnceCell<String>>,
}

impl DeepgramBilling {
    pub fn new(base_url: &str, key: String) -> Self {
        Self {
            api: api(base_url),
            key,
            project: Arc::default(),
        }
    }

    /// `GET path` under the API root; `None` when Deepgram has no such thing (yet).
    /// A 403 is a key without the scope to read it.
    async fn get(&self, path: &str) -> Result<Option<Value>, BillingError> {
        let failed = |e: reqwest::Error| BillingError::Failed(e.to_string());
        let client = http::client(Some(Duration::from_secs(15))).map_err(failed)?;
        let reply = client
            .get(format!("{}{path}", self.api))
            .header("Authorization", format!("Token {}", self.key))
            .send()
            .await
            .map_err(failed)?;
        let status = reply.status();
        if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::BAD_REQUEST {
            return Ok(None);
        }
        if !status.is_success() {
            let body: Value = reply.json().await.unwrap_or_default();
            let why = body["details"].as_str().or(body["message"].as_str());
            let detail = format!("deepgram {status}: {}", why.unwrap_or(""));
            return Err(if status == reqwest::StatusCode::FORBIDDEN {
                BillingError::Forbidden(detail)
            } else {
                BillingError::Failed(detail)
            });
        }
        reply.json().await.map(Some).map_err(failed)
    }

    async fn request_cost(self, request: String) -> Result<Option<f64>, BillingError> {
        let project = self
            .project
            .get_or_try_init(|| async {
                let projects = self.get("/projects").await?.unwrap_or_default();
                let id = projects["projects"][0]["project_id"].as_str();
                id.map(String::from).ok_or_else(|| {
                    BillingError::Failed("deepgram: the key belongs to no project".into())
                })
            })
            .await?;
        let reply = self
            .get(&format!("/projects/{project}/requests/{request}"))
            .await?;
        Ok(reply.as_ref().and_then(usd))
    }
}

/// The cost a request reply gives, nested under `request` or not.
fn usd(reply: &Value) -> Option<f64> {
    let request = reply.get("request").unwrap_or(reply);
    request["response"]["details"]["usd"].as_f64()
}

impl TranscriptionBilling for DeepgramBilling {
    fn cost(&self, request: &str) -> BoxFuture<'static, Result<Option<f64>, BillingError>> {
        Box::pin(self.clone().request_cost(request.into()))
    }
}

/// Final results held until Deepgram ends the utterance.
#[derive(Default)]
struct Held {
    texts: Vec<String>,
    start: Option<f64>,
    end: f64,
}

impl Held {
    fn flush(&mut self) -> Vec<Phrase> {
        let held = std::mem::take(self);
        match held.start {
            Some(start) if !held.texts.is_empty() => vec![Phrase {
                start,
                end: held.end,
                text: held.texts.join(" "),
            }],
            _ => Vec::new(),
        }
    }

    /// The utterance so far: its final results, then `changing`.
    fn so_far(&self, changing: &str) -> Reading {
        let mut words: Vec<&str> = self.texts.iter().map(String::as_str).collect();
        words.extend(Some(changing).filter(|w| !w.is_empty()));
        if words.is_empty() {
            Reading::Nothing
        } else {
            Reading::Partial(words.join(" "))
        }
    }

    fn read(&mut self, text: &str) -> Result<Reading, String> {
        let message: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        match message["type"].as_str() {
            Some("Results") if message["is_final"] == true => {
                let alternative = &message["channel"]["alternatives"][0];
                let transcript = alternative["transcript"].as_str().unwrap_or("").trim();
                if !transcript.is_empty() {
                    let words = alternative["words"].as_array().cloned().unwrap_or_default();
                    let at = |word: Option<&Value>, key| word.and_then(|w| w[key].as_f64());
                    let start = message["start"].as_f64().unwrap_or(0.0);
                    let duration = message["duration"].as_f64().unwrap_or(0.0);
                    self.start
                        .get_or_insert(at(words.first(), "start").unwrap_or(start));
                    self.end = at(words.last(), "end").unwrap_or(start + duration);
                    self.texts.push(transcript.into());
                }
                Ok(if message["speech_final"] == true {
                    Reading::Phrases(self.flush())
                } else {
                    self.so_far("")
                })
            }
            // An interim result: what is held, and the words still changing.
            Some("Results") => {
                let alternative = &message["channel"]["alternatives"][0];
                Ok(self.so_far(alternative["transcript"].as_str().unwrap_or("").trim()))
            }
            // The utterance is over, or the stream is: whatever is held is a phrase.
            Some("UtteranceEnd" | "Metadata") => Ok(Reading::Phrases(self.flush())),
            Some("Error") => Err(format!(
                "deepgram: {}",
                message["description"].as_str().unwrap_or("error")
            )),
            _ => Ok(Reading::Nothing),
        }
    }
}

impl StreamingSpeechToText for DeepgramTranscriber {
    fn transcribe<'a>(
        &'a self,
        frames: BoxStream<'a, Frame>,
    ) -> BoxStream<'a, Result<Heard, TranscriptionError>> {
        let headers = self
            .key
            .iter()
            .map(|key| ("Authorization", format!("Token {key}")))
            .collect();
        let mut held = Held::default();
        let protocol = Protocol {
            audio: |pcm: &[i16]| {
                let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
                Message::Binary(bytes.into())
            },
            finish: vec![Message::Text(r#"{"type":"CloseStream"}"#.into())],
            // Deepgram closes once it has sent everything; a file sent fast
            // may keep it working a while first.
            quiet: Duration::from_secs(30),
            read: move |text: &str| held.read(text),
            request: Some("dg-request-id"),
        };
        websocket::transcribe(self.url(), headers, protocol, frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn results(transcript: &str, words: &[(f64, f64)], speech_final: bool) -> String {
        let words: Vec<Value> = words
            .iter()
            .map(|(start, end)| serde_json::json!({"word": "w", "start": start, "end": end}))
            .collect();
        serde_json::json!({
            "type": "Results", "is_final": true, "speech_final": speech_final,
            "start": 0.0, "duration": 1.0,
            "channel": {"alternatives": [{"transcript": transcript, "words": words}]},
        })
        .to_string()
    }

    fn phrases(reading: Reading) -> Vec<Phrase> {
        match reading {
            Reading::Phrases(phrases) => phrases,
            Reading::Partial(_) | Reading::Nothing => Vec::new(),
        }
    }

    #[test]
    fn finals_join_into_one_phrase_per_utterance() {
        let mut held = Held::default();
        let interim = r#"{"type":"Results","is_final":false,"channel":{"alternatives":[{"transcript":"Bom"}]}}"#;
        assert!(matches!(held.read(interim).unwrap(), Reading::Partial(w) if w == "Bom"));
        let first = results("Bom dia,", &[(1.2, 1.5), (1.5, 1.9)], false);
        assert!(matches!(held.read(&first).unwrap(), Reading::Partial(w) if w == "Bom dia,"));
        let next = r#"{"type":"Results","is_final":false,"channel":{"alternatives":[{"transcript":"tudo"}]}}"#;
        assert!(matches!(held.read(next).unwrap(), Reading::Partial(w) if w == "Bom dia, tudo"));
        let last = results("tudo bem?", &[(2.0, 2.3), (2.3, 2.8)], true);
        let said = phrases(held.read(&last).unwrap());
        assert_eq!(
            said,
            [Phrase {
                start: 1.2,
                end: 2.8,
                text: "Bom dia, tudo bem?".into()
            }]
        );
        let held_on = results("Oi", &[(4.0, 4.2)], false);
        held.read(&held_on).unwrap();
        let ended = r#"{"type":"UtteranceEnd","channel":[0],"last_word_end":4.2}"#;
        assert_eq!(phrases(held.read(ended).unwrap())[0].text, "Oi");
        assert!(phrases(held.read(ended).unwrap()).is_empty());
        let error = r#"{"type":"Error","description":"bad key"}"#;
        assert_eq!(held.read(error).err().unwrap(), "deepgram: bad key");
    }

    #[test]
    fn recorded_audio_comes_back_as_utterances_with_its_request() {
        let reply = serde_json::json!({
            "metadata": {"request_id": "5f0c"},
            "results": {"utterances": [
                {"start": 0.1, "end": 1.4, "transcript": "Bom dia."},
                {"start": 1.6, "end": 1.7, "transcript": " "},
            ]},
        });
        assert_eq!(
            transcript(&reply),
            Transcript {
                phrases: vec![Phrase {
                    start: 0.1,
                    end: 1.4,
                    text: "Bom dia.".into()
                }],
                request: Some("5f0c".into()),
            }
        );
        let stt =
            DeepgramTranscriber::new("wss://api.deepgram.com/v1/listen", "nova-3", "pt", None);
        assert_eq!(
            stt.files_url(),
            "https://api.deepgram.com/v1/listen?model=nova-3&language=pt&punctuate=true&smart_format=true&utterances=true"
        );
    }

    /// Over a real WebSocket: frames go out as binary, the closing message
    /// after them, and the phrase Deepgram ends comes back.
    #[tokio::test]
    async fn a_stream_round_trips_through_a_websocket() {
        use futures::{SinkExt, StreamExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            let mut bytes = 0;
            while let Some(Ok(message)) = socket.next().await {
                match message {
                    Message::Binary(audio) => bytes += audio.len(),
                    Message::Text(text) if text.contains("CloseStream") => break,
                    _ => {}
                }
            }
            let said = results("Bom dia.", &[(0.0, 0.5)], true);
            socket.send(Message::Text(said.into())).await.unwrap();
            socket.close(None).await.unwrap();
            bytes
        });
        let stt =
            DeepgramTranscriber::new(&format!("ws://{address}/v1/listen"), "nova-3", "pt", None);
        let frames = futures::stream::iter(vec![vec![0i16; 512]; 3]).boxed();
        let phrases: Vec<_> = StreamingSpeechToText::transcribe(&stt, frames)
            .collect()
            .await;
        assert_eq!(phrases.len(), 1);
        let Ok(Heard::Phrase(said)) = &phrases[0] else {
            panic!("a phrase")
        };
        assert_eq!(said.text, "Bom dia.");
        assert_eq!(server.await.unwrap(), 3 * 512 * 2);

        let nobody =
            DeepgramTranscriber::new(&format!("ws://{address}/v1/listen"), "nova-3", "pt", None);
        let silent = futures::stream::iter(Vec::<Frame>::new()).boxed();
        let failed: Vec<_> = StreamingSpeechToText::transcribe(&nobody, silent)
            .collect()
            .await;
        assert!(failed[0].is_err(), "nothing listens there any more");
    }

    #[test]
    fn a_request_costs_what_its_details_say() {
        let reply = serde_json::json!({"request": {"request_id": "r",
            "response": {"details": {"duration": 30, "usd": 0.0075}}}});
        assert_eq!(usd(&reply), Some(0.0075));
        assert_eq!(usd(&reply["request"]), Some(0.0075));
        assert_eq!(
            usd(&serde_json::json!({"request": {"response": null}})),
            None
        );
        assert_eq!(
            api("wss://api.deepgram.com/v1/listen"),
            "https://api.deepgram.com/v1"
        );
    }

    #[test]
    fn only_streaming_models_are_offered() {
        let reply = serde_json::json!({"stt": [
            {"canonical_name": "nova-3-general", "streaming": true},
            {"canonical_name": "nova-3-general", "streaming": true},
            {"canonical_name": "batch-only", "streaming": false},
        ]});
        assert_eq!(streaming_models(&reply), ["nova-3-general"]);
    }

    #[test]
    fn the_url_carries_the_stream_format() {
        let stt =
            DeepgramTranscriber::new("wss://api.deepgram.com/v1/listen", "nova-3", "auto", None);
        let url = stt.url();
        assert!(url.starts_with("wss://api.deepgram.com/v1/listen?model=nova-3&language=multi"));
        assert!(url.contains("encoding=linear16&sample_rate=16000&channels=1"));
    }
}
