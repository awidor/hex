//! Quiet notes at the foot of the first run's window: that what you dictate
//! is sent to ElevenLabs, where Hex asks for the key and for the
//! microphone, and why macOS shows the mic as on, from the practice on.

use rune::{El, Icon, TextStyle, Transition, col, icon, row, text};
use rune_kit::glyphs;

use crate::desktop::i18n::{tr, tr_fill};

use super::super::controls::Paint;
use super::stories::{Onboard, Step};

const WINDOW: [f32; 2] = [980.0, 760.0];
const NOTE_FROM_BOTTOM: f32 = 40.0;

/// One quiet line at the foot of the window: where your audio goes, on the
/// key step and again when Hex asks for the microphone, where it matters
/// most. From the practice on, where macOS first shows the mic in use, why
/// it stays on.
pub(super) fn note(p: Paint, s: &Onboard) -> Option<El> {
    match s.step {
        Step::Key | Step::Microphone => Some(foot(
            "first-note",
            marked(
                p,
                &glyphs::MIC,
                tr("Audio you dictate is sent to ElevenLabs for transcription"),
            ),
        )),
        Step::Try | Step::TryHearing | Step::TryHeard | Step::Ready
            if s.live.as_ref().is_some_and(|live| live.mic_stays_open()) =>
        {
            let why = tr(
                "macOS shows the mic as on because Hex keeps it warm, so dictation starts instantly.",
            );
            let only = tr_fill(
                if s.toggle {
                    "It only records after you tap {}."
                } else {
                    "It only records while you hold {}."
                },
                &shortcut(&s.caps),
            );
            Some(foot(
                "mic-note",
                col()
                    .items_center()
                    .child(marked(p, &glyphs::MIC, why))
                    .child(words(p, &only).key("only")),
            ))
        }
        _ => None,
    }
}

/// The dictation shortcut as text where the template's `{}` is: macOS's
/// glyphs run together as its menus write them (`⌥⇧S`), others join with
/// `+`. Keycaps here would make the second line taller than the first.
fn shortcut(caps: &[String]) -> String {
    caps.join(if cfg!(target_os = "macos") { "" } else { "+" })
}

/// Quiet notes at the foot of the window, their first line where the
/// last one's was.
fn foot(key: &str, lines: El) -> El {
    row()
        .key(key)
        .absolute(0.0, WINDOW[1] - NOTE_FROM_BOTTOM - 18.0)
        .w(WINDOW[0])
        .justify_center()
        .transition(Transition::opacity())
        .child(lines)
}

/// A line of the note led by its glyph.
fn marked(p: Paint, glyph: &'static Icon, copy: &str) -> El {
    row()
        .key("marked")
        .gap(6.0)
        .center_y()
        .child(icon(glyph, 13.0).key("glyph").color(p.muted()))
        .child(words(p, copy))
}

fn words(p: Paint, copy: &str) -> El {
    text(copy)
        .key("words")
        .font(TextStyle::new(13.0, 420.0).lh(18.0))
        .color(p.muted())
}
