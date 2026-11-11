//! The first run and its comedy as playground stories (the `playground`
//! experiment, ⇧⌘D, group Onboarding). Each plays itself on open and on R,
//! and in a live desktop window pauses on `"Your turn"` with a focused
//! [`rune::TextEdit`] so you can hold `⌥` and test real voice dictation live.

#[cfg(test)]
mod motion_gate;
mod replay;
mod side;

pub(super) use replay::strip;

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rune::{Cx, El, Play, Playground, Story, TextEdit, TextStyle, Tx, canvas, col, row, text};

use super::super::controls::Paint;
use super::cards::{self, Caption, Side, TAP_BOX, bubble};
use super::dictated::Dictated;
use super::fixture::ANSWER_TRY;
use super::hex_meter::BUILD_SECS;
use super::hud_stage::LiveHud;
use super::live::Live;
use super::scene_script::Script;
use super::sfx::SfxTrack;
use super::typing::Glyph;
use super::{flows, permissions, typing, versus};
use crate::desktop::onboarding::Practice;
use crate::feedback::KeySoundStyle;

static LIVE_WINDOW: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_live_window(live: bool) {
    LIVE_WINDOW.store(live, Ordering::Relaxed);
}

pub(crate) fn is_live_window() -> bool {
    LIVE_WINDOW.load(Ordering::Relaxed)
}

/// How long a story shows the key step before it goes on as if a key had
/// been pasted. The real first run waits for the person.
const KEY_HOLD: f64 = 1.6;
/// How long `"Without Hex · With Hex"` shows its intro before the comedy.
const INTRO_HOLD: f64 = 1.4;
const PERMISSIONS_WAIT: f64 = 0.6;
// Ask for the next display frame, including 120 Hz displays. Rune paces a
// hidden or unpaced window itself rather than spinning on a zero deadline.
pub(super) const FRAME: Duration = Duration::ZERO;
const SIDE_W: f32 = 240.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flow {
    Versus,
    FirstRun,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Step {
    /// The first run's key step: Hex asks for an ElevenLabs API key.
    /// `"Without Hex · With Hex"` has no key to ask for and shows its intro.
    Key,
    Parked,
    YourTurn,
    Hearing,
    Heard,
    Done,
    Microphone,
    Try,
    TryHearing,
    TryHeard,
    Accessibility,
    Ready,
}

impl Step {
    /// The demo and the practice: the pages setup doesn't need, which Skip
    /// passes over.
    pub(crate) fn optional(self) -> bool {
        matches!(
            self,
            Self::Parked
                | Self::YourTurn
                | Self::Hearing
                | Self::Heard
                | Self::Done
                | Self::Try
                | Self::TryHearing
                | Self::TryHeard
        )
    }
}

pub(crate) struct Onboard {
    pub(crate) flow: Flow,
    /// An ElevenLabs API key is set, so Hex can transcribe: the key step is
    /// passed, and the practice can hear.
    pub(crate) has_key: bool,
    /// A key was set when this run began, so its key step never showed.
    pub(crate) had_key: bool,
    /// A run resumed part-way: its key step goes on to what setup still
    /// needs, not to the comedy again.
    pub(crate) resumed: bool,
    /// Words show while the shortcut is held (Scribe's live session), not
    /// only once it lifts.
    pub(crate) streams: bool,
    pub(crate) caps: Vec<String>,
    shortcut_index: usize,
    pub(crate) step: Step,
    pub(crate) now: f64,
    pub(crate) reduced: Cell<bool>,
    pub(crate) motion: super::landing::Motion,
    /// When the story last polled.
    polled: f64,
    pub(crate) parked_from: Option<f64>,
    pub(crate) typing_from: Option<f64>,
    /// The conversation came on screen open, Jay's messages landing in it,
    /// rather than growing out of the Launch Room's clicked row.
    pub(crate) in_place: bool,
    pub(crate) voice_from: Option<f64>,
    /// When the resting HUD plate first showed on `"With Hex"` or
    /// `"Your turn"`: the HUD unfolds from here and keeps running through the
    /// take, so pressing the shortcut never restarts it.
    pub(crate) hud_from: Option<f64>,
    pub(crate) take_from: Option<f64>,
    /// How many `"Without Hex"` bubbles have flown out of the field so far.
    sent_seen: usize,
    /// Whether `"With Hex"`'s reply has flown out of the field.
    hex_seen: bool,
    /// Every pause and the key pace of `"Without Hex"`, tunable live.
    pub(crate) timing: typing::Timing,
    /// `"Without Hex"` as [`Self::timing`] times it.
    pub(crate) scene: typing::Scene,
    pub(crate) partial: &'static str,
    pub(crate) take: &'static super::fixture::Take,
    pub(crate) meter: (f32, f32),
    /// When the story began: its pages move on its clock from then.
    started_from: Option<f64>,
    /// Both permissions are on.
    pub(crate) granted: bool,
    /// The microphone is on.
    pub(crate) mic_granted: bool,
    /// The practice game has started this run.
    played: bool,
    pub(crate) scripted_try: bool,
    pub(crate) permissions_from: Option<f64>,
    ready_from: Option<f64>,
    manual_override: bool,
    pub(crate) auto_due_at: Option<f64>,
    pub(crate) practice: TextEdit,
    pub(crate) live_hold_from: Option<f64>,
    /// The `"Your turn"` plate, on the microphone's real level.
    pub(crate) hud: LiveHud,
    /// A dictation's text landed since the plate last stepped.
    pub(crate) hud_landed: bool,
    pub(crate) sfx: SfxTrack,
    /// Set when this is Hex's real first run (Labs):
    /// what the window's tour reports, in place of the fixtures.
    pub(crate) live: Option<Live>,
    /// `"Hex is ready"`'s Launch at login ask.
    pub(crate) login: super::login::LoginAsk,
    /// The dictated words flying into the practice field.
    pub(crate) dictated: Dictated,
    /// A story's stand-in for a real dictation on the interactive try.
    pub(crate) rehearsal: Option<Rehearsal>,
    /// The interactive try as the practice game (`game`).
    pub(crate) game: Option<super::game::Session>,
    /// The interactive try as the plain practice field, not the game.
    pub(crate) practice_field: bool,
    /// Presses too short to dictate on the practice, for its coaching.
    pub(crate) taps: u8,
    /// The shortcut starts and stops dictation instead of being held.
    pub(crate) toggle: bool,
    /// A story that plays the whole first run by itself: it presses
    /// the Launch Room's row, and its scripted player ends the game.
    pub(crate) autoplay: bool,
    /// Where Hex lives once onboarding closes, under `"Hex is ready"`.
    pub(crate) home: super::landing::Home,
}

/// A story's dictation of [`ANSWER_TRY`] on the interactive `"Your turn"`,
/// through the same live words and paste as Hex's real one.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rehearsal {
    /// Whether the model sends live words while the shortcut is held.
    pub(crate) streams: bool,
    from: f64,
    pasted: bool,
}

