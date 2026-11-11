//! ElevenLabs Scribe, the speech-to-text service every transcript comes from.
//!
//! [`ScribeTranscriber`] is the engine's model. It holds no weights: a
//! selection is a language, a whole clip is one HTTPS request
//! ([`client`], `scribe_v3`), and a live stream is one WebSocket session
//! ([`live`], `scribe_v3_live`). Audio leaves this computer for
//! ElevenLabs on every transcription; nothing is transcribed locally.

pub mod client;
pub mod error;
pub mod key;
pub mod live;

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};
use serde::{Deserialize, Serialize};

pub use client::NumbersFormat;
pub use error::ScribeError;

use crate::transcription::cancel::{CancelToken, job_cancel};
use crate::transcription::engine::StreamFeed;
use crate::transcription::models::{AUTO_LANGUAGE, TranscriptionSelection, validate};
use crate::transcription::transcript::{TimestampLevel, Transcript};

/// A word or segment's start and end (ms from the clip's start) and text.
pub type TimedSpan = (i64, i64, String);

/// Which model's text a dictation pastes.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalText {
    /// The whole clip, transcribed by `scribe_v3` at release: the most
    /// accurate. A live session, when one runs, only shows provisional words.
    #[default]
    Batch,
    /// The live session's committed text: the fastest, and no second upload.
    /// The whole clip is still used when the session fails.
    Live,
}

impl FinalText {
    pub const ALL: [Self; 2] = [Self::Batch, Self::Live];

    /// The name settings save it under.
    #[cfg(test)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Batch => "batch",
            Self::Live => "live",
        }
    }
}

/// Settings › General › Transcription (`scribe` in `settings.json`).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct ScribeSettings {
    /// Leave out filler words, false starts, and non-speech sounds.
    pub no_verbatim: bool,
    /// Whether numbers are written as digits or words.
    pub numbers_format: NumbersFormat,
    pub final_text: FinalText,
}

static NO_VERBATIM: AtomicBool = AtomicBool::new(false);
static NUMBERS: AtomicU8 = AtomicU8::new(0);
static FINAL_FROM_LIVE: AtomicBool = AtomicBool::new(false);

/// Called by `AppSettings` whenever settings load or save, so the next
/// request uses them.
pub(crate) fn apply(settings: ScribeSettings) {
    NO_VERBATIM.store(settings.no_verbatim, Ordering::Relaxed);
    let numbers = NumbersFormat::ALL
        .iter()
        .position(|format| *format == settings.numbers_format)
        .unwrap_or(0);
    NUMBERS.store(numbers as u8, Ordering::Relaxed);
    FINAL_FROM_LIVE.store(settings.final_text == FinalText::Live, Ordering::Relaxed);
}

fn options() -> client::Options {
    client::Options {
        no_verbatim: NO_VERBATIM.load(Ordering::Relaxed),
        numbers: NumbersFormat::ALL
            .get(usize::from(NUMBERS.load(Ordering::Relaxed)))
            .copied()
            .unwrap_or_default(),
    }
}

/// Whether a dictation pastes its live session's text.
pub(crate) fn final_from_live() -> bool {
    FINAL_FROM_LIVE.load(Ordering::Relaxed)
}

/// Whether a dictation opens a live session: only when its text is shown
/// (`live_text`) or pasted, since a session is billed for all it hears.
pub(crate) fn stream_wanted(live_text: bool) -> bool {
    (live_text || final_from_live()) && key::is_set()
}

/// Where requests go: `HEX_ELEVENLABS_URL`, else ElevenLabs' own API.
pub fn base_url() -> String {
    std::env::var(client::BASE_ENVIRONMENT)
        .ok()
        .map(|base| base.trim().trim_end_matches('/').to_string())
        .filter(|base| base.starts_with("https://") || base.starts_with("http://"))
        .unwrap_or_else(|| client::DEFAULT_BASE.to_string())
}

/// When the connection was last warmed, so a held shortcut warms it once.
static WARMED: Mutex<Option<Instant>> = Mutex::new(None);
/// How long a warmed connection is trusted before it is warmed again.
const WARM_FOR: Duration = Duration::from_secs(20);
/// How long an unused connection stays in the pool: longer than most
/// dictations are held, so the one opened at the press is there at release.
const IDLE_CONNECTION: Duration = Duration::from_secs(60);

/// The one agent every request to `base` goes through, so a connection
/// opened ahead of a clip is the connection the clip is uploaded on.
static AGENT: Mutex<Option<(String, ureq::Agent)>> = Mutex::new(None);

