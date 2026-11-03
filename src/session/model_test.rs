//! `model.test`: the smallest real request a registered model answers, told
//! as one `model_test` event: how long it took, or why it failed.

use std::time::Duration;

use futures::StreamExt;
use futures::stream;
use serde_json::{Value, json};
use tokio::time::Instant;

use super::{Stt, Unbuilt, model_key};
use crate::adapters::http::Endpoint;
use crate::adapters::llm_openai::OpenAIChat;
use crate::config::{Config, ModelType, Resolved};
use crate::domain::events::error;
use crate::ports::{Chunk, LanguageModel, Message, Refusal, SAMPLE_RATE};

/// How long a test waits for the provider; the HTTP client waits longer, so
/// this limit is the one that ends a stalled test.
const LIMIT: Duration = Duration::from_secs(15);

/// Why a test failed.
enum Failed {
    /// The model's provider could not be built.
    Unbuilt(Unbuilt),
    /// The provider answered with an error, `refusal` when it classifies.
    Answered(Option<Refusal>, String),
}

impl From<Unbuilt> for Failed {
    fn from(why: Unbuilt) -> Self {
        Self::Unbuilt(why)
    }
}

impl Failed {
    /// The error that tells it for `model`; the key and the reply's body are
    /// never in it, but for a failure that is not a refusal, whose `detail` is
    /// the transport's own words.
    fn told(self, model: &Resolved) -> Value {
        let name = &model.name;
        match self {
            Self::Unbuilt(why) => match model.kind {
                ModelType::Chat => why.model_unavailable(name),
                ModelType::Transcription => why.stt_unavailable(name),
            },
            Self::Answered(Some(Refusal::Credential), _) => match &model.secret {
                Some(secret) => error(
                    "model_test.credential",
                    format!(
                        "{name}: the provider refused the key of secret {}",
                        secret.name
                    ),
                    json!({"name": name, "secret": secret.name}),
                ),
                None => error(
                    "model_test.refused",
                    format!("{name}: the provider refused the request"),
                    json!({"name": name}),
                ),
            },
            Self::Answered(Some(Refusal::NotFound), _) => error(
                "model_test.model_missing",
                format!("{name}: the provider has no model {}", model.model),
                json!({"name": name, "model": model.model}),
            ),
            Self::Answered(None, detail) => error(
                "model_test.failed",
                format!("{name}: {detail}"),
                json!({"name": name, "detail": detail}),
            ),
        }
    }
}

/// A chat model answers a one-token completion.
async fn chat(model: &Resolved) -> Result<(), Failed> {
    let key = model_key(model).await?;
    let endpoint =
        Endpoint::new(&model.base_url, key, LIMIT * 2).map_err(|e| Unbuilt::from(e.0))?;
    let mut fields = model.request_fields();
    fields.insert("max_tokens".into(), json!(1));
    let asked = vec![Message {
        role: "user",
        content: "ok".into(),
        cache: false,
    }];
    let mut answer = OpenAIChat::new(endpoint, fields).stream(&model.model, asked);
    while let Some(chunk) = answer.next().await {
        match chunk {
            Ok(Chunk::Usage(_)) => {}
            Ok(_) => break,
            Err(why) => return Err(Failed::Answered(why.refusal, why.why)),
        }
    }
    Ok(())
}

/// A transcription model hears one second of silence made in memory: through
/// its batch request, or, when it only streams, by opening and closing a stream.
async fn transcription(model: &Resolved) -> Result<(), Failed> {
    let failed = |why: crate::ports::TranscriptionError| Failed::Answered(why.refusal, why.why);
    match Stt::build(model, "auto", true).await? {
        Stt::Segments(stt) => {
            let silence = vec![0i16; SAMPLE_RATE as usize];
            stt.transcribe(&silence).await.map(drop).map_err(failed)
        }
        Stt::Stream(stt) => {
            let mut heard = stt.transcribe(stream::empty().boxed());
            while let Some(next) = heard.next().await {
                next.map_err(failed)?;
            }
            Ok(())
        }
    }
}

fn told(name: &str, failure: &Value) -> Value {
    json!({
        "type": "model_test", "name": name, "ok": false,
        "code": failure["code"], "params": failure["params"], "message": failure["message"],
    })
}

