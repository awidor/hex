//! Why a Scribe request failed, in words a person can act on.
//!
//! Nothing here ever carries the API key, the audio, or the transcript.

use std::fmt;

/// A failed request to ElevenLabs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScribeError {
    /// No API key is saved and `ELEVENLABS_API_KEY` is not set.
    MissingKey,
    /// ElevenLabs refused the key (HTTP 401).
    InvalidKey,
    /// The account has no transcription quota left.
    QuotaExceeded,
    /// Too many requests at once (HTTP 429).
    RateLimited,
    /// ElevenLabs could not be reached: no network, DNS, TLS, or a timeout.
    Unreachable(String),
    /// ElevenLabs refused the request itself (another 4xx).
    Rejected { status: u16, detail: String },
    /// ElevenLabs failed (5xx).
    Server { status: u16 },
    /// The answer was not the transcript the API documents.
    Malformed(String),
    /// The job's waiter gave up before the request was sent.
    Cancelled,
}

impl ScribeError {
    /// Whether sending the same request again may succeed. A refused key,
    /// spent quota, or a bad request never does.
    pub(crate) fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Unreachable(_) | Self::Server { .. } | Self::RateLimited
        )
    }

    /// Whether the audio is worth keeping to transcribe later: every failure
    /// except one the same clip would hit again.
    pub fn worth_retrying_later(&self) -> bool {
        !matches!(self, Self::Rejected { .. } | Self::Cancelled)
    }

    /// The error ElevenLabs answered with `status` and `body`.
    pub(crate) fn from_response(status: u16, body: &str) -> Self {
        let (code, message) = detail(body);
        let code = code.to_ascii_lowercase();
        // ElevenLabs answers spent quota with HTTP 401 too, so the body's
        // status is read before the HTTP status names the key.
        if code.contains("quota") || status == 402 {
            return Self::QuotaExceeded;
        }
        if status == 401 || code.contains("invalid_api_key") || code.contains("missing_api_key") {
            return Self::InvalidKey;
        }
        if status == 429 || code.contains("rate_limit") || code.contains("too_many") {
            return Self::RateLimited;
        }
        if status >= 500 {
            return Self::Server { status };
        }
        let detail = match (code.is_empty(), message.is_empty()) {
            (_, false) => message,
            (false, true) => code,
            (true, true) => String::new(),
        };
        Self::Rejected { status, detail }
    }
}

impl fmt::Display for ScribeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingKey => formatter
                .write_str("no ElevenLabs API key is set (Settings › General, or `hex key set`)"),
            Self::InvalidKey => {
                formatter.write_str("ElevenLabs refused the API key; check it in Settings")
            }
            Self::QuotaExceeded => {
                formatter.write_str("the ElevenLabs account has no transcription quota left")
            }
            Self::RateLimited => {
                formatter.write_str("ElevenLabs is rate-limiting this account; try again shortly")
            }
            Self::Unreachable(reason) => write!(formatter, "couldn't reach ElevenLabs: {reason}"),
            Self::Rejected { status, detail } if detail.is_empty() => {
                write!(formatter, "ElevenLabs refused the request (HTTP {status})")
            }
            Self::Rejected { status, detail } => {
                write!(
                    formatter,
                    "ElevenLabs refused the request (HTTP {status}): {detail}"
                )
            }
            Self::Server { status } => write!(formatter, "ElevenLabs failed (HTTP {status})"),
            Self::Malformed(reason) => {
                write!(
                    formatter,
                    "ElevenLabs sent an answer Hex can't read: {reason}"
                )
            }
            Self::Cancelled => formatter.write_str("transcription was abandoned"),
        }
    }
}

impl std::error::Error for ScribeError {}

/// The `status` code and message of an ElevenLabs error body:
/// `{"detail": {"status": "...", "message": "..."}}`, a plain string detail,
/// or FastAPI's list of validation errors. Unknown shapes give empty strings.
fn detail(body: &str) -> (String, String) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return (String::new(), String::new());
    };
    let text = |value: &serde_json::Value| value.as_str().unwrap_or_default().trim().to_string();
    match &value["detail"] {
        serde_json::Value::String(message) => (String::new(), clip(message.trim())),
        serde_json::Value::Object(detail) => {
            let code = detail.get("status").map(text).unwrap_or_default();
            let message = detail.get("message").map(text).unwrap_or_default();
            (code, clip(&message))
        }
        serde_json::Value::Array(errors) => {
            let message = errors
                .iter()
                .filter_map(|error| error.get("msg").and_then(serde_json::Value::as_str))
                .collect::<Vec<_>>()
                .join("; ");
            (String::new(), clip(&message))
        }
        _ => (String::new(), String::new()),
    }
}

/// A server's message, short enough for a notice and a log line.
fn clip(message: &str) -> String {
    const MAX_CHARS: usize = 200;
    if message.chars().count() <= MAX_CHARS {
        return message.to_string();
    }
    let mut clipped: String = message.chars().take(MAX_CHARS).collect();
    clipped.push('…');
    clipped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_key_is_named_whatever_the_body_says() {
        assert_eq!(
            ScribeError::from_response(401, "nope"),
            ScribeError::InvalidKey
        );
        assert_eq!(
            ScribeError::from_response(
                400,
                r#"{"detail":{"status":"invalid_api_key","message":"Invalid API key"}}"#
            ),
            ScribeError::InvalidKey
        );
    }

    #[test]
    fn spent_quota_and_rate_limits_are_told_apart() {
        // Spent quota arrives as HTTP 401 with its own status in the body.
        for status in [400, 401] {
            assert_eq!(
                ScribeError::from_response(
                    status,
                    r#"{"detail":{"status":"quota_exceeded","message":"You have 0 credits"}}"#
                ),
                ScribeError::QuotaExceeded
            );
        }
        assert_eq!(
            ScribeError::from_response(429, ""),
            ScribeError::RateLimited
        );
        assert!(ScribeError::RateLimited.retryable());
        assert!(!ScribeError::QuotaExceeded.retryable());
    }

    #[test]
    fn a_rejected_request_keeps_the_servers_reason() {
        let error = ScribeError::from_response(
            422,
            r#"{"detail":[{"loc":["body","model_id"],"msg":"field required"}]}"#,
        );
        assert_eq!(
            error,
            ScribeError::Rejected {
                status: 422,
                detail: "field required".into()
            }
        );
        assert_eq!(
            error.to_string(),
            "ElevenLabs refused the request (HTTP 422): field required"
        );
        assert!(!error.retryable());
        assert!(!error.worth_retrying_later());
    }

    #[test]
    fn a_server_failure_is_retried_and_its_audio_kept() {
        let error = ScribeError::from_response(503, "<html>");
        assert_eq!(error, ScribeError::Server { status: 503 });
        assert!(error.retryable());
        assert!(error.worth_retrying_later());
    }

    #[test]
    fn a_long_server_message_is_clipped() {
        let body = format!(r#"{{"detail":"{}"}}"#, "x".repeat(500));
        let ScribeError::Rejected { detail, .. } = ScribeError::from_response(400, &body) else {
            panic!("a 400 is a rejection");
        };
        assert_eq!(detail.chars().count(), 201);
    }
}
