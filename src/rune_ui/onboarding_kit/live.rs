//! Hex's first run: the onboarding story, the one onboarding on every OS.
//! A new install (or an upgrade whose setup isn't ready) opens it at
//! launch, and Run setup again and `hex onboarding reset` replay it. The
//! window's tour ([`onboarding`](super::super::onboarding)) is the state
//! machine underneath: it asks for the permissions, relaunches when macOS
//! needs it, watches the practice dictation, and records completion. This
//! module shows the story's pages over the window and feeds them that real
//! state each frame.
//!
//! The story opens on the key step: Hex transcribes with ElevenLabs, so it
//! asks for an API key before anything else. Continue checks the key with
//! ElevenLabs off the UI thread and saves it once accepted; Skip for now
//! goes on without one, and the practice, which needs a key, is passed
//! over. A key already set passes the step.

use crate::desktop::i18n::tr;
use std::sync::Mutex;
use std::time::Instant;

use rune::{El, TextEdit, Transition, Tx, text};

use super::super::api_key::{self, Check};
use super::super::controls::Paint;
use super::super::paste_access::{self, PasteAccess};
use super::super::platform::Os;
use super::super::{Ui, onboarding};
use super::permissions::{FLOAT_OFF, GRANTED_BEAT};
use super::stories::{FRAME, Flow, Onboard, Step, advance_to};
use crate::desktop::onboarding::{
    Input, ItemStatus, Practice, SetupItem, SetupStatus, Step as TourStep,
};
use crate::transcription::scribe::FinalText;

/// How often the first run looks at the permissions.
const CHECK_EVERY: f64 = 0.25;

/// What the real first run reports to the story's pages.
pub(crate) struct Live {
    /// Hex's own dictation records.
    pub(crate) listening: bool,
    /// The listener runs but its microphone and shortcuts aren't up yet, so
    /// the practice waits instead of inviting a hold.
    pub(crate) warming: bool,
    /// The key step: the field, its check, and whether it was skipped.
    pub(crate) key: KeyStep,
    pub(crate) microphone: ItemStatus,
    pub(crate) accessibility: ItemStatus,
    /// General's "Turn off the mic when not dictating" was on when the
    /// first run opened, so the microphone opens only while you dictate.
    pub(crate) release_microphone: bool,
    /// Every permission is on and one applies only after Hex restarts.
    pub(crate) restart: bool,
    /// macOS asks for Microphone and Accessibility; Windows only shows the
    /// Microphone row while its privacy switch is off.
    pub(crate) accessibility_row: bool,
    /// macOS 27 calls Accessibility "Device Control and Data Access".
    pub(crate) accessibility_renamed: bool,
    /// The shortcut listener failed after setup and Hex won't relaunch by
    /// itself again: Reopen Hex.
    pub(crate) reopen: bool,
    /// macOS needed a restart and Hex couldn't relaunch itself.
    pub(crate) relaunch_failed: bool,
    /// The desktop's paste permission on GNOME and KDE Wayland: the
    /// permissions page explains it and asks before the practice pastes.
    pub(crate) paste: Option<PasteAccess>,
    pub(crate) practice: Practice,
    granted_at: Option<f64>,
    mic_granted_at: Option<f64>,
    checked: Option<Checked>,
    /// A scene: the game deals the same messages every time.
    pub(crate) fixture: bool,
    /// Dictation streams send their live words to the practice field.
    _live_words: crate::dictation::live_text::Demand,
}

/// What the key step shows of the window's API key ([`api_key`]).
pub(crate) struct KeyStep {
    /// The key as typed, masked.
    pub(crate) field: TextEdit,
    pub(crate) check: Check,
    /// Why an accepted key could not be saved.
    pub(crate) error: Option<String>,
    /// The person went on without a key.
    pub(crate) skipped: bool,
}

impl Default for KeyStep {
    fn default() -> Self {
        Self {
            field: TextEdit::default().secure(),
            check: Check::Idle,
            error: None,
            skipped: false,
        }
    }
}

impl Live {
    /// macOS keeps showing the mic in use: Hex holds it open between
    /// dictations unless "Turn off the mic when not dictating" is on. A
    /// listener that failed to start holds nothing.
    pub(crate) fn mic_stays_open(&self) -> bool {
        self.accessibility_row && !self.release_microphone && !self.reopen && !self.relaunch_failed
    }
}

