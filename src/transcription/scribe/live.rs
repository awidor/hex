//! A realtime Speech to Text session (`scribe_v3_live`) as the engine's live
//! stream: audio goes up as it is recorded, partial text comes back for live
//! text, and a commit at release returns the final text.
//!
//! One thread owns the WebSocket. The engine thread only queues audio and
//! reads the latest text, so connecting, a slow network, or a dead one never
//! holds a capture: a failed session reports itself on the next feed or at
//! the finish, and the engine transcribes the whole clip instead.
//!
//! The session commits by itself at a pause once a segment is long
//! ([`CommitPolicy`]). ElevenLabs otherwise commits on its own after about
//! 35 s of audio, mid-word, and against the real service the commit that
//! followed such an automatic one came back empty: the words after it were
//! lost. A session that sees an automatic commit anyway, or gets no text for
//! words it had shown, reports itself unusable so the whole clip decides.
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

/// Uncommitted audio after which the session commits at the next pause.
const COMMIT_AFTER_SAMPLES: usize = 15 * 16_000;
/// The pause that commit waits for.
const COMMIT_PAUSE_SAMPLES: usize = 4_800;
/// Uncommitted audio after which a shorter pause is enough.
const COMMIT_SOON_SAMPLES: usize = 22 * 16_000;
const COMMIT_SOON_PAUSE_SAMPLES: usize = 1_920;
/// Uncommitted audio after which the session commits without a pause, well
/// before ElevenLabs' own commit at about 35 s.
const COMMIT_NOW_SAMPLES: usize = 28 * 16_000;
/// ElevenLabs refuses a commit of less uncommitted audio than this.
const MIN_COMMIT_SAMPLES: usize = 4_800;

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
    /// Commits this session sent, and how many of them were answered.
    commits_sent: u64,
    commits_answered: u64,
    /// The finish's commit was sent.
    final_sent: bool,
    /// ElevenLabs committed without being asked.
    auto_committed: bool,
    /// The finish's commit came back empty although words had been shown.
    dropped_tail: bool,
    failed: Option<String>,
}

impl State {
    /// Every commit sent has its answer, the finish's included.
    fn finished(&self) -> bool {
        self.final_sent && self.commits_answered >= self.commits_sent
    }

