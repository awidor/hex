//! Settings › General › Transcription: the ElevenLabs API key, the language
//! dictation is transcribed in, which text a dictation pastes, and how
//! Scribe writes it.

use rune::{El, row};
use rune_kit::{Button, ButtonVariant, TextInput};

use super::super::api_key::{self, Check, Source};
use super::super::controls::{
    CardRow, Paint, PushSize, folding, icon_described_row, icon_row, icon_toggle_row, note,
};
use super::super::icons::{CHECKS, ERASER, HASH, KEY, LANGUAGES};
use super::super::{Menu, Request, Ui};
use super::{pick_row, pick_select, quiet_select, search};
use crate::desktop::i18n::{tr, tr_fill};
use crate::transcription::models::{self as transcription_models, TranscriptionModelId};
use crate::transcription::scribe::{FinalText, NumbersFormat, key};

/// The field a new key is pasted into.
const KEY_WIDTH: f32 = 180.0;

/// The section's rows, kept while their titles or descriptions match
/// `query`.
pub(super) fn rows(ui: &Ui, p: Paint, layer: &mut Vec<El>, query: &str) -> Vec<CardRow> {
    let settings = &ui.settings.scribe;
    let mut rows = search::Rows::new(query);
    let sent = tr("Audio you dictate is sent to ElevenLabs for transcription");
    rows.extend_described(tr("ElevenLabs API key"), Some(sent), key_rows(ui, p, sent));
    rows.push(tr("Language"), language_row(ui, p, layer));
    let (final_text, final_text_color) = match doubled_usage(ui) {
        Some(warning) => (warning, p.warning()),
        None => (final_text_note(settings.final_text).to_string(), p.muted()),
    };
    rows.push_described(
        tr("Final text"),
        Some(&final_text),
        icon_described_row(
            p,
            "final-text",
            &CHECKS,
            tr("Final text"),
            Some((final_text.clone(), final_text_color)),
            pick_select(
                ui,
                p,
                "final-text",
                tr("Final text"),
                vec![tr("Batch"), tr("Live")],
                FinalText::ALL
                    .iter()
                    .position(|choice| *choice == settings.final_text)
                    .unwrap_or(0),
                |ui, index| {
                    let choice = FinalText::ALL[index];
                    if ui.settings.scribe.final_text != choice {
                        ui.update(|settings| settings.scribe.final_text = choice);
                    }
                },
                layer,
            ),
        ),
    );
    let fillers = tr("Also drops false starts and sounds that aren't speech");
    rows.push_described(
        tr("Leave out filler words"),
        Some(fillers),
        icon_toggle_row(
            p,
            "no-verbatim",
            &ERASER,
            tr("Leave out filler words"),
            fillers,
            settings.no_verbatim,
            |ui: &mut Ui| {
                let on = !ui.settings.scribe.no_verbatim;
                ui.update(|settings| settings.scribe.no_verbatim = on);
            },
        ),
    );
    rows.push(
        tr("Numbers"),
        pick_row(
            ui,
            p,
            "numbers-format",
            &HASH,
            tr("Numbers"),
            vec![tr("Auto"), tr("Digits"), tr("Words")],
            NumbersFormat::ALL
                .iter()
                .position(|format| *format == settings.numbers_format)
                .unwrap_or(0),
            |ui, index| {
                let format = NumbersFormat::ALL[index];
                if ui.settings.scribe.numbers_format != format {
                    ui.update(|settings| settings.scribe.numbers_format = format);
                }
            },
            layer,
        ),
    );
    rows.into_rows()
}

/// The warning that replaces Final text's note while a lab that shows live
/// words has each dictation sent to ElevenLabs twice.
fn doubled_usage(ui: &Ui) -> Option<String> {
    use crate::labs::Feature;
    // Live typing exists on a Wayland session only, as its Labs card does.
    let lab = crate::labs::doubling_usage(&ui.settings, |feature| {
        feature != Feature::LiveTyping || ui.platform.wayland
    })?;
    let name = match lab {
        Feature::LiveTyping => tr("Live typing"),
        _ => tr("Live text"),
    };
    Some(tr_fill(
        "{} is on in Labs, so each dictation is sent to ElevenLabs twice, roughly doubling your usage. Live sends it once",
        name,
    ))
}

/// What each Final text choice trades.
fn final_text_note(choice: FinalText) -> &'static str {
    match choice {
        FinalText::Batch => tr("Most accurate: the whole clip is transcribed when you release"),
        FinalText::Live => tr("Fastest: uses the live model's text"),
    }
}

