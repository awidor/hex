//! The first-run onboarding flow (`t405`), featuring:
//!
//! - Flat, crisp Rune 2D UI (no embossed cards or shader starbursts).
//! - The key step, which asks for an ElevenLabs API key before anything
//!   else: Hex sends what you dictate to ElevenLabs to transcribe it.
//! - Hex's real `Dot scope` phosphor dot-matrix HUD (`rune_hud.wgsl`) on
//!   `"With Hex"` and `"Your turn"`.
//! - Live interactive `"Your turn"` in desktop windows: pauses with a focused
//!   [`rune_kit::TextArea`] so holding `⌥` and speaking into the running Hex
//!   listener pastes real dictated words right into the card.
//! - Calm permission grants and the escaping Accessibility panel (`permissions`).
//! - Auditionable typing key-sound candidates and `#292` `feedback` tones (`sfx`).

use rune::{Anim, El, TextStyle, Transition, col, el, row, text};
use rune_kit::{Button, ButtonVariant, TextArea, TextInput, glyphs};

use super::super::api_key::Check;
use super::super::controls::{DESCRIPTION, KeySize, Paint, PushSize, keycap};
use super::composition::Layout;
use super::dictated::Stage;
use super::field_text::Field;
use super::fixture::ANSWER_TRY;
use super::flight::{Flight, caret_x, ember_tone, flicker, flying, typeset};
use super::flows::continue_button;
use super::hex_meter::hex_meter;
use super::hud_stage::{frame_slot, voice_or_idle_slot};
use super::landing::{LEAD, arrives, keys, ready, title_font};
use super::login::{self, Login, login_ask};
use super::mic_line::mic_capsule;
use super::sparks::impacts;
use super::stories::{Onboard, Step, is_live_window};
use super::{live, permissions, versus};
use crate::desktop::i18n::{tr, tr_fill};

/// The key step's field.
const KEY_W: f32 = 320.0;
const TOP_BAR: f32 = 48.0;
/// Between the split layout's heading column and its stage.
const SPLIT_GAP: f32 = 48.0;
const ACTION_H: f32 = 32.0;

const TRY_W: f32 = 468.0;
const TRY_HUD: [f32; 2] = [TRY_W * 0.5, 36.0];
const TRY_FIELD_X: f32 = 14.0;
const TRY_FIELD_Y: f32 = 96.0;
const TRY_FIELD_W: f32 = TRY_W - 2.0 * TRY_FIELD_X;
const TRY_PAD: f32 = 18.0;
const TRY_TEXT: Field = Field {
    font: TextStyle::new(18.0, 460.0),
    line: 26.0,
    width: TRY_FIELD_W - 2.0 * TRY_PAD,
};
const HOT: f32 = 0.52;
const PRACTICE_HUD_H: f32 = 74.0;
const PRACTICE_GAP: f32 = 16.0;
/// Lines the practice field grows to before it scrolls.
const PRACTICE_ROWS: usize = 5;
const KNOCK: f32 = 8.0;
const BURST: f64 = 0.06;

pub(super) fn view(p: Paint, s: &Onboard) -> Vec<El> {
    let top = row()
        .key("top-bar")
        .w_full()
        .h(TOP_BAR)
        .px(20.0)
        .center_y()
        .justify_center();
    let top = top
        .children(reopen(p, s))
        .children(skip(p, s))
        .children(corner_skip(p, s));
    // The real window has no title bar: the top of the page moves it.
    let top = if s.live.is_some() {
        super::super::sidebar::title_bar(top)
    } else {
        top
    };
    let page = match s.step {
        Step::Key => key_page(p, s),
        Step::Parked | Step::YourTurn | Step::Hearing | Step::Heard | Step::Done => demo_page(p, s),
        Step::Microphone | Step::Accessibility => permissions::page(p, s),
        Step::Try | Step::TryHearing | Step::TryHeard => {
            super::game::page(p, s).unwrap_or_else(|| try_page(p, s))
        }
        Step::Ready => ready_page(p, s),
    };
    let note = super::first_screen::note(p, s);
    [
        top,
        col()
            .key("middle")
            .grow(1.0)
            .shrink()
            .w_full()
            .center()
            .pb(TOP_BAR)
            .child(page),
    ]
    .into_iter()
    .chain(note)
    .collect()
}

