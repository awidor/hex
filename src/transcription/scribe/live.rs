//! A realtime Speech to Text session (`scribe_v3_live`) as the engine's live
//! stream: audio goes up as it is recorded, partial text comes back for live
//! text, and a commit at release returns the final text.
//!
//! One thread owns the WebSocket. The engine thread only queues audio and
//! reads the latest text, so connecting, a slow network, or a dead one never
//! holds a capture: a failed session reports itself on the next feed or at
//! the finish, and the engine transcribes the whole clip instead.
//!
//! The session connects directly; the computer's HTTP proxy (`net::proxy`)
//! is not used for the WebSocket.

use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs as _};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use base64::Engine as _;
use tungstenite::client::IntoClientRequest as _;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use super::client::{keyterm_ok, pcm_s16le};
use crate::transcription::cancel::CancelToken;
use crate::transcription::engine::{Partial, StreamFeed};

/// The realtime model.
pub const MODEL: &str = "scribe_v3_live";
const PATH: &str = "/v1/speech-to-text/realtime";
/// Realtime keyterm limits (the keyterm prompting guide).
const MAX_KEYTERMS: usize = 50;
const MAX_KEYTERM_CHARS: usize = 20;
/// How long resolving, connecting, and both handshakes may take together.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
/// How long one read waits before queued audio is sent: the most a chunk
/// lingers on this computer.
const READ_POLL: Duration = Duration::from_millis(15);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// The most audio in one message (half a second), under the API's chunk limit.
const MAX_CHUNK_SAMPLES: usize = 8_000;
/// How long a finish waits for the committed text. The engine cancels the
/// stream sooner (its own finish deadline); this bounds every other caller.
const COMMIT_TIMEOUT: Duration = Duration::from_secs(10);
/// How often a waiting finish looks at its cancel token.
const CANCEL_POLL: Duration = Duration::from_millis(20);

/// Why a finish did not use a session's text: the session only ever showed
/// provisional words, and the whole clip decides the transcript.
pub const PROVISIONAL: &str = "live text is provisional; the whole clip decides";

/// How to open a session.
pub struct Config {
    /// `https://api.elevenlabs.io` or another base; `http` becomes `ws`.
    pub base: String,
    pub key: String,
    pub language: Option<String>,
    pub keyterms: Vec<String>,
    pub no_verbatim: bool,
    /// Whether the session's text is only shown, never the transcript.
    pub provisional: bool,
}

enum Outgoing {
    Audio(Vec<f32>),
    Commit,
}

#[derive(Default)]
struct State {
    /// Every committed segment so far, joined by spaces.
    committed: String,
    /// The uncommitted segment's text.
    tentative: String,
    /// Commits the server has answered.
    commits: u64,
    failed: Option<String>,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn fail(&self, reason: impl Into<String>) {
        let mut state = self.lock();
        if state.failed.is_none() {
            state.failed = Some(reason.into());
        }
        drop(state);
        self.changed.notify_all();
    }
}

/// One open realtime session.
pub struct LiveSession {
    outgoing: Sender<Outgoing>,
    shared: Arc<Shared>,
    cancel: CancelToken,
    provisional: bool,
}

impl LiveSession {
    /// Starts connecting in the background and returns at once. Audio fed
    /// meanwhile is sent once the session is up.
    pub fn open(config: Config, cancel: &CancelToken) -> Result<Self, String> {
        let (outgoing, incoming) = mpsc::channel();
        let shared = Arc::new(Shared::default());
        let provisional = config.provisional;
        let thread_shared = shared.clone();
        let thread_cancel = cancel.clone();
        std::thread::Builder::new()
            .name("scribe-live".into())
            .spawn(move || run(config, incoming, &thread_shared, &thread_cancel))
            .map_err(|error| format!("could not start the live session: {error}"))?;
        Ok(Self {
            outgoing,
            shared,
            cancel: cancel.clone(),
            provisional,
        })
    }
}

impl LiveSession {
    /// Why the session's thread is gone: the failure it recorded, which
    /// names the cause, or that it simply ended.
    fn ended(&self) -> String {
        self.shared
            .lock()
            .failed
            .clone()
            .unwrap_or_else(|| "the live session ended".into())
    }
}

