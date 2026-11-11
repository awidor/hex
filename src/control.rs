//! `hex` control: the typed command/reply protocol an agent uses to drive the
//! one running Hex, the settings registry behind `hex settings`, and the CLI
//! client. Transports (the macOS local API's `POST /control`) only move
//! [`ControlRequest`]s and [`ControlResponse`]s; the app's host carries out
//! each command, and every rule shared between the window and the CLI lives
//! here or in the app's own setters.

pub mod cli;
#[cfg(all(debug_assertions, target_os = "macos"))]
pub(crate) mod dashboard;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) mod developer_control;
#[cfg(not(target_os = "macos"))]
pub(crate) mod host;
pub mod settings;
pub(crate) mod transcript_cli;

use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::dictation::vocabulary::{self, VocabularyEntry};

/// The wire version of [`ControlRequest`] and [`ControlResponse`]. A change
/// that breaks either shape increments it.
pub const CONTROL_VERSION: u32 = 5;

/// What a capture command does. Commands are idempotent: starting while
/// recording or stopping while idle changes nothing and succeeds.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureAction {
    /// Start recording, locked as after a double tap: the shortcut or
    /// `stop` finishes it.
    Start,
    /// Finish the recording and transcribe it.
    Stop,
    /// Start when idle, otherwise stop.
    Toggle,
    /// Discard the recording: nothing is transcribed, pasted, or saved.
    /// Dictations already transcribing finish.
    Cancel,
}

/// What `hex capture` does (the `capture` experiment).
/// Idempotent like [`CaptureAction`].
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ScrawlCaptureAction {
    Start,
    /// End the capture and write its bundle in the background.
    Stop,
    /// End the capture and discard its bundle.
    Cancel,
    /// Stop the capture that records, else start one: for keybindings that
    /// fire on the press only (GNOME's custom shortcuts, #781).
    Toggle,
    Status,
    /// Export the last 15 s from the rolling clip buffer into a capture bundle
    /// (`labs.annotate_replay`, macOS 27+).
    Replay,
}

/// The window's panes, by the names `hex window show` takes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Pane {
    Settings,
    /// Settings at Transcription: the language and the ElevenLabs API key.
    Models,
    #[value(alias = "replacements")]
    Modes,
    History,
    Meetings,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
