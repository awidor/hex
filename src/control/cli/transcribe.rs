//! `hex transcribe <file>` (Labs › Transcribe files) and `hex model
//! transcribe <wav>`: a saved WAV sent to ElevenLabs, through the running
//! Hex's local API or from this process when Hex is stopped (`hex model
//! transcribe` always sends it from here). Nothing is pasted, saved to
//! History, or processed.

use std::io::{Cursor, Read};
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use super::{Output, key, print};
use crate::control::{ControlReply, model_selection};
use crate::labs::Feature;
use crate::transcription::models::TranscriptionSelection;
use crate::transcription::service::{AudioClip, AudioError};

/// A long file takes a while to upload and transcribe.
const TIMEOUT: Duration = Duration::from_secs(600);
const MAX_RESPONSE: usize = 1024 * 1024;

#[derive(Deserialize)]
struct Transcript {
    transcript: String,
}

/// What `hex transcribe` sends once its file and language are settled.
#[derive(Debug)]
struct Job {
    wav: Vec<u8>,
    selection: TranscriptionSelection,
}

pub(super) fn run(
    file: std::path::PathBuf,
    model: Option<String>,
    language: Option<String>,
    output: Output,
) -> u8 {
    let reply = settings().and_then(|settings| {
        let job = prepare(&settings, &file, model, language)?;
        Ok(transcribe(&settings, &job))
    });
    print(reply.unwrap_or_else(|refusal| refusal), output)
}

/// `hex model transcribe <wav>`: sends the file to ElevenLabs from this
/// process, in the saved language unless one is given.
pub(super) fn run_here(file: &Path, language: Option<String>, output: Output) -> u8 {
    let reply = settings().and_then(|settings| {
        let job = job(&settings, file, None, language)?;
        Ok(here(&settings, &job))
    });
    print(reply.unwrap_or_else(|refusal| refusal), output)
}

/// The saved settings, read without changing the file.
fn settings() -> std::result::Result<crate::app_settings::AppSettings, ControlReply> {
    crate::app_settings::AppSettings::read().map_err(|error| {
        ControlReply::error("failed", format!("couldn't read Hex's settings: {error:#}"))
    })
}

/// The lab's gate, the file, and the model, all checked before Hex is asked.
fn prepare(
    settings: &crate::app_settings::AppSettings,
    file: &Path,
    model: Option<String>,
    language: Option<String>,
) -> std::result::Result<Job, ControlReply> {
    if !Feature::TranscribeFiles.on(settings) {
        let message = match Feature::TranscribeFiles.managed() {
            Some(lock) => lock.refusal().to_owned(),
            None => {
                "Labs › Transcribe files is off; turn it on with `hex labs on transcribe-files`"
                    .to_owned()
            }
        };
        return Err(ControlReply::error("disabled", message));
    }
    job(settings, file, model, language)
}

/// The file as WAV and the selection it is transcribed with: Scribe, in the
/// saved language unless one is given.
fn job(
    settings: &crate::app_settings::AppSettings,
    file: &Path,
    model: Option<String>,
    language: Option<String>,
) -> std::result::Result<Job, ControlReply> {
    let wav = read_wav(file)?;
    let model = model.unwrap_or_else(|| settings.transcription.model.as_str().to_owned());
    let selection = model_selection(settings, &model, language.as_deref())?;
    Ok(Job { wav, selection })
}

/// The whole file in memory, read through the size cap so a file that grew
/// after its metadata was read is still refused, and checked as WAV before
/// anything is sent.
fn read_wav(path: &Path) -> std::result::Result<Vec<u8>, ControlReply> {
    let name = path.display();
    let read_error = |error: std::io::Error| {
        ControlReply::error("failed", format!("couldn't read {name}: {error}"))
    };
    let file = std::fs::File::open(path).map_err(read_error)?;
    let length = file.metadata().map_err(read_error)?.len();
    if length > crate::local_api::MAX_AUDIO_BYTES as u64 {
        return Err(too_large(&name));
    }
    let mut wav = Vec::new();
    file.take(crate::local_api::MAX_AUDIO_BYTES as u64 + 1)
        .read_to_end(&mut wav)
        .map_err(read_error)?;
    if wav.len() > crate::local_api::MAX_AUDIO_BYTES {
        return Err(too_large(&name));
    }
    match AudioClip::decode_wav(Cursor::new(&wav)) {
        Ok(_) => Ok(wav),
        Err(AudioError::ResourceExhausted) => Err(ControlReply::error(
            "resource-exhausted",
            format!("{name} is too long for Hex to transcribe at once"),
        )),
        Err(_) => Err(ControlReply::error(
            "unsupported",
            format!(
                "{name} isn't a WAV file Hex can read; convert it to WAV first, \
                 for example with ffmpeg: ffmpeg -i <file> <file>.wav"
            ),
        )),
    }
}