/// Skip, quiet in the top-right corner of the optional pages only: the
/// demo and the practice. It passes over that part to the next step setup
/// still needs, and never past a permission.
fn skip(p: Paint, s: &Onboard) -> Option<El> {
    s.live.as_ref().filter(|_| s.step.optional()).map(|_| {
        let label = tr("Skip");
        let quiet = p.muted();
        row()
            .key("skip-corner")
            .absolute(0.0, 8.0)
            .w_full()
            .pr(20.0)
            .justify_end()
            .child(
                Button::new(label)
                    .variant(ButtonVariant::Ghost)
                    .label_with(move |font, _| text(label).key("label").font(font).color(quiet))
                    .on_press(live::skip)
                    .build_push(p.k)
                    .key("skip"),
            )
    })
}

/// `Skip →` in the top-right corner, where a composed layout puts it
/// rather than under the demo; the real window has its own.
fn corner_skip(p: Paint, s: &Onboard) -> Option<El> {
    let on = s.live.is_none()
        && Layout::chosen().corner_skip()
        && matches!(
            s.step,
            Step::Parked | Step::YourTurn | Step::Hearing | Step::Heard
        )
        && versus::skippable(s);
    on.then(|| {
        row()
            .key("skip-corner")
            .absolute(0.0, 8.0)
            .w_full()
            .pr(20.0)
            .justify_end()
            .transition(arrives())
            .child(super::flows::skip_button(p, s))
    })
}

/// Hex's shortcut didn't start after setup and Hex won't relaunch itself
/// again: one line at the top with Reopen Hex.
fn reopen(p: Paint, s: &Onboard) -> Option<El> {
    s.live.as_ref().filter(|live| live.reopen).map(|_| {
        row()
            .key("reopen")
            .absolute(0.0, 8.0)
            .w_full()
            .gap(10.0)
            .center()
            .transition(arrives())
            .child(
                text(tr("Hex's shortcut didn't start."))
                    .key("why")
                    .font(LEAD)
                    .color(p.muted()),
            )
            .child(
                Button::new(tr("Reopen Hex"))
                    .variant(ButtonVariant::Neutral)
                    .on_press(live::reopen)
                    .build_push(p.k)
                    .key("reopen-hex"),
            )
    })
}

/// The key step: Hex transcribes with ElevenLabs, so the first run asks for
/// an API key before anything else. Continue checks the typed key with
/// ElevenLabs and saves it; Skip for now goes on without one. A story has
/// nobody to paste a key, so it shows the step at rest.
fn key_page(p: Paint, s: &Onboard) -> El {
    let key = s.live.as_ref().map(|live| &live.key);
    let checking = key.is_some_and(|key| key.check == Check::Checking);
    let typed = key.is_some_and(|key| !key.field.text.trim().is_empty());
    let field = match key {
        Some(key) => TextInput::new(&key.field)
            .on_edit(live::key_field)
            .on_submit(|ui: &mut super::super::Ui, _: &mut rune::Tx| live::continue_key(ui)),
        None => TextInput::new(&rune::TextEdit::default()),
    }
    .secure()
    .label(tr("ElevenLabs API key"))
    .placeholder(tr("Paste your API key"))
    .width(Some(KEY_W))
    .disabled(checking || key.is_none())
    .build(p.k)
    .key("key-field");
    let label = tr("Continue");
    let go = Button::new(label)
        .variant(ButtonVariant::Contrast)
        .disabled(checking || !typed)
        .label_with(move |font, ink| {
            row()
                .gap(6.0)
                .center_y()
                .child(text(label).key("label").font(font).color(ink))
                .child(
                    rune::icon(&glyphs::ARROW_RIGHT, 15.0)
                        .key("arrow")
                        .color(ink),
                )
        });
    let go = if s.live.is_some() {
        go.on_press(live::continue_key)
    } else {
        go
    };
    let said = key.and_then(|key| match (&key.error, &key.check) {
        (Some(error), _) => Some((error.as_str(), p.danger())),
        (None, Check::Checking) => Some((tr("Checking…"), p.muted())),
        (None, Check::Failed(why)) => Some((why.as_str(), p.danger())),
        (None, Check::Idle | Check::Works) => None,
    });
    let skip = Button::new(tr("Skip for now"))
        .variant(ButtonVariant::Ghost)
        .disabled(checking);
    let skip = if s.live.is_some() {
        skip.on_press(live::skip_key)
    } else {
        skip
    };
    col()
        .key("key")
        .gap(22.0)
        .items_center()
        .child(
            s.motion
                .staggered(hex_meter(p, "key-mark", 44.0, 1.0, 1.0), 0.0, 8.0),
        )
        .child(heading(
            p,
            s,
            "key-head",
            tr("Connect ElevenLabs"),
            lead(
                p,
                tr("Paste an API key from your ElevenLabs account so Hex can transcribe what you say."),
            ),
        ))
        .child(
            s.motion.staggered(
                col()
                    .key("key-entry")
                    .gap(10.0)
                    .items_center()
                    .child(
                        row()
                            .key("key-row")
                            .gap(10.0)
                            .center_y()
                            .child(field)
                            .child(go.build_push(p.k).key("continue").default_button()),
                    )
                    // The line keeps its place, so a result never moves the
                    // field above it.
                    .child(
                        row()
                            .key("key-said")
                            .w(TRY_W)
                            .min_h(18.0)
                            .center()
                            .children(said.map(|(copy, color)| {
                                text(copy.to_owned())
                                    .key("key-result")
                                    .font(DESCRIPTION)
                                    .color(color)
                                    .align(rune::Align::Center)
                                    .wrap()
                                    .max_w(TRY_W)
                                    .transition(Transition::opacity())
                            })),
                    ),
                1.0,
                8.0,
            ),
        )
        .child(
            s.motion.staggered(
                col()
                    .key("key-skip")
                    .gap(6.0)
                    .items_center()
                    .child(skip.build_push(p.k).key("skip-key"))
                    .child(no_key_note(p)),
                2.0,
                8.0,
            ),
        )
}