/// The last look at macOS's permissions.
#[derive(Clone, Copy)]
struct Checked {
    at: f64,
    status: SetupStatus,
}

impl Live {
    pub(crate) fn granted_for(&self, now: f64) -> bool {
        self.granted_at
            .is_some_and(|at| now - at >= GRANTED_BEAT + FLOAT_OFF)
    }

    pub(crate) fn mic_granted_for(&self, now: f64) -> bool {
        self.mic_granted_at
            .is_some_and(|at| now - at >= GRANTED_BEAT + FLOAT_OFF)
    }

    /// How far an allowed permission's card has floated off at `now`.
    pub(crate) fn floated(&self, accessibility: bool, now: f64) -> f32 {
        let at = if accessibility {
            self.granted_at
        } else {
            self.mic_granted_at
        };
        at.map_or(0.0, |at| {
            ((now - at - GRANTED_BEAT) / FLOAT_OFF).clamp(0.0, 1.0) as f32
        })
    }
}

/// Where a fixture's clock starts.
const FIXTURE_START: f64 = 100.0;

/// The most one drawn frame moves the first run's clock: two 30 Hz frames.
const MAX_STEP: f64 = 1.0 / 15.0;

/// The first run's clock, which runs on drawn frames rather than the wall.
/// It stands still until the window draws its first frame, and a slow frame
/// moves it at most [`MAX_STEP`]. A cold launch spends most of a second on
/// its first frames (the GPU's pipelines, fonts and glyphs), and a beat's
/// first frame can be slow the same way; on the wall's time the opening
/// and the Launch Room's growth would play out unseen behind them and jump.
struct StoryClock {
    /// Story seconds at `last`.
    story: f64,
    last: Option<Instant>,
}

impl StoryClock {
    fn frame_drawn(&mut self, at: Instant) {
        self.story = self.at(at);
        self.last = Some(at);
    }

    /// Story seconds at `at`, after the last drawn frame.
    fn at(&self, at: Instant) -> f64 {
        self.story
            + self.last.map_or(0.0, |last| {
                at.saturating_duration_since(last)
                    .as_secs_f64()
                    .min(MAX_STEP)
            })
    }
}

static STORY_CLOCK: Mutex<StoryClock> = Mutex::new(StoryClock {
    story: 0.0,
    last: None,
});

/// The window drew a frame.
pub(in crate::rune_ui) fn frame_drawn() {
    if let Ok(mut clock) = STORY_CLOCK.lock() {
        clock.frame_drawn(Instant::now());
    }
}

/// Seconds on the first run's own clock. Fixtures keep the story's own
/// time, so a scene shows one settled moment.
fn clock(ui: &Ui) -> f64 {
    if ui.is_fixture() {
        return ui.first_run.as_ref().map_or(FIXTURE_START, |s| s.now);
    }
    STORY_CLOCK
        .lock()
        .map_or(0.0, |clock| clock.at(Instant::now()))
        + 1.0
}

/// Every permission is ready, the desktop's paste permission included
/// (or passed over), so the story needs no permissions page.
fn granted(status: SetupStatus, paste: Option<PasteAccess>) -> bool {
    use crate::desktop::onboarding::PermissionState::Ready;
    status.microphone == Ready
        && status.accessibility == Ready
        && paste.is_none_or(PasteAccess::settled)
}

/// Whether words show while the shortcut is held: Scribe's live session
/// runs only when its text is shown (Labs › Live text) or pasted (Final
/// text: Live).
fn streams(ui: &Ui) -> bool {
    ui.settings.labs.live_text || ui.settings.scribe.final_text == FinalText::Live
}

