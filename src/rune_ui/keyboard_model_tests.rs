use super::*;
use rune::text::TextSystem;
use rune::{Env, Runtime};

fn runtime(scene: &str) -> Runtime<Ui> {
    let mut state = Ui::with(AppSettings::default(), Service::Fixture(Vec::new()));
    SCENES.iter().find(|(name, _)| *name == scene).unwrap().1(&mut state);
    if scene == "portal" {
        state.platform.caps = platform::Capabilities::LINUX;
    }
    // Only Labs' Models group open, so the lab's card stays inside the
    // window however many labs this OS lists above it.
    for group in crate::labs::Group::ALL {
        state
            .settings
            .labs
            .groups
            .insert(group.id().to_string(), group == crate::labs::Group::Models);
    }
    let text = TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES);
    let mut rt = crate::rune_ui::offscreen::runtime(
        state,
        view,
        (980.0, 1720.0),
        Env {
            system_dark: true,
            scale: 1.0,
        },
        text,
    );
    rt.settle_all();
    rt.frame(0.0);
    rt
}

fn tap(rt: &mut Runtime<Ui>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.settle_all();
    rt.frame(0.0);
}

#[test]
fn keyboard_model_switching_is_a_labs_toggle_wherever_layouts_are_observable() {
    let mut rt = runtime("mac-settings");
    assert!(!rt.state.settings.keyboard_model_switching.enabled);
    assert!(
        rt.find("keyboard-model-switching").is_none(),
        "no General row"
    );
    tap(&mut rt, "nav-labs");
    tap(&mut rt, "lab-keyboard-model-switching");
    assert!(rt.state.settings.keyboard_model_switching.enabled);
    assert_eq!(rt.state.saved, 1);
    tap(&mut rt, "lab-keyboard-model-switching");
    assert!(!rt.state.settings.keyboard_model_switching.enabled);
    let mut rt = runtime("windows-settings");
    tap(&mut rt, "nav-labs");
    assert!(rt.find("lab-keyboard-model-switching").is_some());
    // Wayland (the portal scene) has no layout protocol.
    let mut rt = runtime("portal");
    rt.state.platform.caps.keyboard_model_switching = false;
    tap(&mut rt, "nav-labs");
    assert!(rt.find("lab-keyboard-model-switching").is_none());
}

/// The select item for `language`: Don't switch comes first.
fn item(language: &str) -> usize {
    crate::transcription::models::definition(TranscriptionModelId::ScribeV3)
        .language_choices()
        .iter()
        .position(|code| *code == language)
        .unwrap()
        + 1
}

#[test]
fn keyboard_switching_maps_enabled_sources_to_a_language() {
    use crate::app_settings::KeyboardModelChoice;
    let mut rt = runtime("mac-settings");
    rt.state.platform.input_sources = vec![
        crate::transcription::keyboard_model_switching::InputSource {
            id: "com.apple.keylayout.Spanish".into(),
            name: "Spanish".into(),
            languages: vec!["es".into()],
        },
    ];
    rt.state.advanced = vec![Pane::General];
    rt.state.settings.keyboard_model_switching.enabled = true;
    rt.invalidate();
    rt.settle_all();
    rt.frame(0.0);
    assert!(
        rt.find("input-source-model0-trigger").is_none(),
        "there is one model, so only a language to pick"
    );
    let mapping = |rt: &Runtime<Ui>| {
        rt.state
            .settings
            .keyboard_model_switching
            .mappings
            .get("com.apple.keylayout.Spanish")
            .cloned()
    };
    assert_eq!(mapping(&rt), None, "a source doesn't switch until mapped");
    tap(&mut rt, "input-source-language0-trigger");
    tap(
        &mut rt,
        &format!("input-source-language0-item{}", item("es")),
    );
    assert_eq!(
        mapping(&rt),
        Some(KeyboardModelChoice {
            model: TranscriptionModelId::ScribeV3,
            language: "es".into(),
        })
    );
    tap(&mut rt, "input-source-language0-trigger");
    tap(
        &mut rt,
        &format!("input-source-language0-item{}", item("de")),
    );
    assert_eq!(mapping(&rt).unwrap().language, "de");
    // Don't switch clears the mapping.
    tap(&mut rt, "input-source-language0-trigger");
    tap(&mut rt, "input-source-language0-item0");
    assert_eq!(mapping(&rt), None);
}

#[test]
fn every_source_has_a_language_select_whether_or_not_it_switches() {
    let rt = runtime("mac-input-source-models");
    // ABC and Russian are mapped; Spanish doesn't switch.
    for index in 0..3 {
        assert!(
            rt.find(&format!("input-source-language{index}-trigger"))
                .is_some(),
            "source {index}"
        );
    }
    assert_eq!(rt.state.settings.keyboard_model_switching.mappings.len(), 2);
}
