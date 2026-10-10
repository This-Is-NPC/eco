//! Streaming POST /chat/completions on any OpenAI-compatible API.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use futures::stream::{self, BoxStream, Stream, StreamExt, TryStreamExt};
use serde_json::{Map, Value, json};

use crate::adapters::http::Endpoint;
use crate::ports::{Chunk, CompletionError, LanguageModel, Message, Usage};

/// Providers whose prompt cache needs explicit breakpoints; the rest cache prefixes on their own.
const MARKED_CACHE: [&str; 2] = ["anthropic/", "claude"];

/// A message as the API takes it: the cache mark becomes cache_control where needed.
fn wire(message: &Message, marked: bool) -> Value {
    let content = if message.cache && marked {
        json!([{"type": "text", "text": message.content, "cache_control": {"type": "ephemeral"}}])
    } else {
        json!(message.content)
    };
    json!({"role": message.role, "content": content})
}

fn usage(data: &Value) -> Usage {
    Usage {
        prompt_tokens: data
            .get("prompt_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cached_tokens: data
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cost_usd: data.get("cost").and_then(Value::as_f64),
    }
}

/// The chunks one SSE line carries.
fn parse_line(line: &str) -> Result<Vec<Chunk>, CompletionError> {
    let Some(data) = line.strip_prefix("data: ") else {
        return Ok(Vec::new());
    };
    if data == "[DONE]" {
        return Ok(Vec::new());
    }
    let event: Value =
        serde_json::from_str(data).map_err(|error| CompletionError(error.to_string()))?;
    let mut chunks = Vec::new();
    let delta = |key: &str| {
        event
            .pointer(&format!("/choices/0/delta/{key}"))
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
    };
    // llama.cpp and LM Studio call the reasoning `reasoning_content`; OpenRouter `reasoning`.
    if let Some(text) = delta("reasoning_content").or_else(|| delta("reasoning")) {
        chunks.push(Chunk::Thinking(text.into()));
    }
    if let Some(text) = delta("content") {
        chunks.push(Chunk::Text(text.into()));
    }
    if let Some(data) = event.get("usage").filter(|u| u.is_object()) {
        chunks.push(Chunk::Usage(usage(data)));
    }
    Ok(chunks)
}

/// Server-sent events into chunks, whole lines only, however the bytes are cut.
fn sse<S, E>(bytes: S) -> impl Stream<Item = Result<Chunk, CompletionError>>
where
    S: Stream<Item = Result<bytes::Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    stream::unfold(
        (bytes, Vec::<u8>::new(), VecDeque::<Chunk>::new(), false),
        |(mut bytes, mut buffer, mut ready, mut ended)| async move {
            loop {
                if let Some(chunk) = ready.pop_front() {
                    return Some((Ok(chunk), (bytes, buffer, ready, ended)));
                }
                if let Some(end) = buffer.iter().position(|&b| b == b'\n') {
                    let line: Vec<u8> = buffer.drain(..=end).collect();
                    let line = String::from_utf8_lossy(&line);
                    match parse_line(line.trim_end_matches(['\r', '\n'])) {
                        Ok(chunks) => ready.extend(chunks),
                        // The lines after a garbled one are not read.
                        Err(error) => return Some((Err(error), (bytes, Vec::new(), ready, true))),
                    }
                    continue;
                }
                if ended {
                    return None;
                }
                match bytes.next().await {
                    Some(Ok(more)) => buffer.extend_from_slice(&more),
                    Some(Err(error)) => {
                        return Some((
                            Err(CompletionError(error.to_string())),
                            (bytes, buffer, ready, true),
                        ));
                    }
                    None => {
                        ended = true;
                        buffer.push(b'\n');
                    }
                }
            }
        },
    )
}

/// The assistant's model when it cannot be set up — its key missing, say: every
/// request fails with why, while the rest of eco works on.
pub struct Unavailable(pub String);

impl LanguageModel for Unavailable {
    fn stream(
        &self,
        _: &str,
        _: Vec<Message>,
    ) -> BoxStream<'static, Result<Chunk, CompletionError>> {
        stream::iter([Err(CompletionError(self.0.clone()))]).boxed()
    }
}