impl Rehearsal {
    /// How long after `"Your turn"` shows the shortcut goes down.
    const LEAD: f64 = 1.0;
    /// How long the paste takes after the shortcut comes up.
    const TRANSCRIBE: f64 = 0.45;

    pub(crate) fn new(streams: bool) -> Self {
        Self {
            streams,
            from: f64::INFINITY,
            pasted: false,
        }
    }

    fn elapsed(&self, now: f64) -> f64 {
        now - self.from - Self::LEAD
    }

    fn held(&self, now: f64) -> bool {
        let t = self.elapsed(now);
        (0.0..ANSWER_TRY.release).contains(&t)
    }

    fn partial(&self, now: f64) -> Option<crate::transcription::engine::Partial> {
        (self.streams && self.held(now)).then(|| crate::transcription::engine::Partial {
            stable: ANSWER_TRY.partial(self.elapsed(now)).to_owned(),
            tentative: String::new(),
        })
    }

    fn paste_due(&mut self, now: f64) -> bool {
        let due = !self.pasted && self.elapsed(now) >= ANSWER_TRY.release + Self::TRANSCRIBE;
        self.pasted |= due;
        due
    }
}

impl Drop for Onboard {
    fn drop(&mut self) {
        self.sfx.flush_offline_if_requested(self.now.max(1.0));
    }
}

const PRESET_SHORTCUTS: &[&[&str]] = &[&["⌥"], &["fn"], &["⌃", "Space"], &["R⌘"]];

impl Onboard {
    pub(super) fn new(flow: Flow) -> Self {
        let timing = if flow == Flow::FirstRun {
            typing::Timing::first_run()
        } else {
            typing::Timing::default()
        };
        Self {
            flow,
            // The comedy alone has no key to ask for.
            has_key: flow == Flow::Versus,
            had_key: false,
            resumed: false,
            streams: true,
            caps: crate::app_settings::AppSettings::default()
                .dictation_hotkey
                .keycaps(),
            shortcut_index: 0,
            step: Step::Key,
            now: 0.0,
            reduced: Cell::new(false),
            motion: super::landing::Motion::default(),
            polled: f64::NEG_INFINITY,
            parked_from: None,
            typing_from: None,
            in_place: false,
            voice_from: None,
            hud_from: None,
            take_from: None,
            sent_seen: 0,
            hex_seen: false,
            scene: typing::Scene::build(&timing),
            timing,
            partial: "",
            take: Script::chosen().take(),
            meter: (0.0, 0.0),
            started_from: None,
            granted: false,
            mic_granted: false,
            played: false,
            scripted_try: false,
            permissions_from: None,
            ready_from: None,
            manual_override: false,
            auto_due_at: None,
            practice: TextEdit::default(),
            live_hold_from: None,
            hud: LiveHud::default(),
            hud_landed: false,
            sfx: SfxTrack::default(),
            live: None,
            login: super::login::LoginAsk::default(),
            dictated: Dictated::default(),
            rehearsal: None,
            game: None,
            practice_field: false,
            taps: 0,
            toggle: false,
            autoplay: false,
            home: super::landing::Home::MenuBar,
        }
    }

    /// True when `"Your turn"` should wait for real voice dictation / typing
    /// rather than auto-playing the scripted reply.
    pub(crate) fn interactive_try(&self) -> bool {
        self.flow == Flow::FirstRun
            && (self.live.is_some() || is_live_window() || self.rehearsal.is_some())
            && !self.scripted_try
    }

    /// Cycles the shortcut capsule on `"Your turn"` so clicking it previews
    /// changing the dictation key.
    pub(crate) fn cycle_shortcut(&mut self) {
        self.shortcut_index = (self.shortcut_index + 1) % PRESET_SHORTCUTS.len();
        self.caps = PRESET_SHORTCUTS[self.shortcut_index]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        self.sfx.on_capsule_tap();
    }

    pub(crate) fn choose_key_sound(&mut self, style: KeySoundStyle) {
        self.sfx.set_style_and_preview(style, self.now);
    }

    /// Called when the interactive `"Your turn"` [`TextArea`] changes (e.g. a
    /// real dictation paste from Hex lands or the user types).
    pub(crate) fn on_practice_change(&mut self, _: &mut Tx) {
        if super::game::changed(self) {
            return;
        }
        self.dictated.changed(&self.practice.text, self.now);
        let has_words = !self.practice.text.trim().is_empty();
        self.hud_landed |= has_words;
        if has_words && self.step == Step::Try {
            self.step = Step::TryHeard;
            self.sfx.on_live_dictation_landed(self.now);
        } else if !has_words && self.step == Step::TryHeard {
            self.step = Step::Try;
        }
    }

    /// Clicking `Continue →` immediately advances to the next step.
    pub(crate) fn press_continue(&mut self) {
        self.manual_override = true;
        self.auto_due_at = None;
        self.sfx.on_capsule_tap();
        match self.step {
            // The permission steps move on as the story does, so the model
            // and the permissions still to come decide the next page.
            Step::Microphone | Step::Accessibility => {
                self.auto_due_at = Some(self.now);
            }
            Step::Done => self.jump_to_ready(),
            Step::TryHeard => self.after_game(),
            _ => {}
        }
    }

    /// Skip on the demo or the practice: on to the permissions setup still
    /// needs, else Hex is ready.
    pub(crate) fn skip_optional(&mut self) {
        if !self.step.optional() {
            return;
        }
        if self.granted {
            self.jump_to_ready();
        } else {
            self.resume_setup();
        }
    }

    /// Leaving the practice: Accessibility, if it's still to ask for, then
    /// `"Hex is ready"`.
    pub(crate) fn after_game(&mut self) {
        if self.granted {
            self.jump_to_ready();
        } else {
            self.jump_to_accessibility();
        }
    }

    /// Leaves the practice as Continue would, but keeps the soundtrack so
    /// far, for a story that plays itself.
    pub(super) fn leave_game_keeping_sound(&mut self, now: f64) {
        self.manual_override = true;
        self.auto_due_at = if self.granted {
            self.jump_to_ready();
            None
        } else {
            to_accessibility(self, now).map(|secs| now + secs)
        };
    }

