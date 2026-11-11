//! The batch Speech to Text request: one 16 kHz mono clip in, its text and
//! word times out (`POST /v1/speech-to-text`, model `scribe_v3`).
//!
//! The clip is sent as raw 16-bit PCM (`file_format=pcm_s16le_16`), which
//! ElevenLabs documents as its lowest-latency input and is exactly what the
//! engine holds, so nothing is encoded or written to disk.

use std::time::{Duration, Instant};

use serde::Deserialize;

use super::error::ScribeError;
use crate::transcription::cancel::CancelToken;
use crate::transcription::transcript::{Transcript, TranscriptSegment};

/// The batch model.
pub const MODEL: &str = "scribe_v3";
/// Where requests go unless [`BASE_ENVIRONMENT`] names another host, such
/// as a data-residency endpoint (`https://api.eu.residency.elevenlabs.io`).
pub const DEFAULT_BASE: &str = "https://api.elevenlabs.io";
pub const BASE_ENVIRONMENT: &str = "HEX_ELEVENLABS_URL";

const PATH: &str = "/v1/speech-to-text";
/// ElevenLabs refuses audio shorter than 100 ms; a shorter clip is padded
/// with silence to this many 16 kHz samples.
const MIN_SAMPLES: usize = 1_600;
/// Batch keyterm limits (the API reference's `keyterms`).
pub const MAX_KEYTERMS: usize = 1_000;
const MAX_KEYTERM_CHARS: usize = 50;
const MAX_KEYTERM_WORDS: usize = 5;
const KEYTERM_FORBIDDEN: [char; 7] = ['<', '>', '{', '}', '[', ']', '\\'];
/// How long connecting may take. A dictation waits on this, so it is shorter
/// than a download's.
pub(super) const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
/// A whole request's allowance before the clip's own share: an upload at a
/// slow uplink's pace plus the model's run.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// The answer is JSON text with word times; 16 MiB is hours of speech.
const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;
/// How long a failed request waits before its one retry.
const RETRY_PAUSE: Duration = Duration::from_millis(400);
/// A pause this long between words starts a new segment.
const SEGMENT_GAP_MS: i64 = 1_000;

/// How numbers are written (`numbers_format`, Scribe v3).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NumbersFormat {
    /// The model chooses.
    #[default]
    Auto,
    Digits,
    Words,
}

impl NumbersFormat {
    pub const ALL: [Self; 3] = [Self::Auto, Self::Digits, Self::Words];

    fn value(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Digits => Some("digits"),
            Self::Words => Some("words"),
        }
    }

    /// The name settings save it under.
    #[cfg(test)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Digits => "digits",
            Self::Words => "words",
        }
    }
}

/// What a person chose about how Scribe writes a transcript.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Options {
    /// Leave out filler words, false starts, and non-speech sounds
    /// (`no_verbatim`).
    pub no_verbatim: bool,
    pub numbers: NumbersFormat,
}

/// One clip and how to hear it.
pub struct Clip<'a> {
    /// 16 kHz mono samples.
    pub samples: &'a [f32],
    /// An ISO 639-1 or 639-3 code, or `None` to let the model detect it.
    pub language: Option<&'a str>,
    /// Names and terms to bias toward; ones the API would refuse are skipped.
    pub keyterms: &'a [String],
    /// Whether word times are wanted. Without them the model answers sooner.
    pub word_times: bool,
    pub options: Options,
}

