//! Speech to text: the engine thread, the Scribe transcriber it owns, and
//! the audio preparation every caller shares.

pub(crate) mod app_model_switching;
pub(crate) mod background_priority;
pub(crate) mod cancel;
pub(crate) mod engine;
pub(crate) mod failed_clips;
pub(crate) mod keyboard_model_switching;
pub(crate) mod languages;
pub(crate) mod models;
pub(crate) mod pause_segmenter;
pub(crate) mod scribe;
pub(crate) mod service;
pub(crate) mod stream;
pub(crate) mod transcript;

#[cfg(target_os = "macos")]
pub use crate::transcription::transcript::{Transcript, TranscriptSegment};