/// Where the key in use comes from, in the row's description: a missing
/// key reads as the problem it is.
fn key_status(ui: &Ui, p: Paint) -> (String, rune::Color) {
    match &ui.api_key.source {
        Source::NotSet => (
            tr("Not set. Dictation won't work until you add one").to_owned(),
            p.danger(),
        ),
        Source::Saved(redacted) => (tr_fill("{} saved in Hex", redacted), p.muted()),
        Source::Environment => (
            tr_fill("From the {} environment variable", key::ENVIRONMENT),
            p.muted(),
        ),
    }
}

/// The key's row, what its last check or save said, and that audio leaves
/// this computer. A key from the environment is read-only here: it can be
/// tested, never replaced or removed.
fn key_rows(ui: &Ui, p: Paint, sent: &str) -> Vec<CardRow> {
    let state = &ui.api_key;
    let busy = state.checking();
    let mut controls = row().key("api-key-controls").gap(8.0).center_y();
    if state.source != Source::Environment {
        controls = controls
            .child(
                TextInput::new(&state.field)
                    .secure()
                    .label(tr("ElevenLabs API key"))
                    .placeholder(tr("Paste your API key"))
                    .width(Some(KEY_WIDTH))
                    .disabled(busy)
                    .on_edit(|ui: &mut Ui| &mut ui.api_key.field)
                    .on_submit(|ui: &mut Ui, _: &mut rune::Tx| api_key::save(ui))
                    .build(p.k)
                    .key("api-key-field"),
            )
            .child(
                Button::new(tr("Save"))
                    .variant(ButtonVariant::Neutral)
                    .disabled(busy || !state.has_typed())
                    .on_press(api_key::save)
                    .build_push(p.k)
                    .key("api-key-save"),
            );
    }
    controls = controls.child(
        Button::new(tr("Test key"))
            .variant(ButtonVariant::Neutral)
            .disabled(busy || !(state.is_set() || state.has_typed()))
            .on_press(api_key::test)
            .build_push(p.k)
            .key("api-key-test"),
    );
    if matches!(state.source, Source::Saved(_)) {
        controls = controls.child(
            Button::new(tr("Remove"))
                .variant(ButtonVariant::Neutral)
                .disabled(busy)
                .on_press(api_key::remove)
                .build_push(p.k)
                .key("api-key-remove"),
        );
    }
    let said = match (&state.error, &state.check) {
        (Some(error), _) => Some(note("api-key-result", error, p.danger())),
        (None, Check::Idle) => None,
        (None, Check::Checking) => Some(note("api-key-result", tr("Checking…"), p.muted())),
        (None, Check::Works) => Some(note("api-key-result", tr("Key works"), p.success())),
        (None, Check::Failed(why)) => Some(note("api-key-result", why, p.danger())),
    };
    vec![
        icon_described_row(
            p,
            "api-key",
            &KEY,
            tr("ElevenLabs API key"),
            Some(key_status(ui, p)),
            controls,
        )
        .into(),
        folding("api-key-result-fold", said),
        note("transcription-sent", sent, p.muted()).into(),
    ]
}

/// The language dictation is transcribed in: Auto, then the languages
/// Settings names. A code saved from the command line that Settings doesn't
/// name shows as the choice it is.
fn language_row(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let saved = ui.settings.transcription.language.as_str();
    let codes = transcription_models::definition(TranscriptionModelId::ScribeV3).language_choices();
    let mut labels: Vec<String> = codes
        .iter()
        .map(|code| transcription_models::language_name(code).to_owned())
        .collect();
    let selected = codes
        .iter()
        .position(|code| *code == saved)
        .unwrap_or_else(|| {
            labels.push(transcription_models::language_name(saved).to_owned());
            labels.len() - 1
        });
    let control = quiet_select(
        ui,
        p,
        Menu::Pick("transcription-language"),
        "transcription-language-control".to_owned(),
        tr("Language"),
        labels,
        selected,
        move |ui, index| {
            if let Some(code) = codes.get(index) {
                choose_language(ui, code);
            }
        },
        layer,
    );
    icon_row(
        p,
        "transcription-language",
        &LANGUAGES,
        tr("Language"),
        None,
        control,
    )
}

/// Asks the host to transcribe in `language` from now on.
fn choose_language(ui: &mut Ui, language: &str) {
    ui.open = None;
    if ui.settings.transcription.language == language {
        return;
    }
    ui.send(Request::ChooseModel {
        model: TranscriptionModelId::ScribeV3,
        language: language.to_owned(),
    });
}
