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
    /// ping leaves behind it, and opening a connection, whose handshake may
    /// otherwise wait for the system's TCP timeout.
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
        let opening = connect_async_tls_with_config(request, None, false, connector);
        let (socket, response) = tokio::time::timeout(heard_within, opening)
            .await
            .map_err(|_| format!("stalled: no handshake within {heard_within:?}"))?
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

    /// Audio as silence of its length, [`FINISH`] once it is over, and each
    /// text message the provider sends read by `read`.
    fn protocol<Read>(read: Read) -> Protocol<impl Fn(&[i16]) -> Message + Send + Sync, Read> {
        Protocol {
            audio: |pcm: &[i16]| Message::Binary(vec![0; pcm.len() * 2].into()),
            finish: vec![Message::Text(FINISH.into())],
            quiet: Duration::from_secs(30),
            read,
            request: None,
        }
    }

    /// A text message as one phrase.
    fn phrase(text: &str) -> Result<Reading, String> {
        Ok(Reading::Phrases(vec![Phrase {
            start: 0.0,
            end: 1.0,
            text: text.into(),
        }]))
    }

    /// `frames` streamed to `url` within [`BRIEF`], each text message the
    /// provider sends read as one phrase.
    fn alive(
        url: String,
        frames: BoxStream<'static, Frame>,
    ) -> BoxStream<'static, Result<Heard, TranscriptionError>> {
        stream_alive(url, Vec::new(), protocol(phrase), frames, BRIEF)
    }

    /// `frames` frames, one each 32 ms as capture hands them.
    fn live(frames: usize) -> BoxStream<'static, Frame> {
        stream::unfold(0, move |sent| async move {
            tokio::time::sleep(Duration::from_millis(32)).await;
            (sent < frames).then(|| (vec![0i16; 512], sent + 1))
        })
        .boxed()
    }

    /// What a server holds open while the test runs.
    type Held<T> = std::sync::mpsc::Receiver<T>;

    /// A provider that takes the connection, then neither reads nor answers
    /// while the connection it hands back is held.
    async fn stalled() -> (String, Held<impl Send>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (hold, held) = std::sync::mpsc::channel();
        tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            hold.send(socket).unwrap();
        });
        (format!("ws://{address}"), held)
    }

    /// The error a stream ended with.
    fn last_error(heard: &[Result<Heard, TranscriptionError>]) -> &str {
        &heard.last().expect("a result").as_ref().unwrap_err().0
    }

    #[tokio::test]
    async fn a_provider_that_stops_answering_ends_the_stream_with_an_error() {
        let (url, _held) = stalled().await;
        let began = Instant::now();
        let heard = alive(url, live(usize::MAX));
        let heard: Vec<_> = tokio::time::timeout(10 * BRIEF.heard_within, heard.collect())
            .await
            .expect("a stalled stream ends");
        let why = last_error(&heard);
        assert!(why.starts_with("stalled"), "{why}");
        assert!(began.elapsed() >= BRIEF.heard_within);
    }

    /// A provider that takes the connection and never answers the handshake.
    #[tokio::test]
    async fn a_handshake_that_never_completes_ends_the_stream_with_an_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (hold, _held) = std::sync::mpsc::channel();
        tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            hold.send(socket).unwrap();
        });
        let heard = alive(format!("ws://{address}"), live(usize::MAX));
        let heard: Vec<_> = tokio::time::timeout(10 * BRIEF.heard_within, heard.collect())
            .await
            .expect("a stalled handshake ends");
        let why = last_error(&heard);
        assert!(why.starts_with("stalled: no handshake"), "{why}");
    }

    /// Audio that fills every buffer of a provider that never reads: the write
    /// that cannot complete is cut too.
    #[tokio::test]
    async fn a_write_that_never_completes_ends_the_stream_with_an_error() {
        let (url, _held) = stalled().await;
        let flood = stream::repeat(vec![0i16; 16_000]).boxed();
        let heard = alive(url, flood);
        let heard: Vec<_> = tokio::time::timeout(10 * BRIEF.heard_within, heard.collect())
            .await
            .expect("a blocked write ends");
        let why = last_error(&heard);
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
        assert_eq!(said(&heard), ["done"]);
        let pings = server.await.unwrap();
        // At least half the pings due over the gap, for a busy machine.
        let due = gap.as_millis() / BRIEF.ping_every.as_millis();
        assert!(pings >= due / 2, "{pings} pings");
    }

    /// What a stream says, its texts and errors in order.
    fn said(heard: &[Result<Heard, TranscriptionError>]) -> Vec<String> {
        heard
            .iter()
            .map(|heard| match heard {
                Ok(Heard::Phrase(phrase)) => phrase.text.clone(),
                Ok(other) => format!("{other:?}"),
                Err(TranscriptionError(why)) => format!("error: {why}"),
            })
            .collect()
    }

    #[tokio::test]
    async fn an_address_or_header_that_cannot_be_sent_is_an_error() {
        let heard: Vec<_> = alive("not a url".into(), live(1)).collect().await;
        assert!(said(&heard)[0].starts_with("error: "), "{heard:?}");
        let headers = vec![("authorization", "Token \n".to_string())];
        let url = "ws://127.0.0.1:9".to_string();
        let heard: Vec<_> = stream_alive(url, headers, protocol(phrase), live(1), BRIEF)
            .collect()
            .await;
        assert_eq!(said(&heard), ["error: failed to parse header value"]);
    }

    /// A `wss` address is opened over TLS: a server that does not speak it fails the stream.
    #[tokio::test]
    async fn a_secure_address_that_does_not_speak_tls_is_an_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            use tokio::io::AsyncWriteExt;
            let _ = socket.write_all(b"HTTP/1.1 400 X\r\n\r\n").await;
        });
        let heard: Vec<_> = alive(format!("wss://{address}"), live(1)).collect().await;
        assert_eq!(heard.len(), 1, "{heard:?}");
        let why = last_error(&heard);
        assert!(!why.starts_with("stalled"), "{why}");
    }

    /// A provider that answers in binary and pings, says one thing, and closes
    /// while audio still goes: the stream ends with what it said.
    #[tokio::test]
    async fn a_provider_that_closes_first_ends_the_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            socket.next().await;
            for message in [
                Message::Binary(vec![1, 2].into()),
                Message::Ping(Default::default()),
                Message::Text("bye".into()),
            ] {
                socket.send(message).await.unwrap();
            }
            socket.close(None).await.unwrap();
            while let Some(Ok(_)) = socket.next().await {}
        });
        let heard: Vec<_> = alive(format!("ws://{address}"), live(usize::MAX))
            .collect()
            .await;
        assert_eq!(said(&heard), ["bye"]);
        server.await.unwrap();
    }

    /// A provider whose connection drops without a closing handshake.
    #[tokio::test]
    async fn a_connection_that_drops_ends_the_stream_with_an_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            socket.send(Message::Text("first".into())).await.unwrap();
            drop(socket);
        });
        let heard: Vec<_> = alive(format!("ws://{address}"), live(usize::MAX))
            .collect()
            .await;
        let said = said(&heard);
        assert_eq!(said[0], "first");
        assert!(said[1].starts_with("error: "), "{said:?}");
    }

    /// A provider error read from its message ends the stream with it.
    #[tokio::test]
    async fn a_provider_error_ends_the_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            socket.send(Message::Text("partial".into())).await.unwrap();
            socket.send(Message::Text("refused".into())).await.unwrap();
            while let Some(Ok(_)) = socket.next().await {}
        });
        let read = |text: &str| match text {
            "partial" => Ok(Reading::Partial("so far".into())),
            _ => Err(format!("provider: {text}")),
        };
        let url = format!("ws://{address}");
        let heard: Vec<_> = stream_alive(url, Vec::new(), protocol(read), live(usize::MAX), BRIEF)
            .collect()
            .await;
        assert_eq!(
            said(&heard),
            ["Partial(\"so far\")", "error: provider: refused"]
        );
        server.await.unwrap();
    }
}