fn agent_for(base: &str) -> ureq::Agent {
    let mut shared = AGENT.lock().unwrap_or_else(|error| error.into_inner());
    if let Some((held, agent)) = shared.as_ref()
        && held == base
    {
        return agent.clone();
    }
    let agent = crate::net::agent(base, |config| {
        config
            .http_status_as_error(false)
            .timeout_connect(Some(client::CONNECT_TIMEOUT))
            .max_idle_age(IDLE_CONNECTION)
    });
    *shared = Some((base.to_string(), agent.clone()));
    agent
}

/// Opens the HTTPS connection ahead of a clip, in the background, so the
/// upload at release does not wait for DNS and the TLS handshake. Called as
/// a capture starts. It sends no audio and no key, and does nothing without
/// a key (the clip could not be sent anyway) or when warmed moments ago.
pub(crate) fn warm() {
    // Unit tests never reach ElevenLabs. A recording asks on every pass of
    // its loop, so the cheap answers come first.
    if cfg!(test) || warmed_recently() || !key::is_set() {
        return;
    }
    let base = base_url();
    warm_with(agent_for(&base), base);
}

fn warmed_recently() -> bool {
    WARMED
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .is_some_and(|at| at.elapsed() < WARM_FOR)
}

fn warm_with(agent: ureq::Agent, base: String) {
    {
        let mut warmed = WARMED.lock().unwrap_or_else(|error| error.into_inner());
        if warmed.is_some_and(|at| at.elapsed() < WARM_FOR) {
            return;
        }
        *warmed = Some(Instant::now());
    }
    let spawned = std::thread::Builder::new()
        .name("scribe-warm".into())
        .spawn(move || {
            let started = Instant::now();
            let request = ureq::http::Request::builder()
                .method("HEAD")
                .uri(format!("{base}/"))
                .body(());
            let Ok(request) = request else { return };
            let request = agent
                .configure_request(request)
                .timeout_global(Some(client::CONNECT_TIMEOUT))
                .build();
            match agent.run(request) {
                Ok(_) => tracing::debug!(
                    warm_ms = started.elapsed().as_millis() as u64,
                    "scribe connection warmed"
                ),
                Err(error) => tracing::debug!(
                    reason = crate::net::reason(&error),
                    "scribe connection could not be warmed"
                ),
            }
        });
    if let Err(error) = spawned {
        tracing::debug!(%error, "could not start the scribe warm-up thread");
    }
}

/// The engine's model: Scribe for one language selection.
pub struct ScribeTranscriber {
    selection: TranscriptionSelection,
    agent: ureq::Agent,
    base: String,
}

impl ScribeTranscriber {
    /// A transcriber for `selection`. Nothing is sent and no key is needed
    /// yet: the key is read for each request, so one saved later applies to
    /// the next dictation.
    pub fn load(selection: &TranscriptionSelection) -> Result<Self> {
        validate(selection)?;
        let base = base_url();
        Ok(Self::at(selection.clone(), &base))
    }

    fn at(selection: TranscriptionSelection, base: &str) -> Self {
        Self {
            selection,
            agent: agent_for(base),
            base: base.to_string(),
        }
    }

    pub(crate) fn selection(&self) -> &TranscriptionSelection {
        &self.selection
    }

    pub(crate) fn matches_selection(&self, selection: &TranscriptionSelection) -> bool {
        &self.selection == selection
    }