/// Opens the first run over the tour, at the tour's step: at launch when
/// this run needs onboarding, and from Run setup again.
pub(in crate::rune_ui) fn open(ui: &mut Ui) {
    let Some(tour_step) = ui.tour.as_ref().map(|tour| tour.flow.step()) else {
        return;
    };
    if ui.first_run.is_some() {
        return;
    }
    let has_key = ui.api_key.is_set();
    let status = ui.platform.setup_status(has_key);
    let mic_granted = status.microphone == crate::desktop::onboarding::PermissionState::Ready;
    let granted = granted(status, ui.platform.paste);

    let mut s = Onboard::new(Flow::FirstRun);
    s.toggle = ui.settings.shortcut_mode == crate::dictation::ShortcutMode::Toggle;
    s.caps = ui.settings.dictation_hotkey.keycaps();
    s.has_key = has_key;
    s.streams = streams(ui);
    s.granted = granted;
    s.mic_granted = mic_granted;
    s.home = match ui.platform.caps.os {
        Os::Macos => super::landing::Home::MenuBar,
        Os::Windows => super::landing::Home::Tray,
        Os::Linux => super::landing::Home::Launcher,
    };
    s.live = Some(Live {
        listening: false,
        warming: false,
        key: KeyStep::default(),
        microphone: ItemStatus::Todo,
        accessibility: ItemStatus::Todo,
        release_microphone: ui.settings.release_microphone_while_idle,
        restart: false,
        accessibility_row: ui.platform.caps.permissions,
        accessibility_renamed: ui.platform.accessibility_renamed,
        reopen: false,
        relaunch_failed: false,
        paste: ui.platform.paste,
        practice: Practice::Waiting,
        granted_at: None,
        mic_granted_at: None,
        checked: None,
        fixture: ui.is_fixture(),
        _live_words: crate::dictation::live_text::demand(),
    });
    let now = clock(ui);
    s.now = now;
    match tour_step {
        TourStep::Welcome => s.begin_live(now),
        TourStep::Done => s.jump_to_ready(),
        // A run resumed without a key asks for it again, then goes on to
        // what setup still needs.
        TourStep::Setup | TourStep::TryIt if !has_key => {
            s.resumed = true;
            s.jump_to_key();
        }
        TourStep::Setup => s.resume_setup(),
        TourStep::TryIt => s.jump_to_try(),
    }
    // A key typed into Settings but never saved is not this run's.
    ui.api_key.field.clear();
    ui.first_run = Some(s);
    // The tour waits at setup, where it asks for permissions, while the
    // story plays; with everything granted it goes on to the practice.
    if tour_step == TourStep::Welcome {
        onboarding::input(ui, Input::Continue);
    }
}

/// A page of the first run, for scenes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::rune_ui) enum Page {
    /// The key step: Hex asks for an ElevenLabs API key.
    Key,
    /// The demo, finished: `"With Hex"`'s happy ending.
    Demo,
    /// The first permission the run still asks for: the microphone, else
    /// Accessibility (or the desktop's paste permission).
    Permissions,
    /// `"Your turn"`: the practice game.
    Try,
    Ready,
}

/// The page the first run shows, for the window tests (Linux only).
#[cfg(all(test, target_os = "linux"))]
pub(in crate::rune_ui) fn page(ui: &Ui) -> Option<Page> {
    Some(match ui.first_run.as_ref()?.step {
        Step::Key => Page::Key,
        Step::Parked | Step::YourTurn | Step::Hearing | Step::Heard | Step::Done => Page::Demo,
        Step::Microphone | Step::Accessibility => Page::Permissions,
        Step::Try | Step::TryHearing | Step::TryHeard => Page::Try,
        Step::Ready => Page::Ready,
    })
}

/// Scenes: the first run over the tour a fixture walked, played on its own
/// clock to `page` as a person would see it, then held there.
pub(in crate::rune_ui) fn fixture(ui: &mut Ui, page: Page) {
    ui.first_run = None;
    open(ui);
    if let Some(s) = ui.first_run.as_mut() {
        // From the start, as a new install plays it, whatever the tour's step.
        s.now = FIXTURE_START;
        s.step = Step::Key;
        s.begin_live(FIXTURE_START);
    }
    fixture_resume(ui, page);
}

