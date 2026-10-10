//! An OpenAI-compatible endpoint: base URL, bearer key, and how long it may stay silent.

use std::time::Duration;

use reqwest::{Client, ClientBuilder, RequestBuilder};
use serde_json::Value;

use crate::config::ConfigError;

/// How long a server may take to accept a connection.
const CONNECT: Duration = Duration::from_secs(10);

/// A client builder with rustls on ring; installs ring as the process's TLS provider.
fn builder() -> ClientBuilder {
    // Fails only when a provider is already installed, which is the goal.
    let _ = rustls::crypto::ring::default_provider().install_default();
    Client::builder()
}

/// An HTTP client whose whole request must finish within `timeout`, if given.
pub fn client(timeout: Option<Duration>) -> reqwest::Result<Client> {
    match timeout {
        Some(timeout) => builder().timeout(timeout),
        None => builder(),
    }
    .build()
}

#[derive(Clone)]
pub struct Endpoint {
    client: Client,
    base: String,
    key: Option<String>,
    silence: Duration,
}

impl Endpoint {
    /// `key` is the bearer token; none for the LAN servers. `silence` is the
    /// longest the server may send nothing — before its reply or between the
    /// parts of a streamed one — so a long answer that keeps arriving is never
    /// cut off, while a server that stopped answering is.
    pub fn new(
        base_url: &str,
        key: Option<String>,
        silence: Duration,
    ) -> Result<Self, ConfigError> {
        let client = builder()
            .connect_timeout(CONNECT)
            .read_timeout(silence)
            .build()
            .map_err(|error| ConfigError(format!("http client: {error}")))?;
        Ok(Self {
            client,
            base: format!("{}/", base_url.trim_end_matches('/')),
            key,
            silence,
        })
    }

    /// What a failed request says, `quiet` after the server last sent anything:
    /// that it cannot be reached, that it stayed silent for as long as allowed
    /// when that is what happened, or else the error with its causes and how
    /// long it had been quiet.
    pub fn failure(&self, error: &reqwest::Error, quiet: Duration) -> String {
        if error.is_connect() {
            return format!("cannot reach {}", self.base);
        }
        // The read timeout fires once the silence is reached; a second of slack for timers.
        if error.is_timeout() && quiet + Duration::from_secs(1) >= self.silence {
            return format!("no reply for {} s", self.silence.as_secs());
        }
        let mut detail = error.to_string();
        let mut source = std::error::Error::source(error);
        while let Some(cause) = source {
            detail.push_str(&format!(": {cause}"));
            source = cause.source();
        }
        format!("{detail} (quiet for {} s)", quiet.as_secs())
    }

    /// What a reply that is not the one asked for says: the server's own error
    /// (`{"error": "…"}` or `{"error": {"message": "…"}}`), or its first words;
    /// with a hint when the base URL has no path, since OpenAI-compatible
    /// servers usually serve under /v1.
    pub fn refusal(&self, status: u16, body: &str) -> String {
        let reply: Option<Value> = serde_json::from_str(body).ok();
        let error = reply.as_ref().and_then(|reply| reply.get("error"));
        let said = error
            .and_then(|error| error.as_str().or_else(|| error.get("message")?.as_str()))
            .map_or_else(|| body.chars().take(200).collect(), String::from);
        let pathless = reqwest::Url::parse(&self.base).is_ok_and(|url| url.path() == "/");
        let hint = if pathless {
            format!(
                " ({} has no path; OpenAI-compatible servers usually end in /v1)",
                self.base
            )
        } else {
            String::new()
        };
        format!("{status}: {said}{hint}")
    }

    fn authorized(&self, request: RequestBuilder) -> RequestBuilder {
        match &self.key {
            Some(key) => request.bearer_auth(key),
            None => request,
        }
    }

    pub fn get(&self, path: &str) -> RequestBuilder {
        self.authorized(self.client.get(format!("{}{path}", self.base)))
    }

    pub fn post(&self, path: &str) -> RequestBuilder {
        self.authorized(self.client.post(format!("{}{path}", self.base)))
    }
}

#[cfg(test)]
pub(crate) mod testing {
    //! A scripted HTTP server, so adapters are tested over real HTTP.

    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[derive(Debug, Default, Clone)]
    pub struct Seen {
        pub head: String,
        pub body: Vec<u8>,
    }

    /// Serve one request with `status` and `body`; returns the base URL and what was sent.
    pub async fn serve_once(status: u16, body: Vec<u8>) -> (String, Arc<Mutex<Seen>>) {
        serve_once_as(status, None, body).await
    }