    /// The real first run's next beat is due now.
    pub(crate) fn move_on(&mut self) {
        self.manual_override = true;
        self.auto_due_at = Some(self.now);
    }

    /// The key step, as a new install opens on it.
    pub(crate) fn jump_to_key(&mut self) {
        self.manual_override = true;
        self.sfx.clear();
        self.practice.focused = false;
        self.live_hold_from = None;
        self.game = None;
        self.step = Step::Key;
        self.sfx.on_key_step(self.now);
        // A story shows the step a beat; the real first run waits on it.
        self.auto_due_at = Some(if self.live.is_some() {
            self.now
        } else {
            self.has_key = false;
            self.now + KEY_HOLD
        });
    }

    pub(crate) fn jump_to_demo(&mut self) {
        self.manual_override = true;
        self.sfx.clear();
        self.practice.focused = false;
        self.live_hold_from = None;
        self.voice_from = None;
        self.hud_from = None;
        self.take_from = None;
        self.partial = "";
        self.auto_due_at = None;
        park(self, self.now);
    }

    /// the Launch Room's row: plays `"Without Hex"` from the top, then the
    /// story moves on by itself.
    /// `Skip →` or Escape: leaves the comedy for whatever follows it. True
    /// when there was one to skip.
    pub(crate) fn skip_scene(&mut self) -> bool {
        if !versus::skippable(self) {
            return false;
        }
        self.manual_override = true;
        self.sfx.clear();
        self.sfx.on_capsule_tap();
        let now = self.now;
        self.auto_due_at = if self.flow == Flow::FirstRun {
            leave_comedy(self, now)
        } else {
            self.step = Step::Done;
            next_beat(self, now)
        }
        .map(|secs| now + secs);
        true
    }

    pub(crate) fn start_scene(&mut self) {
        if self.step != Step::Parked || self.typing_from.is_some() {
            return;
        }
        self.manual_override = true;
        // The conversation opens on Jay finishing his question; the scene's
        // clock starts once it lands.
        let from = self.now + self.timing.opening;
        self.typing_from = Some(from);
        self.in_place = false;
        self.sfx.on_opened(self.now);
        self.sfx.on_without_hex(from, &self.scene);
        self.auto_due_at = Some(from + self.scene.duration());
    }

    /// `"Without Hex"` with the conversation already open: Jay's messages
    /// land in it one by one, each with its sound, and the scene's clock
    /// starts after them. The first run opens this way.
    pub(crate) fn open_scene(&mut self) {
        if self.step != Step::Parked || self.typing_from.is_some() {
            return;
        }
        let from = self.now + self.scene.lead_in();
        self.typing_from = Some(from);
        self.in_place = true;
        self.sfx.on_arrivals(from, &self.scene);
        self.sfx.on_without_hex(from, &self.scene);
        self.auto_due_at = Some(from + self.scene.duration());
    }

    /// Rebuilds [`Self::scene`] after a change to [`Self::timing`].
    fn retime(&mut self) {
        self.scene = typing::Scene::build(&self.timing);
        if self.step == Step::Parked && self.typing_from.is_some() {
            // A scene already playing starts over, waiting for the button.
            self.typing_from = None;
            self.auto_due_at = None;
            self.sfx.clear();
        }
    }

    pub(crate) fn jump_to_microphone(&mut self) {
        self.leave_for_permission();
        let now = self.now;
        self.auto_due_at = to_microphone(self, now).map(|secs| now + secs);
    }

    pub(crate) fn jump_to_accessibility(&mut self) {
        self.leave_for_permission();
        let now = self.now;
        self.auto_due_at = to_accessibility(self, now).map(|secs| now + secs);
    }

    fn leave_for_permission(&mut self) {
        self.manual_override = true;
        self.sfx.clear();
        self.practice.focused = false;
        self.live_hold_from = None;
    }

    /// A real first run resumed while macOS still has a permission to ask.
    pub(super) fn resume_setup(&mut self) {
        if self.mic_granted {
            self.jump_to_accessibility();
        } else {
            self.jump_to_microphone();
        }
    }

    pub(crate) fn jump_to_try(&mut self) {
        self.manual_override = true;
        self.auto_due_at = None;
        self.sfx.clear();
        self.take_from = None;
        self.hud_from = Some(self.now);
        self.partial = "";
        self.live_hold_from = None;
        self.practice = TextEdit::default();
        self.practice.focused = self.interactive_try();
        self.dictated.reset();
        self.game = None;
        self.played = true;
        if let Some(rehearsal) = self.rehearsal.as_mut() {
            *rehearsal = Rehearsal::new(rehearsal.streams);
            rehearsal.from = self.now;
        }
        self.step = Step::Try;
        if !self.interactive_try() {
            self.auto_due_at = Some(self.now + 1.2);
        }
    }

    /// The real first run sets these as the story would at its start.
    pub(super) fn begin_live(&mut self, now: f64) {
        self.manual_override = true;
        self.auto_due_at = begin_first_run(self, now).map(|secs| now + secs);
    }

    pub(crate) fn jump_to_ready(&mut self) {
        self.manual_override = true;
        self.auto_due_at = None;
        self.practice.focused = false;
        self.live_hold_from = None;
        self.step = Step::Ready;
        self.ready_from = Some(self.now);
        self.sfx.on_ready(self.now, self.reduced.get());
    }

    /// Seconds since `"Hex is ready"` appeared; a scene opened on it shows
    /// it settled.
    pub(crate) fn ready_age(&self) -> f64 {
        self.ready_from
            .map_or(f64::INFINITY, |from| self.now - from)
    }

    pub(crate) fn listening_peak(&self) -> Option<f32> {
        if self.live_hold_from.is_some() {
            return Some(crate::audio::input_level::recent().unwrap_or(0.0));
        }
        matches!(self.step, Step::Hearing | Step::TryHearing).then_some(self.meter.1)
    }

    /// Hex can transcribe: an API key is set.
    pub(crate) fn can_hear(&self) -> bool {
        self.has_key
    }

    pub(crate) fn streams(&self) -> bool {
        self.streams
    }
}

