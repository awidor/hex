//! The onboarding check (`scripts/check-onboarding.sh`, run before every
//! stable release): every onboarding scene against its approved golden in
//! both appearances, and a first run walked through the real onboarding
//! pages. Like every golden, these render on Linux only: the macOS and
//! Windows scenes are the same window drawn from fixtures.

use super::super::api_key::{self, Check, Source};
use super::super::onboarding_kit::live::{self, Page};
use super::*;
use crate::desktop::onboarding::Step;

/// Every onboarding state a person can see, on macOS and on Linux and
/// Windows. An intentional change refreshes these goldens in the same PR
/// (`HEX_UPDATE_GOLDENS=1`).
pub(super) const SCENES: &[&str] = &[
    // A fresh install: the key step, asking for an ElevenLabs API key.
    "onboarding",
    "mac-onboarding",
    // A pasted key while ElevenLabs is asked about it, and one it refused.
    "onboarding-key-checking",
    "onboarding-key-refused",
    // Skip for now, on to Hex is ready, which says dictation waits for a key.
    "onboarding-no-key",
    // The demo.
    "onboarding-demo",
    // Permissions (macOS): to do, refused, stale entry, waiting for a
    // restart; Windows' microphone privacy switch.
    "mac-onboarding-permissions",
    "mac-onboarding-denied",
    "mac-onboarding-stale",
    "mac-onboarding-restart",
    "windows-onboarding-microphone",
    // GNOME and KDE on Wayland: the desktop's paste permission, explained.
    "onboarding-paste",
    // Your turn: the practice game, and the shortcut that didn't start.
    "onboarding-try",
    "mac-onboarding-try",
    "mac-onboarding-reopen",
    // Hex is ready.
    "onboarding-ready",
    "mac-onboarding-ready",
];

/// The optional pages: the demo and the practice.
const OPTIONAL: &[&str] = &[
    "onboarding-demo",
    "onboarding-try",
    "mac-onboarding-try",
    "mac-onboarding-reopen",
];

/// The scenes on the key step.
const KEY_STEP: &[&str] = &[
    "onboarding",
    "mac-onboarding",
    "onboarding-key-checking",
    "onboarding-key-refused",
];

fn page(rt: &Runtime<Ui>) -> Option<Page> {
    live::page(&rt.state)
}

/// Plays the first run on to `page`, as a fixture's clock never does alone.
fn play_to(rt: &mut Runtime<Ui>, page: Page) {
    rt.update(move |ui, _| live::fixture_resume(ui, page));
    settle(rt);
}

/// Skip shows only on the optional pages, the demo and the practice, and
/// passes over them to the next step setup still needs: a permission, else
/// Hex is ready. It never closes onboarding.
#[test]
fn onboarding_skip() {
    for scene in SCENES {
        let rt = runtime(scene, true, PANE_SIZE);
        assert_eq!(
            rt.find("skip").is_some(),
            OPTIONAL.contains(scene),
            "{scene}: Skip"
        );
    }
    let skip = |rt: &mut Runtime<Ui>, scene: &str, next: Page| {
        tap(rt, "skip");
        settle(rt);
        assert_eq!(page(rt), Some(next), "{scene}: Skip lands here");
        assert!(rt.state.tour.is_some(), "{scene}: onboarding stays open");
        assert_eq!(
            rt.find("skip").is_some(),
            next == Page::Try,
            "{scene}: no Skip on {next:?}"
        );
    };

    // macOS, permissions still to grant: the demo skips to them.
    let mut ui = Ui::with(AppSettings::default(), Service::Fixture(Vec::new()));
    super::super::onboarding_kit::scenes::mac_first_run(
        &mut ui,
        crate::rune_ui::onboarding::MacStep::Setup,
        Page::Demo,
    );
    let mut rt = runtime_for(ui, true, PANE_SIZE);
    assert_eq!(page(&rt), Some(Page::Demo));
    skip(&mut rt, "mac demo", Page::Permissions);

    // Nothing left to set up: the demo and the practice skip to Hex is ready.
    for scene in ["onboarding-demo", "onboarding-try", "mac-onboarding-try"] {
        let mut rt = runtime(scene, true, PANE_SIZE);
        skip(&mut rt, scene, Page::Ready);
    }
}

