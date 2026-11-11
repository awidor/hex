//! The ElevenLabs API key as the window knows it: where the key in use
//! comes from, the field a new one is typed into, and the check that asks
//! ElevenLabs whether a key works. Settings › General › Transcription and
//! the first run's key step share it.
//!
//! The key itself stays in the field and in `scribe::key`. The window shows
//! only its last four characters, and a fixture never reads, saves, or
//! checks a real one.

use std::sync::mpsc::{Receiver, TryRecvError};

use rune::TextEdit;

use super::Ui;
use crate::transcription::scribe::{self, ScribeError, key};

/// How often the window reads where the key comes from while it shows it,
/// so one set with `hex key set` appears without a restart.
const READ_EVERY: f64 = 1.0;

/// Where the key in use comes from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) enum Source {
    #[default]
    NotSet,
    /// Saved in Hex's data folder, as [`key::redacted`] shows it.
    Saved(String),
    /// `ELEVENLABS_API_KEY`, which is not Hex's to change.
    Environment,
}

impl Source {
    /// The key requests are sent with.
    pub(super) fn read() -> Self {
        match key::current() {
            None => Self::NotSet,
            Some(found) => match found.source {
                key::Source::Environment => Self::Environment,
                key::Source::File => Self::Saved(key::redacted(&found.secret)),
            },
        }
    }

    /// A saved key as scenes and tests show one.
    pub(super) fn fixture() -> Self {
        Self::Saved(key::redacted("sk_fixture_0123456789abcdef"))
    }
}

/// What the last check said.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) enum Check {
    #[default]
    Idle,
    Checking,
    Works,
    /// ElevenLabs' answer, or why it could not be asked.
    Failed(String),
}

pub(super) struct ApiKey {
    pub(super) source: Source,
    /// A new key, masked as it is typed.
    pub(super) field: TextEdit,
    pub(super) check: Check,
    /// Why the key could not be saved or removed.
    pub(super) error: Option<String>,
    /// The check in flight.
    pending: Option<Receiver<Result<(), String>>>,
    /// The first run's Continue: the typed key is saved once it passes.
    saving: bool,
    /// The poll clock when the source was last read.
    read_at: f64,
}

impl Default for ApiKey {
    fn default() -> Self {
        Self {
            source: Source::NotSet,
            field: TextEdit::default().secure(),
            check: Check::Idle,
            error: None,
            pending: None,
            saving: false,
            read_at: f64::NEG_INFINITY,
        }
    }
}

impl ApiKey {
    /// Whether a transcription can be asked for.
    pub(super) fn is_set(&self) -> bool {
        self.source != Source::NotSet
    }

    /// The key typed into the field, if any.
    fn typed(&self) -> Option<String> {
        let typed = self.field.text.trim();
        (!typed.is_empty()).then(|| typed.to_owned())
    }

    pub(super) fn has_typed(&self) -> bool {
        self.typed().is_some()
    }

    pub(super) fn checking(&self) -> bool {
        self.check == Check::Checking
    }
}

/// A failure as a sentence: `ScribeError` and `key::save` word theirs to
/// follow a colon.
fn sentence(why: impl std::fmt::Display) -> String {
    let why = why.to_string();
    let mut characters = why.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => why,
    }
}

/// Reads where the key comes from again.
pub(super) fn refresh(ui: &mut Ui) -> bool {
    if !ui.live() {
        return false;
    }
    let source = Source::read();
    let changed = source != ui.api_key.source;
    ui.api_key.source = source;
    changed
}

/// Follows a key set or removed outside the window while `shown`, and adopts
/// a finished check. True when what shows changed.
pub(super) fn poll(ui: &mut Ui, clock: f64, shown: bool) -> bool {
    let mut changed = false;
    if shown && clock >= ui.api_key.read_at + READ_EVERY {
        ui.api_key.read_at = clock;
        changed |= refresh(ui);
    }
    let Some(pending) = &ui.api_key.pending else {
        return changed;
    };
    let result = match pending.try_recv() {
        Ok(result) => result,
        Err(TryRecvError::Empty) => return changed,
        Err(TryRecvError::Disconnected) => Err(sentence("the check stopped")),
    };
    ui.api_key.pending = None;
    answer(ui, result);
    true
}

/// Takes a check's answer. A passing check started by the first run's
/// Continue saves the typed key.
pub(super) fn answer(ui: &mut Ui, result: Result<(), String>) {
    let saving = std::mem::take(&mut ui.api_key.saving);
    ui.api_key.check = match result {
        Ok(()) => Check::Works,
        Err(why) => Check::Failed(why),
    };
    if saving && ui.api_key.check == Check::Works {
        store(ui);
    }
}

/// Test: asks ElevenLabs whether the typed key works, or the key in use when
/// nothing is typed. The answer arrives through [`poll`].
pub(super) fn test(ui: &mut Ui) {
    start(ui, false);
}

/// The first run's Continue: checks the typed key and saves it once
/// ElevenLabs accepts it.
pub(super) fn check_and_save(ui: &mut Ui) {
    start(ui, true);
}