pub enum ControlCommand {
    Status,
    Quit {
        #[serde(default)]
        force: bool,
    },
    Restart {
        #[serde(default)]
        force: bool,
    },
    Dictation {
        action: CaptureAction,
        /// `stop` only: answer with the transcript instead of pasting it.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        print: bool,
        /// `start` and `toggle` only: the dictation this starts pastes the
        /// transcript as heard, skipping the Output pane's steps.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        raw: bool,
    },
    PasteLast,
    /// Labs › Transform typed text (Linux): runs the focused field's text
    /// through its Output steps and replaces it, as its shortcut does.
    TypedText,
    /// Labs › Screenshots (macOS): the display under the pointer, or with
    /// `pick` the picker its shortcut opens.
    Screenshot {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        pick: bool,
    },
    MeetingRecord {
        #[serde(default)]
        title: Option<String>,
    },
    MeetingStop,
    MeetingCopyLast,
    SettingsGet {
        #[serde(default)]
        key: Option<String>,
    },
    SettingsSet {
        key: String,
        value: String,
    },
    /// Several settings at once, all or none (`hex settings apply`).
    SettingsApply {
        values: serde_json::Map<String, serde_json::Value>,
    },
    ModelList,
    /// Sets the language Scribe is told to expect. `model` is `scribe_v3`.
    ModelUse {
        model: String,
        #[serde(default)]
        language: Option<String>,
    },
    WindowShow {
        #[serde(default)]
        pane: Option<Pane>,
    },
    WindowHide,
    /// Replay the first-run onboarding in the window (dev, test, and local
    /// builds); settings, history, the API key, and permissions stay.
    OnboardingReset,
    /// Open the window on a playground story (Labs).
    WindowStory {
        story: String,
        #[serde(default)]
        variant: Option<String>,
        #[serde(default)]
        play: bool,
    },
    HistoryClear,
    CheckForUpdates,
    /// Report Launch at Login, or turn it on or off.
    LaunchAtLogin {
        #[serde(default)]
        enabled: Option<bool>,
    },
    /// The shared vocabulary.
    VocabularyList,
    VocabularyAdd {
        term: String,
        #[serde(default)]
        sounds_like: Vec<String>,
    },
    VocabularyRemove {
        term: String,
    },
    /// A Scrawl capture (the `capture` experiment).
    ScrawlCapture {
        action: ScrawlCaptureAction,
    },
    /// The newest captures in the captures folder, newest first.
    ScrawlCaptureList {
        #[serde(default)]
        limit: Option<usize>,
    },
    /// One capture, by bundle name, path, or `latest`.
    ScrawlCaptureShow {
        capture: String,
    },
    /// Sends a capture to an OpenCode session: `to`, or where its route or
    /// its last send says.
    ScrawlCaptureSend {
        capture: String,
        #[serde(default)]
        to: Option<String>,
        /// Also save a rule sending the capture's site or app there (the
        /// newest routed capture only).
        #[serde(default)]
        always: bool,
    },
    /// Takes the newest send back while it can be undone.
    ScrawlCaptureUndo,
    /// What routing sees for a capture and what it decides. `ask_jev` asks
    /// Jev again (one call).
    ScrawlCaptureRoute {
        capture: String,
        #[serde(default)]
        ask_jev: bool,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ControlRequest {
    pub version: u32,
    #[serde(flatten)]
    pub command: ControlCommand,
}

impl ControlRequest {
    pub fn new(command: ControlCommand) -> Self {
        Self {
            version: CONTROL_VERSION,
            command,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ControlReply {
    /// The command ran. `changed` is false when it was already so.
    Done {
        message: String,
        changed: bool,
    },
    Status(Box<StatusReport>),
    Settings {
        settings: Vec<settings::SettingValue>,
    },
    Models {
        models: Vec<ModelInfo>,
    },
    /// `hex model status`: the model as the `hex` that answered would use it.
    Model(Box<ModelInfo>),
    /// `hex key status`: the API key in use, by its source and last four
    /// characters. No reply ever carries the key.
    ApiKey(ApiKeyStatus),
    Vocabulary {
        entries: Vec<VocabularyEntry>,
    },
    ScrawlCapture(Box<ScrawlCaptureReport>),
    ScrawlCaptures {
        captures: Vec<ScrawlCaptureSummary>,
    },
    ScrawlCaptureDetail(Box<ScrawlCaptureDetail>),
    ScrawlCaptureRoute {
        explanation: String,
    },
    /// `hex dictation stop --print` or `hex transcribe`: the transcript.
    Transcript {
        text: String,
    },
    Error {
        code: String,
        message: String,
    },
}

impl ControlReply {
    pub fn done(message: impl Into<String>) -> Self {
        Self::Done {
            message: message.into(),
            changed: true,
        }
    }

    pub fn unchanged(message: impl Into<String>) -> Self {
        Self::Done {
            message: message.into(),
            changed: false,
        }
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Error {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl From<ControlError> for ControlReply {
    fn from(error: ControlError) -> Self {
        Self::error(error.code, error.message)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ControlResponse {
    pub version: u32,
    #[serde(flatten)]
    pub reply: ControlReply,
}

impl From<ControlReply> for ControlResponse {
    fn from(reply: ControlReply) -> Self {
        Self {
            version: CONTROL_VERSION,
            reply,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScrawlCaptureState {
    #[default]
    Idle,
    Recording,
}

/// A `scrawl-capture` command's answer.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ScrawlCaptureReport {
    /// False when the command found nothing to do.
    pub changed: bool,
    pub state: ScrawlCaptureState,
    /// The recording capture's bundle, or the one `stop`/`cancel` just ended.
    pub bundle: Option<std::path::PathBuf>,
    pub elapsed_ms: Option<u64>,
    /// Stopped captures still transcribing or writing.
    #[serde(default)]
    pub finishing: Vec<std::path::PathBuf>,
    /// The most recent capture that finished, failed, or was cancelled.
    pub last: Option<ScrawlCaptureOutcome>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScrawlCaptureOutcome {
    pub bundle: std::path::PathBuf,
    /// done, failed, or cancelled.
    pub stage: String,
    pub words: Option<usize>,
    /// Spans of a written capture's audio that went missing.
    #[serde(default)]
    pub gaps: Option<usize>,
    /// Why a written capture has no transcript: `no_audio`, or the engine's
    /// reason.
    #[serde(default)]
    pub missing_transcript: Option<String>,
}

/// A capture on disk, as `hex capture list` shows it.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScrawlCaptureSummary {
    /// The bundle's folder name, which `show`, `send`, and `route` take.
    pub id: String,
    pub bundle: std::path::PathBuf,
    pub created_at_ms: u64,
    pub duration_ms: u64,
    /// `screen`, `window`, `area`, or `iphone`.
    pub mode: String,
    pub app: Option<String>,
    pub words: usize,
    /// The session it was sent to, if it was.
    pub sent_to: Option<ScrawlCaptureSession>,
    /// OpenCode didn't take its send yet; Hex retries it.
    #[serde(default)]
    pub queued: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScrawlCaptureSession {
    pub session: String,
    pub title: String,
}

/// `hex capture show`: a capture's summary with its words and files.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScrawlCaptureDetail {
    #[serde(flatten)]
    pub summary: ScrawlCaptureSummary,
    pub window_title: Option<String>,
    pub transcript: String,
    pub note: Option<String>,
    /// Why there is no transcript although there was audio.
    pub missing_transcript: Option<String>,
    pub marks: usize,
    pub keyframes: usize,
    pub video: Option<std::path::PathBuf>,
    pub audio: Option<std::path::PathBuf>,
    pub contact_sheet: Option<std::path::PathBuf>,
}

/// A refused or failed command, with a stable code (see `docs/cli.md`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlError {
    pub code: &'static str,
    pub message: String,
}

impl ControlError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// How long `hex dictation stop --print` waits for its transcript.
pub const PRINT_WAIT: Duration = Duration::from_secs(60);
/// How long the app may take to carry out any other control command.
const CONTROL_DEADLINE: Duration = Duration::from_secs(10);

impl ControlCommand {
    /// How long a transport waits for the app's answer to this command.
    pub fn deadline(&self) -> Duration {
        match self {
            Self::Dictation { print: true, .. } => PRINT_WAIT + Duration::from_secs(5),
            _ => CONTROL_DEADLINE,
        }
    }
}

/// What a printing stop gets back from the listener once its dictation
/// finishes: the transcript, processed as a paste would be, or why there is
/// none. A printed transcript is never pasted.
#[derive(Debug, Eq, PartialEq)]
pub enum PrintedTranscript {
    Text(String),
    NoSpeech,
    /// The Linux and Windows listeners stop by themselves without a recording.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    NotRecording,
    Failed(String),
}

/// The one-shot channel a printing stop rides through the listener's queue.
#[derive(Clone, Debug)]
pub struct PrintReply(SyncSender<PrintedTranscript>);

impl PrintReply {
    pub fn channel() -> (Self, Receiver<PrintedTranscript>) {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        (Self(sender), receiver)
    }

    /// Answers the waiting command; one that gave up is not told.
    pub fn send(self, transcript: PrintedTranscript) {
        let _ = self.0.send(transcript);
    }
}

/// Shortcut events compare by kind; where a reply goes is not part of it.
impl PartialEq for PrintReply {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for PrintReply {}

/// Waits up to `wait` for a printing stop's transcript. A dropped reply
/// means the dictation was cancelled or failed before it had text.
pub fn await_printed(receiver: &Receiver<PrintedTranscript>, wait: Duration) -> ControlReply {
    match receiver.recv_timeout(wait) {
        Ok(PrintedTranscript::Text(text)) => ControlReply::Transcript { text },
        Ok(PrintedTranscript::NoSpeech) => {
            ControlReply::error("no-speech", "no speech was recorded; nothing to print")
        }
        Ok(PrintedTranscript::NotRecording) => ControlReply::error(
            "not-recording",
            "dictation isn't recording; nothing to print",
        ),
        Ok(PrintedTranscript::Failed(error)) => {
            ControlReply::error("failed", format!("couldn't transcribe it: {error}"))
        }
        Err(RecvTimeoutError::Timeout) => ControlReply::error(
            "failed",
            format!(
                "no transcript within {} s; nothing was printed or pasted",
                wait.as_secs()
            ),
        ),
        Err(RecvTimeoutError::Disconnected) => ControlReply::error(
            "failed",
            "the dictation was cancelled or failed; nothing to print",
        ),
    }
}

/// The action a request asks for; `print` applies only to `stop`.
pub fn check_flags(
    action: CaptureAction,
    print: bool,
    raw: bool,
) -> Result<CaptureAction, ControlError> {
    if print && action != CaptureAction::Stop {
        return Err(ControlError::new(
            "invalid-value",
            "--print works only with `hex dictation stop`",
        ));
    }
    if raw && !matches!(action, CaptureAction::Start | CaptureAction::Toggle) {
        return Err(ControlError::new(
            "invalid-value",
            "--raw works only with `hex dictation start` and `hex dictation toggle`",
        ));
    }
    Ok(action)
}

/// One request from a transport to the app's control host.
pub struct ControlCall {
    pub request: ControlRequest,
    pub reply: SyncSender<ControlResponse>,
}

/// Rejects a request from a `hex` built for another protocol version.
pub fn check_version(request: &ControlRequest) -> Result<(), ControlError> {
    if request.version == CONTROL_VERSION {
        return Ok(());
    }
    Err(version_mismatch(request.version.into()))
}

pub fn version_mismatch(version: u64) -> ControlError {
    ControlError::new(
        "version-mismatch",
        format!(
            "this hex speaks control version {version}, but the running Hex speaks \
             {CONTROL_VERSION}; use the hex inside the running app or restart Hex"
        ),
    )
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct StatusReport {
    pub app_version: String,
    pub pid: u32,
    /// The process's physical memory footprint, as Activity Monitor shows it.
    pub memory_footprint_bytes: Option<u64>,
    pub listener: ListenerReport,
    pub capture: CaptureReport,
    pub meeting: MeetingReport,
    pub model: ModelReport,
    pub features: FeatureReport,
    pub permissions: PermissionReport,
    pub window: WindowReport,
    /// current, checking, available, or unavailable.
    pub update: String,
    /// The Labs features turned on (`crate::labs`), by `hex labs` name.
    #[serde(default)]
    pub labs: Vec<String>,
    /// What this OS's Hex can do, by capability name (`rune_ui::platform`).
    /// A command for one it can't do answers `unsupported`.
    #[serde(default)]
    pub capabilities: std::collections::BTreeMap<String, bool>,
}

/// `StatusReport::update` for an updater status.
pub fn update_label(status: &crate::desktop::host::DesktopUpdateStatus) -> &'static str {
    use crate::desktop::host::DesktopUpdateStatus;
    match status {
        DesktopUpdateStatus::Checking | DesktopUpdateStatus::Downloading => "checking",
        DesktopUpdateStatus::Available
        | DesktopUpdateStatus::ReadyToRestart
        | DesktopUpdateStatus::SwitchAvailable { .. }
        | DesktopUpdateStatus::PackageUpdate { .. } => "available",
        DesktopUpdateStatus::Unavailable
        | DesktopUpdateStatus::Managed
        | DesktopUpdateStatus::Failed => "unavailable",
        DesktopUpdateStatus::Current
        | DesktopUpdateStatus::SourceBuild
        | DesktopUpdateStatus::PackageManager => "current",
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ListenerReport {
    pub running: bool,
    pub status: String,
    /// What a Wayland shortcuts portal granted, in the desktop's own keys,
    /// including keys changed only in its settings (#723); empty elsewhere.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shortcuts: Vec<GrantedShortcutReport>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct GrantedShortcutReport {
    pub name: String,
    /// `None` when the desktop refused it or names no key.
    pub trigger: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureState {
    #[default]
    Idle,
    Recording,
    Transcribing,
    /// The listener did not answer in time.
    Unknown,
}

/// What is recording.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureKind {
    Dictation,
    /// A host-audio dictation through the local API (`POST /dictations`).
    Service,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct CaptureReport {
    pub state: CaptureState,
    pub recording: Option<CaptureKind>,
    /// Accepted dictations still transcribing, processing, or pasting.
    pub pending_jobs: usize,
}

/// The listener's capture state, as it answers a status query.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureSnapshot {
    pub recording: Option<CaptureKind>,
    pub pending_jobs: usize,
    /// No microphone is open: none was found, or the stream is reopening.
    pub microphone_unavailable: bool,
}

impl From<CaptureSnapshot> for CaptureReport {
    fn from(snapshot: CaptureSnapshot) -> Self {
        Self {
            state: if snapshot.recording.is_some() {
                CaptureState::Recording
            } else if snapshot.pending_jobs > 0 {
                CaptureState::Transcribing
            } else {
                CaptureState::Idle
            },
            recording: snapshot.recording,
            pending_jobs: snapshot.pending_jobs,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct MeetingReport {
    /// idle, starting, recording, or transcribing.
    pub state: String,
    pub title: Option<String>,
    pub elapsed_seconds: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ModelReport {
    pub id: String,
    pub name: String,
    /// The language Scribe is told to expect: an ISO 639 code, or `auto`.
    pub language: String,
    /// The ElevenLabs API key the running Hex sends audio with; `None`
    /// when none is set, and nothing can be transcribed.
    pub api_key: Option<ApiKeyReport>,
    /// Why the language could not be applied, if it could not.
    pub error: Option<String>,
}

/// Where an API key comes from.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApiKeySource {
    /// `ELEVENLABS_API_KEY`, which overrides a saved key.
    Environment,
    /// The owner-only key file `hex key set` and Settings write.
    Saved,
}

/// An API key as every reply shows it: where it comes from and its last
/// four characters, never the key.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApiKeyReport {
    pub source: ApiKeySource,
    /// `••••abcd`.
    pub redacted: String,
}

impl From<&crate::transcription::scribe::key::ApiKey> for ApiKeyReport {
    fn from(key: &crate::transcription::scribe::key::ApiKey) -> Self {
        use crate::transcription::scribe::key::{Source, redacted};
        Self {
            source: match key.source {
                Source::Environment => ApiKeySource::Environment,
                Source::File => ApiKeySource::Saved,
            },
            redacted: redacted(&key.secret),
        }
    }
}

/// The API key this process would send audio with, for a report.
pub fn api_key_report() -> Option<ApiKeyReport> {
    crate::transcription::scribe::key::current()
        .as_ref()
        .map(ApiKeyReport::from)
}

/// `hex key status`.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApiKeyStatus {
    /// The key in use; `None` when none is set.
    pub api_key: Option<ApiKeyReport>,
    /// The owner-only file `hex key set` saves to, whether or not it exists.
    pub key_file: Option<std::path::PathBuf>,
}

/// `StatusReport::model` for the running Hex: its selection, the API key it
/// sends audio with, and why the selection could not be applied, if so.
pub fn model_report(
    selection: &crate::transcription::models::TranscriptionSelection,
    error: Option<String>,
) -> ModelReport {
    let model = crate::transcription::models::definition(selection.model);
    ModelReport {
        id: model.id.as_str().into(),
        name: model.name.into(),
        language: model.language_for(&selection.language).into(),
        api_key: api_key_report(),
        error,
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct FeatureReport {
    pub meetings: bool,
    pub release_microphone_while_idle: bool,
    pub history_retention: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct PermissionReport {
    /// Every permission granted and an ElevenLabs API key set.
    pub ready: bool,
    /// ready, needs-request, needs-settings, or needs-restart.
    pub microphone: String,
    pub accessibility: String,
    /// Whether transcription can be asked for: an ElevenLabs API key is set.
    pub transcription_model: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct WindowReport {
    pub open: bool,
    pub pane: Option<String>,
}

/// The model as `hex model list` and `hex model status` show it. There is
/// one: every transcript comes from ElevenLabs Scribe, over HTTPS.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelInfo {
    /// `scribe_v3`, the id `hex model use` and the local API take.
    pub id: String,
    pub name: String,
    /// The realtime model its live sessions use.
    pub live: String,
    pub selected: bool,
    /// The language Scribe is told to expect: an ISO 639 code, or `auto`.
    pub language: String,
    /// The API key audio is sent with, where this answer was made; `None`
    /// when none is set.
    pub api_key: Option<ApiKeyReport>,
    /// Where audio is sent.
    pub api_url: String,
}

/// Every model `hex model` can name: Scribe, with the language `selection`
/// asks it for and the API key this process has.
pub fn model_infos(
    selection: &crate::transcription::models::TranscriptionSelection,
) -> Vec<ModelInfo> {
    let api_key = api_key_report();
    let api_url = crate::transcription::scribe::base_url();
    crate::transcription::models::all_models()
        .into_iter()
        .map(|model| ModelInfo {
            id: model.id.as_str().into(),
            name: model.name.into(),
            live: model.live.into(),
            selected: model.id == selection.model,
            language: model.language_for(&selection.language).into(),
            api_key: api_key.clone(),
            api_url: api_url.clone(),
        })
        .collect()
}

/// The selection `hex model use` asks for: `scribe_v3`, the only model, and
/// a language it takes (the saved one when none is given).
pub fn model_selection(
    settings: &crate::app_settings::AppSettings,
    model: &str,
    language: Option<&str>,
) -> Result<crate::transcription::models::TranscriptionSelection, ControlError> {
    use crate::transcription::models::{
        MODELS, TranscriptionModelId, TranscriptionSelection, definition, validate,
    };
    let id = model.trim().parse::<TranscriptionModelId>().map_err(|_| {
        let only = MODELS[0].id.as_str();
        ControlError::new(
            "unknown-model",
            format!("unknown model `{model}`; the only model is `{only}` (ElevenLabs Scribe)"),
        )
    })?;
    let selection = TranscriptionSelection {
        model: id,
        language: match language {
            Some(language) => language.trim().to_ascii_lowercase(),
            // A language saved by an older Hex that Scribe can't be told
            // falls back to detection, as the app does.
            None => definition(id)
                .language_for(&settings.transcription.language)
                .to_owned(),
        },
    };
    validate(&selection).map_err(|error| ControlError::new("invalid-value", error.to_string()))?;
    Ok(selection)
}

/// What `hex model use` answers once `selection` is in use (`changed`), or
/// when it already was.
pub fn model_use_message(
    selection: &crate::transcription::models::TranscriptionSelection,
    changed: bool,
) -> String {
    let name = crate::transcription::models::definition(selection.model).name;
    let adverb = if changed { "now" } else { "already" };
    if selection.language == crate::transcription::models::AUTO_LANGUAGE {
        format!("{name} {adverb} detects the language")
    } else {
        format!(
            "{name} {adverb} expects {}",
            language_label(&selection.language)
        )
    }
}

/// A language as replies name it: `German (de)`, or the code alone when Hex
/// has no name for it.
pub fn language_label(code: &str) -> String {
    match crate::transcription::models::language_name(code) {
        name if name == code => code.to_owned(),
        name => format!("{name} ({code})"),
    }
}

/// Refuses to stop Hex while it records, unless forced. `capture` is the
/// listener's answer (idle when no listener runs or setup gates it); a
/// listener that couldn't answer might be recording, so it refuses too.
pub fn check_quit(
    meeting: &MeetingReport,
    capture: Result<CaptureSnapshot, ControlError>,
    force: bool,
) -> Result<(), ControlError> {
    if force {
        return Ok(());
    }
    if meeting.state != "idle" {
        return Err(ControlError::new(
            "meeting-active",
            format!(
                "a meeting is {}; stop it first (`hex meetings stop`) or pass --force",
                meeting.state
            ),
        ));
    }
    match capture {
        Ok(capture) if capture.recording.is_some() || capture.pending_jobs > 0 => {
            Err(ControlError::new(
                "busy",
                "a dictation is recording or transcribing; wait for it or pass --force",
            ))
        }
        Ok(_) => Ok(()),
        Err(error) => Err(ControlError::new(
            "busy",
            format!(
                "couldn't confirm nothing is recording ({}); try again or pass --force",
                error.message
            ),
        )),
    }
}

/// Carries out `hex vocabulary add|remove` on the saved list. The reply's
/// `changed` says whether the list needs saving.
pub fn edit_vocabulary(
    list: &mut Vec<VocabularyEntry>,
    command: &ControlCommand,
) -> Result<ControlReply, ControlError> {
    let invalid = |message: String| ControlError::new("invalid-value", message);
    match command {
        ControlCommand::VocabularyAdd { term, sounds_like } => {
            let sounds_like: Vec<String> = sounds_like
                .iter()
                .flat_map(|spellings| vocabulary::parse_sounds_like(spellings))
                .collect();
            let existed = vocabulary::contains(list, term);
            Ok(
                match vocabulary::add(list, term, &sounds_like).map_err(invalid)? {
                    false => ControlReply::unchanged(format!(
                        "“{}” is already in the vocabulary",
                        term.trim()
                    )),
                    true if existed => {
                        ControlReply::done(format!("Added spellings to “{}”", term.trim()))
                    }
                    true => ControlReply::done(format!("Added “{}”", term.trim())),
                },
            )
        }
        ControlCommand::VocabularyRemove { term } => Ok(if vocabulary::remove(list, term) {
            ControlReply::done(format!("Removed “{}”", term.trim()))
        } else {
            ControlReply::unchanged(format!("“{}” isn't in the vocabulary", term.trim()))
        }),
        _ => Err(ControlError::new("internal", "not a vocabulary edit")),
    }
}

/// Process memory footprint on macOS; `None` where unmeasured.
pub fn memory_footprint() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: rusage_info_v4 is plain data that proc_pid_rusage fills.
        let mut info = unsafe { std::mem::zeroed::<libc::rusage_info_v4>() };
        // SAFETY: the pointer refers to a live, correctly sized buffer.
        let status = unsafe {
            libc::proc_pid_rusage(
                std::process::id() as i32,
                libc::RUSAGE_INFO_V4,
                (&mut info as *mut libc::rusage_info_v4).cast(),
            )
        };
        (status == 0).then_some(info.ri_phys_footprint)
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_printing_stop_answers_through_its_reply_channel() {
        let wait = Duration::from_millis(50);
        let (reply, printed) = PrintReply::channel();
        reply.send(PrintedTranscript::Text("Ship it.".into()));
        assert_eq!(
            await_printed(&printed, wait),
            ControlReply::Transcript {
                text: "Ship it.".into()
            }
        );
        let (reply, printed) = PrintReply::channel();
        reply.send(PrintedTranscript::NoSpeech);
        assert!(matches!(
            await_printed(&printed, wait),
            ControlReply::Error { code, .. } if code == "no-speech"
        ));
        // A dictation cancelled or failed before it had text drops the reply.
        let (reply, printed) = PrintReply::channel();
        drop(reply);
        assert!(matches!(
            await_printed(&printed, wait),
            ControlReply::Error { code, .. } if code == "failed"
        ));
        let (_reply, printed) = PrintReply::channel();
        assert!(matches!(
            await_printed(&printed, wait),
            ControlReply::Error { code, .. } if code == "failed"
        ));
        // Only `stop` prints, and older requests without the field still parse.
        assert!(check_flags(CaptureAction::Start, true, false).is_err());
        assert!(check_flags(CaptureAction::Stop, true, false).is_ok());
        assert!(check_flags(CaptureAction::Toggle, false, true).is_ok());
        assert!(check_flags(CaptureAction::Stop, false, true).is_err());
        assert_eq!(
            serde_json::from_str::<ControlRequest>(
                r#"{"version":5,"command":"dictation","action":"stop"}"#
            )
            .unwrap()
            .command,
            ControlCommand::Dictation {
                action: CaptureAction::Stop,
                print: false,
                raw: false,
            }
        );
    }

    /// `cancel` discards the recording, and takes neither flag.
    #[test]
    fn cancel_parses_and_takes_no_flags() {
        let cancel = serde_json::from_str::<ControlRequest>(
            r#"{"version":5,"command":"dictation","action":"cancel"}"#,
        )
        .unwrap();
        let ControlCommand::Dictation { action, .. } = cancel.command else {
            panic!("a dictation command");
        };
        assert_eq!(action, CaptureAction::Cancel);
        assert_eq!(check_flags(action, false, false), Ok(CaptureAction::Cancel));
        assert!(check_flags(action, true, false).is_err());
        assert!(check_flags(action, false, true).is_err());
    }

    fn round_trip_request(command: ControlCommand) {
        let request = ControlRequest::new(command);
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(
            serde_json::from_str::<ControlRequest>(&json).unwrap(),
            request
        );
    }

    #[test]
    fn every_command_round_trips() {
        for command in [
            ControlCommand::Status,
            ControlCommand::Quit { force: true },
            ControlCommand::Restart { force: false },
            ControlCommand::Dictation {
                action: CaptureAction::Toggle,
                print: false,
                raw: false,
            },
            ControlCommand::Dictation {
                action: CaptureAction::Stop,
                print: true,
                raw: false,
            },
            ControlCommand::Dictation {
                action: CaptureAction::Start,
                print: false,
                raw: true,
            },
            ControlCommand::PasteLast,
            ControlCommand::TypedText,
            ControlCommand::Screenshot { pick: false },
            ControlCommand::Screenshot { pick: true },
            ControlCommand::MeetingRecord {
                title: Some("Design sync".into()),
            },
            ControlCommand::MeetingStop,
            ControlCommand::MeetingCopyLast,
            ControlCommand::SettingsGet { key: None },
            ControlCommand::SettingsSet {
                key: "release_microphone_while_idle".into(),
                value: "on".into(),
            },
            ControlCommand::SettingsApply {
                values: serde_json::Map::from_iter([("sound_effects".into(), false.into())]),
            },
            ControlCommand::ModelList,
            ControlCommand::ModelUse {
                model: "scribe_v3".into(),
                language: Some("pl".into()),
            },
            ControlCommand::WindowShow {
                pane: Some(Pane::History),
            },
            ControlCommand::WindowHide,
            ControlCommand::OnboardingReset,
            ControlCommand::WindowStory {
                story: "HUD/Processing".into(),
                variant: Some("2".into()),
                play: true,
            },
            ControlCommand::HistoryClear,
            ControlCommand::CheckForUpdates,
            ControlCommand::LaunchAtLogin {
                enabled: Some(true),
            },
            ControlCommand::VocabularyList,
            ControlCommand::VocabularyAdd {
                term: "cappuccino".into(),
                sounds_like: vec!["capuchin".into()],
            },
            ControlCommand::VocabularyRemove {
                term: "cappuccino".into(),
            },
        ] {
            round_trip_request(command);
        }
        let request: ControlRequest =
            serde_json::from_str(r#"{"version":5,"command":"vocabulary-add","term":"Rune"}"#)
                .unwrap();
        assert_eq!(
            request.command,
            ControlCommand::VocabularyAdd {
                term: "Rune".into(),
                sounds_like: vec![],
            }
        );
    }

    #[test]
    fn vocabulary_edits_report_whether_they_changed_the_list() {
        let add = |term: &str, sounds_like: &[&str]| ControlCommand::VocabularyAdd {
            term: term.into(),
            sounds_like: sounds_like.iter().map(|&text| text.into()).collect(),
        };
        let remove = |term: &str| ControlCommand::VocabularyRemove { term: term.into() };
        let mut list = Vec::new();
        assert_eq!(
            edit_vocabulary(&mut list, &add("cappuccino", &["capuchin, cap a chino"])).unwrap(),
            ControlReply::done("Added “cappuccino”")
        );
        assert_eq!(list[0].sounds_like, ["capuchin", "cap a chino"]);
        assert_eq!(
            edit_vocabulary(&mut list, &add("Cappuccino", &[])).unwrap(),
            ControlReply::unchanged("“Cappuccino” is already in the vocabulary")
        );
        assert_eq!(
            edit_vocabulary(&mut list, &add("cappuccino", &["cap you chino"])).unwrap(),
            ControlReply::done("Added spellings to “cappuccino”")
        );
        assert_eq!(
            edit_vocabulary(&mut list, &add(" ", &[])).unwrap_err().code,
            "invalid-value"
        );
        assert_eq!(
            edit_vocabulary(&mut list, &remove("CAPPUCCINO")).unwrap(),
            ControlReply::done("Removed “CAPPUCCINO”")
        );
        assert_eq!(
            edit_vocabulary(&mut list, &remove("cappuccino")).unwrap(),
            ControlReply::unchanged("“cappuccino” isn't in the vocabulary")
        );
        assert!(list.is_empty());
    }

    #[test]
    fn requests_are_flat_and_versioned() {
        let json = serde_json::to_value(ControlRequest::new(ControlCommand::MeetingRecord {
            title: Some("Standup".into()),
        }))
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"version": 5, "command": "meeting-record", "title": "Standup"})
        );
        // Optional fields may be left out.
        let request: ControlRequest =
            serde_json::from_str(r#"{"version":5,"command":"quit"}"#).unwrap();
        assert_eq!(request.command, ControlCommand::Quit { force: false });
        assert!(serde_json::from_str::<ControlRequest>(r#"{"version":5,"command":"x"}"#).is_err());
        let capture: ControlRequest =
            serde_json::from_str(r#"{"version":5,"command":"scrawl-capture","action":"stop"}"#)
                .unwrap();
        assert_eq!(
            capture.command,
            ControlCommand::ScrawlCapture {
                action: ScrawlCaptureAction::Stop
            }
        );
        let send: ControlRequest = serde_json::from_str(
            r#"{"version":5,"command":"scrawl-capture-send","capture":"latest"}"#,
        )
        .unwrap();
        assert_eq!(
            send.command,
            ControlCommand::ScrawlCaptureSend {
                capture: "latest".into(),
                to: None,
                always: false,
            }
        );
    }

    #[test]
    fn every_reply_round_trips() {
        for reply in [
            ControlReply::done("Started dictation"),
            ControlReply::unchanged("Already recording"),
            ControlReply::Status(Box::default()),
            ControlReply::Settings {
                settings: vec![settings::SettingValue {
                    key: "sound_effects".into(),
                    value: serde_json::json!(true),
                    kind: "bool".into(),
                    choices: vec![],
                    writable: true,
                    about: "Play tones".into(),
                    available: true,
                }],
            },
            ControlReply::Models {
                models: model_infos(&Default::default()),
            },
            ControlReply::Model(Box::new(model_infos(&Default::default()).remove(0))),
            ControlReply::ApiKey(ApiKeyStatus {
                api_key: Some(ApiKeyReport {
                    source: ApiKeySource::Saved,
                    redacted: "••••cdef".into(),
                }),
                key_file: Some("/tmp/hex/elevenlabs.key".into()),
            }),
            ControlReply::Vocabulary {
                entries: vec![VocabularyEntry {
                    term: "cappuccino".into(),
                    sounds_like: vec!["capuchin".into()],
                }],
            },
            ControlReply::ScrawlCapture(Box::new(ScrawlCaptureReport {
                changed: true,
                state: ScrawlCaptureState::Idle,
                bundle: Some("/tmp/1.capture".into()),
                elapsed_ms: None,
                finishing: vec!["/tmp/1.capture".into()],
                last: Some(ScrawlCaptureOutcome {
                    bundle: "/tmp/0.capture".into(),
                    stage: "done".into(),
                    words: Some(3),
                    gaps: Some(1),
                    missing_transcript: None,
                }),
            })),
            ControlReply::ScrawlCaptures {
                captures: vec![ScrawlCaptureSummary {
                    id: "1.scrawl".into(),
                    sent_to: Some(ScrawlCaptureSession {
                        session: "ses_1".into(),
                        title: "Hex".into(),
                    }),
                    ..ScrawlCaptureSummary::default()
                }],
            },
            ControlReply::ScrawlCaptureDetail(Box::new(ScrawlCaptureDetail {
                summary: ScrawlCaptureSummary {
                    id: "1.scrawl".into(),
                    words: 3,
                    ..ScrawlCaptureSummary::default()
                },
                transcript: "make it blue".into(),
                video: Some("/tmp/1.scrawl/video.mp4".into()),
                ..ScrawlCaptureDetail::default()
            })),
            ControlReply::ScrawlCaptureRoute {
                explanation: "Capture   1.scrawl".into(),
            },
            ControlReply::Transcript {
                text: "Ship it.".into(),
            },
            ControlReply::error("busy", "Hex is busy"),
        ] {
            let response = ControlResponse::from(reply);
            let json = serde_json::to_string(&response).unwrap();
            assert_eq!(
                serde_json::from_str::<ControlResponse>(&json).unwrap(),
                response
            );
        }
        let json = serde_json::to_value(ControlResponse::from(ControlReply::Transcript {
            text: "Ship it.".into(),
        }))
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"version": 5, "status": "transcript", "text": "Ship it."})
        );
        let json =
            serde_json::to_value(ControlResponse::from(ControlReply::error("busy", "x"))).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"version": 5, "status": "error", "code": "busy", "message": "x"})
        );
    }

    #[test]
    fn a_request_from_another_version_is_refused() {
        let mut request = ControlRequest::new(ControlCommand::Status);
        assert!(check_version(&request).is_ok());
        request.version = 99;
        assert_eq!(
            check_version(&request).unwrap_err().code,
            "version-mismatch"
        );
    }

    #[test]
    fn quitting_waits_for_recordings_unless_forced() {
        let idle = MeetingReport {
            state: "idle".into(),
            ..Default::default()
        };
        let recording = MeetingReport {
            state: "recording".into(),
            ..Default::default()
        };
        let quiet = || Ok(CaptureSnapshot::default());
        assert!(check_quit(&idle, quiet(), false).is_ok());
        assert_eq!(
            check_quit(&recording, quiet(), false).unwrap_err().code,
            "meeting-active"
        );
        assert!(check_quit(&recording, quiet(), true).is_ok());
        let busy = CaptureSnapshot {
            recording: None,
            pending_jobs: 1,
            microphone_unavailable: false,
        };
        assert_eq!(check_quit(&idle, Ok(busy), false).unwrap_err().code, "busy");
        // A listener that can't answer may be recording: only --force quits.
        let unanswered = || {
            Err(ControlError::new(
                "unavailable",
                "the listener didn't answer",
            ))
        };
        assert_eq!(
            check_quit(&idle, unanswered(), false).unwrap_err().code,
            "busy"
        );
        assert!(check_quit(&idle, unanswered(), true).is_ok());
    }

    #[test]
    fn model_selection_takes_only_scribe_and_a_language_it_can_be_told() {
        use crate::transcription::models::{TranscriptionModelId as Id, TranscriptionSelection};
        let settings = crate::app_settings::AppSettings {
            transcription: TranscriptionSelection {
                model: Id::ScribeV3,
                language: "de".into(),
            },
            ..Default::default()
        };
        let pick = |model, language| model_selection(&settings, model, language);
        assert_eq!(pick("scribe_v3", None).unwrap().language, "de");
        assert_eq!(pick("scribe_v3", Some("fr")).unwrap().language, "fr");
        assert_eq!(pick("scribe_v3", Some(" YUE ")).unwrap().language, "yue");
        assert_eq!(pick("scribe_v3", Some("auto")).unwrap().language, "auto");
        for language in ["klingon", "en-US", ""] {
            assert_eq!(
                pick("scribe_v3", Some(language)).unwrap_err().code,
                "invalid-value",
                "{language:?}"
            );
        }
        // A model Hex used to run on this computer is named as unknown, with
        // the one id that works.
        for model in ["parakeet_v3", "whisper_large_v3_turbo", "apple_speech", ""] {
            let error = pick(model, None).unwrap_err();
            assert_eq!(error.code, "unknown-model", "{model:?}");
            assert!(error.message.contains("`scribe_v3`"), "{}", error.message);
        }
    }

    #[test]
    fn a_saved_language_scribe_cannot_be_told_falls_back_to_auto() {
        let mut settings = crate::app_settings::AppSettings::default();
        settings.transcription.language = "zh-Hant".into();
        let selection = model_selection(&settings, "scribe_v3", None).unwrap();
        assert_eq!(selection.language, "auto");
        assert_eq!(model_infos(&settings.transcription)[0].language, "auto");
    }

    #[test]
    fn the_model_list_is_scribe_with_its_language_and_no_key() {
        let selection = crate::transcription::models::TranscriptionSelection {
            language: "pl".into(),
            ..Default::default()
        };
        let models = model_infos(&selection);
        let [model] = models.as_slice() else {
            panic!("one model, got {models:?}");
        };
        assert_eq!(model.id, "scribe_v3");
        assert_eq!(model.name, "Scribe v3");
        assert_eq!(model.live, crate::transcription::scribe::live::MODEL);
        assert!(model.selected);
        assert_eq!(model.language, "pl");
        // Unit tests never read this computer's key.
        assert_eq!(model.api_key, None);
        assert!(model.api_url.starts_with("http"), "{}", model.api_url);
        let report = model_report(&selection, None);
        assert_eq!(
            (report.id.as_str(), report.language.as_str()),
            ("scribe_v3", "pl")
        );
        assert_eq!(report.api_key, None);
    }

    #[test]
    fn a_key_is_reported_by_its_source_and_tail_only() {
        use crate::transcription::scribe::key::{ApiKey, Source};
        let report = ApiKeyReport::from(&ApiKey {
            secret: "sk_0123456789abcdef".into(),
            source: Source::File,
        });
        assert_eq!(report.source, ApiKeySource::Saved);
        let json =
            serde_json::to_string(&ControlResponse::from(ControlReply::ApiKey(ApiKeyStatus {
                api_key: Some(report),
                key_file: Some("/tmp/hex/elevenlabs.key".into()),
            })))
            .unwrap();
        assert!(!json.contains("0123456789"), "{json}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json).unwrap(),
            serde_json::json!({
                "version": 5,
                "status": "api-key",
                "api_key": {"source": "saved", "redacted": "••••cdef"},
                "key_file": "/tmp/hex/elevenlabs.key",
            })
        );
        let environment = ApiKeyReport::from(&ApiKey {
            secret: "sk_0123456789abcdef".into(),
            source: Source::Environment,
        });
        assert_eq!(
            serde_json::to_value(environment.source).unwrap(),
            "environment"
        );
    }

    /// Commands that only served downloads are gone: an older `hex` that
    /// sends one is told its version, not that the command is malformed.
    #[test]
    fn download_commands_are_no_longer_part_of_the_protocol() {
        for command in ["model-install", "model-cancel"] {
            let request = format!(r#"{{"version":5,"command":"{command}","model":"scribe_v3"}}"#);
            assert!(
                serde_json::from_str::<ControlRequest>(&request).is_err(),
                "{command}"
            );
        }
        const { assert!(CONTROL_VERSION > 4, "version 4 had the download commands") };
    }

    #[test]
    fn capture_reports_derive_their_state() {
        let report = CaptureReport::from(CaptureSnapshot {
            recording: Some(CaptureKind::Dictation),
            pending_jobs: 2,
            microphone_unavailable: false,
        });
        assert_eq!(report.state, CaptureState::Recording);
        let report = CaptureReport::from(CaptureSnapshot {
            recording: None,
            pending_jobs: 2,
            microphone_unavailable: true,
        });
        assert_eq!(report.state, CaptureState::Transcribing);
    }
}
