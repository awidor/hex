//! Which Labs features have Hex send a dictation to ElevenLabs twice.
//!
//! A lab that shows words while you speak opens a live session for every
//! dictation. While Final text is Batch the whole clip is still uploaded for
//! the text that is pasted, so ElevenLabs hears, and bills, the audio twice.

use super::Feature;
use crate::app_settings::AppSettings;
use crate::transcription::scribe::FinalText;

/// The warning `hex labs` prints; the Labs cards show its translation.
pub const DOUBLED_USAGE: &str = "While Final text is Batch, each dictation is sent to ElevenLabs twice, roughly doubling your usage";

impl Feature {
    /// Whether this lab shows a live session's words while you dictate.
    pub const fn shows_live_words(self) -> bool {
        matches!(self, Self::LiveText | Self::LiveTyping)
    }

    /// Whether this lab, on, has each dictation sent to ElevenLabs twice
    /// under `settings`: streamed for the words it shows, and uploaded whole
    /// because Final text is Batch.
    pub fn doubles_usage(self, settings: &AppSettings) -> bool {
        self.shows_live_words() && settings.scribe.final_text == FinalText::Batch
    }
}

/// The lab that is on and has dictations sent twice under `settings`, if
/// any. `reaches` says whether this session has the lab at all (Live typing
/// needs Wayland).
pub fn doubling_usage(
    settings: &AppSettings,
    reaches: impl Fn(Feature) -> bool,
) -> Option<Feature> {
    [Feature::LiveText, Feature::LiveTyping]
        .into_iter()
        .find(|feature| {
            feature.saved_on(settings)
                && feature.on_this_os()
                && reaches(*feature)
                && feature.doubles_usage(settings)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(final_text: FinalText, live_text: bool) -> AppSettings {
        let mut settings = AppSettings::default();
        settings.scribe.final_text = final_text;
        settings.labs.live_text = live_text;
        settings
    }

    #[test]
    fn live_words_double_usage_only_while_final_text_is_batch() {
        let batch = settings(FinalText::Batch, false);
        assert!(Feature::LiveText.doubles_usage(&batch));
        assert!(Feature::LiveTyping.doubles_usage(&batch));
        assert!(!Feature::FailureNotices.doubles_usage(&batch));
        let live = settings(FinalText::Live, true);
        assert!(!Feature::LiveText.doubles_usage(&live));
        assert!(!Feature::LiveTyping.doubles_usage(&live));
    }

    #[test]
    fn usage_is_doubled_only_while_such_a_lab_is_on() {
        let on = settings(FinalText::Batch, true);
        assert_eq!(doubling_usage(&on, |_| true), Some(Feature::LiveText));
        assert_eq!(doubling_usage(&on, |_| false), None);
        assert_eq!(
            doubling_usage(&settings(FinalText::Batch, false), |_| true),
            None
        );
        assert_eq!(
            doubling_usage(&settings(FinalText::Live, true), |_| true),
            None
        );
    }
}