    /// The transcriber for `selection`: this one, or a new one in its place
    /// when the selection differs. macOS activates each job's own selection.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn activate(&mut self, selection: &TranscriptionSelection) -> Result<&mut Self> {
        if !self.matches_selection(selection) {
            *self = Self::load(selection)?;
        }
        Ok(self)
    }

    /// The language sent with a request; `None` lets Scribe detect it.
    fn language(&self) -> Option<&str> {
        let language = self.selection.language.as_str();
        (language != AUTO_LANGUAGE && !language.is_empty()).then_some(language)
    }

    /// Transcribes 16 kHz mono `samples` to text.
    pub fn transcribe(&mut self, samples: &[f32]) -> Result<String> {
        Ok(self.request(samples, false)?.text)
    }

    /// Transcribes a complete 16 kHz clip. Scribe needs no padding, so this
    /// is [`Self::transcribe`] for callers that own the clip.
    pub(crate) fn transcribe_clip(&mut self, samples: Vec<f32>) -> Result<String> {
        self.transcribe(&samples)
    }

    /// A complete clip with Scribe's word times and the sentences they form.
    pub(crate) fn transcribe_timed(&mut self, samples: Vec<f32>) -> Result<Transcript> {
        Ok(self.request(&samples, true)?)
    }

    /// One meeting chunk with segment and word times.
    pub(crate) fn transcribe_segments(&mut self, samples: Vec<f32>) -> Result<Transcript> {
        Ok(self.request(&samples, true)?)
    }

    /// Text and word times (ms from the clip's start) for a capture.
    pub fn transcribe_words(&mut self, samples: &[f32]) -> Result<(String, Vec<TimedSpan>)> {
        let transcript = self.request(samples, true)?;
        let words = transcript
            .words
            .into_iter()
            .map(|word| (word.start_ms, word.end_ms, word.text))
            .collect();
        Ok((transcript.text, words))
    }

    /// Scribe times every word.
    pub(crate) fn timestamps(&self) -> TimestampLevel {
        TimestampLevel::Word
    }

    fn request(
        &mut self,
        samples: &[f32],
        word_times: bool,
    ) -> std::result::Result<Transcript, ScribeError> {
        let key = key::current().ok_or(ScribeError::MissingKey)?;
        self.request_with(&key.secret, samples, word_times)
    }

    fn request_with(
        &mut self,
        key: &str,
        samples: &[f32],
        word_times: bool,
    ) -> std::result::Result<Transcript, ScribeError> {
        let keyterms = crate::dictation::vocabulary::keyterms();
        let clip = client::Clip {
            samples,
            language: self.language(),
            keyterms: &keyterms,
            word_times,
            options: options(),
        };
        let cancel = job_cancel();
        client::transcribe(&self.agent, &self.base, key, &clip, cancel.as_ref())
    }

    /// Whether a live session can be opened at all. Whether a dictation
    /// wants one is [`stream_wanted`].
    pub(crate) fn streams(&self) -> bool {
        true
    }

    /// Opens a live session and hands it to `drive`. `provisional` sessions
    /// only show their text: their finish reports [`live::PROVISIONAL`].
    pub(crate) fn with_stream<R>(
        &mut self,
        cancel: &CancelToken,
        provisional: bool,
        drive: impl FnOnce(&mut dyn StreamFeed) -> R,
    ) -> std::result::Result<R, String> {
        let key = key::current().ok_or_else(|| ScribeError::MissingKey.to_string())?;
        let mut session = live::LiveSession::open(
            live::Config {
                base: self.base.clone(),
                key: key.secret,
                language: self.language().map(str::to_string),
                keyterms: crate::dictation::vocabulary::keyterms(),
                no_verbatim: options().no_verbatim,
                provisional,
            },
            cancel,
        )?;
        Ok(drive(&mut session))
    }
}

/// A transcriber for whichever selection was asked for last, for callers
/// that take a selection with each request (the local API, History).
#[derive(Default)]
pub(crate) struct WarmScribe {
    active: Option<ScribeTranscriber>,
}

impl WarmScribe {
    /// The transcriber for `selection`, replacing one for another selection.
    pub(crate) fn activate(
        &mut self,
        selection: &TranscriptionSelection,
    ) -> Result<&mut ScribeTranscriber> {
        if !self
            .active
            .as_ref()
            .is_some_and(|model| model.matches_selection(selection))
        {
            self.active = Some(ScribeTranscriber::load(selection)?);
        }
        self.active
            .as_mut()
            .ok_or_else(|| eyre!("the transcriber is unavailable"))
    }
}

/// Transcribes a WAV file's bytes with `selection`, for `hex transcribe`.
pub fn transcribe_wav_bytes(wav: &[u8], selection: &TranscriptionSelection) -> Result<String> {
    let clip = crate::transcription::service::AudioClip::decode_wav(std::io::Cursor::new(wav))
        .map_err(|error| eyre!("{error:?}"))?;
    ScribeTranscriber::load(selection)?.transcribe(clip.samples())
}

/// Checks the saved key against ElevenLabs without sending audio or spending
/// quota: `GET /v1/user`, which any valid key may read.
pub fn check_key(secret: &str) -> std::result::Result<(), ScribeError> {
    let base = base_url();
    let agent = crate::net::agent(&base, |config| {
        config
            .http_status_as_error(false)
            .timeout_connect(Some(client::CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(15)))
    });
    check_key_at(&agent, &base, secret)
}