/// The key step: only there is a key asked for. Continue checks the typed
/// key and goes on only once ElevenLabs accepts it; a refused key stays on
/// the step with why; and the key itself is never drawn.
#[test]
fn onboarding_key_step() {
    for scene in SCENES {
        let rt = runtime(scene, true, PANE_SIZE);
        for key in ["key-field", "skip-key"] {
            assert_eq!(
                rt.find(key).is_some(),
                KEY_STEP.contains(scene),
                "{scene}: {key}"
            );
        }
        let shown = texts(&rt).join("\n");
        assert!(!shown.contains("sk_"), "{scene} draws a key: {shown}");
    }

    let mut rt = runtime("onboarding", true, PANE_SIZE);
    assert_eq!(page(&rt), Some(Page::Key));
    // Nothing typed: Continue has nothing to check.
    tap(&mut rt, "continue");
    assert_eq!(rt.state.api_key.check, Check::Idle);
    play_to(&mut rt, Page::Demo);
    assert_eq!(page(&rt), Some(Page::Key), "the step holds without a key");

    rt.state
        .api_key
        .field
        .replace_text("sk_refused_key_00000001");
    settle(&mut rt);
    tap(&mut rt, "continue");
    assert!(rt.state.api_key.checking(), "ElevenLabs is asked");
    assert!(texts(&rt).contains(&"Checking…".to_owned()));
    let refused = "ElevenLabs refused the API key; check it in Settings";
    rt.update(|ui, _| api_key::answer(ui, Err(refused.into())));
    settle(&mut rt);
    assert!(texts(&rt).contains(&refused.to_owned()), "{:?}", texts(&rt));
    assert!(!rt.state.api_key.is_set(), "a refused key is not saved");
    play_to(&mut rt, Page::Demo);
    assert_eq!(page(&rt), Some(Page::Key), "and the step holds");

    rt.state
        .api_key
        .field
        .replace_text("sk_accepted_key_0000abcd");
    settle(&mut rt);
    tap(&mut rt, "continue");
    rt.update(|ui, _| api_key::answer(ui, Ok(())));
    settle(&mut rt);
    assert_eq!(rt.state.api_key.source, Source::Saved("••••abcd".into()));
    assert!(rt.state.api_key.field.text.is_empty());
    play_to(&mut rt, Page::Demo);
    assert_eq!(page(&rt), Some(Page::Demo), "an accepted key goes on");
}

/// Skip for now on the key step: onboarding goes on without a key, passes
/// over the practice (which needs one), says on Hex is ready that dictation
/// waits for a key, and Settings says so afterwards.
#[test]
fn onboarding_without_a_key() {
    let note = "Dictation won't work until you add a key in Settings › General.";
    let mut rt = runtime("onboarding", true, PANE_SIZE);
    assert!(
        texts(&rt).contains(&note.to_owned()),
        "Skip says what it means"
    );
    tap(&mut rt, "skip-key");
    settle(&mut rt);
    assert_eq!(page(&rt), Some(Page::Demo), "on to the demo");
    assert!(rt.state.tour.is_some(), "onboarding stays open");
    play_to(&mut rt, Page::Ready);
    assert_eq!(page(&rt), Some(Page::Ready), "no practice without a key");
    assert!(texts(&rt).contains(&note.to_owned()), "{:?}", texts(&rt));
    assert!(rt.find("try-again").is_none(), "nothing to try with");

    tap(&mut rt, "start-using-hex");
    settle(&mut rt);
    assert!(rt.state.first_run.is_none(), "onboarding closed");
    assert!(rt.state.tour.is_none(), "and its tour");
    let shown = texts(&rt);
    for expected in [
        "Not set. Dictation won't work until you add one",
        "Hex can't dictate until an ElevenLabs API key is set",
    ] {
        assert!(
            shown.contains(&expected.to_owned()),
            "{expected}: {shown:?}"
        );
    }

    // With a key, Hex is ready offers the practice again and says nothing.
    let rt = runtime("onboarding-ready", true, PANE_SIZE);
    assert!(!texts(&rt).contains(&note.to_owned()));
    assert!(rt.find("try-again").is_some());
}