/// What going on without a key means, under Skip for now and on
/// `"Hex is ready"`.
fn no_key_note(p: Paint) -> El {
    text(tr(
        "Dictation won't work until you add a key in Settings › General.",
    ))
    .key("no-key-note")
    .font(DESCRIPTION)
    .color(p.muted())
    .align(rune::Align::Center)
    .wrap()
    .w(TRY_W)
}

fn ready_page(p: Paint, s: &Onboard) -> El {
    let core = ready(p, &s.caps, s.ready_age(), s.reduced.get(), s.motion, s.home);

    if s.live.is_some() {
        return col()
            .key("ready-live-wrap")
            .gap(20.0)
            .items_center()
            .child(core)
            // Without a key Hex is set up but can't dictate yet: say so,
            // and offer no practice, which needs one.
            .children((!s.has_key).then(|| no_key_note(p).transition(arrives())))
            .children(
                login_ask(p, &s.login, s.now, s.reduced.get(), false, login::toggle)
                    .map(|ask| ask.transition(arrives())),
            )
            .child(
                row()
                    .key("ready-actions")
                    .gap(10.0)
                    .center_y()
                    .transition(arrives())
                    .children(s.has_key.then(|| {
                        Button::new(tr("Try again"))
                            .variant(ButtonVariant::Neutral)
                            .on_press(live::on_ui(Onboard::jump_to_try))
                            .build_push(p.k)
                            .key("try-again")
                    }))
                    .child(
                        Button::new(tr("Start using Hex"))
                            .variant(ButtonVariant::Contrast)
                            .on_press(live::finish)
                            .build_push(p.k)
                            .key("start-using-hex")
                            .default_button(),
                    ),
            );
    }
    let ask = login_ask(
        p,
        &s.login,
        s.now,
        s.reduced.get(),
        true,
        |s: &mut Onboard| {
            let next = if s.login.login == Login::On {
                Login::Off
            } else {
                Login::On
            };
            if s.login.set(next, s.now) {
                s.sfx.on_login(s.now);
            }
        },
    );
    if !is_live_window() {
        return col()
            .key("ready-live-wrap")
            .gap(20.0)
            .items_center()
            .child(core)
            .children(ask);
    }
    col()
        .key("ready-live-wrap")
        .gap(20.0)
        .items_center()
        .child(core)
        .children(ask)
        .child(
            Button::new("Try dictating again")
                .variant(ButtonVariant::Neutral)
                .on_press(Onboard::jump_to_try)
                .build_push(p.k)
                .key("try-again")
                .transition(arrives()),
        )
}

fn action(content: El) -> El {
    row().key("action").h(ACTION_H).center().child(content)
}

fn none() -> El {
    el().key("none")
}

fn lead(p: Paint, copy: impl Into<String>) -> El {
    text(copy.into())
        .key("lead")
        .font(LEAD)
        .color(p.muted())
        .align(rune::Align::Center)
        .transition(Transition::opacity())
}