/// Test the model `payload` names (`{"name"}`) of the saved `config`, presets
/// among its models, with its own secret and address.
pub(super) async fn run(payload: &str, config: &Config) -> Value {
    let request: Value = serde_json::from_str(payload).unwrap_or_default();
    let name = request["name"].as_str().unwrap_or_default();
    let Some(model) = config.model(name).and_then(|model| config.resolve(model)) else {
        let unknown = error(
            "model_test.unknown",
            format!("no model {name}"),
            json!({"name": name}),
        );
        return told(name, &unknown);
    };
    let started = Instant::now();
    let tested = tokio::time::timeout(LIMIT, async {
        match model.kind {
            ModelType::Chat => chat(&model).await,
            ModelType::Transcription => transcription(&model).await,
        }
    })
    .await;
    match tested {
        Ok(Ok(())) => json!({
            "type": "model_test", "name": name, "ok": true,
            "ms": u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        }),
        Ok(Err(failed)) => told(name, &failed.told(&model)),
        Err(_) => {
            let stalled = error(
                "model_test.timeout",
                format!("{name}: no answer within {} s", LIMIT.as_secs()),
                json!({"name": name, "seconds": LIMIT.as_secs()}),
            );
            told(name, &stalled)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::http::testing::serve;
    use crate::config;

    const CHAT: &[u8] = br#"{"choices":[{"message":{"content":"o"}}]}"#;

    /// A chat model "c" and a transcription model "t", each on its own
    /// provider at `base_url`, keyed by the secret "key" when `keyed`.
    fn config_at(base_url: &str, keyed: bool) -> Config {
        config::testing::set_env("ECO_TEST_MODEL_TEST_KEY", "sk-test");
        let secret = keyed.then_some("key");
        Config::from_value(json!({
            "secrets": [{"name": "key", "env": "ECO_TEST_MODEL_TEST_KEY"}],
            "providers": [{"name": "p", "base_url": base_url, "secret": secret}],
            "models": [
                {"name": "c", "type": "chat", "provider": "p", "model": "qwen", "reasoning": "low"},
                {"name": "t", "type": "transcription", "provider": "p", "model": "whisper-1"},
            ],
        }))
        .unwrap()
    }

    async fn test(config: &Config, name: &str) -> Value {
        run(&json!({"name": name}).to_string(), config).await
    }

    #[tokio::test]
    async fn a_chat_model_answers_one_token_and_the_time_is_told() {
        let (url, seen) = serve(vec![(200, Some("application/json"), CHAT.to_vec())]).await;
        let reply = test(&config_at(&url, true), "c").await;
        assert_eq!(reply["type"], "model_test");
        assert_eq!(reply["name"], "c");
        assert_eq!(reply["ok"], true);
        assert!(reply["ms"].is_u64());
        let seen = seen.lock().unwrap();
        assert!(seen.head.starts_with("POST /v1/chat/completions"));
        assert!(seen.head.contains("sk-test"));
        let body: Value = serde_json::from_slice(&seen.body).unwrap();
        assert_eq!(body["max_tokens"], 1);
        assert_eq!(body["model"], "qwen");
        assert_eq!(body["reasoning"], json!({"effort": "low"}));
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_transcription_model_hears_a_second_of_silence() {
        let reply = br#"{"text":""}"#.to_vec();
        let (url, seen) = serve(vec![(200, Some("application/json"), reply)]).await;
        let reply = test(&config_at(&url, true), "t").await;
        assert_eq!(reply["ok"], true);
        let seen = seen.lock().unwrap();
        assert!(seen.head.starts_with("POST /v1/audio/transcriptions"));
        let riff = seen.body.windows(4).position(|w| w == b"RIFF").unwrap();
        let data = riff + 36 + 8;
        assert_eq!(&seen.body[riff + 36..riff + 40], b"data");
        let clip = &seen.body[data..data + 2 * SAMPLE_RATE as usize];
        assert!(clip.iter().all(|byte| *byte == 0));
    }

    #[tokio::test]
    async fn a_chat_model_that_reports_usage_first_is_still_tested() {
        let body = b"data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1}}\n\n\
                     data: {\"choices\":[{\"delta\":{\"content\":\"o\"}}]}\n\n";
        let (url, _) = serve(vec![(200, Some("text/event-stream"), body.to_vec())]).await;
        let reply = test(&config_at(&url, true), "c").await;
        assert_eq!(reply["ok"], true);
    }

    #[tokio::test]
    async fn a_streaming_model_is_tested_by_opening_and_closing_a_stream() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/elevenlabs.io/stt", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            let message = socket.next().await.unwrap().unwrap();
            socket.close(None).await.unwrap();
            message.into_text().unwrap().to_string()
        });
        let reply = test(&config_at(&url, true), "t").await;
        assert_eq!(reply["ok"], true, "{reply}");
        // The last, empty chunk of audio is what the stream sends before it ends.
        assert!(server.await.unwrap().contains("audio_base_64"));
    }

    #[tokio::test]
    async fn a_streaming_model_that_refuses_the_key_names_its_secret() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/elevenlabs.io/stt", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let _ = socket.read(&mut [0; 2048]).await.unwrap();
            let reply = "HTTP/1.1 401 No\r\nContent-Length: 0\r\n\r\n";
            socket.write_all(reply.as_bytes()).await.unwrap();
        });
        let reply = test(&config_at(&url, true), "t").await;
        assert_eq!(reply["code"], "model_test.credential");
        assert_eq!(reply["params"], json!({"name": "t", "secret": "key"}));
    }

    #[tokio::test]
    async fn a_rejected_key_names_its_secret() {
        let (url, _) = serve(vec![(
            401,
            None,
            b"{\"error\":\"bad key sk-test\"}".to_vec(),
        )])
        .await;
        let reply = test(&config_at(&url, true), "c").await;
        assert_eq!(reply["ok"], false);
        assert_eq!(reply["code"], "model_test.credential");
        assert_eq!(reply["params"], json!({"name": "c", "secret": "key"}));
        assert!(!reply.to_string().contains("sk-test"));
    }

    #[tokio::test]
    async fn a_rejected_request_without_a_secret_is_refused() {
        let (url, _) = serve(vec![(403, None, Vec::new())]).await;
        let reply = test(&config_at(&url, false), "c").await;
        assert_eq!(reply["code"], "model_test.refused");
    }

    #[tokio::test]
    async fn a_model_the_provider_lacks_is_told_by_its_id() {
        let (url, _) = serve(vec![(404, None, b"no such model".to_vec())]).await;
        let reply = test(&config_at(&url, true), "t").await;
        assert_eq!(reply["code"], "model_test.model_missing");
        assert_eq!(reply["params"], json!({"name": "t", "model": "whisper-1"}));
    }

    #[tokio::test]
    async fn a_network_that_is_down_says_so_with_its_detail() {
        let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", closed.local_addr().unwrap());
        drop(closed);
        for name in ["c", "t"] {
            let reply = test(&config_at(&url, true), name).await;
            assert_eq!(reply["ok"], false);
            assert_eq!(reply["code"], "model_test.failed");
            assert!(!reply["params"]["detail"].as_str().unwrap().is_empty());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_provider_that_stalls_is_given_up_on_after_fifteen_seconds() {
        let stalled = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", stalled.local_addr().unwrap());
        let held = tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = stalled.accept().await {
                held.push(socket);
            }
        });
        let started = Instant::now();
        let reply = test(&config_at(&url, true), "c").await;
        assert_eq!(reply["code"], "model_test.timeout");
        assert_eq!(reply["params"], json!({"name": "c", "seconds": 15}));
        assert_eq!(started.elapsed(), LIMIT);
        held.abort();
    }

    #[tokio::test]
    async fn a_model_that_cannot_be_built_is_told_as_a_session_would() {
        let mut config = config_at("http://lan/v1", true);
        config.secrets[0].env = Some("ECO_TEST_MODEL_TEST_UNSET".into());
        assert_eq!(test(&config, "c").await["code"], "model.unavailable");
        assert_eq!(test(&config, "t").await["code"], "stt.unavailable");
    }

    #[tokio::test]
    async fn an_unknown_model_is_told_and_no_request_is_made() {
        let reply = test(&config_at("http://lan/v1", true), "nope").await;
        assert_eq!(reply["code"], "model_test.unknown");
        assert_eq!(reply["params"], json!({"name": "nope"}));
        let reply = run("not json", &config_at("http://lan/v1", true)).await;
        assert_eq!(reply["code"], "model_test.unknown");
    }
}
