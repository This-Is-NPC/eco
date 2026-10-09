//! A streaming transcription over a WebSocket: frames go out as the provider's
//! protocol wants them, finished phrases come back.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::stream::{self, BoxStream};
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{Connector, connect_async_tls_with_config};

use crate::ports::{Frame, Heard, Phrase, TranscriptionError};

/// Frames already waiting go out together, up to one second of audio: live
/// audio still leaves frame by frame, a file in a few large messages.
const MAX_CHUNK_FRAMES: usize = 31;

/// What a provider's messages mean.
pub enum Reading {
    Phrases(Vec<Phrase>),
    /// The words of the phrase being spoken so far.
    Partial(String),
    /// Nothing to report: partial results, metadata, keep-alives.
    Nothing,
}

/// One provider's side of the conversation.
pub struct Protocol<Audio, Read> {
    /// The message carrying a stretch of audio.
    pub audio: Audio,
    /// What tells the provider the audio is over.
    pub finish: Vec<Message>,
    /// Once the audio is over, a provider quiet this long has said everything.
    pub quiet: Duration,
    /// What a text message from the provider says; `Err` for a provider error.
    pub read: Read,
    /// The response header naming the request the provider bills the stream under.
    pub request: Option<&'static str>,
}

fn tls() -> Result<Arc<rustls::ClientConfig>, String> {
    use rustls_platform_verifier::ConfigVerifierExt;
    // Fails only when a provider is already installed, which is the goal.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = rustls::ClientConfig::with_platform_verifier().map_err(|e| e.to_string())?;
    Ok(Arc::new(config))
}

/// Stream `frames` to `url` (with `headers`), speaking `protocol`.
pub fn transcribe<'a, Audio, Read>(
    url: String,
    headers: Vec<(&'static str, String)>,
    protocol: Protocol<Audio, Read>,
    frames: BoxStream<'a, Frame>,
) -> BoxStream<'a, Result<Heard, TranscriptionError>>
where
    Audio: Fn(&[i16]) -> Message + Send + Sync + 'a,
    Read: FnMut(&str) -> Result<Reading, String> + Send + 'a,
{
    let Protocol {
        audio,
        finish,
        quiet,
        mut read,
        request: billed,
    } = protocol;
    let (found, phrases) = futures::channel::mpsc::unbounded();
    let failed = found.clone();
    let session = async move {
        let mut request = url.into_client_request().map_err(|e| e.to_string())?;
        for (name, value) in headers {
            let value = HeaderValue::from_str(&value).map_err(|e| e.to_string())?;
            request.headers_mut().insert(name, value);
        }
        let connector = match request.uri().scheme_str() {
            Some("wss") => Some(Connector::Rustls(tls()?)),
            _ => None,
        };
        let (socket, response) = connect_async_tls_with_config(request, None, false, connector)
            .await
            .map_err(|e| e.to_string())?;
        let id = billed.and_then(|name| response.headers().get(name)?.to_str().ok());
        if let Some(id) = id {
            let _ = found.unbounded_send(Ok(Heard::Request(id.into())));
        }
        let (mut outgoing, mut incoming) = socket.split();
        let mut chunks = frames.ready_chunks(MAX_CHUNK_FRAMES);
        let send = async {
            while let Some(chunk) = chunks.next().await {
                outgoing.send(audio(&chunk.concat())).await?;
            }
            for message in finish {
                outgoing.send(message).await?;
            }
            Ok::<_, tokio_tungstenite::tungstenite::Error>(())
        };
        let finished = AtomicBool::new(false);
        let receive = async {
            loop {
                let next = incoming.next();
                let message = if finished.load(Ordering::Relaxed) {
                    tokio::time::timeout(quiet, next).await.ok().flatten()
                } else {
                    next.await
                };
                let Some(message) = message else { break };
                let text = match message.map_err(|e| e.to_string())? {
                    Message::Text(text) => text,
                    Message::Close(_) => break,
                    _ => continue,
                };
                match read(&text)? {
                    Reading::Phrases(phrases) => {
                        for phrase in phrases {
                            let _ = found.unbounded_send(Ok(Heard::Phrase(phrase)));
                        }
                    }
                    Reading::Partial(words) => {
                        let _ = found.unbounded_send(Ok(Heard::Partial(words)));
                    }
                    Reading::Nothing => {}
                }
            }
            Ok::<_, String>(())
        };
        // Receiving runs until the provider closes, or goes quiet once the
        // audio is over.
        tokio::pin!(receive);
        let sent = tokio::select! {
            sent = send => sent.map_err(|e| e.to_string()),
            received = &mut receive => return received,
        };
        sent?;
        finished.store(true, Ordering::Relaxed);
        receive.await
    };
    let session = async move {
        if let Err(detail) = session.await {
            let _ = failed.unbounded_send(Err(TranscriptionError(detail)));
        }
    };
    // The session runs as the phrases are read; both end together.
    stream::select(
        stream::once(session).filter_map(|()| async { None }),
        phrases,
    )
    .boxed()
}
