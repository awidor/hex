//! The runtime thread: it applies the window's and the menu bar's requests,
//! saves the transcription language, follows the keyboard layout and app
//! switchers, and publishes what the window and the menu bar show.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::app_settings::AppSettings;
use crate::desktop::activity::{DesktopActivity, last_completed_at};
use crate::desktop::events::EventReader;
use crate::desktop::host::{
    DesktopListenerSnapshot, DesktopShortcuts, DesktopTranscriptionSnapshot,
};
use crate::transcription::models::TranscriptionSelection;

use super::{Published, RUNTIME_TICK, Request, Shared, UPDATER, lock, update_status};

/// The listener's status while no ElevenLabs API key is set.
const API_KEY_REQUIRED: &str = "API key required";

pub(super) fn run_runtime(
    settings: AppSettings,
    initial_input_source: Option<String>,
    requests: Receiver<Request>,
    shared: &Shared,
    shutdown: &AtomicBool,
    recognition_start: Option<SyncSender<()>>,
    listener_failure: Arc<Mutex<Option<String>>>,
) {
    let mut event_reader = EventReader::open(shared.event_path().to_path_buf());
    let mut activity = DesktopActivity::default();
    activity.refresh(&mut event_reader);
    let last_completed_at = last_completed_at(event_reader.events().iter());
    let mut runtime = Runtime {
        keyboard_switcher: crate::transcription::keyboard_model_switching::Switcher::new(
            settings.keyboard_model_switching.clone(),
            initial_input_source,
        ),
        app_switcher: crate::transcription::app_model_switching::Switcher::new(
            settings.labs.app_model_switching,
            settings
                .labs
                .app_model_switching
                .then(|| crate::transcription::app_model_switching::startup_choice(&settings))
                .flatten(),
        ),
        settings,
        transcription_error: None,
        recognition_start,
        listener_failure,
        event_reader,
        activity,
        last_completed_at,
        started_at: shared.started_at,
    };
    loop {
        let mut next = match requests.recv_timeout(RUNTIME_TICK) {
            Ok(request) => Some(request),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        while let Some(request) = next {
            runtime.handle(request);
            next = requests.try_recv().ok();
        }
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        runtime.tick();
        runtime.refresh_activity();
        let published = Arc::new(runtime.publish());
        let changed = {
            let mut current = lock(&shared.published);
            let changed = current.as_deref() != Some(&published);
            if changed {
                *current = Some(published);
            }
            changed
        };
        if changed {
            for wake in lock(&shared.wakers).iter() {
                wake();
            }
        }
    }
}

/// Saves `selection` on top of the current `settings.json`, so a change made
/// outside the runtime survives, and returns the settings as saved. The
/// listener's dictation worker follows the saved selection
/// (`recognition::listen`).
#[cfg(not(test))]
fn save_selection(
    _current: &AppSettings,
    selection: TranscriptionSelection,
) -> Result<AppSettings, String> {
    let mut settings = AppSettings::load().map_err(|error| format!("{error:#}"))?;
    if settings.transcription != selection {
        settings
            .save_transcription(selection)
            .map_err(|error| format!("{error:#}"))?;
    }
    Ok(settings)
}

/// Unit tests never read or write this computer's settings.
#[cfg(test)]
fn save_selection(
    current: &AppSettings,
    selection: TranscriptionSelection,
) -> Result<AppSettings, String> {
    let mut settings = current.clone();
    settings.transcription = selection;
    Ok(settings)
}

pub(super) struct Runtime {
    pub(super) settings: AppSettings,
    /// Why the last chosen selection could not be saved, until one is.
    pub(super) transcription_error: Option<String>,
    pub(super) keyboard_switcher: crate::transcription::keyboard_model_switching::Switcher,
    /// Labs › Switch model with app: the foreground application's mode choice.
    pub(super) app_switcher: crate::transcription::app_model_switching::Switcher,
    /// Starts the listener once setup is ready; `None` once started.
    pub(super) recognition_start: Option<SyncSender<()>>,
    /// Why the listener stopped, once it has.
    pub(super) listener_failure: Arc<Mutex<Option<String>>>,
    /// The listener's event log, read on this thread so the window can show a
    /// failed dictation without another polling thread.
    pub(super) event_reader: EventReader,
    pub(super) activity: DesktopActivity,
    /// When the current session's newest dictation completed.
    pub(super) last_completed_at: Option<u64>,
    /// When this runtime started, in the event log's wall-clock ms. Failures
    /// older than this belong to the previous session.
    pub(super) started_at: u64,
}

impl Runtime {
    pub(super) fn handle(&mut self, request: Request) {
        match request {
            Request::ChooseModel { model, language } => {
                self.manual_choice();
                self.apply_transcription(TranscriptionSelection { model, language });
            }
            Request::InputSourceChanged(source) => {
                self.keyboard_switcher
                    .source_changed(source, Instant::now());
            }
            Request::ReloadSettings => self.reload_settings(),
            Request::Desktop(_)
            | Request::CaptureShortcutFor(_)
            | Request::CancelShortcut
            | Request::SetShortcut { .. } => {}
        }
    }

    fn reload_settings(&mut self) {
        match AppSettings::load() {
            Ok(settings) => {
                self.keyboard_switcher
                    .configure(settings.keyboard_model_switching.clone(), Instant::now());
                self.app_switcher
                    .configure(settings.labs.app_model_switching);
                self.settings = settings;
            }
            Err(error) => tracing::warn!(%error, "could not reload settings"),
        }
    }

    /// A choice made by hand cancels a keyboard or app switch still waiting
    /// for its debounce, so it cannot later undo the choice.
    fn manual_choice(&mut self) {
        self.keyboard_switcher.manual_choice();
        self.app_switcher.manual_choice();
    }

    /// Saves `selection`, which the listener uses from its next dictation.
    /// There is nothing to download or load.
    fn apply_transcription(&mut self, selection: TranscriptionSelection) {
        let saved = crate::transcription::models::validate(&selection)
            .map_err(|error| format!("{error:#}"))
            .and_then(|_| save_selection(&self.settings, selection));
        match saved {
            Ok(settings) => {
                self.settings = settings;
                self.transcription_error = None;
            }
            Err(error) => {
                tracing::warn!(%error, "could not select the transcription language");
                self.transcription_error = Some(error);
            }
        }
    }

    /// Follows the switchers and starts a listener that waited for setup.
    fn tick(&mut self) {
        self.tick_at(Instant::now());
    }

    /// Follows the keyboard layout switcher's due choice, if any.
    fn switch_with_keyboard(&mut self, now: Instant) {
        if let Some(choice) = self.keyboard_switcher.take_due(now) {
            let selection = TranscriptionSelection {
                model: choice.model,
                language: choice.language,
            };
            if selection != self.settings.transcription {
                self.apply_transcription(selection);
            }
        }
    }

    /// Feeds one foreground read to the app switcher. Hex's own window and a
    /// moment with no application in front carry no mode, so they leave the
    /// last match and any switch it scheduled alone. A dictation that records
    /// or transcribes holds a due switch until it finishes.
    pub(super) fn follow_app(
        &mut self,
        observation: crate::transcription::app_model_switching::Observation<'_>,
        now: Instant,
    ) {
        if !self.settings.labs.app_model_switching {
            return;
        }
        self.app_switcher.observe(
            observation,
            |application| {
                crate::transcription::app_model_switching::mode_choice(&self.settings, application)
            },
            now,
        );
        if crate::transcription::engine::dictation_busy() {
            return;
        }
        let Some(choice) = self
            .app_switcher
            .take_due(now, &self.settings.transcription)
        else {
            return;
        };
        self.apply_transcription(TranscriptionSelection {
            model: choice.model,
            language: choice.language,
        });
    }

    /// Reads the listener's newest events, bounded by `EventReader`'s
    /// retention, so the window's warning follows failed dictations.
    fn refresh_activity(&mut self) {
        self.activity.refresh(&mut self.event_reader);
        self.last_completed_at = last_completed_at(self.event_reader.events().iter());
    }

    pub(super) fn tick_at(&mut self, now: Instant) {
        self.switch_with_keyboard(now);
        // Read the frontmost application only while the lab is on. Hex's own
        // window names no mode and is ignored.
        let application = self
            .settings
            .labs
            .app_model_switching
            .then(crate::dictation::context::frontmost_other_application)
            .flatten();
        self.follow_app(
            crate::transcription::app_model_switching::Observation::frontmost(
                application.as_deref(),
            ),
            now,
        );
        // Setup is the permissions and an ElevenLabs API key, which every
        // transcription needs; the listener starts by itself once both are
        // there.
        if self.recognition_start.is_some()
            && crate::desktop::onboarding::status(&self.settings.transcription).ready()
            && let Some(start) = self.recognition_start.take()
        {
            let _ = start.try_send(());
        }
    }

    fn publish(&self) -> Published {
        let failure = lock(&self.listener_failure).clone();
        let running = self.recognition_start.is_none() && failure.is_none();
        Published {
            listener: DesktopListenerSnapshot {
                running,
                warming: false,
                status: if failure.is_some() {
                    "Stopped"
                } else if running {
                    "Enabled"
                } else if !crate::transcription::models::is_ready() {
                    API_KEY_REQUIRED
                } else {
                    "Waiting for setup"
                }
                .into(),
                warning: failure,
                notice: None,
                last_failure: self
                    .activity
                    .visible_failure_since(self.started_at, self.last_completed_at),
                shortcuts: DesktopShortcuts::Registered,
                shortcuts_configurable: false,
            },
            transcription: DesktopTranscriptionSnapshot {
                error: self.transcription_error.clone(),
                selection: self.settings.transcription.clone(),
            },
            update_status: update_status(),
            update_notes: UPDATER.get().and_then(crate::updater::Updater::notes),
            settings: self.settings.clone(),
        }
    }
}
