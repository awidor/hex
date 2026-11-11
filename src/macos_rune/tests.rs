//! Runtime request/scheduling regressions; no settings writes.

use super::runtime::Runtime;
use super::*;
use crate::app_settings::{DictationMode, KeyboardModelChoice, KeyboardModelSwitching};
// `Request` moved beside `ShortcutTarget` into `macos_rune/request.rs`, so the
// runtime's own imports no longer name the model IDs and selections these
// tests build.
use crate::transcription::models::{TranscriptionModelId, TranscriptionSelection};

/// The pump ticks every frame only while the HUD animates; otherwise it
/// idles and senders wake it (#1378).
#[test]
fn the_pump_idles_unless_the_hud_animates() {
    assert!(pump_interval(true) < 0.02);
    assert!(pump_interval(false) >= 0.2);
}

/// ⌘Tab back to Hex opens its closed window (#1298), but never for a
/// menu-bar-only Hex, an overlay that took the keyboard, or an open window.
#[test]
fn switching_to_hex_opens_its_closed_window() {
    assert!(reopen_on_activation(true, false, false));
    assert!(!reopen_on_activation(false, false, false));
    assert!(!reopen_on_activation(true, true, false));
    assert!(!reopen_on_activation(true, false, true));
}

/// Setup or a missing menu-bar item always opens the window; otherwise a
/// shown Dock icon opens it unless macOS started Hex at login (#1321), and a
/// hidden one never does. `hex app --hidden` opens it only without the
/// menu-bar item.
#[test]
fn login_item_launch_starts_without_the_window() {
    for setup_ready in [false, true] {
        for onboarding_completed in [false, true] {
            for status_item_available in [false, true] {
                for show_dock_icon in [false, true] {
                    let ready = setup_ready && onboarding_completed && status_item_available;
                    let launch = LaunchWindow::decide(
                        false,
                        show_dock_icon,
                        setup_ready,
                        onboarding_completed,
                        status_item_available,
                    );
                    assert_eq!(launch.opens(true), !ready, "{launch:?} at login");
                    assert_eq!(
                        launch.opens(false),
                        !ready || show_dock_icon,
                        "{launch:?} opened by hand"
                    );
                    let hidden = LaunchWindow::decide(
                        true,
                        show_dock_icon,
                        setup_ready,
                        onboarding_completed,
                        status_item_available,
                    );
                    assert_eq!(
                        hidden.opens(false),
                        !status_item_available,
                        "{hidden:?} hidden"
                    );
                }
            }
        }
    }
}

/// A runtime whose chosen selection changes in memory only (`save_selection`
/// writes nothing in tests).
struct Fixture {
    runtime: Runtime,
}

impl Fixture {
    fn new(settings: AppSettings) -> Self {
        Self {
            runtime: Runtime {
                keyboard_switcher: crate::transcription::keyboard_model_switching::Switcher::new(
                    settings.keyboard_model_switching.clone(),
                    Some("ABC".into()),
                ),
                app_switcher: crate::transcription::app_model_switching::Switcher::new(
                    settings.labs.app_model_switching,
                    None,
                ),
                settings,
                transcription_error: None,
                recognition_start: None,
                listener_failure: Default::default(),
                event_reader: crate::desktop::events::EventReader::open(""),
                activity: crate::desktop::activity::DesktopActivity::default(),
                last_completed_at: None,
                started_at: 0,
            },
        }
    }

    fn source(&mut self, source: &str) {
        self.runtime
            .handle(Request::InputSourceChanged(Some(source.into())));
    }

    fn due(&mut self) {
        self.runtime
            .tick_at(Instant::now() + Duration::from_secs(1));
    }

    /// Feeds one foreground read to the app switcher.
    fn app(&mut self, application: Option<&str>, due: bool) {
        let now = Instant::now()
            + if due {
                Duration::from_secs(1)
            } else {
                Duration::ZERO
            };
        self.runtime.follow_app(
            crate::transcription::app_model_switching::Observation::frontmost(application),
            now,
        );
    }

    /// A choice made by hand, as the window's language menu sends it.
    fn choose(&mut self, language: &str) {
        self.runtime.handle(Request::ChooseModel {
            model: TranscriptionModelId::ScribeV3,
            language: language.into(),
        });
    }

    /// The language the runtime transcribes in now.
    fn language(&self) -> &str {
        &self.runtime.settings.transcription.language
    }
}

fn selection(language: &str) -> TranscriptionSelection {
    TranscriptionSelection {
        model: TranscriptionModelId::ScribeV3,
        language: language.into(),
    }
}

fn choice(language: &str) -> KeyboardModelChoice {
    KeyboardModelChoice {
        model: TranscriptionModelId::ScribeV3,
        language: language.into(),
    }
}