    /// One answer to a commit. `text` is the committed segment, `None` when
    /// ElevenLabs answered that there was nothing to commit.
    fn answer(&mut self, text: Option<&str>) {
        let asked = self.commits_answered < self.commits_sent;
        if asked {
            self.commits_answered += 1;
        } else if text.is_some() {
            self.auto_committed = true;
        }
        let segment = text.unwrap_or_default();
        if asked && self.finished() && segment.is_empty() && !self.tentative.is_empty() {
            self.dropped_tail = true;
        }
        if !segment.is_empty() {
            if !self.committed.is_empty() {
                self.committed.push(' ');
            }
            self.committed.push_str(segment);
        }
        self.tentative.clear();
    }
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
        if let Some(reason) = &self.shared.lock().failed {
            return Err(reason.clone());
        }
        self.outgoing
            .send(Outgoing::Commit)
            .map_err(|_| self.ended())?;
        let deadline = Instant::now() + COMMIT_TIMEOUT;
        let mut state = self.shared.lock();
        loop {
            // An answer that arrived stands, even when the connection closed
            // right after it.
            if state.finished() {
                if state.auto_committed {
                    return Err("ElevenLabs committed the live session on its own".into());
                }
                if state.dropped_tail {
                    return Err("the live session returned no text for its last words".into());
                }
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

/// When the session commits by itself: at a pause once the segment is long,
/// at a shorter pause once it is longer, and without one before ElevenLabs
/// would commit on its own.
#[derive(Default)]
struct CommitPolicy {
    speech: crate::transcription::pause_segmenter::SpeechFrames,
    /// Samples not yet a whole 20 ms frame.
    frame: Vec<f32>,
    /// Audio sent since the last commit.
    uncommitted: usize,
    /// Quiet audio at the end of what was sent.
    quiet: usize,
}

impl CommitPolicy {
    /// Takes `samples` in, returning the length of the prefix after which
    /// to commit, when a commit is due inside them.
    fn commit_within(&mut self, samples: &[f32]) -> Option<usize> {
        use crate::transcription::pause_segmenter::FRAME_SAMPLES;
        let mut taken = 0;
        while taken < samples.len() {
            let take = (FRAME_SAMPLES - self.frame.len()).min(samples.len() - taken);
            self.frame.extend_from_slice(&samples[taken..taken + take]);
            taken += take;
            self.uncommitted += take;
            if self.frame.len() < FRAME_SAMPLES {
                break;
            }
            if self.speech.classify(&self.frame).quiet {
                self.quiet += FRAME_SAMPLES;
            } else {
                self.quiet = 0;
            }
            self.frame.clear();
            let due = self.uncommitted >= COMMIT_NOW_SAMPLES
                || (self.uncommitted >= COMMIT_SOON_SAMPLES
                    && self.quiet >= COMMIT_SOON_PAUSE_SAMPLES)
                || (self.uncommitted >= COMMIT_AFTER_SAMPLES && self.quiet >= COMMIT_PAUSE_SAMPLES);
            if due {
                self.committed();
                return Some(taken);
            }
        }
        None
    }

    fn committed(&mut self) {
        self.uncommitted = 0;
        self.quiet = 0;
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
    let mut policy = CommitPolicy::default();
    loop {
        loop {
            match incoming.try_recv() {
                Ok(Outgoing::Audio(samples)) => {
                    let mut rest = samples.as_slice();
                    while !rest.is_empty() {
                        let cut = policy.commit_within(rest);
                        let (now, later) = rest.split_at(cut.unwrap_or(rest.len()));
                        for chunk in now.chunks(MAX_CHUNK_SAMPLES) {
                            send(socket, &audio_message(chunk, false))?;
                            *sent_samples += chunk.len();
                        }
                        if cut.is_some() {
                            commit(socket, shared, false)?;
                        }
                        rest = later;
                    }
                }
                Ok(Outgoing::Commit) => {
                    // ElevenLabs refuses a commit of almost no audio; then
                    // the commits already answered are the whole text.
                    if policy.uncommitted >= MIN_COMMIT_SAMPLES {
                        commit(socket, shared, true)?;
                    } else {
                        shared.lock().final_sent = true;
                        shared.changed.notify_all();
                    }
                    policy.committed();
                }
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

/// Sends a commit and counts it, so its answer is told from an automatic
/// commit; `last` marks the finish's.
fn commit(socket: &mut Socket, shared: &Shared, last: bool) -> Result<(), String> {
    // Counted before it is sent: its answer cannot arrive first.
    {
        let mut state = shared.lock();
        state.commits_sent += 1;
        state.final_sent |= last;
    }
    send(socket, &audio_message(&[], true))
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
            shared.lock().answer(Some(&body("text")));
            shared.changed.notify_all();
        }
        // A commit with nothing said answers with one of these and no text.
        "insufficient_audio_activity" | "commit_throttled" => {
            shared.lock().answer(None);
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

        session.feed(&vec![0.5; 6_400]).unwrap();
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
        assert_eq!(bytes.len(), 6_400 * 2);
        assert_eq!(commit["commit"], true);
        assert_eq!(commit["audio_base_64"], "");
    }

    /// A tone with a quiet stretch, as a sentence, a pause, and another.
    fn speech(samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|index| (index as f32 * 0.05).sin() * 0.3)
            .collect()
    }

    /// Reads audio messages until a commit arrives; returns the samples seen.
    fn audio_until_commit(socket: &mut WebSocket<TcpStream>) -> usize {
        let mut samples = 0;
        loop {
            let message = next_json(socket);
            if message["commit"] == true {
                return samples;
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(message["audio_base_64"].as_str().unwrap())
                .unwrap();
            samples += bytes.len() / 2;
        }
    }

    #[test]
    fn a_long_segment_is_committed_at_its_next_pause() {
        let (base, server) = serve(|socket, _| {
            let first = audio_until_commit(socket);
            say(
                socket,
                serde_json::json!({"message_type": "committed_transcript", "text": "First part."}),
            );
            say(
                socket,
                serde_json::json!({"message_type": "partial_transcript", "text": "second"}),
            );
            let second = audio_until_commit(socket);
            say(
                socket,
                serde_json::json!({"message_type": "committed_transcript", "text": "Second part."}),
            );
            (first, second)
        });
        let cancel = CancelToken::new();
        let mut session = LiveSession::open(config(&base, false), &cancel).unwrap();

        // Quiet first, as a capture's pre-roll is, so the noise floor starts
        // low. Then 16 s of speech, a 0.5 s pause, and 2 s more: the pause is
        // where the long segment is committed, with no finish asked yet.
        let mut clip = vec![0.0; 8_000];
        clip.extend(speech(16 * 16_000));
        clip.extend(vec![0.0; 8_000]);
        clip.extend(speech(2 * 16_000));
        for chunk in clip.chunks(1_600) {
            session.feed(chunk).unwrap();
        }
        let partial = wait_for(&session, |partial| !partial.tentative.is_empty());
        assert_eq!(partial.stable, "First part.");
        assert_eq!(partial.tentative, " second");
        assert_eq!(session.finalize().unwrap(), "First part. Second part.");

        let (first, second) = server.join().unwrap();
        // Committed inside the pause: after the speech and 0.3 s of quiet.
        assert_eq!(first, 8_000 + 16 * 16_000 + COMMIT_PAUSE_SAMPLES);
        assert_eq!(first + second, clip.len());
    }

    #[test]
    fn continuous_speech_is_committed_before_elevenlabs_would() {
        let mut policy = CommitPolicy::default();
        // Speech without a pause: one 20 ms frame in ten dips, as syllables
        // do, which keeps the noise floor low and is far too short to
        // commit at.
        let mut clip = vec![0.0; 8_000];
        for frame in 0..(40 * 50) {
            if frame % 10 == 9 {
                clip.extend(vec![0.0; 320]);
            } else {
                clip.extend(speech(320));
            }
        }
        let cut = policy.commit_within(&clip).expect("a commit is due");
        assert_eq!(cut, COMMIT_NOW_SAMPLES);
        assert!(cut < 35 * 16_000, "before the automatic commit");
        // The count starts again after it.
        assert_eq!(policy.commit_within(&clip[cut..]), None);
    }

    #[test]
    fn a_session_elevenlabs_committed_on_its_own_is_not_trusted() {
        let (base, server) = serve(|socket, _| {
            next_json(socket);
            // Nothing asked for this one.
            say(
                socket,
                serde_json::json!({"message_type": "committed_transcript", "text": "First part-"}),
            );
            next_json(socket);
            say(
                socket,
                serde_json::json!({"message_type": "committed_transcript", "text": ""}),
            );
        });
        let cancel = CancelToken::new();
        let mut session = LiveSession::open(config(&base, false), &cancel).unwrap();

        session.feed(&speech(6_400)).unwrap();
        wait_for(&session, |partial| !partial.stable.is_empty());
        let error = session.finalize().unwrap_err();
        assert_eq!(error, "ElevenLabs committed the live session on its own");
        server.join().unwrap();
    }

    #[test]
    fn a_finish_that_returns_nothing_for_shown_words_is_not_trusted() {
        let (base, server) = serve(|socket, _| {
            next_json(socket);
            say(
                socket,
                serde_json::json!({"message_type": "partial_transcript", "text": "the last words"}),
            );
            next_json(socket);
            say(
                socket,
                serde_json::json!({"message_type": "committed_transcript", "text": ""}),
            );
        });
        let cancel = CancelToken::new();
        let mut session = LiveSession::open(config(&base, false), &cancel).unwrap();

        session.feed(&speech(6_400)).unwrap();
        wait_for(&session, |partial| !partial.tentative.is_empty());
        let error = session.finalize().unwrap_err();
        assert_eq!(
            error,
            "the live session returned no text for its last words"
        );
        server.join().unwrap();
    }

    #[test]
    fn a_finish_with_almost_no_new_audio_sends_no_commit() {
        // ElevenLabs refuses a commit of under 0.3 s of audio.
        let (base, server) = serve(|socket, _| {
            next_json(socket);
            // The session ends without a commit: the next read is its close.
            matches!(socket.read(), Ok(Message::Text(_)))
        });
        let cancel = CancelToken::new();
        let mut session = LiveSession::open(config(&base, false), &cancel).unwrap();

        session.feed(&speech(1_600)).unwrap();
        assert_eq!(session.finalize().unwrap(), "");
        drop(session);
        assert!(!server.join().unwrap(), "no commit was sent");
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
            audio_until_commit(socket);
            let _ = socket.read();
        });
        let cancel = CancelToken::new();
        let mut session = LiveSession::open(config(&base, false), &cancel).unwrap();
        session.feed(&speech(6_400)).unwrap();
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

    /// The one test that opens a real session, and so the only check of the
    /// realtime protocol as ElevenLabs speaks it. It is billed (about three
    /// seconds of audio) and runs only when asked:
    /// `ELEVENLABS_API_KEY=… cargo test --bin hex the_real_live_api -- --ignored --nocapture`.
    #[test]
    #[ignore = "streams three seconds of audio to ElevenLabs with ELEVENLABS_API_KEY"]
    fn the_real_live_api_streams_partials_and_commits_the_text() {
        let key = std::env::var("ELEVENLABS_API_KEY").expect("ELEVENLABS_API_KEY is set");
        // "Send the patch to OpenCode for review.", or the 16 kHz mono WAV
        // HEX_LIVE_TEST_WAV names (a long one crosses ElevenLabs' own commit
        // at about 36 s; its text is then only printed).
        let own = std::env::var("HEX_LIVE_TEST_WAV").ok();
        let wav = own.clone().unwrap_or_else(|| {
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/vocabulary/audio/term-opencode-2.wav"
            )
            .to_string()
        });
        let samples: Vec<f32> = hound::WavReader::open(wav)
            .unwrap()
            .samples::<i16>()
            .map(|sample| f32::from(sample.unwrap()) / f32::from(i16::MAX))
            .collect();
        let cancel = CancelToken::new();
        let mut session = LiveSession::open(
            Config {
                base: crate::transcription::scribe::client::DEFAULT_BASE.into(),
                key: key.trim().into(),
                language: Some("en".into()),
                keyterms: vec!["OpenCode".into()],
                no_verbatim: false,
                provisional: false,
            },
            &cancel,
        )
        .unwrap();

        // At speaking pace, as a held shortcut feeds it.
        let mut heard_partial = false;
        for chunk in samples.chunks(1_600) {
            session.feed(chunk).expect("the session takes audio");
            std::thread::sleep(Duration::from_millis(100));
            let partial = session.partial();
            heard_partial |= !partial.stable.is_empty() || !partial.tentative.is_empty();
        }
        let committed = Instant::now();
        let text = session.finalize().expect("the commit is answered");
        println!(
            "text: {text:?}; partial while speaking: {heard_partial}; commit answered in {} ms",
            committed.elapsed().as_millis()
        );
        assert!(
            own.is_some() || text.to_lowercase().contains("patch"),
            "{text:?}"
        );
    }

    #[test]
    fn a_commit_with_nothing_said_returns_no_text() {
        let (base, server) = serve(|socket, _| {
            audio_until_commit(socket);
            say(
                socket,
                serde_json::json!({"message_type": "insufficient_audio_activity", "error": "quiet"}),
            );
        });
        let cancel = CancelToken::new();
        let mut session = LiveSession::open(config(&base, false), &cancel).unwrap();

        session.feed(&vec![0.0; 6_400]).unwrap();
        assert_eq!(session.finalize().unwrap(), "");
        server.join().unwrap();
    }
}