/// Transcribes `clip`, retrying once when the failure may be passing.
/// `cancel` is checked before each attempt; a request already sent runs to
/// its timeout.
pub fn transcribe(
    agent: &ureq::Agent,
    base: &str,
    key: &str,
    clip: &Clip<'_>,
    cancel: Option<&CancelToken>,
) -> Result<Transcript, ScribeError> {
    let (content_type, body) = multipart(clip);
    let audio_ms = clip.samples.len() / 16;
    let timeout = REQUEST_TIMEOUT + Duration::from_millis(audio_ms as u64);
    let url = format!("{}{PATH}", base.trim_end_matches('/'));
    let started = Instant::now();
    let mut attempt = 1;
    loop {
        if cancel.is_some_and(CancelToken::is_cancelled) {
            return Err(ScribeError::Cancelled);
        }
        match send(agent, &url, key, &content_type, &body, timeout) {
            Ok(transcript) => {
                tracing::info!(
                    audio_ms,
                    upload_bytes = body.len(),
                    round_trip_ms = started.elapsed().as_millis() as u64,
                    attempt,
                    keyterms = keyterms(clip.keyterms).count(),
                    "scribe transcribed a clip"
                );
                return Ok(transcript);
            }
            Err(Failure { error, timed_out }) => {
                // A timeout already spent the clip's whole allowance.
                if attempt == 1 && error.retryable() && !timed_out {
                    tracing::warn!(%error, "scribe request failed; retrying once");
                    attempt += 1;
                    std::thread::sleep(RETRY_PAUSE);
                    continue;
                }
                tracing::warn!(
                    %error,
                    audio_ms,
                    round_trip_ms = started.elapsed().as_millis() as u64,
                    attempt,
                    "scribe request failed"
                );
                return Err(error);
            }
        }
    }
}

struct Failure {
    error: ScribeError,
    timed_out: bool,
}

fn send(
    agent: &ureq::Agent,
    url: &str,
    key: &str,
    content_type: &str,
    body: &[u8],
    timeout: Duration,
) -> Result<Transcript, Failure> {
    let fail = |error| Failure {
        error,
        timed_out: false,
    };
    let request = ureq::http::Request::builder()
        .method("POST")
        .uri(url)
        .header("xi-api-key", key)
        .header("content-type", content_type)
        .header("accept", "application/json")
        .body(body)
        .map_err(|_| fail(ScribeError::InvalidKey))?;
    let request = agent
        .configure_request(request)
        .timeout_global(Some(timeout))
        .build();
    let mut response = agent.run(request).map_err(|error| Failure {
        timed_out: matches!(error, ureq::Error::Timeout(_)),
        error: ScribeError::Unreachable(crate::net::reason(&error)),
    })?;
    let status = response.status().as_u16();
    let text = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_string()
        .map_err(|error| Failure {
            timed_out: matches!(error, ureq::Error::Timeout(_)),
            error: ScribeError::Unreachable(crate::net::reason(&error)),
        })?;
    if !(200..300).contains(&status) {
        return Err(fail(ScribeError::from_response(status, &text)));
    }
    parse(&text).map_err(fail)
}

/// The keyterms the API accepts, first in list order, up to its limit.
pub fn keyterms(terms: &[String]) -> impl Iterator<Item = &str> {
    terms
        .iter()
        .map(|term| term.trim())
        .filter(|term| keyterm_ok(term, MAX_KEYTERM_CHARS))
        .take(MAX_KEYTERMS)
}

/// Whether the API takes `term` as a keyterm of at most `max_chars`.
pub(super) fn keyterm_ok(term: &str, max_chars: usize) -> bool {
    !term.is_empty()
        && term.chars().count() <= max_chars
        && term.split_whitespace().count() <= MAX_KEYTERM_WORDS
        && !term.contains(KEYTERM_FORBIDDEN)
}