    /// `serve_once`, saying the body is of `kind`.
    pub async fn serve_once_as(
        status: u16,
        kind: Option<&'static str>,
        body: Vec<u8>,
    ) -> (String, Arc<Mutex<Seen>>) {
        serve(vec![(status, kind, body)]).await
    }

    /// Serve one request per reply `(status, kind, body)`, in order, each on
    /// its own connection; returns the base URL and the last request sent.
    pub async fn serve(
        replies: Vec<(u16, Option<&'static str>, Vec<u8>)>,
    ) -> (String, Arc<Mutex<Seen>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Seen::default()));
        let record = Arc::clone(&seen);
        tokio::spawn(async move {
            for (status, kind, body) in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0u8; 8192];
                let ended = |request: &[u8]| request.windows(4).position(|w| w == b"\r\n\r\n");
                while ended(&request).is_none() {
                    let read = socket.read(&mut buffer).await.unwrap();
                    request.extend_from_slice(&buffer[..read]);
                }
                let head_end = ended(&request).unwrap() + 4;
                let head = String::from_utf8_lossy(&request[..head_end]).to_string();
                let length = head
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().to_string())
                    })
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0);
                while request.len() < head_end + length {
                    let read = socket.read(&mut buffer).await.unwrap();
                    request.extend_from_slice(&buffer[..read]);
                }
                *record.lock().unwrap() = Seen {
                    head,
                    body: request[head_end..].to_vec(),
                };
                let kind =
                    kind.map_or_else(String::new, |kind| format!("content-type: {kind}\r\n"));
                let reply = format!(
                    "HTTP/1.1 {status} X\r\n{kind}content-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                socket.write_all(reply.as_bytes()).await.unwrap();
                socket.write_all(&body).await.unwrap();
            }
        });
        (format!("http://{address}/v1"), seen)
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::testing::serve_once;
    use super::*;

    const SILENCE: Duration = Duration::from_secs(5);

    /// A server that answers `reply`, raw, to one request and closes.
    async fn raw(reply: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0u8; 4096];
            let _ = socket.read(&mut buffer).await;
            socket.write_all(reply).await.unwrap();
        });
        format!("http://{address}/v1")
    }

    #[tokio::test]
    async fn requests_carry_the_key_when_there_is_one() {
        let (base, seen) = serve_once(200, Vec::new()).await;
        let endpoint = Endpoint::new(&format!("{base}/"), Some("sk-1".into()), SILENCE).unwrap();
        endpoint.post("chat/completions").send().await.unwrap();
        let head = seen.lock().unwrap().head.clone();
        assert!(head.starts_with("POST /v1/chat/completions "), "{head}");
        assert!(head.contains("authorization: Bearer sk-1"), "{head}");
    }

    #[tokio::test]
    async fn a_server_that_is_not_there_cannot_be_reached() {
        let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", closed.local_addr().unwrap());
        drop(closed);
        let endpoint = Endpoint::new(&base, None, SILENCE).unwrap();
        let error = endpoint.get("models").send().await.unwrap_err();
        assert_eq!(
            endpoint.failure(&error, Duration::ZERO),
            format!("cannot reach {base}/")
        );
    }

    #[tokio::test]
    async fn a_reply_cut_short_says_why_and_how_long_it_was_quiet() {
        let base = raw(b"HTTP/1.1 200 OK\r\ncontent-length: 100\r\n\r\nshort").await;
        let endpoint = Endpoint::new(&base, None, SILENCE).unwrap();
        let reply = endpoint.get("models").send().await.unwrap();
        let error = reply.text().await.unwrap_err();
        let said = endpoint.failure(&error, Duration::from_secs(2));
        assert!(said.starts_with("error decoding response body"), "{said}");
        // The causes follow, down to the connection's own.
        assert!(
            said.ends_with(": end of file before message length reached (quiet for 2 s)"),
            "{said}"
        );
    }

    #[test]
    fn a_refusal_says_the_servers_error_or_its_first_words() {
        let endpoint = Endpoint::new("http://lan:8080/v1", None, SILENCE).unwrap();
        let nested = r#"{"error":{"message":"model not found"}}"#;
        assert_eq!(endpoint.refusal(404, nested), "404: model not found");
        let long = "x".repeat(300);
        assert_eq!(
            endpoint.refusal(500, &long),
            format!("500: {}", "x".repeat(200))
        );
        let odd = r#"{"error":{"code":7}}"#;
        assert_eq!(endpoint.refusal(400, odd), format!("400: {odd}"));
    }
}