fn heading(p: Paint, s: &Onboard, key: &str, title: &str, under: El) -> El {
    s.motion.heading(
        col()
            .key(key.to_string())
            .gap(8.0)
            .items_center()
            .child(text(title).key("title").font(title_font(p)).color(p.text()))
            .child(row().key("lead-row").h(28.0).center().child(under)),
    )
}

fn hold(p: Paint, s: &Onboard, rest: &str, down: bool) -> El {
    row()
        .key("hold")
        .gap(8.0)
        .center_y()
        .transition(Transition::opacity())
        .child(text(tr("Hold")).key("hold").font(LEAD).color(p.muted()))
        .child(keys(p, "keys", &s.caps, down))
        .child(text(rest).key("rest").font(LEAD).color(p.muted()))
}

fn demo_page(p: Paint, s: &Onboard) -> El {
    let layout = Layout::chosen();
    if layout.split() {
        return row()
            .key("demo")
            .gap(SPLIT_GAP)
            .center_y()
            .child(versus::heading(p, s))
            .child(versus::stage(p, s));
    }
    let page = col()
        .key("demo")
        .gap(16.0)
        .items_center()
        .child(versus::heading(p, s))
        .child(versus::stage(p, s));
    if layout.corner_skip() {
        return page;
    }
    page.child(action(if s.step == Step::Done {
        continue_button(p, s)
    } else if versus::skippable(s) {
        super::flows::skip_button(p, s)
    } else {
        none()
    }))
}

fn try_heard_at(s: &Onboard) -> Vec<f64> {
    let count = ANSWER_TRY.text().split_whitespace().count();
    (0..count)
        .map(|i| {
            if s.streams() {
                ANSWER_TRY
                    .updates
                    .iter()
                    .find(|(_, partial)| partial.split_whitespace().count() > i)
                    .map_or(ANSWER_TRY.release, |(at, _)| *at)
            } else {
                ANSWER_TRY.release + i as f64 * BURST
            }
        })
        .collect()
}

fn try_lines() -> usize {
    TRY_TEXT.wrap(ANSWER_TRY.text()).len()
}

fn try_field_h() -> f32 {
    try_lines() as f32 * TRY_TEXT.line + 2.0 * TRY_PAD
}

fn try_text_origin() -> [f32; 2] {
    [TRY_FIELD_X + TRY_PAD, TRY_FIELD_Y + TRY_PAD]
}

fn try_flights(s: &Onboard) -> Vec<(Flight, super::field_text::Slot<'static>)> {
    let Some(from) = s.take_from.filter(|_| s.step >= Step::TryHearing) else {
        return Vec::new();
    };
    let origin = try_text_origin();
    TRY_TEXT
        .slots(ANSWER_TRY.text())
        .into_iter()
        .zip(try_heard_at(s))
        .enumerate()
        .map(|(i, (slot, at))| {
            let to = [origin[0] + slot.centre[0], origin[1] + slot.centre[1]];
            (
                Flight::new(TRY_HUD, to, from + at, HOT, TRY_TEXT.font.size, i),
                slot,
            )
        })
        .collect()
}

fn try_page(p: Paint, s: &Onboard) -> El {
    let holding = s.step == Step::TryHearing || s.live_hold_from.is_some();
    let warming = s.live.as_ref().is_some_and(|live| live.warming);
    let head = match s.step {
        Step::TryHeard => heading(
            p,
            s,
            "try-heard",
            tr("That's Hex"),
            lead(p, tr("It types wherever your cursor is, in any app.")),
        ),
        _ if warming => heading(p, s, "try", tr("Your turn"), lead(p, live::warming_copy())),
        _ => heading(
            p,
            s,
            "try",
            tr("Your turn"),
            hold(p, s, tr("and say anything."), holding),
        ),
    };
    let stage = if s.interactive_try() {
        interactive_try_stage(p, s)
    } else {
        try_stage(p, s)
    };
    col()
        .key("try-page")
        .gap(22.0)
        .items_center()
        .child(head)
        .child(s.motion.staggered(stage, 1.0, 8.0))
        .child(
            s.motion.staggered(
                col()
                    .key("try-foot")
                    .gap(14.0)
                    .items_center()
                    .child(action(if s.step == Step::TryHeard {
                        continue_button(p, s)
                    } else {
                        none()
                    }))
                    .child(instrument_bar(p, s)),
                2.0,
                8.0,
            ),
        )
}