impl StreamFeed for LiveSession {
    fn feed(&mut self, samples: &[f32]) -> Result<(), String> {
        if let Some(reason) = &self.shared.lock().failed {
            return Err(reason.clone());
        }
        self.outgoing
            .send(Outgoing::Audio(samples.to_vec()))
            .map_err(|_| self.ended())
    }

    fn finalize(&mut self) -> Result<String, String> {
        if self.provisional {
            return Err(PROVISIONAL.into());
        }
        let answered = {
            let state = self.shared.lock();
            if let Some(reason) = &state.failed {
                return Err(reason.clone());
            }
            state.commits
        };
        self.outgoing
            .send(Outgoing::Commit)
            .map_err(|_| self.ended())?;
        let deadline = Instant::now() + COMMIT_TIMEOUT;
        let mut state = self.shared.lock();
        loop {
            // An answer that arrived stands, even when the connection closed
            // right after it.
            if state.commits > answered {
                return Ok(state.committed.trim().to_string());
            }
            if let Some(reason) = &state.failed {
                return Err(reason.clone());
            }
            if self.cancel.is_cancelled() {
                return Err("the live session was cancelled".into());
            }
            if Instant::now() >= deadline {
                return Err("the live session did not answer the commit".into());
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, CANCEL_POLL)
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
    }

    fn partial(&self) -> Partial {
        let state = self.shared.lock();
        Partial {
            stable: state.committed.clone(),
            tentative: match (state.committed.is_empty(), state.tentative.is_empty()) {
                (false, false) => format!(" {}", state.tentative),
                _ => state.tentative.clone(),
            },
        }
    }
}

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

fn run(config: Config, incoming: Receiver<Outgoing>, shared: &Shared, cancel: &CancelToken) {
    let started = Instant::now();
    let mut socket = match connect(&config) {
        Ok(socket) => socket,
        Err(reason) => {
            tracing::warn!(%reason, "scribe live session could not connect");
            shared.fail(reason);
            return;
        }
    };
    tracing::debug!(
        connect_ms = started.elapsed().as_millis() as u64,
        "scribe live session connected"
    );
    let mut sent_samples = 0usize;
    let outcome = pump(&mut socket, &incoming, shared, cancel, &mut sent_samples);
    let _ = socket.close(None);
    let _ = socket.flush();
    match outcome {
        Ok(()) => tracing::info!(
            audio_ms = sent_samples / 16,
            session_ms = started.elapsed().as_millis() as u64,
            "scribe live session ended"
        ),
        Err(reason) => {
            tracing::warn!(%reason, audio_ms = sent_samples / 16, "scribe live session failed");
            shared.fail(reason);
        }
    }
}

/// Sends queued audio and reads the server's text until the session's owner
/// drops it, cancels it, or it fails.
fn pump(
    socket: &mut Socket,
    incoming: &Receiver<Outgoing>,
    shared: &Shared,
    cancel: &CancelToken,
    sent_samples: &mut usize,
) -> Result<(), String> {
    loop {
        loop {
            match incoming.try_recv() {
                Ok(Outgoing::Audio(samples)) => {
                    for chunk in samples.chunks(MAX_CHUNK_SAMPLES) {
                        send(socket, &audio_message(chunk, false))?;
                        *sent_samples += chunk.len();
                    }
                }
                Ok(Outgoing::Commit) => send(socket, &audio_message(&[], true))?,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
        }
        if cancel.is_cancelled() {
            return Ok(());
        }
        match socket.read() {
            Ok(Message::Text(text)) => receive(text.as_str(), shared)?,
            Ok(Message::Close(_)) => return Err("ElevenLabs closed the live session".into()),
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return Err("ElevenLabs closed the live session".into());
            }
            Err(error) => return Err(format!("the live session's connection failed ({error})")),
        }
    }
}

fn send(socket: &mut Socket, message: &str) -> Result<(), String> {
    socket
        .send(Message::text(message))
        .map_err(|error| format!("the live session's connection failed ({error})"))
}

/// One `input_audio_chunk` message.
fn audio_message(samples: &[f32], commit: bool) -> String {
    serde_json::json!({
        "message_type": "input_audio_chunk",
        "audio_base_64": base64::engine::general_purpose::STANDARD.encode(pcm_s16le(samples)),
        "commit": commit,
        "sample_rate": 16_000,
    })
    .to_string()
}

/// Applies one server message. A message that ends the session is an error.
fn receive(text: &str, shared: &Shared) -> Result<(), String> {
    let Ok(message) = serde_json::from_str::<serde_json::Value>(text) else {
        return Ok(());
    };
    let kind = message["message_type"].as_str().unwrap_or_default();
    let body = |field: &str| {
        message[field]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    match kind {
        "partial_transcript" => {
            shared.lock().tentative = body("text");
            shared.changed.notify_all();
        }
        "committed_transcript" | "committed_transcript_with_timestamps" => {
            let segment = body("text");
            let mut state = shared.lock();
            if !segment.is_empty() {
                if !state.committed.is_empty() {
                    state.committed.push(' ');
                }
                state.committed.push_str(&segment);
            }
            state.tentative.clear();
            state.commits += 1;
            drop(state);
            shared.changed.notify_all();
        }
        // A commit with nothing said answers with one of these and no text.
        "insufficient_audio_activity" | "commit_throttled" => {
            let mut state = shared.lock();
            state.tentative.clear();
            state.commits += 1;
            drop(state);
            shared.changed.notify_all();
        }
        "session_started" | "warning" | "" => {}
        "auth_error" => return Err("ElevenLabs refused the API key".into()),
        "quota_exceeded" => {
            return Err("the ElevenLabs account has no transcription quota left".into());
        }
        "rate_limited" | "queue_overflow" | "resource_exhausted" => {
            return Err("ElevenLabs is rate-limiting this account".into());
        }
        "unaccepted_terms" => {
            return Err("the ElevenLabs account has not accepted the terms for this model".into());
        }
        other if other.ends_with("error") || other.contains("exceeded") => {
            let detail = body("error");
            return Err(if detail.is_empty() {
                format!("ElevenLabs ended the live session ({other})")
            } else {
                format!("ElevenLabs ended the live session ({other}: {detail})")
            });
        }
        _ => {}
    }
    Ok(())
}

/// The session's URL: the base with a WebSocket scheme, and its options.
fn url(config: &Config) -> Result<url::Url, String> {
    let base = config.base.trim_end_matches('/');
    let base = match base.split_once("://") {
        Some(("https", rest)) => format!("wss://{rest}"),
        Some(("http", rest)) => format!("ws://{rest}"),
        _ => return Err("the ElevenLabs address is not http or https".into()),
    };
    let mut url = url::Url::parse(&format!("{base}{PATH}"))
        .map_err(|_| "the ElevenLabs address is not a URL".to_string())?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("model_id", MODEL)
            .append_pair("audio_format", "pcm_16000")
            .append_pair("commit_strategy", "manual");
        if let Some(language) = &config.language {
            query.append_pair("language_code", language);
        }
        if config.no_verbatim {
            query.append_pair("no_verbatim", "true");
        }
        for term in config
            .keyterms
            .iter()
            .map(|term| term.trim())
            .filter(|term| keyterm_ok(term, MAX_KEYTERM_CHARS))
            .take(MAX_KEYTERMS)
        {
            query.append_pair("keyterms", term);
        }
    }
    Ok(url)
}

/// Connects and upgrades, all within [`CONNECT_TIMEOUT`], and leaves the
/// socket polling for reads.
fn connect(config: &Config) -> Result<Socket, String> {
    let url = url(config)?;
    let host = url.host_str().ok_or("the ElevenLabs address has no host")?;
    let port = url
        .port_or_known_default()
        .ok_or("the ElevenLabs address has no port")?;
    let unreachable = |reason: String| format!("couldn't reach ElevenLabs: {reason}");
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|_| unreachable("the name couldn't be looked up (DNS)".into()))?;
    let mut stream = None;
    let mut last = "no address".to_string();
    for address in addresses {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            last = "the connection timed out".into();
            break;
        }
        match TcpStream::connect_timeout(&address, left) {
            Ok(connected) => {
                stream = Some(connected);
                break;
            }
            Err(error) => last = format!("the connection failed ({error})"),
        }
    }
    let stream = stream.ok_or_else(|| unreachable(last))?;
    let left = deadline
        .saturating_duration_since(Instant::now())
        .max(Duration::from_millis(250));
    let timeouts = |read: Duration| -> std::io::Result<()> {
        stream.set_read_timeout(Some(read))?;
        stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
        stream.set_nodelay(true)
    };
    timeouts(left).map_err(|error| unreachable(error.to_string()))?;
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|_| "the ElevenLabs address is not a WebSocket URL".to_string())?;
    let key = tungstenite::http::HeaderValue::from_str(&config.key)
        .map_err(|_| "the API key is not valid in a header".to_string())?;
    request.headers_mut().insert("xi-api-key", key);
    let poll = stream
        .try_clone()
        .map_err(|error| unreachable(error.to_string()))?;
    let (socket, _response) =
        tungstenite::client_tls(request, stream).map_err(|error| match error {
            tungstenite::HandshakeError::Failure(tungstenite::Error::Http(response)) => {
                match response.status().as_u16() {
                    401 | 403 => "ElevenLabs refused the API key".to_string(),
                    status => format!("ElevenLabs refused the live session (HTTP {status})"),
                }
            }
            tungstenite::HandshakeError::Failure(error) => unreachable(error.to_string()),
            tungstenite::HandshakeError::Interrupted(_) => {
                unreachable("the connection timed out".into())
            }
        })?;
    // The clone shares the socket, so this sets the session's read poll.
    poll.set_read_timeout(Some(READ_POLL))
        .map_err(|error| unreachable(error.to_string()))?;
    Ok(socket)
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    /// A fake realtime endpoint on loopback: `script` drives one accepted
    /// WebSocket and returns what it wants checked.
    // The handshake callback's error type is tungstenite's to choose.
    #[allow(clippy::result_large_err)]
    fn serve<R: Send + 'static>(
        script: impl FnOnce(&mut WebSocket<TcpStream>, String) -> R + Send + 'static,
    ) -> (String, std::thread::JoinHandle<R>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut target = String::new();
            let mut socket = tungstenite::accept_hdr(
                stream,
                |request: &tungstenite::handshake::server::Request, response| {
                    target = format!(
                        "{} key={}",
                        request.uri(),
                        request
                            .headers()
                            .get("xi-api-key")
                            .and_then(|value| value.to_str().ok())
                            .unwrap_or_default()
                    );
                    Ok(response)
                },
            )
            .unwrap();
            script(&mut socket, target)
        });
        (base, server)
    }

    fn config(base: &str, provisional: bool) -> Config {
        Config {
            base: base.to_string(),
            key: "sk_live_test".into(),
            language: Some("en".into()),
            keyterms: vec![
                "Hex".into(),
                "a term far longer than twenty characters".into(),
            ],
            no_verbatim: true,
            provisional,
        }
    }

    fn next_json(socket: &mut WebSocket<TcpStream>) -> serde_json::Value {
        loop {
            if let Message::Text(text) = socket.read().unwrap() {
                return serde_json::from_str(text.as_str()).unwrap();
            }
        }
    }

    fn say(socket: &mut WebSocket<TcpStream>, value: serde_json::Value) {
        socket.send(Message::text(value.to_string())).unwrap();
    }

    fn wait_for(session: &LiveSession, ready: impl Fn(&Partial) -> bool) -> Partial {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let partial = session.partial();
            if ready(&partial) {
                return partial;
            }
            assert!(Instant::now() < deadline, "no partial in time: {partial:?}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn audio_goes_up_partials_come_back_and_a_commit_returns_the_text() {
        let (base, server) = serve(|socket, target| {
            say(
                socket,
                serde_json::json!({"message_type": "session_started"}),
            );
            let audio = next_json(socket);
            say(
                socket,
                serde_json::json!({"message_type": "partial_transcript", "text": "hello wor"}),
            );
            let commit = next_json(socket);
            say(
                socket,
                serde_json::json!({"message_type": "committed_transcript", "text": " Hello world. "}),
            );
            (target, audio, commit)
        });
        let cancel = CancelToken::new();
        let mut session = LiveSession::open(config(&base, false), &cancel).unwrap();

        session.feed(&vec![0.5; 1_600]).unwrap();
        let partial = wait_for(&session, |partial| !partial.tentative.is_empty());
        assert_eq!(partial.stable, "");
        assert_eq!(partial.tentative, "hello wor");
        assert_eq!(session.finalize().unwrap(), "Hello world.");

        let (target, audio, commit) = server.join().unwrap();
        assert!(
            target.starts_with("/v1/speech-to-text/realtime?"),
            "{target}"
        );
        for expected in [
            "model_id=scribe_v3_live",
            "audio_format=pcm_16000",
            "commit_strategy=manual",
            "language_code=en",
            "no_verbatim=true",
            "keyterms=Hex",
            "key=sk_live_test",
        ] {
            assert!(
                target.contains(expected),
                "{expected} missing from {target}"
            );
        }
        assert!(!target.contains("twenty"), "an overlong keyterm was sent");
        assert_eq!(audio["message_type"], "input_audio_chunk");
        assert_eq!(audio["commit"], false);
        assert_eq!(audio["sample_rate"], 16_000);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(audio["audio_base_64"].as_str().unwrap())
            .unwrap();
        assert_eq!(bytes.len(), 1_600 * 2);
        assert_eq!(commit["commit"], true);
        assert_eq!(commit["audio_base_64"], "");
    }

    #[test]
    fn committed_segments_accumulate_ahead_of_the_tentative_text() {
        let (base, server) = serve(|socket, _| {
            next_json(socket);
            say(
                socket,
                serde_json::json!({"message_type": "committed_transcript", "text": "First part."}),
            );
            say(
                socket,
                serde_json::json!({"message_type": "partial_transcript", "text": "second"}),
            );
            next_json(socket);
            say(
                socket,
                serde_json::json!({"message_type": "committed_transcript", "text": "Second part."}),
            );
        });
        let cancel = CancelToken::new();
        let mut session = LiveSession::open(config(&base, false), &cancel).unwrap();

        session.feed(&vec![0.1; 800]).unwrap();
        let partial = wait_for(&session, |partial| !partial.tentative.is_empty());
        assert_eq!(partial.stable, "First part.");
        assert_eq!(partial.tentative, " second");
        assert_eq!(session.finalize().unwrap(), "First part. Second part.");
        server.join().unwrap();
    }

    #[test]
    fn a_provisional_session_never_gives_the_transcript() {
        let (base, server) = serve(|socket, _| {
            next_json(socket);
        });
        let cancel = CancelToken::new();
        let mut session = LiveSession::open(config(&base, true), &cancel).unwrap();

        session.feed(&vec![0.1; 800]).unwrap();
        assert_eq!(session.finalize().unwrap_err(), PROVISIONAL);
        server.join().unwrap();
    }

    #[test]
    fn a_refused_key_fails_the_next_feed() {
        let (base, server) = serve(|socket, _| {
            say(
                socket,
                serde_json::json!({"message_type": "auth_error", "error": "bad key"}),
            );
        });
        let cancel = CancelToken::new();
        let mut session = LiveSession::open(config(&base, false), &cancel).unwrap();
        server.join().unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let error = loop {
            match session.feed(&[0.0; 160]) {
                Err(error) => break error,
                Ok(()) => assert!(Instant::now() < deadline, "the failure never surfaced"),
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(error, "ElevenLabs refused the API key");
        assert_eq!(session.finalize().unwrap_err(), error);
    }

    #[test]
    fn a_host_that_is_down_fails_the_session_not_the_caller() {
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", closed.local_addr().unwrap());
        drop(closed);
        let cancel = CancelToken::new();
        let mut session = LiveSession::open(config(&base, false), &cancel).unwrap();

        let error = session.finalize().unwrap_err();
        assert!(error.starts_with("couldn't reach ElevenLabs"), "{error}");
    }

    #[test]
    fn a_cancelled_finish_stops_waiting() {
        let (base, server) = serve(|socket, _| {
            // Never answers the commit.
            next_json(socket);
            let _ = socket.read();
        });
        let cancel = CancelToken::new();
        let mut session = LiveSession::open(config(&base, false), &cancel).unwrap();
        let canceller = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            canceller.cancel();
        });

        let error = session.finalize().unwrap_err();
        assert_eq!(error, "the live session was cancelled");
        drop(session);
        server.join().unwrap();
    }

    #[test]
    fn a_commit_with_nothing_said_returns_no_text() {
        let (base, server) = serve(|socket, _| {
            next_json(socket);
            say(
                socket,
                serde_json::json!({"message_type": "insufficient_audio_activity", "error": "quiet"}),
            );
        });
        let cancel = CancelToken::new();
        let mut session = LiveSession::open(config(&base, false), &cancel).unwrap();

        assert_eq!(session.finalize().unwrap(), "");
        server.join().unwrap();
    }
}