/// Scenes: plays a fixture's first run on from where it is to `page`.
pub(in crate::rune_ui) fn fixture_resume(ui: &mut Ui, page: Page) {
    // One poll fills in what the tour reports.
    let mut tx = Tx::default();
    poll(ui, &mut tx);
    let Some(s) = ui.first_run.as_mut() else {
        return;
    };
    let reached = |s: &Onboard| match page {
        Page::Key => s.step == Step::Key,
        Page::Demo => s.step == Step::Heard,
        Page::Permissions => matches!(s.step, Step::Microphone | Step::Accessibility),
        Page::Try => s.step == Step::Try,
        Page::Ready => s.step == Step::Ready,
    };
    if page == Page::Ready {
        s.jump_to_ready();
    }
    for _ in 0..64 {
        if reached(s) {
            break;
        }
        let Some(next) = super::stories::first_run_next(s, s.now) else {
            break;
        };
        s.now += next;
    }
    if page == Page::Try {
        super::game::step(s, s.now);
    }
    // Everything that entered has settled; nothing more is due.
    s.now += 10.0;
    s.auto_due_at = None;
    poll(ui, &mut tx);
}

/// "Run setup again" and `hex onboarding reset`: forgets that onboarding
/// finished and opens it now. Settings, history, the API key, and
/// permissions stay as they are, so the run finds them ready and passes
/// their steps.
pub(crate) fn restart(ui: &mut Ui, tx: &mut Tx) -> Result<(), String> {
    if ui.live() {
        crate::desktop::onboarding::reset().map_err(|error| format!("{error:#}"))?;
    }
    ui.select_pane(super::super::Pane::General, tx);
    ui.first_run = None;
    ui.tour = Some(onboarding::Tour::replay(ui.api_key.is_set(), &ui.platform));
    open(ui);
    tx.settle();
    if ui.first_run.is_none() {
        return Err("Hex could not open onboarding".into());
    }
    Ok(())
}

/// Feeds the first run fresh state and moves it on; true while it shows.
pub(in crate::rune_ui) fn poll(ui: &mut Ui, tx: &mut Tx) -> bool {
    if ui.first_run.is_none() {
        return false;
    }
    let now = clock(ui);
    let view = ui.tour.as_ref().map(|tour| tour.flow.view());
    let caps = ui.settings.dictation_hotkey.keycaps();
    let toggle = ui.settings.shortcut_mode == crate::dictation::ShortcutMode::Toggle;
    let warming = ui.view.as_ref().is_some_and(|view| view.listener.warming);
    let has_key = ui.api_key.is_set();
    let streams = streams(ui);
    let (field, check, error) = (
        ui.api_key.field.clone(),
        ui.api_key.check.clone(),
        ui.api_key.error.clone(),
    );
    // The macOS checks run a few times a second, not every frame.
    let checked = ui
        .first_run
        .as_ref()
        .and_then(|s| s.live.as_ref())
        .and_then(|live| live.checked)
        // A fixture's clock stands still, so it looks every time.
        .filter(|checked| !ui.is_fixture() && now - checked.at < CHECK_EVERY);
    let status = match checked {
        Some(checked) => SetupStatus {
            transcription_model: has_key,
            ..checked.status
        },
        None => ui.platform.setup_status(has_key),
    };
    let checked = checked.unwrap_or(Checked { at: now, status });
    // Setup that is ready never moves the tour past it by itself, and the
    // tour watches the practice dictation only once it has.
    if view
        .as_ref()
        .is_some_and(|view| view.ready && view.step == TourStep::Setup)
    {
        onboarding::input(ui, Input::Continue);
    }
    let asks_permissions = ui.platform.caps.permissions;
    let login = super::login::of(&ui.platform);
    let accessibility_renamed = ui.platform.accessibility_renamed;
    let relaunch_failed = ui.platform.relaunch_failed;
    let paste = ui.platform.paste;
    let live_window = !ui.is_fixture();
    let Some(s) = ui.first_run.as_mut() else {
        return false;
    };
    if s.login.set(login, now) {
        s.sfx.on_login(now);
    }
    let Some(live) = s.live.as_mut() else {
        return false;
    };
    live.warming = warming;
    live.checked = Some(checked);
    live.key.field = field;
    live.key.check = check;
    live.key.error = error;
    // Linux and Windows ask for no permissions: both rows are ready.
    if !asks_permissions {
        live.microphone = ItemStatus::Ready;
        live.accessibility = ItemStatus::Ready;
    }

    live.paste = paste;

    // Permissions, as the tour's setup rows show them.
    if let Some(view) = &view {
        for row in &view.rows {
            match row.item {
                SetupItem::Microphone => live.microphone = row.status,
                SetupItem::Accessibility => live.accessibility = row.status,
                _ => {}
            }
        }
        live.restart = view.restart;
        if view.practice == Practice::TooShort && live.practice != Practice::TooShort {
            s.taps = s.taps.saturating_add(1);
        }
        live.accessibility_renamed = accessibility_renamed;
        live.relaunch_failed = relaunch_failed;
        live.reopen = view.reopen;
        live.practice = view.practice;
    }
    let mic_granted = status.microphone == crate::desktop::onboarding::PermissionState::Ready;
    if mic_granted {
        live.mic_granted_at.get_or_insert(now);
    } else {
        live.mic_granted_at = None;
    }

    let granted = granted(status, paste);
    if granted {
        live.granted_at.get_or_insert(now);
    } else {
        live.granted_at = None;
    }
    live.listening = live.practice == Practice::Listening;

    s.granted = granted;
    s.mic_granted = mic_granted;
    s.caps = caps;
    s.toggle = toggle;
    s.has_key = has_key;
    s.streams = streams;

    // Hex's state arrives between frames, so the first run keeps polling;
    // a fixture shows one moment and stays still.
    advance_to(s, now, tx);
    if live_window {
        tx.wake_after(FRAME);
    }
    true
}

