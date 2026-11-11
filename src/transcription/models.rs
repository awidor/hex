//! What a transcription is asked for: the Scribe model and a language.
//!
//! There is one model and nothing to download or install. The type names
//! and wire names stay stable so `settings.json`, the CLI, and the local API
//! keep one vocabulary; a setting saved by upstream Hex (which named a local
//! model here) loads as Scribe with its language kept.

use std::str::FromStr;

use color_eyre::eyre::{Result, bail, eyre};
use serde::{Deserialize, Serialize};

pub use super::languages::{LANGUAGES, language_name};

/// The model a transcript comes from. Saved as its wire name (`scribe_v3`).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum TranscriptionModelId {
    #[default]
    ScribeV3,
}

impl Serialize for TranscriptionModelId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TranscriptionModelId {
    /// Any saved name loads as Scribe: settings written by upstream Hex name
    /// local models (`parakeet_v3`, `custom-…`) this build no longer has.
    /// Requests that name a model are checked with [`FromStr`] instead.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?;
        Ok(Self::ScribeV3)
    }
}

impl TranscriptionModelId {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ScribeV3 => "scribe_v3",
        }
    }
}

impl std::fmt::Display for TranscriptionModelId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for TranscriptionModelId {
    type Err = color_eyre::Report;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "scribe_v3" => Ok(Self::ScribeV3),
            _ => Err(eyre!("unknown transcription model: {value}")),
        }
    }
}

/// The model and the language it is told to expect.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct TranscriptionSelection {
    pub model: TranscriptionModelId,
    /// An ISO 639-1 or 639-3 code, or [`AUTO_LANGUAGE`].
    pub language: String,
}

impl Default for TranscriptionSelection {
    fn default() -> Self {
        Self {
            model: TranscriptionModelId::default(),
            language: AUTO_LANGUAGE.into(),
        }
    }
}

/// What Hex knows about a model.
pub struct ModelDefinition {
    pub id: TranscriptionModelId,
    pub name: &'static str,
    /// The realtime model its live sessions use.
    pub live: &'static str,
}

impl ModelDefinition {
    /// Scribe takes any language code it knows and detects the language
    /// when given none.
    pub fn supports_language(&self, language: &str) -> bool {
        language == AUTO_LANGUAGE || is_language_code(language)
    }

    /// The languages Settings offers, Auto first, in [`LANGUAGES`] order.
    /// Any other ISO code Scribe supports can still be set from the CLI.
    pub fn language_choices(&self) -> Vec<&'static str> {
        LANGUAGES.iter().map(|(code, _)| *code).collect()
    }

    pub fn default_language(&self) -> &'static str {
        AUTO_LANGUAGE
    }

    /// `language` when it is one Scribe can be told, else Auto.
    pub fn language_for<'a>(&self, language: &'a str) -> &'a str {
        if self.supports_language(language) {
            language
        } else {
            self.default_language()
        }
    }

    /// How many of `entries`, first in list order, a transcription is biased
    /// toward as keyterms (`dictation::vocabulary::keyterms`).
    pub fn vocabulary_terms_read(
        &self,
        entries: &[crate::dictation::vocabulary::VocabularyEntry],
    ) -> usize {
        crate::dictation::vocabulary::keyterms_of(entries).len()
    }
}

pub const SCRIBE_V3: ModelDefinition = ModelDefinition {
    id: TranscriptionModelId::ScribeV3,
    name: "Scribe v3",
    live: crate::transcription::scribe::live::MODEL,
};

pub const MODELS: &[ModelDefinition] = &[SCRIBE_V3];

pub const AUTO_LANGUAGE: &str = "auto";

pub fn definition(id: TranscriptionModelId) -> &'static ModelDefinition {
    match id {
        TranscriptionModelId::ScribeV3 => &MODELS[0],
    }
}

pub fn all_models() -> Vec<&'static ModelDefinition> {
    MODELS.iter().collect()
}

/// An ISO 639-1 or 639-3 code as Scribe takes it: two or three lowercase
/// ASCII letters. Whether Scribe knows the language is its to say.
fn is_language_code(language: &str) -> bool {
    (2..=3).contains(&language.len()) && language.bytes().all(|byte| byte.is_ascii_lowercase())
}

/// Whether a saved selection can be used as it is.
pub fn validate_saved(selection: &TranscriptionSelection) -> Result<()> {
    validate(selection).map(drop)
}

pub fn validate(selection: &TranscriptionSelection) -> Result<&'static ModelDefinition> {
    let model = definition(selection.model);
    if !model.supports_language(&selection.language) {
        bail!(
            "{} is not a language code {} takes (use `auto`, or a code like `en` or `yue`)",
            selection.language,
            model.name
        );
    }
    Ok(model)
}

/// Whether a transcription can be asked for: an ElevenLabs API key is set.
/// Nothing else needs preparing.
pub fn is_ready() -> bool {
    crate::transcription::scribe::key::is_set()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_model_saves_as_its_wire_name() {
        let selection = TranscriptionSelection::default();
        assert_eq!(
            serde_json::to_value(&selection).unwrap(),
            serde_json::json!({"model": "scribe_v3", "language": "auto"})
        );
        assert_eq!(
            "scribe_v3".parse::<TranscriptionModelId>().unwrap(),
            TranscriptionModelId::ScribeV3
        );
    }

    #[test]
    fn a_selection_saved_by_upstream_hex_keeps_its_language() {
        for model in ["parakeet_v3", "apple_speech", "custom-0123456789abcdef"] {
            let saved = serde_json::json!({"model": model, "language": "de"});
            let selection: TranscriptionSelection = serde_json::from_value(saved).unwrap();
            assert_eq!(selection.model, TranscriptionModelId::ScribeV3);
            assert_eq!(selection.language, "de");
        }
    }

    #[test]
    fn a_request_naming_a_local_model_is_refused() {
        assert!("parakeet_v3".parse::<TranscriptionModelId>().is_err());
        assert!("".parse::<TranscriptionModelId>().is_err());
    }

    #[test]
    fn auto_and_iso_codes_validate_and_anything_else_does_not() {
        let with = |language: &str| TranscriptionSelection {
            model: TranscriptionModelId::ScribeV3,
            language: language.into(),
        };
        for language in ["auto", "en", "yue", "fil"] {
            assert!(validate(&with(language)).is_ok(), "{language}");
        }
        for language in ["", "EN", "english", "e", "en-US", "zh_cn"] {
            assert!(validate(&with(language)).is_err(), "{language:?}");
            assert!(validate_saved(&with(language)).is_err(), "{language:?}");
        }
    }

    #[test]
    fn settings_offer_auto_first_and_every_named_language() {
        let choices = definition(TranscriptionModelId::ScribeV3).language_choices();
        assert_eq!(choices[0], AUTO_LANGUAGE);
        assert!(choices.contains(&"en"));
        assert_eq!(choices.len(), LANGUAGES.len());
    }

    #[test]
    fn a_language_scribe_cannot_be_told_falls_back_to_auto() {
        let model = definition(TranscriptionModelId::ScribeV3);
        assert_eq!(model.language_for("pl"), "pl");
        assert_eq!(model.language_for("pl-PL"), AUTO_LANGUAGE);
    }
}