fn check_key_at(
    agent: &ureq::Agent,
    base: &str,
    secret: &str,
) -> std::result::Result<(), ScribeError> {
    let request = ureq::http::Request::builder()
        .method("GET")
        .uri(format!("{base}/v1/user"))
        .header("xi-api-key", secret)
        .header("accept", "application/json")
        .body(())
        .map_err(|_| ScribeError::InvalidKey)?;
    let mut response = agent
        .run(request)
        .map_err(|error| ScribeError::Unreachable(crate::net::reason(&error)))?;
    let status = response.status().as_u16();
    if (200..300).contains(&status) {
        return Ok(());
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(64 * 1024)
        .read_to_string()
        .unwrap_or_default();
    // A key limited to Speech to Text may not read the account: it is valid.
    match ScribeError::from_response(status, &body) {
        ScribeError::Rejected { status: 403, .. } => Ok(()),
        error => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::client::tests::{HELLO, serve};
    use super::*;
    use crate::transcription::models::TranscriptionModelId;

    fn transcriber(base: &str, language: &str) -> ScribeTranscriber {
        let selection = TranscriptionSelection {
            model: TranscriptionModelId::ScribeV3,
            language: language.into(),
        };
        let mut transcriber = ScribeTranscriber::at(selection, base);
        transcriber.agent = super::client::tests::agent();
        transcriber
    }

    #[test]
    fn a_selection_is_a_language_and_auto_sends_none() {
        assert_eq!(
            transcriber("http://127.0.0.1:9", "de").language(),
            Some("de")
        );
        assert_eq!(transcriber("http://127.0.0.1:9", "auto").language(), None);
        let english = TranscriptionSelection {
            model: TranscriptionModelId::ScribeV3,
            language: "en".into(),
        };
        assert!(transcriber("http://127.0.0.1:9", "en").matches_selection(&english));
        assert!(!transcriber("http://127.0.0.1:9", "de").matches_selection(&english));
    }

    #[test]
    fn a_key_is_checked_without_audio() {
        let (base, server) = serve(vec![(200, r#"{"user_id":"u"}"#.into())]);

        check_key_at(&super::client::tests::agent(), &base, "sk_ok").unwrap();

        let received = server.join().unwrap();
        assert!(received[0].head.starts_with("GET /v1/user "));
        assert_eq!(received[0].header("xi-api-key").as_deref(), Some("sk_ok"));
        assert!(received[0].body.is_empty());
    }

    #[test]
    fn a_refused_key_is_reported_and_a_scoped_key_is_accepted() {
        let agent = super::client::tests::agent();
        let (base, server) = serve(vec![(
            401,
            r#"{"detail":{"status":"invalid_api_key","message":"Invalid API key"}}"#.into(),
        )]);
        assert_eq!(
            check_key_at(&agent, &base, "sk_bad"),
            Err(ScribeError::InvalidKey)
        );
        server.join().unwrap();

        // A key limited to Speech to Text cannot read the account.
        let (base, server) = serve(vec![(
            403,
            r#"{"detail":{"status":"missing_permissions","message":"no user_read"}}"#.into(),
        )]);
        assert_eq!(check_key_at(&agent, &base, "sk_scoped"), Ok(()));
        server.join().unwrap();
    }

    #[test]
    fn settings_reach_the_next_request() {
        apply(ScribeSettings {
            no_verbatim: true,
            numbers_format: NumbersFormat::Words,
            final_text: FinalText::Live,
        });
        assert_eq!(
            options(),
            client::Options {
                no_verbatim: true,
                numbers: NumbersFormat::Words
            }
        );
        assert!(final_from_live());
        apply(ScribeSettings::default());
        assert_eq!(options(), client::Options::default());
        assert!(!final_from_live());
    }

    #[test]
    fn words_come_back_as_spans_for_a_capture() {
        let (base, server) = serve(vec![(200, HELLO.into())]);
        let mut transcriber = transcriber(&base, "en");

        let transcript = transcriber
            .request_with("sk_test", &vec![0.0; 16_000], true)
            .unwrap();

        assert_eq!(transcript.text, "Hello there. How are you?");
        assert_eq!(transcript.words.len(), 5);
        assert_eq!(transcript.segments.len(), 2);
        let request = server.join().unwrap().remove(0);
        assert_eq!(request.field("language_code").as_deref(), Some("en"));
        assert_eq!(
            request.field("timestamps_granularity").as_deref(),
            Some("word")
        );
    }
}