fn instrument_bar(p: Paint, s: &Onboard) -> El {
    let down = s.step == Step::TryHearing || s.live_hold_from.is_some();
    row()
        .key("instrument-bar")
        .gap(8.0)
        .center_y()
        .child(mic_capsule(p, s.listening_peak()))
        .child(shortcut_capsule(p, &s.caps, down, s.live.is_none()))
}

/// The real first run shows the saved shortcut, which Settings changes.
fn shortcut_capsule(p: Paint, caps: &[String], down: bool, cycles: bool) -> El {
    let k = p.k;
    let c = &k.c;
    let label = if cycles {
        tr("Change shortcut")
    } else {
        tr("Your shortcut")
    };
    let pill = row()
        .key("change-shortcut")
        .h(28.0)
        .pl(5.0)
        .pr(11.0)
        .gap(7.0)
        .center_y()
        .radius(14.0)
        .fill(c.surface_raised)
        .border(p.hair, c.border_weak)
        .child(
            row().key("mini-keys").gap(2.0).center_y().children(
                caps.iter()
                    .enumerate()
                    .map(|(i, cap)| keycap(p, i, cap, KeySize::Small, down).scale(0.9)),
            ),
        )
        .child(text(label).key("label").font(DESCRIPTION).color(p.muted()));
    let mut pill = if cycles {
        pill.role("button")
            .label(label)
            .focusable(true)
            .focus_ring(1.5, 1.0, c.border_focus)
            .hover(c.hover)
            .press(c.pressed)
            .on_press(Onboard::cycle_shortcut)
    } else {
        pill
    };
    for shadow in k.shadow_layers(&k.elevation.button_neutral) {
        pill = pill.box_shadow(shadow);
    }
    pill
}

/// Live interactive `"Your turn"` stage: real `Dot scope` HUD above a focused
/// [`TextArea`] that receives real dictated pastes from the running Hex app (or
/// direct typing).
fn interactive_try_stage(p: Paint, s: &Onboard) -> El {
    let hud_el = if let Some(frame) = s.hud.frame() {
        Some(frame_slot("try-dot-hud", TRY_HUD, 1.14, frame))
    } else if s.hud.finished() {
        None
    } else {
        voice_or_idle_slot("try-dot-hud", TRY_HUD, 1.14, true, &ANSWER_TRY, None, s).0
    };

    let shortcut = s.caps.join("");
    let stage = practice_stage(p);
    let reduced = s.reduced.get();
    let warming = s.live.as_ref().is_some_and(|live| live.warming);
    let editor = TextArea::new(&s.practice)
        .placeholder(if s.dictated.hearing() {
            String::new()
        } else if warming {
            live::warming_copy().to_owned()
        } else {
            tr_fill("Hold {} and say a sentence…", &shortcut)
        })
        .width(Some(TRY_FIELD_W))
        .auto_grow(3, PRACTICE_ROWS)
        .spans(s.dictated.spans(p, &stage, s.now, reduced));
    let editor = if s.live.is_some() {
        editor
            .on_edit(live::practice)
            .on_change(live::practice_changed)
    } else {
        editor
            .on_edit(|s: &mut Onboard| &mut s.practice)
            .on_change(Onboard::on_practice_change)
    }
    .build(p.k)
    .key("try-live-editor");

    col()
        .key("try-stage")
        .w(TRY_W)
        .items_center()
        .gap(PRACTICE_GAP)
        .child(
            el().key("try-hud-box")
                .size(TRY_W, PRACTICE_HUD_H)
                .children(hud_el),
        )
        .child(editor)
        .children(s.live.as_ref().and_then(|live| live::hint(p, live)))
        .child(s.dictated.overlay(p, "dictated", &stage, s.now, reduced))
}

/// Where the practice field's text sits under the plate: the kit's
/// [`TextArea`] pads its text 8 points and sets it in the UI face.
fn practice_stage(p: Paint) -> Stage {
    const PAD: f32 = 8.0;
    let line = (13.0f32 * 1.35 * 2.0).round() / 2.0;
    let field_y = PRACTICE_HUD_H + PRACTICE_GAP;
    Stage {
        from: TRY_HUD,
        origin: [TRY_FIELD_X + PAD, field_y + PAD],
        field: Field {
            font: p.k.ty.ui.lh(line),
            line,
            width: TRY_FIELD_W - 2.0 * PAD,
        },
        rows: PRACTICE_ROWS,
        size: [TRY_W, field_y + PRACTICE_ROWS as f32 * line + 2.0 * PAD],
    }
}