/// 16-bit little-endian PCM of `samples`, clamped.
pub(super) fn pcm_s16le(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// The request's `multipart/form-data` content type and body.
fn multipart(clip: &Clip<'_>) -> (String, Vec<u8>) {
    let boundary = boundary();
    let mut body = Vec::with_capacity(clip.samples.len().max(MIN_SAMPLES) * 2 + 2_048);
    let mut field = |name: &str, value: &str| {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    };
    field("model_id", MODEL);
    field("file_format", "pcm_s16le_16");
    // A dictation pastes what was said, never "(laughter)".
    field("tag_audio_events", "false");
    field("diarize", "false");
    field(
        "timestamps_granularity",
        if clip.word_times { "word" } else { "none" },
    );
    if let Some(language) = clip.language {
        field("language_code", language);
    }
    if clip.options.no_verbatim {
        field("no_verbatim", "true");
    }
    if let Some(numbers) = clip.options.numbers.value() {
        field("numbers_format", numbers);
    }
    // A list is one form field per item; a JSON array in one field is refused.
    for term in keyterms(clip.keyterms) {
        field("keyterms", term);
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; \
             filename=\"audio.pcm\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(&pcm_s16le(clip.samples));
    // Silence up to the API's minimum length.
    let missing = MIN_SAMPLES.saturating_sub(clip.samples.len());
    body.resize(body.len() + missing * 2, 0);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

/// A boundary no clip contains by chance: 32 random hex digits.
fn boundary() -> String {
    let mut random = [0u8; 16];
    if getrandom::fill(&mut random).is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        random = nanos.to_le_bytes();
    }
    let digits: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("hex-scribe-{digits}")
}

#[derive(Deserialize)]
struct Body {
    text: String,
    #[serde(default)]
    words: Vec<BodyWord>,
}

#[derive(Deserialize)]
struct BodyWord {
    #[serde(default)]
    text: String,
    #[serde(default)]
    start: Option<f64>,
    #[serde(default)]
    end: Option<f64>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
}

/// The transcript in a `200` answer. Words are the API's own; segments are
/// its words grouped into sentences, with the first word's start and the
/// last word's end, since Scribe names no segments itself.
fn parse(text: &str) -> Result<Transcript, ScribeError> {
    let body: Body = serde_json::from_str(text).map_err(|error| {
        ScribeError::Malformed(format!("line {}, column {}", error.line(), error.column()))
    })?;
    let mut words = Vec::new();
    let mut segments: Vec<TranscriptSegment> = Vec::new();
    let mut open: Option<TranscriptSegment> = None;
    for entry in &body.words {
        match entry.kind.as_deref() {
            Some("audio_event") => continue,
            Some("spacing") => {
                if let Some(segment) = open.as_mut() {
                    segment.text.push_str(&entry.text);
                }
                continue;
            }
            _ => {}
        }
        let (Some(start), Some(end)) = (entry.start, entry.end) else {
            continue;
        };
        let (start_ms, end_ms) = (milliseconds(start), milliseconds(end));
        let word = entry.text.trim();
        if word.is_empty() {
            continue;
        }
        words.push(TranscriptSegment {
            start_ms,
            end_ms,
            text: word.to_string(),
        });
        if open
            .as_ref()
            .is_some_and(|segment| start_ms - segment.end_ms >= SEGMENT_GAP_MS)
        {
            segments.extend(open.take().map(trimmed));
        }
        let segment = open.get_or_insert_with(|| TranscriptSegment {
            start_ms,
            end_ms,
            text: String::new(),
        });
        segment.text.push_str(&entry.text);
        segment.end_ms = end_ms;
        if word.ends_with(['.', '?', '!', '。', '？', '！']) {
            segments.extend(open.take().map(trimmed));
        }
    }
    segments.extend(open.take().map(trimmed));
    Ok(Transcript {
        text: body.text.trim().to_string(),
        segments,
        words,
    })
}

fn trimmed(mut segment: TranscriptSegment) -> TranscriptSegment {
    segment.text = segment.text.trim().to_string();
    segment
}

fn milliseconds(seconds: f64) -> i64 {
    (seconds * 1_000.0).round() as i64
}

#[cfg(test)]
pub(super) mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    use super::*;

    /// One request a fake ElevenLabs received.
    pub(in crate::transcription::scribe) struct Received {
        pub head: String,
        pub body: Vec<u8>,
    }

    impl Received {
        /// The value of the form field `name`, when the request has it.
        pub fn field(&self, name: &str) -> Option<String> {
            self.fields(name).into_iter().next()
        }

        pub fn fields(&self, name: &str) -> Vec<String> {
            let body = String::from_utf8_lossy(&self.body);
            let marker = format!("name=\"{name}\"\r\n\r\n");
            body.match_indices(&marker)
                .filter_map(|(at, _)| {
                    let value = &body[at + marker.len()..];
                    value.find("\r\n").map(|end| value[..end].to_string())
                })
                .collect()
        }

        /// The uploaded file's bytes.
        pub fn file(&self) -> Vec<u8> {
            let marker = b"Content-Type: application/octet-stream\r\n\r\n";
            let start = self
                .body
                .windows(marker.len())
                .position(|window| window == marker)
                .expect("the request uploads a file")
                + marker.len();
            let boundary = self
                .head
                .lines()
                .find_map(|line| line.split("boundary=").nth(1))
                .expect("a multipart boundary")
                .trim()
                .to_string();
            let closing = format!("\r\n--{boundary}--\r\n");
            let end = self.body.len() - closing.len();
            assert_eq!(&self.body[end..], closing.as_bytes());
            self.body[start..end].to_vec()
        }

        pub fn header(&self, name: &str) -> Option<String> {
            self.head.lines().find_map(|line| {
                let (header, value) = line.split_once(':')?;
                header
                    .eq_ignore_ascii_case(name)
                    .then(|| value.trim().to_string())
            })
        }
    }

    /// A fake ElevenLabs on loopback that answers each connection with the
    /// next `(status, body)`, then returns what it received.
    pub(in crate::transcription::scribe) fn serve(
        answers: Vec<(u16, String)>,
    ) -> (String, std::thread::JoinHandle<Vec<Received>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut received = Vec::new();
            for (status, answer) in answers {
                let (mut stream, _) = listener.accept().unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0u8; 16_384];
                let (head_end, length) = loop {
                    let read = stream.read(&mut buffer).unwrap();
                    assert!(read > 0, "the request ended before its head");
                    bytes.extend_from_slice(&buffer[..read]);
                    if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&bytes[..end]).to_string();
                        let length = head
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            // A GET carries no body and no length.
                            .unwrap_or(0);
                        break (end + 4, length);
                    }
                };
                while bytes.len() < head_end + length {
                    let read = stream.read(&mut buffer).unwrap();
                    assert!(read > 0, "the request ended before its body");
                    bytes.extend_from_slice(&buffer[..read]);
                }
                received.push(Received {
                    head: String::from_utf8_lossy(&bytes[..head_end]).to_string(),
                    body: bytes[head_end..head_end + length].to_vec(),
                });
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                );
                stream.write_all(reply.as_bytes()).unwrap();
            }
            received
        });
        (base, server)
    }

    pub(in crate::transcription::scribe) fn agent() -> ureq::Agent {
        crate::net::loopback_agent(|config| config.http_status_as_error(false))
    }

    pub(in crate::transcription::scribe) const HELLO: &str = r#"{
        "language_code": "eng",
        "language_probability": 0.98,
        "text": " Hello there. How are you?",
        "words": [
            {"text": "Hello", "start": 0.10, "end": 0.42, "type": "word"},
            {"text": " ", "start": 0.42, "end": 0.50, "type": "spacing"},
            {"text": "there.", "start": 0.50, "end": 0.90, "type": "word"},
            {"text": " ", "start": 0.90, "end": 1.00, "type": "spacing"},
            {"text": "(laughter)", "start": 1.00, "end": 1.20, "type": "audio_event"},
            {"text": "How", "start": 1.30, "end": 1.45, "type": "word"},
            {"text": " ", "start": 1.45, "end": 1.50, "type": "spacing"},
            {"text": "are", "start": 1.50, "end": 1.62, "type": "word"},
            {"text": " ", "start": 1.62, "end": 1.70, "type": "spacing"},
            {"text": "you?", "start": 1.70, "end": 2.00, "type": "word"}
        ]
    }"#;

    fn clip<'a>(samples: &'a [f32], keyterms: &'a [String]) -> Clip<'a> {
        Clip {
            samples,
            language: Some("en"),
            keyterms,
            word_times: true,
            options: Options {
                no_verbatim: true,
                numbers: NumbersFormat::Digits,
            },
        }
    }

    #[test]
    fn a_clip_is_uploaded_as_raw_pcm_with_its_options() {
        let (base, server) = serve(vec![(200, HELLO.into())]);
        let samples = vec![0.5f32; 3_200];
        let terms = vec![
            "Hex".to_string(),
            "  Anomaly Innovations ".to_string(),
            "bad<term>".to_string(),
            "one two three four five six".to_string(),
            "x".repeat(51),
            String::new(),
        ];

        let transcript = transcribe(
            &agent(),
            &base,
            "sk_test_key",
            &clip(&samples, &terms),
            None,
        )
        .unwrap();

        assert_eq!(transcript.text, "Hello there. How are you?");
        let received = server.join().unwrap();
        let request = &received[0];
        assert!(request.head.starts_with("POST /v1/speech-to-text HTTP/1.1"));
        assert_eq!(request.header("xi-api-key").as_deref(), Some("sk_test_key"));
        assert_eq!(request.field("model_id").as_deref(), Some("scribe_v3"));
        assert_eq!(
            request.field("file_format").as_deref(),
            Some("pcm_s16le_16")
        );
        assert_eq!(request.field("tag_audio_events").as_deref(), Some("false"));
        assert_eq!(
            request.field("timestamps_granularity").as_deref(),
            Some("word")
        );
        assert_eq!(request.field("language_code").as_deref(), Some("en"));
        assert_eq!(request.field("no_verbatim").as_deref(), Some("true"));
        assert_eq!(request.field("numbers_format").as_deref(), Some("digits"));
        assert_eq!(request.fields("keyterms"), ["Hex", "Anomaly Innovations"]);
        let file = request.file();
        assert_eq!(file.len(), 3_200 * 2);
        assert_eq!(&file[..2], &16_384i16.to_le_bytes());
    }

    #[test]
    fn defaults_send_no_optional_field_and_no_word_times() {
        let (base, server) = serve(vec![(200, r#"{"text":"ok","words":[]}"#.into())]);
        let samples = vec![0.0f32; 16_000];
        let request = Clip {
            samples: &samples,
            language: None,
            keyterms: &[],
            word_times: false,
            options: Options::default(),
        };

        transcribe(&agent(), &base, "sk", &request, None).unwrap();

        let received = server.join().unwrap();
        let request = &received[0];
        assert_eq!(
            request.field("timestamps_granularity").as_deref(),
            Some("none")
        );
        for absent in ["language_code", "no_verbatim", "numbers_format", "keyterms"] {
            assert!(request.field(absent).is_none(), "{absent} must not be sent");
        }
    }

    #[test]
    fn a_clip_shorter_than_the_api_takes_is_padded_with_silence() {
        let (base, server) = serve(vec![(200, r#"{"text":"","words":[]}"#.into())]);
        let samples = vec![0.25f32; 400];

        transcribe(&agent(), &base, "sk", &clip(&samples, &[]), None).unwrap();

        let file = server.join().unwrap().remove(0).file();
        assert_eq!(file.len(), MIN_SAMPLES * 2);
        assert!(file[400 * 2..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn words_keep_their_times_and_group_into_sentences() {
        let transcript = parse(HELLO).unwrap();

        let words: Vec<_> = transcript
            .words
            .iter()
            .map(|word| (word.start_ms, word.end_ms, word.text.as_str()))
            .collect();
        assert_eq!(
            words,
            [
                (100, 420, "Hello"),
                (500, 900, "there."),
                (1_300, 1_450, "How"),
                (1_500, 1_620, "are"),
                (1_700, 2_000, "you?"),
            ]
        );
        let segments: Vec<_> = transcript
            .segments
            .iter()
            .map(|segment| (segment.start_ms, segment.end_ms, segment.text.as_str()))
            .collect();
        assert_eq!(
            segments,
            [(100, 900, "Hello there."), (1_300, 2_000, "How are you?")]
        );
    }

    #[test]
    fn a_long_pause_ends_a_segment_without_punctuation() {
        let transcript = parse(
            r#"{"text":"one two","words":[
                {"text":"one","start":0.0,"end":0.3,"type":"word"},
                {"text":" ","start":0.3,"end":0.4,"type":"spacing"},
                {"text":"two","start":1.5,"end":1.8,"type":"word"}]}"#,
        )
        .unwrap();

        assert_eq!(transcript.segments.len(), 2);
        assert_eq!(transcript.segments[0].text, "one");
        assert_eq!(transcript.segments[1].text, "two");
    }

    #[test]
    fn a_refused_key_fails_at_once_without_a_retry() {
        let (base, server) = serve(vec![(
            401,
            r#"{"detail":{"status":"invalid_api_key","message":"Invalid API key"}}"#.into(),
        )]);
        let samples = vec![0.0f32; 16_000];

        let error = transcribe(&agent(), &base, "sk_bad", &clip(&samples, &[]), None).unwrap_err();

        assert_eq!(error, ScribeError::InvalidKey);
        assert_eq!(server.join().unwrap().len(), 1);
    }

    #[test]
    fn a_server_failure_is_retried_once() {
        let (base, server) = serve(vec![(503, "busy".into()), (200, HELLO.into())]);
        let samples = vec![0.0f32; 16_000];

        let transcript = transcribe(&agent(), &base, "sk", &clip(&samples, &[]), None).unwrap();

        assert_eq!(transcript.text, "Hello there. How are you?");
        assert_eq!(server.join().unwrap().len(), 2);
    }

    #[test]
    fn a_second_server_failure_is_the_answer() {
        let (base, server) = serve(vec![(500, "no".into()), (502, "no".into())]);
        let samples = vec![0.0f32; 16_000];

        let error = transcribe(&agent(), &base, "sk", &clip(&samples, &[]), None).unwrap_err();

        assert_eq!(error, ScribeError::Server { status: 502 });
        assert_eq!(server.join().unwrap().len(), 2);
    }

    #[test]
    fn nothing_is_sent_for_a_cancelled_job() {
        let cancel = CancelToken::new();
        cancel.cancel();
        let samples = vec![0.0f32; 16_000];

        // No server listens: a request would fail as unreachable instead.
        let error = transcribe(
            &agent(),
            "http://127.0.0.1:9",
            "sk",
            &clip(&samples, &[]),
            Some(&cancel),
        )
        .unwrap_err();

        assert_eq!(error, ScribeError::Cancelled);
    }

    #[test]
    fn an_unreachable_host_is_named_as_such() {
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", closed.local_addr().unwrap());
        drop(closed);
        let samples = vec![0.0f32; 16_000];

        let error = transcribe(&agent(), &base, "sk", &clip(&samples, &[]), None).unwrap_err();

        assert!(matches!(error, ScribeError::Unreachable(_)), "{error:?}");
        assert!(error.worth_retrying_later());
    }

    /// The one test that reaches ElevenLabs, and so the only check of the
    /// real request shape. It is billed (one second of audio) and runs only
    /// when asked:
    /// `ELEVENLABS_API_KEY=… cargo test --bin hex the_real_api -- --ignored`.
    #[test]
    #[ignore = "sends one second of audio to ElevenLabs with ELEVENLABS_API_KEY"]
    fn the_real_api_accepts_the_request_hex_sends() {
        let key = std::env::var("ELEVENLABS_API_KEY").expect("ELEVENLABS_API_KEY is set");
        let agent = crate::net::agent(DEFAULT_BASE, |config| {
            config
                .http_status_as_error(false)
                .timeout_connect(Some(CONNECT_TIMEOUT))
        });
        let samples: Vec<f32> = (0..16_000)
            .map(|index| (index as f32 * 0.05).sin() * 0.2)
            .collect();
        let terms = vec!["Hex".to_string()];
        let request = Clip {
            samples: &samples,
            language: Some("en"),
            keyterms: &terms,
            word_times: true,
            options: Options {
                no_verbatim: true,
                numbers: NumbersFormat::Digits,
            },
        };

        // A tone has no words; what matters is that every field is accepted.
        let transcript = transcribe(&agent, DEFAULT_BASE, key.trim(), &request, None)
            .expect("ElevenLabs accepted the request");
        println!(
            "text: {:?}; words: {}",
            transcript.text,
            transcript.words.len()
        );
    }

    #[test]
    fn an_answer_that_is_not_a_transcript_is_malformed() {
        let error = parse("<html>gateway</html>").unwrap_err();
        assert!(matches!(error, ScribeError::Malformed(_)));
        assert!(!error.to_string().contains("gateway"));
    }
}