/// The mic note follows the shortcut: Hold says hold, Toggle says tap,
/// and a mic released while idle has nothing to explain.
#[test]
fn the_mic_note_follows_the_shortcut() {
    let note = |toggle: bool, release: bool| {
        let mut ui = Ui::with(AppSettings::default(), Service::Fixture(Vec::new()));
        super::super::onboarding_kit::scenes::mac_first_run(
            &mut ui,
            crate::rune_ui::onboarding::MacStep::TryIt,
            Page::Try,
        );
        // Each poll reads both from settings again.
        if toggle {
            ui.settings.shortcut_mode = crate::dictation::ShortcutMode::Toggle;
        }
        ui.settings.release_microphone_while_idle = release;
        let s = ui.first_run.as_mut().expect("the first run is open");
        s.toggle = toggle;
        if let Some(live) = s.live.as_mut() {
            live.release_microphone = release;
        }
        texts(&runtime_for(ui, true, PANE_SIZE))
            .into_iter()
            .find(|t| t.starts_with("It only records"))
    };
    let hold = note(false, false).expect("the note shows");
    assert!(
        hold.starts_with("It only records while you hold "),
        "{hold}"
    );
    let tap = note(true, false).expect("the note shows");
    assert!(tap.starts_with("It only records after you tap "), "{tap}");
    assert_eq!(note(false, true), None, "no note once the mic is released");
}

#[test]
fn onboarding_goldens() {
    let _goldens = Goldens::collect();
    for scene in SCENES {
        for dark in [true, false] {
            check_golden(scene, dark, PANE_SIZE);
        }
    }
}

/// A first run on Linux or Windows, walked as a person would: the key
/// step, a key ElevenLabs accepts, the demo, the practice game, and Hex is
/// ready. The window is a fixture: nothing reaches ElevenLabs or the data
/// folder, and the test answers the key's check itself.
#[test]
fn onboarding_walkthrough() {
    let mut ui = portal_fixture();
    ui.platform.caps = platform::Capabilities::LINUX;
    super::super::onboarding::fixture(&mut ui, Step::Welcome);
    live::open(&mut ui);
    assert!(ui.first_run.is_some(), "the first run shows");
    let mut rt = runtime_for(ui, true, PANE_SIZE);
    let step = |rt: &Runtime<Ui>| rt.state.tour.as_ref().map(|tour| tour.flow.step());

    // A new install opens on the key step, and asks the host for nothing.
    assert_eq!(page(&rt), Some(Page::Key));
    let shown = texts(&rt);
    assert!(
        shown.contains(&"Connect ElevenLabs".to_owned()),
        "{shown:?}"
    );
    assert!(
        shown.contains(&"Audio you dictate is sent to ElevenLabs for transcription".to_owned()),
        "the page says where audio goes: {shown:?}"
    );
    assert_eq!(step(&rt), Some(Step::Setup));
    assert!(sent(&rt).is_empty(), "{:?}", sent(&rt));

    // A key is pasted, checked, and saved once ElevenLabs accepts it.
    rt.state
        .api_key
        .field
        .replace_text("sk_walkthrough_key_0001");
    settle(&mut rt);
    tap(&mut rt, "continue");
    assert!(rt.state.api_key.checking());
    rt.update(|ui, _| api_key::answer(ui, Ok(())));
    settle(&mut rt);
    assert!(rt.state.api_key.is_set());

    // The story plays on: the demo, then the practice game ("With Hex"),
    // then Hex is ready.
    play_to(&mut rt, Page::Try);
    assert_eq!(step(&rt), Some(Step::TryIt));
    assert!(texts(&rt).contains(&"With Hex".to_owned()));
    assert!(rt.find("skip").is_some(), "Skip shows on the practice too");
    tap(&mut rt, "done");
    assert!(texts(&rt).contains(&"Hex is ready".to_owned()));
    tap(&mut rt, "start-using-hex");
    assert!(rt.state.tour.is_none(), "Start using Hex closes onboarding");
    assert!(rt.state.first_run.is_none());
}
