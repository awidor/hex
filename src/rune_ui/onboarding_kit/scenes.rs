//! The first run's scenes: the tour state a scene starts from, then the
//! first run played to a page.

use super::super::{Ui, onboarding};
use super::live::Page;

/// The tour state a Linux first-run scene starts from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::rune_ui) enum FirstRun {
    /// A new install: no API key yet.
    Fresh,
    /// A key was pasted and ElevenLabs is being asked whether it works.
    Checking,
    /// ElevenLabs refused the pasted key.
    Refused,
    /// The person went on without a key.
    Skipped,
    /// An API key is set.
    Ready,
    /// An API key is set, and the desktop asks before Hex's first paste
    /// (GNOME and KDE on Wayland).
    PasteAccess,
}

pub(in crate::rune_ui) fn first_run(ui: &mut Ui, state: FirstRun, page: Page) {
    use super::super::api_key;
    *ui = super::super::portal_fixture();
    onboarding::fixture(ui, crate::desktop::onboarding::Step::Setup);
    if matches!(state, FirstRun::Ready | FirstRun::PasteAccess) {
        ui.api_key.source = api_key::Source::fixture();
    }
    if state == FirstRun::PasteAccess {
        ui.platform.paste = Some(super::super::paste_access::PasteAccess::Ask);
    }
    onboarding::poll(ui);
    // Without a key the run holds on the key step, where the states below
    // happen.
    let key_set = ui.api_key.is_set();
    super::live::fixture(ui, if key_set { page } else { Page::Key });
    match state {
        FirstRun::Checking | FirstRun::Refused => {
            ui.api_key
                .field
                .replace_text("sk_fixture_key_that_is_typed");
            super::live::continue_key(ui);
            if state == FirstRun::Refused {
                api_key::answer(
                    ui,
                    Err("ElevenLabs refused the API key; check it in Settings".into()),
                );
            }
        }
        FirstRun::Skipped => super::live::skip_key(ui),
        FirstRun::Fresh | FirstRun::Ready | FirstRun::PasteAccess => return,
    }
    onboarding::poll(ui);
    super::live::fixture_resume(ui, page);
}

/// A macOS first run at `step`, played to `page`. The key step comes first,
/// so every page past it has a key.
pub(in crate::rune_ui) fn mac_first_run(ui: &mut Ui, step: onboarding::MacStep, page: Page) {
    *ui = super::super::mac_fixture();
    onboarding::mac_fixture(ui, step);
    if page != Page::Key {
        ui.api_key.source = super::super::api_key::Source::fixture();
        onboarding::poll(ui);
    }
    super::live::fixture(ui, page);
}