pub struct OpenAIChat {
    endpoint: Endpoint,
    extra: Map<String, Value>,
}

impl OpenAIChat {
    pub fn new(endpoint: Endpoint, extra: Map<String, Value>) -> Self {
        Self { endpoint, extra }
    }

    fn body(&self, model: &str, messages: &[Message]) -> Value {
        let marked = MARKED_CACHE.iter().any(|prefix| model.starts_with(prefix));
        let mut body = Map::new();
        body.insert("stream_options".into(), json!({"include_usage": true}));
        body.extend(self.extra.clone());
        body.insert("model".into(), json!(model));
        body.insert(
            "messages".into(),
            messages.iter().map(|m| wire(m, marked)).collect(),
        );
        body.insert("stream".into(), json!(true));
        Value::Object(body)
    }
}

impl LanguageModel for OpenAIChat {
    fn stream(
        &self,
        model: &str,
        messages: Vec<Message>,
    ) -> BoxStream<'static, Result<Chunk, CompletionError>> {
        let request = self
            .endpoint
            .post("chat/completions")
            .json(&self.body(model, &messages));
        let endpoint = self.endpoint.clone();
        // When the server last sent anything, so a failure says how long it had been quiet.
        let heard = Arc::new(Mutex::new(Instant::now()));
        let opened = async move {
            let quiet = |heard: &Mutex<Instant>| heard.lock().expect("not poisoned").elapsed();
            let response = request
                .send()
                .await
                .map_err(|error| CompletionError(endpoint.failure(&error, quiet(&heard))))?;
            let status = response.status();
            let json = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|kind| kind.to_str().ok())
                .is_some_and(|kind| kind.starts_with("application/json"));
            // A refusal, or a whole answer from a server that does not stream.
            if !status.is_success() || json {
                let body = response.text().await.unwrap_or_default();
                let answer = serde_json::from_str::<Value>(&body).ok().and_then(|reply| {
                    let content = reply.pointer("/choices/0/message/content")?.as_str()?;
                    Some(content.to_string())
                });
                return match answer.filter(|_| status.is_success()) {
                    Some(text) => Ok(stream::iter(vec![Ok(Chunk::Text(text))]).boxed()),
                    None => Err(CompletionError(endpoint.refusal(status.as_u16(), &body))),
                };
            }
            let bytes = response.bytes_stream().map(move |part| match part {
                Ok(bytes) => {
                    *heard.lock().expect("not poisoned") = Instant::now();
                    Ok(bytes)
                }
                Err(error) => Err(endpoint.failure(&error, quiet(&heard))),
            });
            Ok(sse(bytes).boxed())
        };
        stream::once(opened).try_flatten().boxed()
    }
}