/// Settings that transcribe in English, whatever this computer's interface
/// language makes the default.
fn english_settings() -> AppSettings {
    AppSettings {
        transcription: selection("en"),
        ..Default::default()
    }
}

/// English in use, with the "Spanish" input source mapped to `language`.
fn mapped_settings(language: &str) -> AppSettings {
    AppSettings {
        keyboard_model_switching: KeyboardModelSwitching {
            enabled: true,
            mappings: [("Spanish".into(), choice(language))].into(),
        },
        ..english_settings()
    }
}

#[test]
fn a_mapping_in_another_language_switches_the_language_once_it_is_due() {
    let mut fixture = Fixture::new(mapped_settings("es"));
    fixture.source("Spanish");
    assert_eq!(
        fixture.language(),
        "en",
        "the switch started before its debounce"
    );
    fixture.due();
    assert_eq!(fixture.language(), "es");
    assert_eq!(fixture.runtime.transcription_error, None);
}

#[test]
fn a_mapping_to_the_language_in_use_changes_nothing() {
    let mut fixture = Fixture::new(mapped_settings("en"));
    let before = fixture.runtime.settings.clone();
    fixture.source("Spanish");
    fixture.due();
    assert!(fixture.runtime.settings == before);
}

#[test]
fn a_modes_speech_choice_switches_when_its_app_comes_to_the_front() {
    let mut settings = english_settings();
    settings.labs.app_model_switching = true;
    settings.dictation_processing.modes = vec![DictationMode {
        name: "Spanish".into(),
        applications: vec!["Notes".into()],
        speech: Some(choice("es")),
        ..DictationMode::default()
    }];
    let mut fixture = Fixture::new(settings);
    fixture.app(Some("Notes"), false);
    assert_eq!(
        fixture.language(),
        "en",
        "the switch started before its debounce"
    );
    fixture.app(Some("Notes"), true);
    assert_eq!(fixture.language(), "es");
}

#[test]
fn hexs_own_window_applies_nothing_and_an_edit_waits_for_another_app() {
    let mut settings = english_settings();
    settings.labs.app_model_switching = true;
    settings.dictation_processing.default_mode.speech = Some(choice("es"));
    let mut fixture = Fixture::new(settings);
    // Hex's own window names no mode, even though Global matches every app.
    fixture.app(None, true);
    assert_eq!(
        fixture.language(),
        "en",
        "Hex's own window switched the language"
    );
    // Editing Global while Hex is in front changes nothing yet.
    fixture
        .runtime
        .settings
        .dictation_processing
        .default_mode
        .speech = Some(choice("fr"));
    fixture.app(None, true);
    assert_eq!(fixture.language(), "en");
    // The next app in front applies the edited choice after its debounce.
    fixture.app(Some("Finder"), false);
    assert_eq!(
        fixture.language(),
        "en",
        "the switch started before its debounce"
    );
    fixture.app(Some("Finder"), true);
    assert_eq!(fixture.language(), "fr");
}

#[test]
fn a_choice_made_by_hand_applies_at_once_and_cancels_a_debounced_keyboard_switch() {
    let mut fixture = Fixture::new(mapped_settings("es"));
    fixture.source("Spanish");
    fixture.choose("de");
    assert_eq!(fixture.language(), "de");
    fixture.due();
    assert_eq!(
        fixture.language(),
        "de",
        "the keyboard switch undid a newer choice"
    );
    assert_eq!(
        fixture
            .runtime
            .keyboard_switcher
            .take_due(Instant::now() + Duration::from_secs(1)),
        None
    );
}

#[test]
fn choosing_the_language_in_use_still_cancels_a_debounced_keyboard_switch() {
    let mut fixture = Fixture::new(mapped_settings("es"));
    fixture.source("Spanish");
    fixture.choose("en");
    fixture.due();
    assert_eq!(fixture.language(), "en");
}

#[test]
fn cycling_through_a_mapped_source_switches_nothing() {
    let mut fixture = Fixture::new(mapped_settings("es"));
    fixture.source("Spanish");
    fixture.source("ABC");
    fixture.due();
    assert_eq!(
        fixture.language(),
        "en",
        "a pass-through source must not change the language"
    );
}

#[test]
fn a_language_scribe_cannot_be_told_is_refused_until_a_later_choice() {
    let mut fixture = Fixture::new(english_settings());
    fixture.choose("english");
    assert_eq!(fixture.language(), "en");
    assert!(fixture.runtime.transcription_error.is_some());
    fixture.choose("de");
    assert_eq!(fixture.language(), "de");
    assert_eq!(fixture.runtime.transcription_error, None);
}