pub(crate) fn add<S: 'static>(playground: Playground<S>) -> Playground<S> {
    playground
        .story(
            Story::new(
                "Prototypes/Onboarding/Without Hex · With Hex",
                || Onboard::new(Flow::Versus),
                view,
            )
            .poll(poll)
            .trigger('r', "Play", play)
            .side(side::side),
        )
        .story(first_run())
        .story(Story::new(
            "Prototypes/Onboarding/Tapbacks",
            || Onboard::new(Flow::Versus),
            tapback_sheet,
        ))
}

/// Every tapback glyph, other people's then the reader's own, over stacks
/// on both sides of a thread, to hold against Messages.
fn tapback_sheet(_: &Onboard, cx: &mut Cx<Onboard>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let glyphs = [
        Glyph::Heart,
        Glyph::ThumbsUp,
        Glyph::ThumbsDown,
        Glyph::Haha,
        Glyph::Bang,
        Glyph::Question,
    ];
    let set = |key: &str, own: bool| {
        row()
            .key(key.to_string())
            .gap(8.0)
            .children(glyphs.iter().enumerate().map(|(i, &glyph)| {
                canvas(cards::tapback(p, glyph, Side::Them, own, true))
                    .key(format!("g{i}"))
                    .size(TAP_BOX, TAP_BOX)
            }))
    };
    let line = |key: &str, side: Side, copy: &str, taps: &[Glyph]| {
        bubble(
            p,
            key.to_string(),
            side,
            Caption::None,
            copy,
            taps,
            1.0,
            true,
        )
    };
    let (fill, rim) = cards::material(p);
    col()
        .key("tapback-sheet")
        .w(380.0)
        .padding(20.0)
        .gap(10.0)
        .radius(18.0)
        .continuous_corners()
        .fill(fill)
        .border(p.hair, rim)
        .child(set("theirs", false))
        .child(set("own", true))
        .child(line(
            "they",
            Side::Them,
            "they can see my screen",
            &[Glyph::Haha],
        ))
        .child(line(
            "sorry",
            Side::You,
            "sorry fart fingers",
            &[Glyph::Haha, Glyph::Haha],
        ))
        .child(line(
            "incontinence",
            Side::You,
            "So sorry for the incontinence",
            &[Glyph::Haha, Glyph::Haha, Glyph::Bang],
        ))
        .child(line(
            "desk",
            Side::Them,
            "Clear your desk.",
            &[Glyph::ThumbsUp, Glyph::ThumbsUp, Glyph::Heart],
        ))
}

fn first_run() -> Story<Onboard> {
    Story::new(
        "Prototypes/Onboarding/First run",
        || {
            super::composition::Layout::Today.choose();
            let mut s = Onboard::new(Flow::FirstRun);
            s.login = super::login::LoginAsk::preview();
            s
        },
        view,
    )
    .poll(poll)
    .trigger('r', "Play", play)
    .trigger('t', "Jump to Your turn", |s, _| s.jump_to_try())
    .trigger('s', "Next typing sound", |s, _| {
        let next = s.sfx.style.next();
        s.choose_key_sound(next);
    })
    .variant("New install", |_| {})
    .variant("API key already set", |s| s.has_key = true)
    .variant("Permissions already granted", |s| s.granted = true)
    .variant("Hex is ready", |s| {
        s.has_key = true;
        s.jump_to_ready();
    })
    .variant("Auto-play video", |s| s.scripted_try = true)
    .variant("Your turn, live words", |s| {
        s.rehearsal = Some(Rehearsal::new(true));
    })
    .variant("Your turn, words on release", |s| {
        s.rehearsal = Some(Rehearsal::new(false));
    })
    .variant("Whole run, scripted game", |s| {
        s.rehearsal = Some(Rehearsal::new(true));
        s.autoplay = true;
    })
    .variant("Whole run, layout A: tighter centre", |s| {
        whole_run_in(s, super::composition::Layout::Centre)
    })
    .variant("Whole run, layout B: split", |s| {
        whole_run_in(s, super::composition::Layout::Split)
    })
    .variant("Whole run, layout C: laptop", |s| {
        whole_run_in(s, super::composition::Layout::Laptop)
    })
    .variant("Your turn, keeps tapping", |s| {
        s.rehearsal = Some(Rehearsal::new(true));
        s.taps = TAPS_BEFORE_TOGGLE;
    })
    .variant("Your turn, practice field", |s| {
        s.rehearsal = Some(Rehearsal::new(true));
        s.practice_field = true;
    })
    .variant("Quick motion", |s| {
        s.motion.enter = 0.28;
        s.motion.exit = 0.20;
        s.motion.keyboard = 0.5;
    })
    .variant("Gentle motion", |s| {
        s.motion.enter = 0.4;
        s.motion.exit = 0.3;
        s.motion.keyboard = 0.8;
    })
    .side(side::side)
}

/// The scripted whole run, composed in `layout`.
fn whole_run_in(s: &mut Onboard, layout: super::composition::Layout) {
    layout.choose();
    s.rehearsal = Some(Rehearsal::new(true));
    s.autoplay = true;
}

fn view(s: &Onboard, cx: &mut Cx<Onboard>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    s.reduced.set(cx.is_reduced_motion());
    flows::view(p, s)
}

fn play(s: &mut Onboard, play: &mut Play<'_, Onboard>) {
    // A story opened on `"Hex is ready"` plays that page again.
    let on_ready = s.step == Step::Ready;
    s.now = play.now();
    s.ready_from = None;
    s.manual_override = false;
    s.auto_due_at = None;
    s.practice.focused = false;
    s.live_hold_from = None;
    s.sfx.clear();
    s.started_from = Some(play.now());
    if s.rehearsal.is_some() && !s.autoplay {
        s.has_key = true;
        s.granted = true;
        s.jump_to_try();
        play.wake_after(Duration::ZERO);
        return;
    }
    if on_ready {
        s.jump_to_ready();
        play.wake_after(Duration::ZERO);
        return;
    }
    if s.flow == Flow::FirstRun {
        start_first_run(s, play);
        return;
    }
    play.after(INTRO_HOLD, advance);
    play.wake_after(Duration::ZERO);
}

fn start_first_run(s: &mut Onboard, play: &mut Play<'_, Onboard>) {
    if let Some(delay) = begin_first_run(s, play.now()) {
        play.after(delay, advance);
    }
    play.wake_after(Duration::ZERO);
}

/// `"Without Hex"`, waiting for the Launch Room's row.
fn park(s: &mut Onboard, now: f64) {
    s.step = Step::Parked;
    s.parked_from = Some(now);
    s.typing_from = None;
    s.sfx.on_parked(now);
}