/// Model ids offered by GET /models on an OpenAI-compatible API.
pub async fn list_models(endpoint: &Endpoint) -> Result<Vec<String>, String> {
    let started = Instant::now();
    let response = endpoint
        .get("models")
        .send()
        .await
        .map_err(|e| endpoint.failure(&e, started.elapsed()))?;
    let status = response.status().as_u16();
    let body = response
        .text()
        .await
        .map_err(|e| endpoint.failure(&e, started.elapsed()))?;
    let reply: Option<Value> = serde_json::from_str(&body).ok();
    let mut ids: Vec<String> = reply
        .as_ref()
        .filter(|_| (200..300).contains(&status))
        .and_then(|reply| reply.get("data"))
        .and_then(Value::as_array)
        .ok_or_else(|| endpoint.refusal(status, &body))?
        .iter()
        .filter_map(|model| model.get("id").and_then(Value::as_str).map(String::from))
        .collect();
    ids.sort();
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::adapters::http::testing::{serve_once, serve_once_as};

    fn sse_body(chunks: &[Value]) -> Vec<u8> {
        let mut body: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
        body.push_str(": keep-alive\n\ndata: [DONE]\n\n");
        body.into_bytes()
    }

    async fn collect(
        status: u16,
        reply: Vec<u8>,
        extra: Value,
        model: &str,
        messages: Vec<Message>,
    ) -> (Vec<Result<Chunk, CompletionError>>, Value) {
        let (base, seen) = serve_once(status, reply).await;
        let endpoint = Endpoint::new(&base, None, Duration::from_secs(5)).unwrap();
        let chat = OpenAIChat::new(endpoint, extra.as_object().cloned().unwrap_or_default());
        let chunks = chat.stream(model, messages).collect().await;
        let body = serde_json::from_slice(&seen.lock().unwrap().body).unwrap_or(Value::Null);
        (chunks, body)
    }

    /// The one error a stream ended with, and nothing before it.
    fn only_error(chunks: &[Result<Chunk, CompletionError>]) -> &str {
        assert_eq!(chunks.len(), 1, "{chunks:?}");
        &chunks[0].as_ref().unwrap_err().0
    }

    fn hello() -> Vec<Message> {
        vec![Message {
            role: "user",
            content: "oi".into(),
            cache: false,
        }]
    }

    #[tokio::test]
    async fn streams_content_deltas_with_extra_fields() {
        let reply = sse_body(&[
            json!({"choices": [{"delta": {"role": "assistant"}}]}),
            json!({"choices": [{"delta": {"content": "Olá"}}]}),
            json!({"choices": [{"delta": {"content": " mundo"}}]}),
            json!({"choices": [], "usage": {"prompt_tokens": 1200, "prompt_tokens_details": {"cached_tokens": 1024}, "cost": 0.0004}}),
        ]);
        let (chunks, body) = collect(
            200,
            reply,
            json!({"reasoning": {"effort": "minimal"}}),
            "m",
            hello(),
        )
        .await;
        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"], json!({"include_usage": true}));
        assert_eq!(body["reasoning"], json!({"effort": "minimal"}));
        let chunks: Vec<Chunk> = chunks.into_iter().map(Result::unwrap).collect();
        assert_eq!(
            chunks,
            [
                Chunk::Text("Olá".into()),
                Chunk::Text(" mundo".into()),
                Chunk::Usage(Usage {
                    prompt_tokens: 1200,
                    cached_tokens: 1024,
                    cost_usd: Some(0.0004)
                }),
            ]
        );
    }

    #[tokio::test]
    async fn reasoning_streams_apart_from_the_answer() {
        let reply = sse_body(&[
            json!({"choices": [{"delta": {"role": "assistant", "reasoning_content": "Pensando"}}]}),
            json!({"choices": [{"delta": {"reasoning": " mais"}}]}),
            json!({"choices": [{"delta": {"content": "ok"}}]}),
        ]);
        let (chunks, _) = collect(200, reply, json!({}), "m", hello()).await;
        let chunks: Vec<Chunk> = chunks.into_iter().map(Result::unwrap).collect();
        assert_eq!(
            chunks,
            [
                Chunk::Thinking("Pensando".into()),
                Chunk::Thinking(" mais".into()),
                Chunk::Text("ok".into()),
            ]
        );
    }

    #[tokio::test]
    async fn a_silent_server_fails_with_how_long_it_was_silent() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        // Accepts the request and never answers.
        let held = tokio::spawn(async move { listener.accept().await });
        let endpoint = Endpoint::new(&base, None, Duration::from_secs(1)).unwrap();
        let chat = OpenAIChat::new(endpoint, Map::new());
        let chunks: Vec<_> = chat.stream("m", hello()).collect().await;
        let error = only_error(&chunks);
        assert_eq!(error, "no reply for 1 s");
        held.abort();
    }

    #[tokio::test]
    async fn a_json_refusal_says_why_and_hints_at_v1() {
        // LM Studio on a base URL without /v1: 200, and an error instead of a stream.
        let refusal = br#"{"error":"Unexpected endpoint or method. (POST /chat/completions)"}"#;
        let (base, _) = serve_once_as(200, Some("application/json"), refusal.to_vec()).await;
        let base = base.trim_end_matches("/v1").to_string();
        let endpoint = Endpoint::new(&base, None, Duration::from_secs(5)).unwrap();
        let chunks: Vec<_> = OpenAIChat::new(endpoint, Map::new())
            .stream("m", hello())
            .collect()
            .await;
        let error = only_error(&chunks);
        assert_eq!(
            error,
            format!(
                "200: Unexpected endpoint or method. (POST /chat/completions) ({base}/ has no path; OpenAI-compatible servers usually end in /v1)"
            )
        );
    }

    #[tokio::test]
    async fn a_whole_json_answer_is_the_answer() {
        let answer = br#"{"choices":[{"message":{"role":"assistant","content":"ok"}}]}"#;
        let (base, _) = serve_once_as(200, Some("application/json"), answer.to_vec()).await;
        let endpoint = Endpoint::new(&base, None, Duration::from_secs(5)).unwrap();
        let chunks: Vec<Chunk> = OpenAIChat::new(endpoint, Map::new())
            .stream("m", hello())
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .map(Result::unwrap)
            .collect();
        assert_eq!(chunks, [Chunk::Text("ok".into())]);
    }

    #[tokio::test]
    async fn a_model_list_refusal_says_why() {
        let refusal = br#"{"error":{"message":"Unexpected endpoint"}}"#;
        let (base, _) = serve_once_as(404, Some("application/json"), refusal.to_vec()).await;
        let endpoint = Endpoint::new(&base, None, Duration::from_secs(5)).unwrap();
        assert_eq!(
            list_models(&endpoint).await,
            Err("404: Unexpected endpoint".to_string())
        );
    }

    #[tokio::test]
    async fn http_error_carries_status_and_body() {
        let (chunks, _) = collect(
            402,
            b"insufficient credits".to_vec(),
            json!({}),
            "m",
            hello(),
        )
        .await;
        let error = only_error(&chunks);
        assert_eq!(error, "402: insufficient credits");
    }

    #[tokio::test]
    async fn cache_marks_become_cache_control_for_anthropic_only() {
        let marked = vec![
            Message {
                role: "system",
                content: "regras".into(),
                cache: true,
            },
            Message {
                role: "user",
                content: "oi".into(),
                cache: false,
            },
        ];
        let (_, claude) = collect(
            200,
            sse_body(&[]),
            json!({}),
            "anthropic/claude-haiku-4.5",
            marked.clone(),
        )
        .await;
        assert_eq!(
            claude["messages"][0]["content"],
            json!([{"type": "text", "text": "regras", "cache_control": {"type": "ephemeral"}}])
        );
        assert_eq!(
            claude["messages"][1],
            json!({"role": "user", "content": "oi"})
        );
        let (_, gemini) = collect(
            200,
            sse_body(&[]),
            json!({}),
            "google/gemini-3.5-flash-lite",
            marked,
        )
        .await;
        assert_eq!(
            gemini["messages"][0],
            json!({"role": "system", "content": "regras"})
        );
    }

    #[tokio::test]
    async fn lines_split_across_packets_still_parse() {
        let text = "data: {\"choices\":[{\"delta\":{\"content\":\"Olá\"}}]}\n\ndata: [DONE]\n\n";
        let pieces: Vec<Result<bytes::Bytes, std::io::Error>> = text
            .as_bytes()
            .chunks(7)
            .map(|c| Ok(bytes::Bytes::copy_from_slice(c)))
            .collect();
        let chunks: Vec<Chunk> = sse(stream::iter(pieces))
            .map(Result::unwrap)
            .collect()
            .await;
        assert_eq!(chunks, [Chunk::Text("Olá".into())]);
    }

    #[tokio::test]
    async fn lists_models_sorted() {
        let (base, seen) =
            serve_once(200, br#"{"data": [{"id": "b"}, {"id": "a"}]}"#.to_vec()).await;
        let endpoint = Endpoint::new(&base, None, Duration::from_secs(5)).unwrap();
        assert_eq!(list_models(&endpoint).await.unwrap(), ["a", "b"]);
        assert!(seen.lock().unwrap().head.starts_with("GET /v1/models "));
    }

    #[tokio::test]
    async fn a_garbled_event_ends_the_stream_with_an_error() {
        let reply = b"data: {\"choices\":[{\"delta\":{\"content\":\"Ol\"}}]}\n\ndata: {oops\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"never\"}}]}\n\n";
        let (chunks, _) = collect(200, reply.to_vec(), json!({}), "m", hello()).await;
        assert_eq!(chunks.len(), 2, "{chunks:?}");
        assert_eq!(chunks[0].as_ref().unwrap(), &Chunk::Text("Ol".into()));
        let error = &chunks[1].as_ref().unwrap_err().0;
        assert_eq!(error, "key must be a string at line 1 column 2");
    }

    #[tokio::test]
    async fn a_stream_cut_short_ends_with_why() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0u8; 8192];
            let _ = socket.read(&mut buffer).await;
            let event = "data: {\"choices\":[{\"delta\":{\"content\":\"Ol\"}}]}\n\n";
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n",
                event.len() + 100
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(event.as_bytes()).await.unwrap();
        });
        let endpoint = Endpoint::new(&base, None, Duration::from_secs(5)).unwrap();
        let chunks: Vec<_> = OpenAIChat::new(endpoint, Map::new())
            .stream("m", hello())
            .collect()
            .await;
        assert_eq!(chunks.len(), 2, "{chunks:?}");
        assert_eq!(chunks[0].as_ref().unwrap(), &Chunk::Text("Ol".into()));
        let error = &chunks[1].as_ref().unwrap_err().0;
        assert!(error.ends_with("(quiet for 0 s)"), "{error}");
    }

    #[tokio::test]
    async fn a_server_that_is_not_there_cannot_be_reached() {
        let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", closed.local_addr().unwrap());
        drop(closed);
        let endpoint = Endpoint::new(&base, None, Duration::from_secs(5)).unwrap();
        let chunks: Vec<_> = OpenAIChat::new(endpoint.clone(), Map::new())
            .stream("m", hello())
            .collect()
            .await;
        let error = only_error(&chunks);
        assert_eq!(error, format!("cannot reach {base}/"));
        assert_eq!(
            list_models(&endpoint).await,
            Err(format!("cannot reach {base}/"))
        );
    }

    #[tokio::test]
    async fn a_refusal_in_json_with_an_answer_shape_is_still_a_refusal() {
        let refusal = br#"{"choices":[{"message":{"content":"ok"}}],"error":"over quota"}"#;
        let (base, _) = serve_once_as(429, Some("application/json"), refusal.to_vec()).await;
        let endpoint = Endpoint::new(&base, None, Duration::from_secs(5)).unwrap();
        let chunks: Vec<_> = OpenAIChat::new(endpoint, Map::new())
            .stream("m", hello())
            .collect()
            .await;
        let error = only_error(&chunks);
        assert_eq!(error, "429: over quota");
    }

    #[tokio::test]
    async fn an_unavailable_model_fails_every_request_with_why() {
        let chunks: Vec<_> = Unavailable("no key".into())
            .stream("m", hello())
            .collect()
            .await;
        let error = only_error(&chunks);
        assert_eq!(error, "no key");
    }

    #[tokio::test]
    async fn usage_without_counts_is_zero_and_a_null_usage_is_none() {
        let reply = sse_body(&[
            json!({"choices": [{"delta": {"content": ""}}], "usage": null}),
            json!({"choices": [], "usage": {}}),
        ]);
        let (chunks, _) = collect(200, reply, json!({}), "m", hello()).await;
        let chunks: Vec<Chunk> = chunks.into_iter().map(Result::unwrap).collect();
        assert_eq!(
            chunks,
            [Chunk::Usage(Usage {
                prompt_tokens: 0,
                cached_tokens: 0,
                cost_usd: None
            })]
        );
    }

    #[tokio::test]
    async fn a_model_list_without_data_is_a_refusal() {
        let (base, _) = serve_once(200, br#"{"models": []}"#.to_vec()).await;
        let endpoint = Endpoint::new(&base, None, Duration::from_secs(5)).unwrap();
        assert_eq!(
            list_models(&endpoint).await,
            Err(r#"200: {"models": []}"#.to_string())
        );
    }
}