/// The first run over the whole window, while it shows.
pub(in crate::rune_ui) fn overlay(ui: &Ui, p: Paint, reduced: bool) -> Option<El> {
    let s = ui.first_run.as_ref()?;
    s.reduced.set(reduced);
    // Scenes are the release first run, without the replay strip.
    let strip = (!ui.is_fixture())
        .then(|| super::stories::strip(p, s))
        .flatten();
    Some(
        super::flows::view(p, s)
            .children(strip)
            .key("first-run")
            .role("dialog")
            .label(tr("Setting up Hex"))
            .absolute(0.0, 0.0)
            .on_press(|_: &mut Ui| {}),
    )
}

/// A story handler run on the window's first run.
pub(super) fn on_ui(f: fn(&mut Onboard)) -> impl Fn(&mut Ui) + 'static {
    move |ui: &mut Ui| {
        if let Some(s) = ui.first_run.as_mut() {
            f(s);
        }
    }
}

pub(super) fn press_continue(ui: &mut Ui) {
    if let Some(s) = ui.first_run.as_mut() {
        s.move_on();
    }
}

/// Skip on the demo or the practice: passes over that part to the next
/// step setup still needs. It never records completion; only Hex is ready
/// does.
pub(in crate::rune_ui) fn skip(ui: &mut Ui) {
    if let Some(s) = ui.first_run.as_mut() {
        s.skip_optional();
    }
}

/// "Start using Hex": closes the first run and the tour under it. Every
/// permission is on by now; a run that skipped the key still counts as
/// done, and Settings says dictation waits for one.
pub(super) fn finish(ui: &mut Ui) {
    use crate::desktop::onboarding::PermissionState::Ready;
    let status = ui.platform.setup_status(ui.api_key.is_set());
    if ui.live()
        && status.microphone == Ready
        && status.accessibility == Ready
        && !crate::desktop::onboarding::completion_recorded()
        && let Err(error) = crate::desktop::onboarding::record_completion()
    {
        tracing::warn!(%error, "could not record onboarding completion");
    }
    ui.api_key.field.clear();
    ui.first_run = None;
    ui.tour = None;
}

/// The key step's field.
pub(super) fn key_field(ui: &mut Ui) -> &mut TextEdit {
    &mut ui.api_key.field
}

/// Continue on the key step: asks ElevenLabs whether the typed key works,
/// off the UI thread, and saves it once accepted. The story moves on when
/// the key is set.
pub(in crate::rune_ui) fn continue_key(ui: &mut Ui) {
    if ui.api_key.has_typed() {
        api_key::check_and_save(ui);
    }
}

/// Skip for now on the key step: on without a key. Dictation waits for one,
/// so the practice is passed over.
pub(in crate::rune_ui) fn skip_key(ui: &mut Ui) {
    if ui.api_key.checking() {
        return;
    }
    ui.api_key.field.clear();
    ui.api_key.check = Check::Idle;
    ui.api_key.error = None;
    onboarding::input(ui, Input::SkipKey);
    if let Some(s) = ui.first_run.as_mut() {
        if let Some(live) = s.live.as_mut() {
            live.key.skipped = true;
        }
        s.move_on();
    }
}

