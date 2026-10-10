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
    use crate::adapters::http::testing::{serve, serve_once};

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
                    Message::Text(text) if text.contains("CloseStream") => break,
                    audio => bytes += audio.len(),
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
        assert!(
            matches!(&phrases[0], Ok(Heard::Phrase(said)) if said.text == "Bom dia."),
            "{phrases:?}"
        );
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

    /// A transcriber whose endpoints are the local server at `base` (`http://…/v1`).
    fn local(base: &str, key: Option<&str>) -> DeepgramTranscriber {
        let listen = format!("{}/listen", base.replacen("http://", "ws://", 1));
        DeepgramTranscriber::new(&listen, "nova-3", "pt", key.map(String::from))
    }

    #[tokio::test]
    async fn recorded_audio_is_posted_as_wav_with_the_key() {
        let reply = serde_json::json!({
            "metadata": {"request_id": "5f0c"},
            "results": {"utterances": [{"start": 0.0, "end": 0.5, "transcript": "Oi."}]},
        });
        let (base, seen) = serve_once(200, reply.to_string().into_bytes()).await;
        let stt = local(&base, Some("dg-key"));
        let heard = SpeechToText::transcribe(&stt, &[0; 160]).await.unwrap();
        assert_eq!(heard.phrases[0].text, "Oi.");
        assert_eq!(heard.request.as_deref(), Some("5f0c"));
        let seen = seen.lock().unwrap();
        assert!(
            seen.head
                .starts_with("POST /v1/listen?model=nova-3&language=pt&")
        );
        assert!(seen.head.contains("authorization: Token dg-key"));
        assert!(seen.head.contains("content-type: audio/wav"));
        assert!(seen.body.starts_with(b"RIFF"));
    }

    #[tokio::test]
    async fn recorded_audio_refused_or_garbled_is_an_error() {
        let (base, _) = serve_once(401, b"{}".to_vec()).await;
        let refused = SpeechToText::transcribe(&local(&base, None), &[0; 160]).await;
        let why = refused.unwrap_err().0;
        assert!(why.contains("401"), "{why}");
        let (base, _) = serve_once(200, b"<html>".to_vec()).await;
        let garbled = SpeechToText::transcribe(&local(&base, None), &[0; 160]).await;
        assert!(garbled.is_err());
    }

    #[tokio::test]
    async fn the_streaming_models_come_from_the_api_root() {
        let reply = serde_json::json!({"stt": [
            {"canonical_name": "nova-3", "streaming": true},
            {"canonical_name": "whisper", "streaming": false},
        ]});
        let (base, seen) = serve_once(200, reply.to_string().into_bytes()).await;
        let listen = format!("{}/listen", base.replacen("http://", "ws://", 1));
        assert_eq!(
            models(&listen, Some("dg-key".into())).await.unwrap(),
            ["nova-3"]
        );
        let head = seen.lock().unwrap().head.clone();
        assert!(head.starts_with("GET /v1/models "), "{head}");
        assert!(head.contains("authorization: Token dg-key"));
        let (base, _) = serve_once(403, b"{}".to_vec()).await;
        let listen = format!("{}/listen", base.replacen("http://", "ws://", 1));
        let refused = models(&listen, None).await.unwrap_err();
        assert!(refused.contains("403"), "{refused}");
        let (base, _) = serve_once(200, b"not json".to_vec()).await;
        let listen = format!("{}/listen", base.replacen("http://", "ws://", 1));
        assert!(models(&listen, None).await.is_err());
    }

    fn json(value: Value) -> (u16, Option<&'static str>, Vec<u8>) {
        (
            200,
            Some("application/json"),
            value.to_string().into_bytes(),
        )
    }

    fn status(code: u16, body: &str) -> (u16, Option<&'static str>, Vec<u8>) {
        (code, Some("application/json"), body.as_bytes().to_vec())
    }

    fn billing(base: &str) -> DeepgramBilling {
        DeepgramBilling::new(&format!("{base}/listen"), "dg-key".into())
    }

    #[tokio::test]
    async fn a_cost_is_read_under_the_keys_project_once_found() {
        let projects =
            serde_json::json!({"projects": [{"project_id": "p1"}, {"project_id": "p2"}]});
        let details =
            |usd: f64| serde_json::json!({"request": {"response": {"details": {"usd": usd}}}});
        let (base, seen) = serve(vec![
            json(projects),
            json(details(0.0075)),
            json(details(0.25)),
        ])
        .await;
        let billing = billing(&base);
        assert_eq!(billing.cost("r1").await.unwrap(), Some(0.0075));
        assert_eq!(billing.cost("r2").await.unwrap(), Some(0.25));
        let head = seen.lock().unwrap().head.clone();
        assert!(
            head.starts_with("GET /v1/projects/p1/requests/r2 "),
            "{head}"
        );
        assert!(head.contains("authorization: Token dg-key"));
    }

    #[tokio::test]
    async fn a_request_deepgram_does_not_know_yet_has_no_cost() {
        let projects = serde_json::json!({"projects": [{"project_id": "p1"}]});
        let (base, _) = serve(vec![
            json(projects),
            status(404, "{}"),
            status(400, "{}"),
            json(serde_json::json!({"request": {}})),
        ])
        .await;
        let billing = billing(&base);
        for request in ["r1", "r2", "r3"] {
            assert_eq!(billing.cost(request).await.unwrap(), None);
        }
    }

    #[tokio::test]
    async fn a_key_without_the_scope_is_told_apart_from_a_failure() {
        let (base, _) = serve(vec![status(403, r#"{"details":"missing usage:read"}"#)]).await;
        let forbidden = billing(&base).cost("r").await.unwrap_err();
        assert!(
            matches!(&forbidden, BillingError::Forbidden(why) if why == "deepgram 403 Forbidden: missing usage:read"),
            "{forbidden:?}"
        );
        let (base, _) = serve(vec![status(500, r#"{"message":"down"}"#)]).await;
        let failed = billing(&base).cost("r").await.unwrap_err();
        assert!(
            matches!(&failed, BillingError::Failed(why) if why == "deepgram 500 Internal Server Error: down"),
            "{failed:?}"
        );
        let (base, _) = serve(vec![status(502, "<html>")]).await;
        let failed = billing(&base).cost("r").await.unwrap_err();
        assert!(
            matches!(&failed, BillingError::Failed(why) if why == "deepgram 502 Bad Gateway: "),
            "{failed:?}"
        );
    }

    #[tokio::test]
    async fn a_key_in_no_project_or_an_unreadable_reply_fails() {
        let (base, _) = serve(vec![json(serde_json::json!({"projects": []}))]).await;
        let lonely = billing(&base).cost("r").await.unwrap_err();
        assert!(
            matches!(&lonely, BillingError::Failed(why) if why == "deepgram: the key belongs to no project"),
            "{lonely:?}"
        );
        let (base, _) = serve(vec![status(404, "{}")]).await;
        let unknown = billing(&base).cost("r").await.unwrap_err();
        assert!(matches!(unknown, BillingError::Failed(_)));
        let (base, _) = serve(vec![(200, None, b"not json".to_vec())]).await;
        assert!(matches!(
            billing(&base).cost("r").await,
            Err(BillingError::Failed(_))
        ));
        let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", closed.local_addr().unwrap());
        drop(closed);
        assert!(matches!(
            billing(&base).cost("r").await,
            Err(BillingError::Failed(_))
        ));
    }

    /// A Deepgram that names the request in its handshake, then sends `said`
    /// and closes once the audio is over.
    async fn deepgram(said: Vec<String>) -> String {
        use futures::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            // tungstenite fixes the callback's error type.
            #[allow(clippy::result_large_err)]
            let named = |_: &Request, mut response: Response| {
                let id = "req-42".parse().unwrap();
                response.headers_mut().insert("dg-request-id", id);
                Ok(response)
            };
            let mut socket = tokio_tungstenite::accept_hdr_async(socket, named)
                .await
                .unwrap();
            while let Some(Ok(message)) = socket.next().await {
                if matches!(&message, Message::Text(text) if text.contains("CloseStream")) {
                    break;
                }
            }
            for message in said {
                socket.send(Message::Text(message.into())).await.unwrap();
            }
            let _ = socket.close(None).await;
        });
        format!("ws://{address}/v1/listen")
    }

    async fn stream(said: Vec<String>) -> Vec<Result<Heard, TranscriptionError>> {
        use futures::StreamExt;
        let url = deepgram(said).await;
        let stt = DeepgramTranscriber::new(&url, "nova-3", "auto", Some("dg-key".into()));
        let frames = futures::stream::iter(vec![vec![0i16; 512]]).boxed();
        StreamingSpeechToText::transcribe(&stt, frames)
            .collect()
            .await
    }

    #[tokio::test]
    async fn a_stream_names_its_request_and_says_what_it_hears() {
        let interim = r#"{"type":"Results","is_final":false,"channel":{"alternatives":[{"transcript":"Bom"}]}}"#;
        let heard = stream(vec![
            r#"{"type":"SpeechStarted"}"#.into(),
            interim.into(),
            results("Bom dia.", &[(0.2, 0.9)], false),
            r#"{"type":"Metadata","request_id":"req-42"}"#.into(),
        ])
        .await;
        let heard: Vec<Heard> = heard.into_iter().map(Result::unwrap).collect();
        assert_eq!(
            heard,
            [
                Heard::Request("req-42".into()),
                Heard::Partial("Bom".into()),
                Heard::Partial("Bom dia.".into()),
                Heard::Phrase(Phrase {
                    start: 0.2,
                    end: 0.9,
                    text: "Bom dia.".into()
                }),
            ]
        );
    }

    #[tokio::test]
    async fn a_stream_deepgram_refuses_or_garbles_ends_with_an_error() {
        let refused = stream(vec![r#"{"type":"Error","description":"bad audio"}"#.into()]).await;
        let why = &refused.last().expect("a result").as_ref().unwrap_err().0;
        assert_eq!(why, "deepgram: bad audio");
        let garbled = stream(vec!["not json".into()]).await;
        assert!(matches!(garbled.last(), Some(Err(_))), "{garbled:?}");
    }

    #[test]
    fn finals_without_words_or_text_are_timed_by_the_message() {
        let mut held = Held::default();
        let silent = results("  ", &[], false);
        assert!(phrases(held.read(&silent).unwrap()).is_empty());
        let unworded = serde_json::json!({
            "type": "Results", "is_final": true, "speech_final": true,
            "start": 2.0, "duration": 1.5,
            "channel": {"alternatives": [{"transcript": "Oi."}]},
        });
        assert_eq!(
            phrases(held.read(&unworded.to_string()).unwrap()),
            [Phrase {
                start: 2.0,
                end: 3.5,
                text: "Oi.".into()
            }]
        );
        let ended = results("", &[], true);
        assert!(phrases(held.read(&ended).unwrap()).is_empty());
        let nameless = r#"{"type":"Error"}"#;
        assert_eq!(held.read(nameless).err().unwrap(), "deepgram: error");
        let untimed = serde_json::json!({"results": {"utterances": [{"transcript": "Oi"}]}});
        assert_eq!(
            transcript(&untimed),
            Transcript {
                phrases: vec![Phrase {
                    start: 0.0,
                    end: 0.0,
                    text: "Oi".into()
                }],
                request: None,
            }
        );
    }
}