/// Starts the first run at `now`: on the key step, or past it with a key
/// already set. The story's next beat is due after the returned seconds.
pub(super) fn begin_first_run(s: &mut Onboard, now: f64) -> Option<f64> {
    s.now = now;
    s.started_from = Some(now);
    s.resumed = false;
    s.had_key = s.has_key;
    if s.has_key {
        park(s, now);
        return Some(0.0);
    }
    s.step = Step::Key;
    s.sfx.on_key_step(now);
    // A story shows the step a beat; the real first run waits on it.
    Some(if s.live.is_some() { 0.0 } else { KEY_HOLD })
}

fn advance(s: &mut Onboard, play: &mut Play<'_, Onboard>) {
    if s.manual_override {
        return;
    }
    s.now = play.now();
    if let Some(secs) = next_beat(s, play.now()) {
        play.after(secs, advance);
    }
    play.wake_after(Duration::ZERO);
}

/// Moves the story one beat on at `now`; the next is due after the returned
/// seconds, or waits for the user.
fn next_beat(s: &mut Onboard, now: f64) -> Option<f64> {
    if s.flow == Flow::FirstRun {
        return first_run_next(s, now);
    }
    match s.step {
        // The comedy's intro, then the Launch Room's row to press.
        Step::Key => {
            park(s, now);
            None
        }
        Step::Parked if s.typing_from.is_none() => None,
        Step::Parked => {
            s.step = Step::YourTurn;
            s.voice_from = Some(now);
            s.hud_from = Some(now);
            s.sfx.on_rewind(now);
            Some(versus::BEFORE_KEY)
        }
        Step::YourTurn => {
            s.step = Step::Hearing;
            hear(s, now);
            Some(Script::chosen().take().release + 0.1)
        }
        Step::Hearing => {
            s.step = Step::Heard;
            Some(versus::reply_secs(s))
        }
        Step::Heard => {
            s.step = Step::Done;
            s.sfx.on_score(now);
            Some(SCORE_HOLD)
        }
        Step::Done => {
            s.step = Step::Ready;
            s.ready_from = Some(now);
            s.sfx.on_ready(now, s.reduced.get());
            None
        }
        _ => None,
    }
}

/// How long the score holds before the story moves on, when nobody is
/// there to press `Continue →`.
const SCORE_HOLD: f64 = 4.5;

/// `"With Hex"`: the shortcut goes down and Jay's answer is spoken.
fn hear(s: &mut Onboard, now: f64) {
    s.take = Script::chosen().take();
    s.take_from = Some(now);
    s.sfx.on_voice_take(now, s.take);
    s.sfx.on_words(
        versus::launches(s.streams()).into_iter().map(|at| now + at),
        &mut super::sfx::WordHits::default(),
    );
    let beats = versus::hex_beats(s.streams(), &s.timing);
    s.sfx.on_hex_reply(now, &beats);
}

/// The first run: the API key, `"Without Hex"` and a scripted `"With Hex"`,
/// then the microphone, the practice game as the user's own `"With Hex"`
/// when a key is set, Accessibility, and `"Hex is ready"`.
pub(super) fn first_run_next(s: &mut Onboard, now: f64) -> Option<f64> {
    const WAIT: f64 = 0.25;
    match s.step {
        // The real permissions wait for macOS, then hold their checks a beat.
        Step::Microphone
            if s.live
                .as_ref()
                .is_some_and(|live| !live.mic_granted_for(now)) =>
        {
            Some(WAIT)
        }
        Step::Accessibility if s.live.as_ref().is_some_and(|live| !live.granted_for(now)) => {
            Some(WAIT)
        }
        // The real first run holds the key step until a key is set or the
        // person skips it.
        Step::Key
            if s.live
                .as_ref()
                .is_some_and(|live| !s.has_key && !live.key.skipped) =>
        {
            Some(WAIT)
        }
        Step::Key => {
            // A story goes on as if a key had been pasted.
            if s.live.is_none() {
                s.has_key = true;
            }
            if s.has_key {
                s.sfx.on_key_set(now);
            }
            if s.resumed {
                return leave_comedy(s, now);
            }
            park(s, now);
            first_run_next(s, now)
        }
        Step::Parked if s.typing_from.is_none() => {
            s.open_scene();
            Some(s.scene.lead_in() + s.scene.duration() + COMEDY_HOLD)
        }
        // `"Without Hex"` plays out and its last line holds a beat; then
        // `"With Hex"` shows how it goes, with nothing asked of the user
        // yet.
        Step::Parked => {
            let from = s.typing_from?;
            let left = from + s.scene.duration() + COMEDY_HOLD - now;
            if left > 1e-3 {
                return Some(left);
            }
            s.step = Step::YourTurn;
            s.voice_from = Some(now);
            s.hud_from = Some(now);
            s.sfx.on_rewind(now);
            Some(versus::BEFORE_KEY)
        }
        Step::YourTurn => {
            s.step = Step::Hearing;
            hear(s, now);
            Some(Script::chosen().take().release + 0.1)
        }
        Step::Hearing => {
            s.step = Step::Heard;
            Some(versus::reply_secs(s))
        }
        // The happy ending holds a beat before the permissions come in.
        Step::Heard => {
            let done = s
                .take_from
                .map_or(now, |from| from + versus::hex_done_secs(s));
            let left = done + ENDING_HOLD - now;
            if left > 1e-3 {
                return Some(left);
            }
            leave_comedy(s, now)
        }
        Step::Microphone => to_practice(s, now),
        Step::Try => {
            if s.interactive_try() {
                return None;
            }
            s.step = Step::TryHearing;
            s.take = &ANSWER_TRY;
            s.take_from = Some(now);
            s.sfx.on_voice_take(now, &ANSWER_TRY);
            Some(ANSWER_TRY.release + 0.1)
        }
        Step::TryHearing => {
            s.step = Step::TryHeard;
            Some(2.2)
        }
        Step::TryHeard => {
            if s.interactive_try() {
                return None;
            }
            if s.granted {
                s.jump_to_ready();
                None
            } else {
                to_accessibility(s, now)
            }
        }
        // A real first run that had to ask before the practice plays it now.
        Step::Accessibility if !s.played => to_practice(s, now),
        Step::Accessibility => {
            s.jump_to_ready();
            None
        }
        // The first run has no score: `"With Hex"` goes on to the microphone.
        Step::Done | Step::Ready => None,
    }
}

/// Short presses on the practice before it offers tap to start instead.
pub(crate) const TAPS_BEFORE_TOGGLE: u8 = 2;