/// "Reopen Hex" once the listener failed and Hex won't relaunch itself.
pub(super) fn reopen(ui: &mut Ui) {
    onboarding::input(ui, Input::Reopen);
}

/// Allow on a permission row.
pub(super) fn allow(item: SetupItem) -> impl Fn(&mut Ui) + 'static {
    move |ui: &mut Ui| onboarding::input(ui, Input::Grant(item))
}

/// Allow on the paste permission: the desktop's dialog opens.
pub(super) fn allow_paste(ui: &mut Ui) {
    paste_access::ask(ui);
}

/// Continue after the desktop's paste permission was refused: the desktop
/// asks again at the first paste.
pub(super) fn paste_later(ui: &mut Ui) {
    paste_access::later(ui);
    if let Some(s) = ui.first_run.as_mut() {
        s.move_on();
    }
}

/// "Restart Hex" once macOS applies a grant only after a restart.
pub(super) fn restart_hex(ui: &mut Ui) {
    onboarding::input(ui, Input::Restart);
}

pub(super) fn practice(ui: &mut Ui) -> &mut TextEdit {
    match ui.first_run.as_mut() {
        Some(s) => &mut s.practice,
        // The field shows only with the first run.
        None => Box::leak(Box::default()),
    }
}

pub(super) fn practice_changed(ui: &mut Ui, tx: &mut Tx) {
    if let Some(s) = ui.first_run.as_mut() {
        s.on_practice_change(tx);
    }
}

/// What the practice says in place of the shortcut invitation while the
/// listener brings up its microphone and shortcuts.
pub(super) fn warming_copy() -> &'static str {
    tr("Getting Hex ready to listen…")
}

/// What the practice dictation's last try needs said.
pub(super) fn hint_copy(live: &Live) -> Option<&'static str> {
    Some(match live.practice {
        Practice::TooShort => "Keep holding the shortcut while you talk, then let go.",
        Practice::NothingHeard => tr("Hex didn't hear any words. Try again a little louder."),
        Practice::Failed => tr("That didn't work. Try again."),
        Practice::Transcribing => tr("Typing what you said…"),
        _ => return None,
    })
}

/// The practice's `Use tap to start`: the shortcut starts and stops
/// dictation from now on, as General's setting would.
pub(super) fn use_toggle(ui: &mut Ui) {
    ui.update(|settings| settings.shortcut_mode = crate::dictation::ShortcutMode::Toggle);
    if let Some(s) = ui.first_run.as_mut() {
        s.toggle = true;
        s.practice.focused = true;
    }
}

/// [`hint_copy`] under the practice field.
pub(super) fn hint(p: Paint, live: &Live) -> Option<El> {
    let copy = hint_copy(live)?;
    Some(
        text(copy)
            .key("practice-hint")
            .font(p.k.ty.small)
            .color(p.muted())
            .transition(Transition::opacity()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn the_story_waits_for_the_first_frame_and_slow_frames_dont_skip_it() {
        let open = Instant::now();
        let mut clock = StoryClock {
            story: 0.0,
            last: None,
        };
        // A cold first frame takes most of a second: the story stands still.
        assert_eq!(clock.at(open + Duration::from_millis(700)), 0.0);
        let first = open + Duration::from_millis(700);
        clock.frame_drawn(first);
        assert_eq!(clock.story, 0.0);
        // Frames at 60 Hz move it on the wall's time.
        let frame = Duration::from_micros(16_667);
        clock.frame_drawn(first + frame);
        assert!((clock.story - frame.as_secs_f64()).abs() < 1e-9);
        // A 400 ms frame moves it one step, and time between frames
        // never runs ahead of the next frame.
        let slow = first + frame + Duration::from_millis(400);
        let between = clock.at(slow);
        clock.frame_drawn(slow);
        assert!((clock.story - (frame.as_secs_f64() + MAX_STEP)).abs() < 1e-9);
        assert!(between <= clock.story);
    }
}
