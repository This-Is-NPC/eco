//! A streaming transcription over a WebSocket: frames go out as the provider's
//! protocol wants them, finished phrases come back.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::stream::{self, BoxStream};
use futures::{SinkExt, StreamExt};
use tokio::time::{Instant, MissedTickBehavior};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{Connector, connect_async_tls_with_config};

use crate::ports::{Frame, Heard, Phrase, TranscriptionError};

/// Frames already waiting go out together, up to one second of audio: live
/// audio still leaves frame by frame, a file in a few large messages.
const MAX_CHUNK_FRAMES: usize = 31;

/// How a connection proves it is alive while audio goes out.
struct Liveness {
    /// A ping goes out this often, so a provider with nothing to say still
    /// answers something.
    ping_every: Duration,
    /// A connection that brought nothing back — no pong, no message — for this
    /// long is stalled. It also bounds a write that never completes, since no
    /// ping leaves behind it.
    heard_within: Duration,
}

/// Three pings unanswered end the connection.
const LIVENESS: Liveness = Liveness {
    ping_every: Duration::from_secs(5),
    heard_within: Duration::from_secs(15),
};

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
    stream_alive(url, headers, protocol, frames, LIVENESS)
}

/// [`transcribe`], ended with an error once the connection fails `liveness`.
fn stream_alive<'a, Audio, Read>(
    url: String,
    headers: Vec<(&'static str, String)>,
    protocol: Protocol<Audio, Read>,
    frames: BoxStream<'a, Frame>,
    liveness: Liveness,
) -> BoxStream<'a, Result<Heard, TranscriptionError>>
where
    Audio: Fn(&[i16]) -> Message + Send + Sync + 'a,
    Read: FnMut(&str) -> Result<Reading, String> + Send + 'a,
{
    let Liveness {
        ping_every,
        heard_within,
    } = liveness;
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
            let mut ping = tokio::time::interval_at(Instant::now() + ping_every, ping_every);
            ping.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                let message = tokio::select! {
                    chunk = chunks.next() => match chunk {
                        Some(chunk) => audio(&chunk.concat()),
                        None => break,
                    },
                    _ = ping.tick() => Message::Ping(Default::default()),
                };
                outgoing.send(message).await?;
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
                    tokio::time::timeout(heard_within, next)
                        .await
                        .map_err(|_| format!("stalled: nothing heard for {heard_within:?}"))?
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
        // Receiving runs until the provider closes, stalls, or goes quiet once
        // the audio is over.
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    const FINISH: &str = "finish";

    /// Short enough for real time: a paused clock jumps ahead while a real
    /// socket is awaited.
    const BRIEF: Liveness = Liveness {
        ping_every: Duration::from_millis(100),
        heard_within: Duration::from_secs(1),
    };

    /// `frames` streamed to `url` within [`BRIEF`], each text message the
    /// provider sends read as one phrase.
    fn alive(
        url: String,
        frames: BoxStream<'static, Frame>,
    ) -> BoxStream<'static, Result<Heard, TranscriptionError>> {
        let protocol = Protocol {
            audio: |pcm: &[i16]| Message::Binary(vec![0; pcm.len() * 2].into()),
            finish: vec![Message::Text(FINISH.into())],
            quiet: Duration::from_secs(30),
            read: |text: &str| {
                Ok(Reading::Phrases(vec![Phrase {
                    start: 0.0,
                    end: 1.0,
                    text: text.into(),
                }]))
            },
            request: None,
        };
        stream_alive(url, Vec::new(), protocol, frames, BRIEF)
    }

    /// `frames` frames, one each 32 ms as capture hands them.
    fn live(frames: usize) -> BoxStream<'static, Frame> {
        stream::unfold(0, move |sent| async move {
            tokio::time::sleep(Duration::from_millis(32)).await;
            (sent < frames).then(|| (vec![0i16; 512], sent + 1))
        })
        .boxed()
    }

    /// A provider that takes the connection, then neither reads nor answers.
    async fn stalled() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let _held = tokio_tungstenite::accept_async(socket).await.unwrap();
            std::future::pending::<()>().await;
        });
        format!("ws://{address}")
    }

    #[tokio::test]
    async fn a_provider_that_stops_answering_ends_the_stream_with_an_error() {
        let url = stalled().await;
        let began = Instant::now();
        let heard = alive(url, live(usize::MAX));
        let heard: Vec<_> = tokio::time::timeout(10 * BRIEF.heard_within, heard.collect())
            .await
            .expect("a stalled stream ends");
        let Some(Err(TranscriptionError(why))) = heard.last() else {
            panic!("an error, not {} results", heard.len())
        };
        assert!(why.starts_with("stalled"), "{why}");
        assert!(began.elapsed() >= BRIEF.heard_within);
    }

    /// Audio that fills every buffer of a provider that never reads: the write
    /// that cannot complete is cut too.
    #[tokio::test]
    async fn a_write_that_never_completes_ends_the_stream_with_an_error() {
        let url = stalled().await;
        let flood = stream::repeat(vec![0i16; 16_000]).boxed();
        let heard = alive(url, flood);
        let heard: Vec<_> = tokio::time::timeout(10 * BRIEF.heard_within, heard.collect())
            .await
            .expect("a blocked write ends");
        let Some(Err(TranscriptionError(why))) = heard.last() else {
            panic!("an error, not {} results", heard.len())
        };
        assert!(why.starts_with("stalled"), "{why}");
    }

    /// A provider silent through a long gap in the audio, answering only the
    /// pings, keeps the stream until the audio is over.
    #[tokio::test]
    async fn a_provider_answering_pings_keeps_a_long_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            let mut pings = 0;
            while let Some(Ok(message)) = socket.next().await {
                match message {
                    Message::Ping(_) => pings += 1,
                    Message::Text(text) if text == FINISH => break,
                    _ => {}
                }
            }
            socket.send(Message::Text("done".into())).await.unwrap();
            socket.close(None).await.unwrap();
            pings
        });
        let gap = 3 * BRIEF.heard_within;
        let frames = live(1)
            .chain(stream::once(async move {
                tokio::time::sleep(gap).await;
                vec![0i16; 512]
            }))
            .boxed();
        let heard: Vec<_> = alive(format!("ws://{address}"), frames).collect().await;
        let said: Vec<_> = heard
            .iter()
            .map(|heard| match heard {
                Ok(Heard::Phrase(phrase)) => phrase.text.clone(),
                other => panic!("only the phrase, not {other:?}"),
            })
            .collect();
        assert_eq!(said, ["done"]);
        let pings = server.await.unwrap();
        // At least half the pings due over the gap, for a busy machine.
        let due = gap.as_millis() / BRIEF.ping_every.as_millis();
        assert!(pings >= due / 2, "{pings} pings");
    }
}