fn too_large(name: &std::path::Display<'_>) -> ControlReply {
    let limit = crate::local_api::MAX_AUDIO_BYTES / (1024 * 1024);
    ControlReply::error(
        "resource-exhausted",
        format!("{name} is larger than {limit} MiB; transcribe a shorter or lower-rate WAV"),
    )
}

fn transcribe(settings: &crate::app_settings::AppSettings, job: &Job) -> ControlReply {
    if crate::local_api::running_pid().is_some() {
        return in_running_hex(job);
    }
    here(settings, job)
}

/// The running Hex sends the file to ElevenLabs with its own API key, its
/// vocabulary, and its Scribe settings, queued behind any dictation.
fn in_running_hex(job: &Job) -> ControlReply {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("model", job.selection.model.as_str())
        .append_pair("language", &job.selection.language)
        .finish();
    let path = format!("/transcriptions?{query}");
    match crate::local_api::post_wav(&path, &job.wav, TIMEOUT, MAX_RESPONSE) {
        Ok((200, body)) => transcript(&body),
        Ok((status, body)) => {
            let code = crate::local_api::error_code(status, &body);
            let message = crate::local_api::error_message(&body);
            refusal(status, &code, message.as_deref())
        }
        Err(error) => ControlReply::error("transport", format!("{error:#}")),
    }
}

fn transcript(body: &[u8]) -> ControlReply {
    match serde_json::from_slice::<Transcript>(body) {
        Ok(Transcript { transcript }) if transcript.trim().is_empty() => no_speech(),
        Ok(Transcript { transcript }) => ControlReply::Transcript { text: transcript },
        Err(_) => ControlReply::error("failed", "Hex sent an unreadable transcript"),
    }
}

/// What the running Hex's refusal means for `hex transcribe`. `message` is
/// Hex's own account of it, when it gave one.
fn refusal(status: u16, code: &str, message: Option<&str>) -> ControlReply {
    let said = |fallback: &str| message.unwrap_or(fallback).to_owned();
    match code {
        "api-key-missing" => ControlReply::error(
            "api-key-missing",
            "the running Hex has no ElevenLabs API key; run `hex key set`",
        ),
        "unknown-model" => ControlReply::error(
            "unknown-model",
            said("the running Hex doesn't know that model; restart Hex (`hex restart`)"),
        ),
        "unsupported-language" | "unsupported-model" => ControlReply::error(
            "invalid-value",
            said("the running Hex can't transcribe in that language"),
        ),
        "invalid-request" => ControlReply::error("failed", "Hex refused the request; update Hex"),
        "unsupported-audio" | "invalid-audio" => ControlReply::error(
            "unsupported",
            "Hex couldn't read the file as WAV; convert it to WAV first",
        ),
        "length-required" => ControlReply::error("failed", "Hex refused the upload; update Hex"),
        "resource-exhausted" => ControlReply::error(
            "resource-exhausted",
            "Hex says the file is too large or too long to transcribe",
        ),
        "model-not-ready" => ControlReply::error("model-unavailable", not_ready()),
        "queue-full" => {
            ControlReply::error("busy", "Hex is busy with another transcription; try again")
        }
        "upload-timeout" => {
            ControlReply::error("failed", "Hex timed out reading the file; try again")
        }
        "cancelled" => ControlReply::error("failed", "Hex cancelled the transcription; try again"),
        "service-gone" => ControlReply::error(
            "unavailable",
            "Hex's transcription service stopped; restart Hex",
        ),
        "model-load-failed" | "transcription-failed" => ControlReply::error(
            "failed",
            match message {
                Some(why) => format!("Hex couldn't transcribe it: {why}"),
                None => "Hex couldn't transcribe it; try again".into(),
            },
        ),
        _ => match status {
            401 => ControlReply::error("failed", "Hex rejected the request; restart Hex"),
            408 => ControlReply::error("failed", "Hex timed out reading the file; try again"),
            409 => ControlReply::error("model-unavailable", not_ready()),
            413 => ControlReply::error(
                "resource-exhausted",
                "Hex says the file is too large to transcribe",
            ),
            415 => ControlReply::error(
                "unsupported",
                "Hex couldn't read the file as WAV; convert it to WAV first",
            ),
            429 => ControlReply::error("busy", "Hex is busy with another transcription; try again"),
            _ if code.is_empty() || code.starts_with("http-") => {
                ControlReply::error("failed", format!("Hex answered HTTP {status}"))
            }
            _ => ControlReply::error("failed", format!("Hex answered HTTP {status} ({code})")),
        },
    }
}