fn start(ui: &mut Ui, saving: bool) {
    if ui.api_key.checking() {
        return;
    }
    let typed = ui.api_key.typed();
    ui.api_key.error = None;
    if typed.is_none() && !ui.api_key.is_set() {
        ui.api_key.check = Check::Failed(sentence(ScribeError::MissingKey));
        return;
    }
    ui.api_key.check = Check::Checking;
    ui.api_key.saving = saving;
    // A fixture never reaches ElevenLabs: a test answers with `answer`.
    if !ui.live() {
        return;
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let waker = ui.modes.waker.clone();
    let spawned = std::thread::Builder::new()
        .name("hex-key-check".into())
        .spawn(move || {
            let secret = typed.or_else(|| key::current().map(|key| key.secret));
            let result = match secret {
                Some(secret) => scribe::check_key(&secret),
                None => Err(ScribeError::MissingKey),
            };
            let _ = sender.send(result.map_err(sentence));
            if let Some(waker) = waker {
                waker();
            }
        });
    match spawned {
        Ok(_) => ui.api_key.pending = Some(receiver),
        Err(error) => {
            ui.api_key.saving = false;
            ui.api_key.check = Check::Failed(sentence(error));
        }
    }
}

/// Save: keeps the typed key for this user, replacing any saved one, and
/// checks it.
pub(super) fn save(ui: &mut Ui) {
    if ui.api_key.checking() || !store(ui) {
        return;
    }
    test(ui);
}

/// Saves the typed key and empties the field. False when nothing was typed
/// or it could not be saved.
fn store(ui: &mut Ui) -> bool {
    let Some(typed) = ui.api_key.typed() else {
        return false;
    };
    let saved = if ui.live() {
        key::save(&typed)
            .map(|()| Source::read())
            .map_err(|error| sentence(format!("{error:#}")))
    } else if ui.is_fixture() {
        Ok(Source::Saved(key::redacted(&typed)))
    } else {
        return false;
    };
    match saved {
        Ok(source) => {
            ui.api_key.source = source;
            ui.api_key.field.clear();
            ui.api_key.error = None;
            true
        }
        Err(why) => {
            ui.api_key.error = Some(why);
            false
        }
    }
}

/// Remove: forgets the saved key. A key from the environment stays.
pub(super) fn remove(ui: &mut Ui) {
    if ui.api_key.checking() || !matches!(ui.api_key.source, Source::Saved(_)) {
        return;
    }
    let source = if ui.live() {
        match key::clear() {
            Ok(_) => Source::read(),
            Err(error) => {
                ui.api_key.error = Some(sentence(format!("{error:#}")));
                return;
            }
        }
    } else {
        Source::NotSet
    };
    ui.api_key.source = source;
    ui.api_key.check = Check::Idle;
    ui.api_key.error = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui() -> Ui {
        Ui::with(
            crate::app_settings::AppSettings::default(),
            super::super::Service::Fixture(Vec::new()),
        )
    }

    #[test]
    fn a_saved_key_shows_only_its_tail_and_empties_the_field() {
        let mut ui = ui();
        assert!(!ui.api_key.is_set());
        ui.api_key.field.replace_text("  sk_0123456789abcdef  ");
        save(&mut ui);
        assert_eq!(ui.api_key.source, Source::Saved("••••cdef".into()));
        assert!(ui.api_key.field.text.is_empty());
        assert_eq!(ui.api_key.check, Check::Checking, "a saved key is checked");
        answer(&mut ui, Ok(()));
        assert_eq!(ui.api_key.check, Check::Works);
    }

    #[test]
    fn testing_with_no_key_says_so_without_asking_elevenlabs() {
        let mut ui = ui();
        test(&mut ui);
        let Check::Failed(why) = &ui.api_key.check else {
            panic!("expected a failure, got {:?}", ui.api_key.check);
        };
        assert!(why.starts_with("No ElevenLabs API key is set"), "{why}");
    }

    #[test]
    fn the_first_runs_key_is_saved_only_once_it_passes() {
        let mut ui = ui();
        ui.api_key.field.replace_text("sk_wrong_key_00000000");
        check_and_save(&mut ui);
        assert!(ui.api_key.checking());
        answer(&mut ui, Err(sentence(ScribeError::InvalidKey)));
        assert!(!ui.api_key.is_set(), "a refused key is not saved");
        assert_eq!(ui.api_key.field.text, "sk_wrong_key_00000000");
        assert_eq!(
            ui.api_key.check,
            Check::Failed("ElevenLabs refused the API key; check it in Settings".into())
        );

        check_and_save(&mut ui);
        answer(&mut ui, Ok(()));
        assert_eq!(ui.api_key.source, Source::Saved("••••0000".into()));
        assert!(ui.api_key.field.text.is_empty());
    }

    #[test]
    fn only_a_saved_key_can_be_removed() {
        let mut ui = ui();
        ui.api_key.source = Source::Environment;
        remove(&mut ui);
        assert_eq!(ui.api_key.source, Source::Environment);
        ui.api_key.source = Source::fixture();
        ui.api_key.check = Check::Works;
        remove(&mut ui);
        assert_eq!(ui.api_key.source, Source::NotSet);
        assert_eq!(ui.api_key.check, Check::Idle);
    }
}