/// How long `"Without Hex"`'s last line holds before the first run moves on,
/// after the scene's own hold on the removal ([`typing::Timing::fired`]).
const COMEDY_HOLD: f64 = 0.8;
/// How long `"With Hex"`'s happy ending stays before the first run moves on.
const ENDING_HOLD: f64 = 1.5;

/// Out of the comedy, by `"With Hex"`'s end or `Skip →`.
fn leave_comedy(s: &mut Onboard, now: f64) -> Option<f64> {
    if s.mic_granted || s.granted {
        to_practice(s, now)
    } else {
        to_microphone(s, now)
    }
}

fn to_microphone(s: &mut Onboard, now: f64) -> Option<f64> {
    s.step = Step::Microphone;
    s.permissions_from = Some(now);
    if s.live.is_some() {
        return Some(PERMISSIONS_WAIT);
    }
    s.sfx.on_microphone(now);
    Some(permissions::MIC_SECS)
}

fn to_accessibility(s: &mut Onboard, now: f64) -> Option<f64> {
    s.step = Step::Accessibility;
    s.permissions_from = Some(now);
    if s.live.is_some() {
        return Some(PERMISSIONS_WAIT);
    }
    s.sfx.on_accessibility(now);
    Some(permissions::AX_SECS)
}

/// On to the practice game as `"With Hex"`. Hex's shortcut works only with
/// Accessibility, so a real first run without it asks for it first. Without
/// an API key Hex can't hear the practice: `"Hex is ready"` says so.
fn to_practice(s: &mut Onboard, now: f64) -> Option<f64> {
    if s.live.is_some() && !s.granted {
        return to_accessibility(s, now);
    }
    if !s.can_hear() {
        s.jump_to_ready();
        return None;
    }
    s.step = Step::Try;
    s.played = true;
    s.take_from = None;
    s.hud_from = Some(now);
    s.partial = "";
    s.practice = TextEdit::default();
    s.dictated.reset();
    if let Some(rehearsal) = s.rehearsal.as_mut() {
        *rehearsal = Rehearsal::new(rehearsal.streams);
        rehearsal.from = now;
    }
    if s.interactive_try() {
        s.practice.focused = true;
        None
    } else {
        Some(1.2)
    }
}

#[cfg(target_os = "macos")]
pub(super) fn physical_shortcut_held(caps: &[String]) -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceFlagsState(state_id: u32) -> u64;
    }
    const HID_SYSTEM_STATE: u32 = 1;
    if caps.is_empty() {
        return false;
    }
    let flags = unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) };
    caps.iter().all(|cap| {
        if cap.contains('⌥') || cap.contains("Alt") {
            (flags & 0x0008_0000) != 0
        } else if cap.contains('⌘') {
            (flags & 0x0010_0000) != 0
        } else if cap.contains('⌃') || cap.contains("Ctrl") {
            (flags & 0x0004_0000) != 0
        } else if cap.contains('⇧') || cap.contains("Shift") {
            (flags & 0x0002_0000) != 0
        } else if cap == "fn" {
            (flags & 0x0080_0000) != 0
        } else {
            false
        }
    })
}

#[cfg(not(target_os = "macos"))]
pub(super) fn physical_shortcut_held(_: &[String]) -> bool {
    false
}

fn poll(s: &mut Onboard, now: f64, tx: &mut Tx) -> bool {
    if advance_to(s, now, tx) {
        tx.wake_after(FRAME);
    }
    true
}