/// A Hex that answers "not ready" is an older one, waiting on something
/// this `hex` no longer has.
fn not_ready() -> &'static str {
    "the running Hex isn't ready to transcribe; restart it with `hex restart`"
}

fn no_speech() -> ControlReply {
    ControlReply::error(
        "no-speech",
        "no speech was heard in the file; nothing to print",
    )
}

/// Sends the file to ElevenLabs from this process, with the saved Scribe
/// settings. A missing key is refused before anything is uploaded.
fn here(settings: &crate::app_settings::AppSettings, job: &Job) -> ControlReply {
    if !crate::transcription::scribe::key::is_set() {
        return ControlReply::error("api-key-missing", key::missing_key_message());
    }
    crate::transcription::scribe::apply(settings.scribe);
    match crate::transcription::scribe::transcribe_wav_bytes(&job.wav, &job.selection) {
        Ok(text) if text.trim().is_empty() => no_speech(),
        Ok(text) => ControlReply::Transcript { text },
        Err(error) => ControlReply::error("failed", format!("couldn't transcribe it: {error:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::AppSettings;

    fn not_a_wav() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "hex-transcribe-not-a-wav-{}.wav",
            std::process::id()
        ));
        std::fs::write(&path, b"this is not a wav").unwrap();
        path
    }

    fn wav() -> Vec<u8> {
        let samples = [0_i16; 160];
        let data = (samples.len() * 2) as u32;
        let mut bytes = Vec::from(*b"RIFF");
        bytes.extend_from_slice(&(36 + data).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&16_000_u32.to_le_bytes());
        bytes.extend_from_slice(&(16_000_u32 * 2).to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn the_lab_off_refuses_and_names_its_toggle() {
        let reply = prepare(
            &AppSettings::default(),
            Path::new("recording.wav"),
            None,
            None,
        )
        .unwrap_err();
        assert!(
            matches!(reply, ControlReply::Error { ref code, ref message }
                if code == "disabled" && message.contains("hex labs on transcribe-files")),
            "{reply:?}"
        );
    }

    #[test]
    fn a_file_that_is_not_wav_is_refused_before_any_transcription() {
        let mut settings = AppSettings::default();
        settings.labs.transcribe_files = true;
        let path = not_a_wav();
        let reply = prepare(&settings, &path, None, None).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert!(
            matches!(reply, ControlReply::Error { ref code, .. } if code == "unsupported"),
            "{reply:?}"
        );
    }

    #[test]
    fn a_wav_uses_the_selected_model_and_language() {
        let mut settings = AppSettings::default();
        settings.labs.transcribe_files = true;
        let path =
            std::env::temp_dir().join(format!("hex-transcribe-wav-{}.wav", std::process::id()));
        std::fs::write(&path, wav()).unwrap();
        let job = prepare(&settings, &path, None, None).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(job.selection.model, settings.transcription.model);
        assert_eq!(job.selection.language, settings.transcription.language);
    }

    #[test]
    fn an_empty_transcript_is_no_speech() {
        assert!(matches!(
            transcript(br#"{"transcript":"  ","durationMs":1000}"#),
            ControlReply::Error { ref code, .. } if code == "no-speech"
        ));
    }

    fn error(reply: ControlReply) -> (String, String) {
        match reply {
            ControlReply::Error { code, message } => (code, message),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn only_scribe_can_be_named_and_the_language_can_be_given() {
        let mut settings = AppSettings::default();
        settings.labs.transcribe_files = true;
        let path = std::env::temp_dir().join(format!(
            "hex-transcribe-language-{}.wav",
            std::process::id()
        ));
        std::fs::write(&path, wav()).unwrap();
        let named = prepare(
            &settings,
            &path,
            Some("scribe_v3".into()),
            Some("yue".into()),
        );
        let unknown = prepare(&settings, &path, Some("parakeet_v3".into()), None);
        let language = prepare(&settings, &path, None, Some("cantonese".into()));
        let _ = std::fs::remove_file(&path);
        assert_eq!(named.unwrap().selection.language, "yue");
        let (code, message) = error(unknown.unwrap_err());
        assert_eq!(code, "unknown-model");
        assert!(message.contains("scribe_v3"), "{message}");
        assert_eq!(error(language.unwrap_err()).0, "invalid-value");
    }

    /// Unit tests have no API key, so nothing here reaches ElevenLabs.
    #[test]
    fn transcribing_here_without_a_key_is_refused_before_any_upload() {
        let settings = AppSettings::default();
        let job = Job {
            wav: wav(),
            selection: settings.transcription.clone(),
        };
        let (code, message) = error(here(&settings, &job));
        assert_eq!(code, "api-key-missing");
        assert!(message.contains("hex key set"), "{message}");
    }

    #[test]
    fn a_refusal_follows_hexs_code_rather_than_its_status() {
        let (code, message) = error(refusal(415, "unsupported-audio", None));
        assert_eq!(code, "unsupported");
        assert!(
            message.contains("couldn't read the file as WAV"),
            "{message}"
        );

        let (code, message) = error(refusal(400, "invalid-audio", None));
        assert_eq!(code, "unsupported");
        assert!(
            message.contains("couldn't read the file as WAV"),
            "{message}"
        );

        let (code, message) = error(refusal(409, "api-key-missing", Some("no key")));
        assert_eq!(code, "api-key-missing");
        assert!(message.contains("hex key set"), "{message}");

        let only = "unknown model `parakeet_v2`; the only model is `scribe_v3`";
        let (code, message) = error(refusal(400, "unknown-model", Some(only)));
        assert_eq!(code, "unknown-model");
        assert_eq!(message, only);
        // An older Hex names no reason.
        let (code, message) = error(refusal(400, "unknown-model", None));
        assert_eq!(code, "unknown-model");
        assert!(message.contains("hex restart"), "{message}");

        let (code, _) = error(refusal(400, "unsupported-language", None));
        assert_eq!(code, "invalid-value");

        let (code, message) = error(refusal(409, "model-not-ready", None));
        assert_eq!(code, "model-unavailable");
        assert!(message.contains("hex restart"), "{message}");
        assert!(!message.contains("install"), "{message}");

        let why = "ElevenLabs refused the API key; check it in Settings";
        let (code, message) = error(refusal(500, "transcription-failed", Some(why)));
        assert_eq!(code, "failed");
        assert!(message.ends_with(why), "{message}");

        let (code, message) = error(refusal(429, "queue-full", None));
        assert_eq!(code, "busy");
        assert!(
            message.contains("busy with another transcription"),
            "{message}"
        );

        let (code, _) = error(refusal(413, "resource-exhausted", None));
        assert_eq!(code, "resource-exhausted");

        let (code, _) = error(refusal(409, "cancelled", None));
        assert_eq!(code, "failed");

        let (code, _) = error(refusal(503, "service-gone", None));
        assert_eq!(code, "unavailable");

        let (code, _) = error(refusal(401, "http-401", None));
        assert_eq!(code, "failed");

        let (code, _) = error(refusal(413, "http-413", None));
        assert_eq!(code, "resource-exhausted");
    }
}
