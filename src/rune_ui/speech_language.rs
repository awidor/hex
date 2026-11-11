//! The language a person can switch to: an input source's mapping in
//! General, and a dictation mode's choice in Modes. Both offer Don't switch,
//! then the languages Settings names.

use rune::El;

use super::controls::{Choice, Paint, select, setting_row};
use super::settings::toggle_menu;
use super::{Menu, Ui};
use crate::app_settings::KeyboardModelChoice;
use crate::desktop::i18n::tr;
use crate::transcription::models::{self as transcription_models, TranscriptionModelId};

/// Whether the Modes pane offers a language per mode: Labs › Switch model
/// with app is on where the runtime can follow the frontmost app.
pub(super) fn enabled(ui: &Ui) -> bool {
    ui.platform.caps.app_model_switching && ui.settings.labs.app_model_switching
}

/// One row's keys and copy.
pub(super) struct LanguageRow<'a> {
    /// The row's key.
    pub(super) key: &'a str,
    /// The select's key.
    pub(super) select_key: &'a str,
    pub(super) title: &'a str,
    pub(super) description: Option<&'a str>,
    /// The saved choice; None shows Don't switch.
    pub(super) choice: Option<KeyboardModelChoice>,
    pub(super) menu: Menu,
}

/// A setting row choosing Don't switch or a language. `save` writes the
/// choice, or clears it with None.
pub(super) fn setting(
    ui: &Ui,
    p: Paint,
    keys: LanguageRow<'_>,
    save: impl Fn(&mut Ui, Option<KeyboardModelChoice>) + Clone + 'static,
    layer: &mut Vec<El>,
) -> El {
    let LanguageRow {
        key,
        select_key,
        title,
        description,
        choice,
        menu,
    } = keys;
    let saved = choice.as_ref().map(|choice| choice.language.as_str());
    let mut choices = vec![Choice {
        label: tr("Don't switch").into(),
        detail: None,
        selected: saved.is_none(),
        choose: {
            let save = save.clone();
            Box::new(move |ui| {
                ui.open = None;
                save(ui, None);
            })
        },
    }];
    let model = transcription_models::definition(TranscriptionModelId::ScribeV3);
    for code in model.language_choices() {
        let save = save.clone();
        choices.push(Choice {
            label: transcription_models::language_name(code).to_owned(),
            detail: None,
            selected: saved == Some(code),
            choose: Box::new(move |ui| {
                ui.open = None;
                save(
                    ui,
                    Some(KeyboardModelChoice {
                        model: TranscriptionModelId::ScribeV3,
                        language: code.to_owned(),
                    }),
                );
            }),
        });
    }
    let label = saved.map_or(tr("Don't switch"), transcription_models::language_name);
    setting_row(
        p,
        key,
        title,
        description,
        select(
            p,
            select_key,
            tr("Language"),
            label,
            ui.open == Some(menu),
            choices,
            move |ui| toggle_menu(ui, menu),
            layer,
        ),
    )
}