/// Moves the story to `now`; true while anything on it still moves.
pub(super) fn advance_to(s: &mut Onboard, now: f64, tx: &mut Tx) -> bool {
    s.now = now;
    tx.trace_channel("onboarding.step", "phase", s.step as u32 as f32);
    tx.trace_channel("onboarding.mic", "level", s.listening_peak().unwrap_or(0.0));
    // A sent `"Without Hex"` reply lifts out of the field into its bubble.
    let sent = versus::sent_count(s);
    for i in s.sent_seen..sent {
        tx.launch("typed/line0", format!("sent-bubble-{i}"));
    }
    s.sent_seen = sent;
    // `"With Hex"`'s answer lifts out of the field from its first word.
    let hex_sent = versus::hex_sent(s);
    if hex_sent && !s.hex_seen {
        tx.launch("w0", "hex-bubble");
    }
    s.hex_seen = hex_sent;
    if s.manual_override
        && let Some(due) = s.auto_due_at
        && now >= due
    {
        s.auto_due_at = next_beat(s, now).map(|secs| now + secs);
    }

    let trying = s.interactive_try() && matches!(s.step, Step::Try | Step::TryHeard);
    if trying && super::game::step(s, now) {
        s.hud.clear();
    } else if trying {
        // The real first run follows Hex's own dictation, which sounds its
        // own tones; a rehearsal plays one without them.
        let real = match s.rehearsal {
            Some(rehearsal) => Some(rehearsal.held(now)),
            None => s.live.as_ref().map(|live| live.listening),
        };
        let held = real.unwrap_or_else(|| physical_shortcut_held(&s.caps));
        match (s.live_hold_from, held) {
            (None, true) => {
                s.live_hold_from = Some(now);
                if real.is_none() {
                    s.sfx.on_live_shortcut_down(now);
                }
            }
            (Some(_), false) => {
                s.live_hold_from = None;
                if real.is_none() {
                    s.sfx.on_live_shortcut_up(now);
                }
            }
            _ => {}
        }
        let partial = match s.rehearsal {
            Some(rehearsal) => rehearsal.partial(now),
            None => crate::dictation::live_text::current(),
        };
        s.dictated.hear(partial.as_ref(), now);
        if s.live.as_ref().is_some_and(|live| {
            matches!(
                live.practice,
                Practice::TooShort | Practice::NothingHeard | Practice::Failed
            )
        }) {
            s.dictated.forget();
        }
        if s.rehearsal.as_mut().is_some_and(|r| r.paste_due(now)) {
            s.practice.insert(ANSWER_TRY.text());
            s.on_practice_change(tx);
        }
        s.dictated.settle(now);
        let landed = std::mem::take(&mut s.hud_landed);
        s.hud.step(
            now,
            s.live_hold_from.is_some(),
            crate::audio::input_level::recent(),
            landed,
            s.reduced.get(),
        );
    } else {
        s.game = None;
        s.live_hold_from = None;
        s.hud.clear();
    }

    // The conversation takes over from the plate it grew on in one frame;
    // its own surface would otherwise fade in as the plate goes, a flash.
    if versus::grown(s, s.polled, now) {
        tx.instant();
    }
    s.polled = now;
    s.sfx.advance_to(now);
    if s.ready_from.is_some_and(|from| now - from >= BUILD_SECS) {
        s.sfx.flush_offline_if_requested(now + 1.5);
    }
    let mut moving = s.sfx.has_pending(now)
        || s.auto_due_at.is_some()
        || s.live_hold_from.is_some()
        || (s.interactive_try() && matches!(s.step, Step::Try | Step::TryHeard))
        || (s.started_from.is_some() && (s.step < Step::Ready || !s.reduced.get()))
        || (s.step == Step::Ready && !s.reduced.get() && s.ready_age() < BUILD_SECS);
    moving |= s.dictated.moving();

    match s.take_from {
        Some(from) => {
            let elapsed = now - from;
            let done = s.take.finished(elapsed);
            s.partial = if done || s.streams() {
                s.take.partial(elapsed)
            } else {
                ""
            };
            s.meter = s.take.meter(elapsed);
            moving |= !done;
        }
        None => {
            let hum = ((now * 9.0).sin() * (now * 4.3).sin()).abs() as f32;
            s.meter = (0.03 + 0.04 * hum, 0.05 + 0.06 * hum);
        }
    }
    moving
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_story_shows_the_key_step_then_goes_on_as_if_a_key_were_pasted() {
        let mut s = Onboard::new(Flow::FirstRun);
        assert!(!s.can_hear(), "a new install has no key");
        s.begin_live(0.0);
        let mut tx = Tx::default();
        poll(&mut s, KEY_HOLD - 0.1, &mut tx);
        assert_eq!(s.step, Step::Key);
        poll(&mut s, KEY_HOLD + 0.1, &mut tx);
        assert_eq!(s.step, Step::Parked, "the comedy follows the key");
        assert!(s.can_hear());
        assert!(!s.had_key, "the key step showed this run");
    }

    #[test]
    fn a_run_with_a_key_already_set_opens_on_the_comedy() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.has_key = true;
        s.begin_live(0.0);
        assert_eq!(s.step, Step::Parked);
        assert!(s.had_key);
    }

    #[test]
    fn a_resumed_key_step_goes_on_to_setup_not_the_comedy() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.resumed = true;
        s.jump_to_key();
        let mut tx = Tx::default();
        poll(&mut s, KEY_HOLD + 0.1, &mut tx);
        assert_eq!(s.step, Step::Microphone);
    }

    #[test]
    fn the_comedy_alone_opens_on_its_intro_and_waits_for_the_row() {
        let mut s = Onboard::new(Flow::Versus);
        assert!(s.can_hear(), "it has no key to ask for");
        assert_eq!(next_beat(&mut s, 1.0), None);
        assert_eq!(s.step, Step::Parked);
        assert!(versus::waiting_to_start(&s));
    }

    #[test]
    fn live_try_pauses_and_completes_when_real_words_paste_in() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.has_key = true;
        s.jump_to_try();
        assert_eq!(s.step, Step::Try);
        s.practice.text = "Testing live dictation in Hex.".into();
        let mut tx = Tx::default();
        s.on_practice_change(&mut tx);
        assert_eq!(s.step, Step::TryHeard);
        s.press_continue();
        assert_eq!(s.step, Step::Accessibility);
    }

    #[test]
    fn the_practice_goes_straight_to_ready_once_both_permissions_are_on() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.has_key = true;
        s.granted = true;
        s.jump_to_try();
        s.practice.text = "Testing live dictation in Hex.".into();
        s.on_practice_change(&mut Tx::default());
        s.press_continue();
        assert_eq!(s.step, Step::Ready);
    }

    #[test]
    fn the_conversation_opens_in_place_and_jay_arrives_in_it() {
        use crate::feedback::{OnboardingTone, Tone};
        let mut s = Onboard::new(Flow::FirstRun);
        s.has_key = true;
        s.begin_live(0.0);
        let mut tx = Tx::default();
        poll(&mut s, 0.0, &mut tx);
        assert_eq!(s.step, Step::Parked);
        assert!(!versus::waiting_to_start(&s), "no row to click");
        let opened = s.scene.lead_in();
        assert_eq!(s.typing_from, Some(opened));
        // Empty as it comes on, then Jay's question lands in it, each line
        // with its sound, and he types again.
        assert_eq!(s.scene.shown(-opened), 0);
        assert_eq!(s.scene.shown(-0.2), 2);
        assert!(s.scene.dots(-0.2).is_some());
        poll(&mut s, opened - 0.01, &mut tx);
        let incoming = s
            .sfx
            .fired_cues()
            .iter()
            .filter(|&&(at, t, _)| at < opened && t == Tone::Onboarding(OnboardingTone::Incoming))
            .count();
        assert_eq!(incoming, 2);
        let end = opened + s.scene.duration();
        poll(&mut s, end - 0.05, &mut tx);
        assert_eq!(s.step, Step::Parked);
        poll(&mut s, end + COMEDY_HOLD + 0.01, &mut tx);
        assert_eq!(s.step, Step::YourTurn);
    }

    #[test]
    fn with_hex_plays_itself_after_without_hex_then_asks_for_the_microphone() {
        use crate::feedback::{OnboardingTone, Tone};
        let mut s = Onboard::new(Flow::FirstRun);
        s.has_key = true;
        s.begin_live(0.0);
        let mut tx = Tx::default();
        poll(&mut s, 30.0, &mut tx);
        let mut now = 30.0;
        let mut shortcut = None;
        let mut steps = vec![s.step];
        while now < 120.0 && s.step != Step::Microphone {
            poll(&mut s, now, &mut tx);
            if s.step == Step::Hearing {
                shortcut.get_or_insert(now);
            }
            if steps.last() != Some(&s.step) {
                steps.push(s.step);
            }
            now += 0.05;
        }
        assert_eq!(
            steps,
            [
                Step::Parked,
                Step::YourTurn,
                Step::Hearing,
                Step::Heard,
                Step::Microphone
            ],
            "nothing asked of the user on the way"
        );
        // From `⌥` going down to the microphone, polled every 0.05 s: the
        // demo, then the end of the scene held for a beat.
        let with_hex = now - shortcut.unwrap();
        assert!(
            with_hex <= super::super::triumph::MAX_SECS + ENDING_HOLD + 0.1,
            "{with_hex:.1} s"
        );
        let demo = versus::hex_done_secs(&s);
        assert!(with_hex >= demo + ENDING_HOLD - 0.1, "{with_hex:.1} s");
        let fired = |tone: Tone| s.sfx.fired_cues().iter().any(|&(_, t, _)| t == tone);
        assert!(fired(Tone::Onboarding(OnboardingTone::ShortcutDown)));
        assert!(
            s.sfx
                .fired_cues()
                .iter()
                .any(|(_, t, _)| matches!(t, Tone::Onboarding(OnboardingTone::Triumph(_))))
        );
    }

    #[test]
    fn the_first_run_asks_for_the_microphone_then_plays_then_asks_for_accessibility() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.scripted_try = true;
        s.begin_live(0.0);
        let mut tx = Tx::default();
        let mut steps = vec![s.step];
        let mut now = 0.0;
        while now < 120.0 && s.step != Step::Ready {
            poll(&mut s, now, &mut tx);
            if steps.last() != Some(&s.step) {
                steps.push(s.step);
            }
            now += 0.05;
        }
        let order: Vec<Step> = [
            Step::Key,
            Step::Parked,
            Step::Heard,
            Step::Microphone,
            Step::Try,
            Step::Accessibility,
            Step::Ready,
        ]
        .into_iter()
        .filter(|step| steps.contains(step))
        .collect();
        assert_eq!(order.len(), 7, "{steps:?}");
        let at = |step| steps.iter().position(|s| *s == step).unwrap();
        assert!(order.windows(2).all(|w| at(w[0]) < at(w[1])), "{steps:?}");
    }

    #[test]
    fn the_autoplaying_story_plays_the_game_and_reaches_ready_by_itself() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.rehearsal = Some(Rehearsal::new(true));
        s.autoplay = true;
        s.manual_override = true;
        s.auto_due_at = begin_first_run(&mut s, 0.0);
        let mut tx = Tx::default();
        let mut now = 0.0;
        let mut played = false;
        while now < 150.0 && s.step != Step::Ready {
            poll(&mut s, now, &mut tx);
            played |= s.game.is_some();
            now += 1.0 / 60.0;
        }
        assert!(played);
        assert_eq!(s.step, Step::Ready);
        assert!(now < 120.0, "{now}");
        let triumphs = s
            .sfx
            .fired_cues()
            .iter()
            .filter(|(_, t, _)| {
                matches!(
                    t,
                    crate::feedback::Tone::Onboarding(crate::feedback::OnboardingTone::Triumph(_))
                )
            })
            .count();
        assert_eq!(
            triumphs, 2,
            "`With Hex`'s payoff and the game's first score card"
        );
    }

    #[test]
    fn without_a_key_the_practice_is_passed_over_for_hex_is_ready() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.granted = true;
        s.has_key = false;
        s.step = Step::Microphone;
        assert_eq!(first_run_next(&mut s, 5.0), None);
        assert_eq!(s.step, Step::Ready, "Hex can't hear a practice yet");

        // Skip on the demo never offers the practice either.
        let mut s = Onboard::new(Flow::FirstRun);
        s.granted = true;
        s.step = Step::Heard;
        s.skip_optional();
        assert_eq!(s.step, Step::Ready);
    }

    #[test]
    fn skip_shows_two_seconds_in_and_leaves_the_comedy() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.has_key = true;
        s.begin_live(0.0);
        let mut tx = Tx::default();
        poll(&mut s, 30.0, &mut tx);
        let opened = 30.0 + s.scene.lead_in();
        poll(&mut s, opened + 1.0, &mut tx);
        assert!(!versus::skippable(&s));
        assert!(!s.skip_scene());
        poll(&mut s, opened + 2.1, &mut tx);
        assert!(versus::skippable(&s));
        assert!(s.skip_scene());
        assert!(matches!(
            s.step,
            Step::Microphone | Step::Accessibility | Step::Try
        ));
    }

    #[test]
    fn skip_leaves_with_hex_too() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.has_key = true;
        s.begin_live(0.0);
        let mut tx = Tx::default();
        poll(&mut s, 30.0, &mut tx);
        let mut now = 30.0;
        while now < 120.0 && s.step != Step::Heard {
            poll(&mut s, now, &mut tx);
            now += 0.05;
        }
        assert!(versus::skippable(&s));
        assert!(s.skip_scene());
        assert_eq!(s.step, Step::Microphone);
    }

    #[test]
    fn a_retimed_scene_plays_at_its_new_length() {
        let mut s = Onboard::new(Flow::FirstRun);
        let before = s.scene.duration();
        s.timing.freeze += 1.0;
        s.retime();
        assert!((s.scene.duration() - before - 1.0).abs() < 1e-9);
    }

    #[test]
    fn reduced_motion_does_not_suppress_onboarding_sounds() {
        let simulate = |reduced: bool| -> Vec<(f64, crate::feedback::Tone, u8)> {
            let mut s = Onboard::new(Flow::FirstRun);
            s.scripted_try = true;
            s.reduced.set(reduced);
            s.begin_live(0.0);
            let mut tx = Tx::default();
            let mut now = 0.0;
            while now < 90.0 && s.step != Step::Ready {
                poll(&mut s, now, &mut tx);
                now += 0.05;
            }
            poll(&mut s, now + 0.3, &mut tx);
            assert_eq!(s.step, Step::Ready);
            s.sfx.fired_cues().to_vec()
        };
        let full = simulate(false);
        let reduced = simulate(true);
        // The one exception: Reduced Motion shows the ready logo already
        // built, so only its last note sounds.
        let build = |t: &crate::feedback::Tone| {
            matches!(
                t,
                crate::feedback::Tone::Onboarding(crate::feedback::OnboardingTone::Build(_))
            )
        };
        let before = |cues: &[(f64, crate::feedback::Tone, u8)]| -> Vec<_> {
            cues.iter().filter(|(_, t, _)| !build(t)).copied().collect()
        };
        assert_eq!(before(&full), before(&reduced));
        let notes = |cues: &[(f64, crate::feedback::Tone, u8)]| -> Vec<_> {
            cues.iter()
                .filter(|(_, t, _)| build(t))
                .map(|&(_, t, _)| t)
                .collect()
        };
        assert_eq!(notes(&full).len(), 3);
        assert_eq!(
            notes(&reduced),
            [crate::feedback::Tone::Onboarding(
                crate::feedback::OnboardingTone::Build(2)
            )]
        );
        assert!(
            full.len() >= 35,
            "expected full sound track, got {}",
            full.len()
        );
    }
}