fn try_stage(p: Paint, s: &Onboard) -> El {
    let reduced = s.reduced.get();
    let hearing = s.step == Step::TryHearing;
    let flights = try_flights(s);
    let (knock, flash) = if reduced {
        ([0.0, 0.0], 0.0)
    } else {
        let knock = flights.iter().fold([0.0, 0.0], |[x, y], (f, _)| {
            let [dx, dy] = f.hit(s.now);
            [x + dx, y + dy]
        });
        let flash = flights
            .iter()
            .map(|(f, _)| f.flash(s.now))
            .fold(0.0, f32::max);
        (knock.map(|v| v.clamp(-KNOCK, KNOCK)), flash)
    };
    let h = TRY_FIELD_Y + try_field_h() + 14.0;

    let hud_el = voice_or_idle_slot(
        "try-dot-hud",
        TRY_HUD,
        1.14,
        s.step == Step::Try,
        &ANSWER_TRY,
        s.take_from.filter(|_| s.step >= Step::TryHearing),
        s,
    )
    .0;

    let k = p.k;
    let border = if hearing {
        k.c.border_focus
    } else {
        k.c.border_base
    };
    let speaking = flights.iter().any(|(f, _)| s.now >= f.start());
    let field = el()
        .key("try-field")
        .absolute(TRY_FIELD_X, TRY_FIELD_Y)
        .size(TRY_FIELD_W, try_field_h())
        .radius(14.0)
        .continuous_corners()
        .fill(k.c.bg_base)
        .border(
            p.hair,
            border.mix(ember_tone(p, 0.65), (flash * 0.9).min(0.85)),
        )
        .offset(knock[0], knock[1])
        .offset_motion(Anim::INSTANT)
        .children((!speaking).then(|| {
            text(tr("Say anything…"))
                .key("placeholder")
                .font(TRY_TEXT.font.lh(TRY_TEXT.line))
                .color(p.faint())
                .absolute(TRY_PAD + 3.0, TRY_PAD)
                .transition(Transition::opacity())
        }));
    let field = k.elevate(field, &k.elevation.raised);
    let only: Vec<Flight> = flights.iter().map(|(f, _)| *f).collect();
    el().key("try-stage")
        .size(TRY_W, h)
        .children(hud_el)
        .child(field)
        .child(try_words(p, s, &flights, knock, h))
        .child(impacts(p, "impacts", [TRY_W, h], &only, s.now, reduced).absolute(0.0, 0.0))
}

fn try_words(
    p: Paint,
    s: &Onboard,
    flights: &[(Flight, super::field_text::Slot<'static>)],
    knock: [f32; 2],
    h: f32,
) -> El {
    let reduced = s.reduced.get();
    let origin = try_text_origin();
    let font = TRY_TEXT.font.lh(TRY_TEXT.line);
    let weight = 500.0 + 220.0 * HOT;
    let mut out = el()
        .key("try-words")
        .absolute(0.0, 0.0)
        .size(TRY_W, h)
        .instant_changes();
    let mut targets = Vec::new();
    let mut line = 0;
    for (i, (flight, slot)) in flights.iter().enumerate() {
        if let Some(landed) = flight.landed(s.now, reduced) {
            let at = [
                origin[0] + slot.centre[0] + knock[0],
                origin[1] + slot.centre[1] + knock[1],
            ];
            out = out.children(typeset(
                p,
                &format!("w{i}"),
                slot.word,
                font,
                at,
                landed,
                flicker(s.now, i),
            ));
        }
        if reduced || s.now >= flight.arrives() - 0.10 {
            if slot.line != line {
                targets.clear();
                line = slot.line;
            }
            targets.push((flight.arrives() - 0.10, slot.right + 1.5));
        }
        if let Some(pose) = flight.pose(s.now, reduced) {
            out = out.child(flying(p, &format!("f{i}"), slot.word, weight, pose));
        }
    }
    if matches!(s.step, Step::Try | Step::TryHearing) {
        let x = if reduced {
            targets.last().map_or(0.0, |t| t.1)
        } else {
            caret_x(0.0, &targets, s.now)
        };
        out = out.child(
            el().key("caret-at")
                .absolute(origin[0] + knock[0], origin[1] + knock[1])
                .child(TRY_TEXT.caret(p, "caret", x, line)),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_try_fits_its_field_and_flies_hot() {
        assert!(try_lines() <= 2, "{}", try_lines());
        const { assert!(HOT > 0.42) };
        let [x, y] = try_text_origin();
        assert!(x > TRY_FIELD_X && y > TRY_HUD[1] + 24.0);
    }
}
