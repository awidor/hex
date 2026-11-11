use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;
use std::time::Instant;

use color_eyre::eyre::{Result, eyre};

use crate::dictation::DictationClip;
use crate::dictation::context::ContextSnapshot;
use crate::dictation::processor::ProcessingObservation;
use crate::history::{History, HistoryDraft, HistoryKind};
use crate::meeting;
use crate::paste::{PasteMode, Paster};
use crate::suppression::InputActivity;
use crate::transcription::engine::{PendingTranscript, TranscriptionEngine};
use crate::transcription::models::TranscriptionSelection;
use crate::transcription::scribe::ScribeTranscriber;

const TRANSCRIPTION_SAMPLES_PER_MS: usize = 16;
const MAX_PENDING_OUTPUTS: usize = 16;

pub enum WorkerEvent {
    ModelFailed(String),
    Completed {
        job_id: DictationJobId,
        target: TranscriptionTarget,
        result: Result<String, String>,
        processing: Option<ProcessingObservation>,
    },
    Stage {
        job_id: DictationJobId,
        stage: DictationJobStage,
    },
    Cancelled {
        job_id: DictationJobId,
    },
    Pasted {
        kind: PasteKind,
        result: Result<String, String>,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DictationJobId(u64);

impl DictationJobId {
    pub const fn value(self) -> u64 {
        self.0
    }

    #[cfg(test)]
    pub(crate) const fn for_test(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DictationJobStage {
    Transcribing,
    Processing,
}

#[derive(Clone, Copy, Debug)]
pub enum PasteKind {
    LastTranscript,
    MeetingDelta,
}

#[derive(Clone, Copy, Debug)]
pub enum TranscriptionTarget {
    Paste,
    /// A recording microphone interrupted a shortcut dictation: processed
    /// like a paste, but copied to the clipboard instead of pasted, still
    /// available to Paste Last Dictation and kept in History.
    CopyOnlyPaste,
    /// The same interrupted recording from Dictate without processing: copied
    /// as heard, skipping the Output pane's steps.
    RawCopyOnlyPaste,
    /// Dictate without processing: pasted as heard, skipping the Output
    /// pane's steps.
    RawPaste,
    Service,
    /// `hex dictation stop --print`: processed as a paste, then handed to
    /// the waiting command instead of pasted.
    Print,
    /// A printing stop of a raw dictation: the transcript as heard.
    RawPrint,
}

struct InferenceJob {
    job_id: DictationJobId,
    control: Arc<JobControl>,
    submitted_at: Instant,
    transcript: PendingTranscript,
    audio_ms: u64,
    prepare_ms: u128,
    target: TranscriptionTarget,
    context: ContextSnapshot,
    /// Labs › Return to app: the app the capture started in, which the
    /// paste returns to (macOS).
    return_to: Option<i32>,
    /// The language the model was asked for at submission.
    language: String,
    /// The clip, kept for History while Labs › Keep recordings is on.
    recording: Option<crate::history::Recording>,
}

struct ProcessorJob {
    job_id: DictationJobId,
    control: Arc<JobControl>,
    target: TranscriptionTarget,
    text: String,
    context: ContextSnapshot,
    return_to: Option<i32>,
    language: String,
    timings: JobTimings,
    recording: Option<crate::history::Recording>,
}

/// Pipeline timings carried from inference through output for the final log.
#[derive(Clone, Copy)]
struct JobTimings {
    total_started: Instant,
    queue_ms: u128,
    audio_ms: u64,
    prepare_ms: u128,
    inference_ms: u128,
}

struct CompletedTranscript {
    text: String,
    /// Corrected transcript before mode processing.
    raw: String,
    application: Option<String>,
    /// Labs › Return to app: the app the capture started in, which the
    /// paste returns to (macOS).
    return_to: Option<i32>,
    timings: JobTimings,
    processing: Option<ProcessingObservation>,
    recording: Option<crate::history::Recording>,
}

enum OutputJob {
    PreparePaste,
    Completed {
        job_id: DictationJobId,
        control: Arc<JobControl>,
        target: TranscriptionTarget,
        result: Box<Result<CompletedTranscript, String>>,
    },
    Cancelled {
        job_id: DictationJobId,
    },
    Paste {
        sequence: u64,
        kind: PasteKind,
    },
}

impl OutputJob {
    fn sequence(&self) -> u64 {
        match self {
            Self::PreparePaste => u64::MAX,
            Self::Completed { job_id, .. } | Self::Cancelled { job_id } => job_id.0,
            Self::Paste { sequence, .. } => *sequence,
        }
    }
}

#[derive(Default)]
struct JobControl {
    cancelled: AtomicBool,
    output_started: Mutex<bool>,
    /// A printing stop's reply; dropping the job unanswered tells it the
    /// dictation was cancelled or failed.
    print: Mutex<Option<crate::control::PrintReply>>,
}

impl JobControl {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn answer_print(&self, transcript: crate::control::PrintedTranscript) {
        if let Some(reply) = self
            .print
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            reply.send(transcript);
        }
    }

    fn begin_output(&self) -> bool {
        let mut output_started = self
            .output_started
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.is_cancelled() {
            return false;
        }
        *output_started = true;
        true
    }
}

#[derive(Default)]
struct OrderedOutputs {
    waiting: BTreeMap<u64, OutputJob>,
    next_sequence: u64,
}

impl OrderedOutputs {
    fn push(&mut self, job: OutputJob) -> Vec<OutputJob> {
        if job.sequence() < self.next_sequence {
            return Vec::new();
        }
        self.waiting.insert(job.sequence(), job);
        let mut ready = Vec::new();
        while let Some(job) = self.waiting.remove(&self.next_sequence) {
            ready.push(job);
            self.next_sequence += 1;
        }
        ready
    }
}

pub struct DictationWorker {
    engine: Option<Arc<TranscriptionEngine>>,
    inference_jobs: Option<SyncSender<InferenceJob>>,
    output_jobs: Option<SyncSender<OutputJob>>,
    events: Receiver<WorkerEvent>,
    event_sender: mpsc::Sender<WorkerEvent>,
    state: Arc<Mutex<WorkerState>>,
    inference_worker: Option<thread::JoinHandle<()>>,
    processor_workers: Vec<thread::JoinHandle<()>>,
    output_worker: Option<thread::JoinHandle<()>>,
}

struct WorkerState {
    next_sequence: u64,
    jobs: BTreeMap<DictationJobId, Arc<JobControl>>,
    pending_pastes: usize,
    /// Labs › Return to app: the app in front when the capture started,
    /// which its output returns to (macOS).
    return_to: Option<i32>,
}

impl WorkerState {
    fn next_output_sequence(&self) -> Result<u64, &'static str> {
        // Count accepted work even after it leaves a channel for ordered buffering.
        if self.jobs.len() + self.pending_pastes >= MAX_PENDING_OUTPUTS {
            return Err("dictation queue is full");
        }
        Ok(self.next_sequence)
    }
}

impl DictationWorker {
    pub fn start(
        activity: InputActivity,
        transformations: Arc<crate::personal_commands::TransformationClient>,
        history: Option<History>,
    ) -> Self {
        const PROCESSOR_WORKERS: usize = 2;
        // Admission caps accepted jobs, so a submitted job always has a slot.
        let (inference_jobs, inference_receiver) =
            mpsc::sync_channel::<InferenceJob>(MAX_PENDING_OUTPUTS);
        let (processor_jobs, processor_receiver) = mpsc::sync_channel::<ProcessorJob>(4);
        let (output_jobs, output_receiver) = mpsc::sync_channel::<OutputJob>(8);
        let (event_sender, events) = mpsc::channel();
        let state = Arc::new(Mutex::new(WorkerState {
            next_sequence: 0,
            jobs: BTreeMap::new(),
            pending_pastes: 0,
            return_to: None,
        }));

        let (_, selection) = crate::app_settings::transcription_selection();
        // Nothing is loaded or sent here: the transcriber only remembers the
        // language, and each dictation activates its own selection.
        let engine = ScribeTranscriber::load(&selection).and_then(TranscriptionEngine::start);
        let engine = match engine {
            Ok(engine) => {
                let _ = engine.run(|_| prioritize_inference_thread());
                let engine = Arc::new(engine);
                engine.share();
                Some(engine)
            }
            Err(error) => {
                let _ = event_sender.send(WorkerEvent::ModelFailed(error.to_string()));
                None
            }
        };

        let output_worker = thread::spawn({
            let state = state.clone();
            let events = event_sender.clone();
            move || run_output_worker(output_receiver, activity, history, &state, &events)
        });

        let processor_receiver = Arc::new(Mutex::new(processor_receiver));
        let processor_workers = (0..PROCESSOR_WORKERS)
            .map(|_| {
                let jobs = processor_receiver.clone();
                let output = output_jobs.clone();
                let events = event_sender.clone();
                let transformations = transformations.clone();
                thread::spawn(move || {
                    run_processor_worker(&jobs, &output, &events, &transformations)
                })
            })
            .collect();

        let inference_worker = thread::spawn({
            let output = output_jobs.clone();
            let events = event_sender.clone();
            move || run_inference_worker(inference_receiver, &processor_jobs, &output, &events)
        });

        Self {
            engine,
            inference_jobs: Some(inference_jobs),
            output_jobs: Some(output_jobs),
            events,
            event_sender,
            state,
            inference_worker: Some(inference_worker),
            processor_workers,
            output_worker: Some(output_worker),
        }
    }

    /// Labs › Return to app (macOS): remembers the app in front as this
    /// capture starts, so its paste can bring it back. Read only while the
    /// lab is on, and never Hex itself.
    pub fn remember_return_to_app(&self) {
        let return_to = crate::paste::return_to_app::capture_start();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.return_to = return_to;
    }

    /// Hands the clip to the engine at once, so its live stream finishes
    /// before any later capture begins one; the job then waits in order.
    pub fn transcribe(
        &self,
        clip: DictationClip,
        target: TranscriptionTarget,
        context: ContextSnapshot,
    ) -> Result<DictationJobId, &'static str> {
        self.submit(clip, target, context, None)
    }

    /// Like a paste, but the transcript (processed unless `raw`) goes to
    /// `print`.
    pub fn transcribe_printed(
        &self,
        clip: DictationClip,
        print: crate::control::PrintReply,
        raw: bool,
        context: ContextSnapshot,
    ) -> Result<DictationJobId, &'static str> {
        let target = if raw {
            TranscriptionTarget::RawPrint
        } else {
            TranscriptionTarget::Print
        };
        self.submit(clip, target, context, Some(print))
    }

    fn submit(
        &self,
        mut clip: DictationClip,
        target: TranscriptionTarget,
        context: ContextSnapshot,
        print: Option<crate::control::PrintReply>,
    ) -> Result<DictationJobId, &'static str> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let job_id = DictationJobId(state.next_output_sequence()?);
        // Labs › Return to app: only a paste returns to an app; a printing
        // stop and the service never paste.
        let return_to = matches!(
            target,
            TranscriptionTarget::Paste | TranscriptionTarget::RawPaste
        )
        .then(|| state.return_to.take())
        .flatten();
        let (Some(engine), Some(jobs)) = (&self.engine, &self.inference_jobs) else {
            return Err("dictation worker is unavailable");
        };
        let control = Arc::new(JobControl {
            print: Mutex::new(print),
            ..JobControl::default()
        });
        let (_, selection) = crate::app_settings::transcription_selection();
        let language = selection.language.clone();
        let submitted_at = Instant::now();
        let audio_ms = clip.duration_ms();
        let stream = clip.take_stream();
        let _capture = clip.take_capture_marker();
        let samples = clip.into_transcription_samples();
        crate::dictation::diagnostics::persist(&samples);
        let recording = match target {
            TranscriptionTarget::Service => None,
            _ => crate::history::Recording::keep(&samples),
        };
        let prepare_ms = submitted_at.elapsed().as_millis();
        let offline = {
            let control = control.clone();
            Box::new(move |model: &mut ScribeTranscriber, samples: Vec<f32>| {
                if control.is_cancelled() {
                    return Err("dictation was cancelled".to_string());
                }
                let _busy = DictationInference::begin();
                let transcriber = model
                    .activate(&selection)
                    .map_err(|error| error.to_string())?;
                // A clip that could not be sent is kept to transcribe later.
                transcriber
                    .transcribe(&samples)
                    .map_err(|error| crate::transcription::failed_clips::describe(&samples, &error))
            })
        };
        let transcript = engine
            .finish_with(stream, samples, offline)
            .map_err(|_| "dictation worker is unavailable")?;
        jobs.try_send(InferenceJob {
            job_id,
            control: control.clone(),
            submitted_at,
            transcript,
            audio_ms,
            prepare_ms,
            target,
            context,
            return_to,
            language,
            recording,
        })
        .map(|()| {
            state.next_sequence += 1;
            state.jobs.insert(job_id, control);
            job_id
        })
        .map_err(queue_error)
    }

    /// The engine that streams hotkey captures while they are recorded.
    pub fn engine(&self) -> Option<Arc<TranscriptionEngine>> {
        self.engine.clone()
    }

    pub fn prepare_paste(&self) {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.jobs.is_empty() || state.pending_pastes > 0 {
            return;
        }
        drop(state);
        let Some(output_jobs) = &self.output_jobs else {
            return;
        };
        let _ = output_jobs.try_send(OutputJob::PreparePaste);
    }

    /// Switches the language after the work already handed to the engine.
    pub fn reload(&self, selection: TranscriptionSelection) -> Result<(), &'static str> {
        let events = self.event_sender.clone();
        self.engine
            .as_ref()
            .ok_or("dictation worker is unavailable")?
            .run(move |model| {
                let _busy = DictationInference::begin();
                if let Err(error) = model.activate(&selection) {
                    let _ = events.send(WorkerEvent::ModelFailed(error.to_string()));
                }
            })
            .map_err(|_| "dictation worker is unavailable")
    }

    pub fn paste_last(&self) -> Result<(), &'static str> {
        self.submit_paste(PasteKind::LastTranscript)
    }

    pub fn paste_meeting(&self) -> Result<(), &'static str> {
        self.submit_paste(PasteKind::MeetingDelta)
    }

    fn submit_paste(&self, kind: PasteKind) -> Result<(), &'static str> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let sequence = state.next_output_sequence()?;
        self.output_jobs
            .as_ref()
            .ok_or("dictation worker is unavailable")?
            .try_send(OutputJob::Paste { sequence, kind })
            .map(|()| {
                state.next_sequence += 1;
                state.pending_pastes += 1;
            })
            .map_err(queue_error)
    }

    pub fn try_recv(&self) -> Option<WorkerEvent> {
        self.events.try_recv().ok()
    }

    pub fn is_busy(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        !state.jobs.is_empty() || state.pending_pastes > 0
    }

    pub fn pending_count(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .jobs
            .len()
    }

    fn shutdown(&mut self) {
        // Accepted jobs still finish: the inference worker drains its queue.
        self.inference_jobs.take();
        join_worker(self.inference_worker.take(), "dictation inference");
        for worker in self.processor_workers.drain(..) {
            join_worker(Some(worker), "dictation processing");
        }
        self.output_jobs.take();
        join_worker(self.output_worker.take(), "dictation output");
    }
}

impl Drop for DictationWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_output_worker(
    jobs: Receiver<OutputJob>,
    activity: InputActivity,
    history: Option<History>,
    state: &Mutex<WorkerState>,
    events: &mpsc::Sender<WorkerEvent>,
) {
    let mut paster = Paster::new(activity);
    let mut last_transcript = None;
    let mut ordered = OrderedOutputs::default();
    while let Ok(job) = jobs.recv() {
        if matches!(job, OutputJob::PreparePaste) {
            paster.prepare();
            continue;
        }
        for job in ordered.push(job) {
            let event = finish_output(
                job,
                &mut |text, mode, commit| paster.paste(text, mode, commit),
                &mut last_transcript,
                history.as_ref(),
            );
            let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
            match &event {
                WorkerEvent::Completed { job_id, .. } | WorkerEvent::Cancelled { job_id } => {
                    state.jobs.remove(job_id);
                }
                WorkerEvent::Pasted { .. } => {
                    state.pending_pastes = state.pending_pastes.saturating_sub(1);
                }
                WorkerEvent::ModelFailed(_) | WorkerEvent::Stage { .. } => {}
            }
            drop(state);
            if events.send(event).is_err() {
                return;
            }
        }
    }
}

fn run_processor_worker(
    jobs: &Mutex<Receiver<ProcessorJob>>,
    output: &SyncSender<OutputJob>,
    events: &mpsc::Sender<WorkerEvent>,
    transformations: &crate::personal_commands::TransformationClient,
) {
    loop {
        let job = jobs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .recv();
        let Ok(job) = job else { break };
        if job.control.is_cancelled() {
            let _ = output.send(OutputJob::Cancelled { job_id: job.job_id });
            continue;
        }
        let mut processed =
            process_job_text(&job, crate::dictation::config::dictation_profiles, || {
                let _ = events.send(WorkerEvent::Stage {
                    job_id: job.job_id,
                    stage: DictationJobStage::Processing,
                });
            });
        processed.apply_transformations(
            transformations,
            &job.context,
            &job.control.cancelled,
            crate::dictation::processor::Source::Dictation,
        );
        if job.control.is_cancelled() {
            let _ = output.send(OutputJob::Cancelled { job_id: job.job_id });
            continue;
        }
        if let Some(observation) = &processed.observation
            && let Some(error) = &observation.fallback
        {
            tracing::warn!(
                profile = observation.profile,
                %error,
                "dictation processing fell back to the previous pipeline output"
            );
        }
        if output
            .send(OutputJob::Completed {
                job_id: job.job_id,
                control: job.control,
                target: job.target,
                result: Box::new(Ok(CompletedTranscript {
                    text: processed.text,
                    raw: job.text,
                    application: job.context.application,
                    return_to: job.return_to,
                    timings: job.timings,
                    processing: processed.observation,
                    recording: job.recording,
                })),
            })
            .is_err()
        {
            break;
        }
    }
}

/// Select processing when the worker reaches the job; the stage precedes processing.
fn process_job_text(
    job: &ProcessorJob,
    profiles: impl FnOnce() -> crate::dictation::processor::Profiles,
    processing: impl FnOnce(),
) -> crate::dictation::processor::Processed {
    let profiles = profiles();
    if profiles.processes(&job.context) {
        processing();
    }
    profiles.process_cancellable(
        &job.text,
        &job.language,
        &job.context,
        &job.control.cancelled,
        crate::dictation::processor::Source::Dictation,
    )
}

/// The longest audio the capture path hands the model at once; longer
/// captures are transcribed in consecutive chunks with their times offset.
#[cfg(hex_capture)]
const WORD_CHUNK_SAMPLES: usize = 30 * 16_000;
/// How long a capture chunk waits for dictation to go idle before it is
/// queued anyway; the engine still never runs it inside a live stream.
#[cfg(hex_capture)]
const WORD_DICTATION_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// Transcribes a capture's 16 kHz audio with word timestamps through the
/// engine (the `capture` experiment). Before each chunk it waits while
/// dictation is busy (from the shortcut press through its transcript); each
/// chunk is then a foreground request: after the dictation already sent,
/// before meeting chunks, and never inside a dictation live stream, which
/// finishes first. Only a weak reference to the engine is kept between
/// chunks. Quitting stops it between chunks. Every chunk runs on `selection`.
#[cfg(hex_capture)]
pub fn transcribe_with_words(
    engine: &std::sync::Weak<TranscriptionEngine>,
    selection: &crate::transcription::models::TranscriptionSelection,
    samples: Vec<f32>,
) -> Result<crate::transcription::Transcript> {
    if samples.is_empty() {
        return Ok(crate::transcription::Transcript::default());
    }
    transcribe_in_chunks(samples, WORD_CHUNK_SAMPLES, |chunk| {
        wait_for_dictation_idle()?;
        let selection = selection.clone();
        let (send, transcript) = mpsc::sync_channel(1);
        let offline: crate::transcription::engine::Offline = Box::new(move |model, samples| {
            let _busy = DictationInference::begin();
            let transcriber = model
                .activate(&selection)
                .map_err(|error| format!("{error:#}"))?;
            let result = transcriber
                .transcribe_segments(samples)
                .map_err(|error| format!("{error:#}"))?;
            let text = result.text.clone();
            let _ = send.send(result);
            Ok(text)
        });
        let pending = engine
            .upgrade()
            .ok_or_else(|| eyre!("the transcription engine stopped"))?
            .foreground(chunk, offline)?;
        pending.wait().map_err(|error| eyre!(error))?;
        transcript
            .try_recv()
            .map_err(|_| eyre!("the transcription returned no words"))
    })
}

#[cfg(hex_capture)]
fn wait_for_dictation_idle() -> Result<()> {
    let started = Instant::now();
    while dictation_busy()
        && !crate::desktop::process_exit::quitting()
        && started.elapsed() < WORD_DICTATION_WAIT
    {
        thread::sleep(std::time::Duration::from_millis(50));
    }
    if crate::desktop::process_exit::quitting() {
        return Err(eyre!("Hex is quitting"));
    }
    Ok(())
}

/// Transcribes `samples` in chunks of at most `chunk_samples`, offsetting
/// each chunk's segment and word times by where it starts.
#[cfg(hex_capture)]
fn transcribe_in_chunks(
    samples: Vec<f32>,
    chunk_samples: usize,
    mut transcribe: impl FnMut(Vec<f32>) -> Result<crate::transcription::Transcript>,
) -> Result<crate::transcription::Transcript> {
    use crate::transcription::{Transcript, TranscriptSegment};
    let shift = |spans: Vec<TranscriptSegment>, offset_ms: i64| {
        spans.into_iter().filter_map(move |span| {
            let text = span.text.trim();
            let start_ms = span.start_ms.max(0);
            (!text.is_empty()).then(|| TranscriptSegment {
                start_ms: start_ms + offset_ms,
                end_ms: span.end_ms.max(start_ms) + offset_ms,
                text: text.to_string(),
            })
        })
    };
    let mut merged = Transcript::default();
    let mut texts = Vec::new();
    for (index, chunk) in samples.chunks(chunk_samples.max(1)).enumerate() {
        let offset_ms = (index * chunk_samples / TRANSCRIPTION_SAMPLES_PER_MS) as i64;
        let result = transcribe(chunk.to_vec())?;
        let text = result.text.trim();
        if !text.is_empty() {
            texts.push(text.to_string());
        }
        merged.segments.extend(shift(result.segments, offset_ms));
        merged.words.extend(shift(result.words, offset_ms));
    }
    merged.text = texts.join(" ");
    Ok(merged)
}

/// Waits for each accepted job's transcript in submission order, then hands
/// it to processing. The engine thread runs the inference itself.
fn run_inference_worker(
    jobs: Receiver<InferenceJob>,
    processor_jobs: &SyncSender<ProcessorJob>,
    output: &SyncSender<OutputJob>,
    events: &mpsc::Sender<WorkerEvent>,
) {
    while let Ok(job) = jobs.recv() {
        if job.control.is_cancelled() {
            let _ = output.send(OutputJob::Cancelled { job_id: job.job_id });
            continue;
        }
        let _busy = DictationInference::begin();
        let _ = events.send(WorkerEvent::Stage {
            job_id: job.job_id,
            stage: DictationJobStage::Transcribing,
        });
        let total_started = job.submitted_at;
        let queue_ms = total_started.elapsed().as_millis();
        let inference_started = Instant::now();
        let audio_ms = job.audio_ms;
        let result = job.transcript.wait().map(|transcribed| {
            tracing::info!(
                source = transcribed.source.label(),
                finish_ms = transcribed.finish_ms,
                audio_ms,
                "transcribed dictation"
            );
            let text = transcribed.text;
            let corrected = if matches!(job.target, TranscriptionTarget::Service) {
                text.clone()
            } else {
                text.trim().to_string()
            };
            tracing::debug!(
                raw_transcript = text,
                corrected_transcript = corrected,
                "prepared transcript"
            );
            corrected
        });
        let timings = JobTimings {
            total_started,
            queue_ms,
            audio_ms,
            prepare_ms: job.prepare_ms,
            inference_ms: inference_started.elapsed().as_millis(),
        };
        if job.control.is_cancelled() {
            let _ = output.send(OutputJob::Cancelled { job_id: job.job_id });
            continue;
        }
        let target = job.target;
        match result {
            Ok(text)
                if matches!(
                    target,
                    TranscriptionTarget::Paste
                        | TranscriptionTarget::CopyOnlyPaste
                        | TranscriptionTarget::Print
                ) && !text.trim().is_empty() =>
            {
                if processor_jobs
                    .send(ProcessorJob {
                        job_id: job.job_id,
                        control: job.control,
                        target,
                        text,
                        context: job.context,
                        return_to: job.return_to,
                        language: job.language,
                        timings,
                        recording: job.recording,
                    })
                    .is_err()
                {
                    break;
                }
            }
            result => {
                let application = job.context.application;
                let recording = job.recording;
                let result = result.map(|text| CompletedTranscript {
                    raw: text.clone(),
                    text,
                    application,
                    return_to: job.return_to,
                    timings,
                    processing: None,
                    recording,
                });
                if output
                    .send(OutputJob::Completed {
                        job_id: job.job_id,
                        control: job.control,
                        target,
                        result: Box::new(result),
                    })
                    .is_err()
                {
                    break;
                }
            }
        }
    }
}

fn join_worker(worker: Option<thread::JoinHandle<()>>, name: &str) {
    if let Some(worker) = worker
        && worker.join().is_err()
    {
        tracing::error!(worker = name, "worker panicked during shutdown");
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn prioritize_inference_thread() {
    // The engine thread sends each dictation's request and waits for its
    // transcript, so it runs at the class the person waiting is served at.
    const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
    unsafe extern "C" {
        fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }
    // SAFETY: This configures only the calling worker thread using the public
    // macOS pthread QoS API. Relative priority zero is valid for this class.
    let status = unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) };
    if status == 0 {
        tracing::info!("dictation inference worker uses user-interactive QoS");
    } else {
        tracing::warn!(status, "could not raise dictation inference worker QoS");
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn prioritize_inference_thread() {}

static DICTATION_INFERENCE: AtomicUsize = AtomicUsize::new(0);

struct DictationInference;

impl DictationInference {
    fn begin() -> Self {
        DICTATION_INFERENCE.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for DictationInference {
    fn drop(&mut self) {
        DICTATION_INFERENCE.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Makes [`dictation_busy`] hold until dropped, as a dictation would.
#[cfg(test)]
pub(crate) struct DictationBusyForTest {
    _busy: DictationInference,
}

#[cfg(test)]
pub(crate) fn dictation_busy_for_test() -> DictationBusyForTest {
    DictationBusyForTest {
        _busy: DictationInference::begin(),
    }
}

/// Whether dictation is capturing or running inference. Meeting
/// transcription starts no chunk while this holds. A capture counts from its
/// press: a chunk already running then finishes during the hold threshold
/// and release tail, before the clip needs the model.
pub(crate) fn dictation_busy() -> bool {
    DICTATION_INFERENCE.load(Ordering::Acquire) > 0
        || crate::audio::recording_environment::capture_active()
        || crate::audio::recording_environment::intentional_recording_active()
}

/// Runs `work` at utility QoS on the dictation inference thread, then
/// restores its user-interactive class.
pub(crate) fn at_background_priority<R>(work: impl FnOnce() -> R) -> R {
    const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
    const QOS_CLASS_UTILITY: u32 = 0x11;
    unsafe extern "C" {
        fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }
    // SAFETY: These configure only the calling thread using the public macOS
    // pthread QoS API. Relative priority zero is valid for both classes.
    unsafe { pthread_set_qos_class_self_np(QOS_CLASS_UTILITY, 0) };
    let result = work();
    unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) };
    result
}

/// Runs the calling thread at utility QoS for background transcription.
pub(crate) fn deprioritize_background_thread() {
    const QOS_CLASS_UTILITY: u32 = 0x11;
    unsafe extern "C" {
        fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }
    // SAFETY: This configures only the calling thread using the public macOS
    // pthread QoS API. Relative priority zero is valid for this class.
    let status = unsafe { pthread_set_qos_class_self_np(QOS_CLASS_UTILITY, 0) };
    if status != 0 {
        tracing::warn!(status, "could not lower background transcription QoS");
    }
}

fn queue_error(error: TrySendError<impl Sized>) -> &'static str {
    match error {
        TrySendError::Full(_) => "dictation queue is full",
        TrySendError::Disconnected(_) => "dictation worker is unavailable",
    }
}

fn finish_output(
    job: OutputJob,
    paste: &mut impl FnMut(&str, PasteMode, &dyn Fn() -> bool) -> Result<()>,
    last_transcript: &mut Option<String>,
    history: Option<&History>,
) -> WorkerEvent {
    match job {
        OutputJob::PreparePaste => unreachable!("paste preparation bypasses ordered output"),
        OutputJob::Completed {
            job_id,
            control,
            target,
            result,
        } if !control.is_cancelled() => {
            let result = *result;
            let processing = result
                .as_ref()
                .ok()
                .and_then(|result| result.processing.clone());
            let result = result.and_then(|mut completed| {
                if control.is_cancelled() {
                    return Err("dictation was cancelled".into());
                }
                let paste_started = Instant::now();
                if !completed.text.trim().is_empty() {
                    let commit = || control.begin_output();
                    match target {
                        TranscriptionTarget::Paste
                        | TranscriptionTarget::CopyOnlyPaste
                        | TranscriptionTarget::RawCopyOnlyPaste
                        | TranscriptionTarget::RawPaste => {
                            // Labs › Copy dictation only, read as the output
                            // commits: dictations go to the clipboard
                            // unpasted; Paste last dictation still pastes.
                            // A rescued microphone-loss dictation is always
                            // copied: the disruption may have taken focus.
                            let mode = if crate::labs::copy_only()
                                || matches!(
                                    target,
                                    TranscriptionTarget::CopyOnlyPaste
                                        | TranscriptionTarget::RawCopyOnlyPaste
                                )
                            {
                                PasteMode::CopyOnly
                            } else {
                                PasteMode::Continue
                            };
                            // Labs › Return to app: the app the capture
                            // started in comes back first. When it can't,
                            // nothing is pasted; the text stays for Paste
                            // last dictation, and History keeps only pasted
                            // output, as on any failed paste. A cancelled job
                            // leaves the last result alone, as it does
                            // everywhere else.
                            if matches!(mode, PasteMode::Continue)
                                && let Err(error) =
                                    crate::paste::return_to_app::bring_to_front(completed.return_to)
                            {
                                if !control.is_cancelled() {
                                    *last_transcript = Some(completed.text.clone());
                                }
                                tracing::warn!(
                                    %error,
                                    "a dictation found another app in front and couldn't return to its own"
                                );
                                return Err(error.to_string());
                            }
                            paste(&completed.text, mode, &commit)
                        }
                        TranscriptionTarget::Service
                        | TranscriptionTarget::Print
                        | TranscriptionTarget::RawPrint => commit()
                            .then_some(())
                            .ok_or_else(|| eyre!("dictation was cancelled")),
                    }
                    .map_err(|error| error.to_string())?;
                    if matches!(
                        target,
                        TranscriptionTarget::Paste
                            | TranscriptionTarget::CopyOnlyPaste
                            | TranscriptionTarget::RawCopyOnlyPaste
                            | TranscriptionTarget::RawPaste
                    ) {
                        *last_transcript = Some(completed.text.clone());
                    }
                    if let Some(history) = history {
                        record_history(history, target, &mut completed);
                    }
                }
                tracing::info!(
                    audio_ms = completed.timings.audio_ms,
                    queue_ms = completed.timings.queue_ms,
                    prepare_ms = completed.timings.prepare_ms,
                    inference_ms = completed.timings.inference_ms,
                    paste_ms = paste_started.elapsed().as_millis(),
                    total_ms = completed.timings.total_started.elapsed().as_millis(),
                    "dictation pipeline completed"
                );
                Ok(completed.text)
            });
            if matches!(
                target,
                TranscriptionTarget::Print | TranscriptionTarget::RawPrint
            ) && !control.is_cancelled()
            {
                control.answer_print(match &result {
                    Ok(text) if text.trim().is_empty() => {
                        crate::control::PrintedTranscript::NoSpeech
                    }
                    Ok(text) => crate::control::PrintedTranscript::Text(text.clone()),
                    Err(error) => crate::control::PrintedTranscript::Failed(error.clone()),
                });
            }
            if control.is_cancelled() {
                WorkerEvent::Cancelled { job_id }
            } else {
                WorkerEvent::Completed {
                    job_id,
                    target,
                    result,
                    processing,
                }
            }
        }
        OutputJob::Completed { job_id, .. } | OutputJob::Cancelled { job_id } => {
            WorkerEvent::Cancelled { job_id }
        }
        OutputJob::Paste { kind, .. } => {
            let result = match kind {
                PasteKind::LastTranscript => last_transcript
                    .as_deref()
                    .ok_or_else(|| "no previous transcript is available".to_string())
                    .and_then(|text| {
                        paste(text, PasteMode::Continue, &|| true)
                            .map_err(|error| error.to_string())?;
                        Ok(text.to_string())
                    }),
                PasteKind::MeetingDelta => {
                    paste_meeting_delta(paste).map_err(|error| error.to_string())
                }
            };
            WorkerEvent::Pasted { kind, result }
        }
    }
}

/// Record one successfully pasted result. History failures must never fail
/// the paste that already happened.
fn record_history(
    history: &History,
    target: TranscriptionTarget,
    completed: &mut CompletedTranscript,
) {
    let kind = match target {
        TranscriptionTarget::Paste | TranscriptionTarget::CopyOnlyPaste => HistoryKind::Dictation,
        TranscriptionTarget::RawPaste | TranscriptionTarget::RawCopyOnlyPaste => HistoryKind::Raw,
        TranscriptionTarget::Print | TranscriptionTarget::RawPrint => HistoryKind::Printed,
        TranscriptionTarget::Service => return,
    };
    let draft = HistoryDraft {
        kind,
        raw_text: completed.raw.clone(),
        final_text: completed.text.clone(),
        application: completed.application.clone(),
        processing: completed.processing.clone(),
        audio_ms: completed.timings.audio_ms,
        inference_ms: completed.timings.inference_ms as u64,
        total_ms: completed.timings.total_started.elapsed().as_millis() as u64,
        recording: completed.recording.take(),
        model: crate::history::dictation_model(),
    };
    if let Err(error) = history.record(draft) {
        tracing::warn!(%error, "could not record dictation history");
    }
}

fn paste_meeting_delta(
    paste: &mut impl FnMut(&str, PasteMode, &dyn Fn() -> bool) -> Result<()>,
) -> Result<String> {
    meeting::consume_active_or_latest_delta(|text| paste(text, PasteMode::Standalone, &|| true))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::meeting::{MeetingSource, TranscriptEntry, format_meeting_delta};

    fn processing_job(target: TranscriptionTarget) -> ProcessorJob {
        ProcessorJob {
            job_id: DictationJobId(0),
            control: Arc::new(JobControl::default()),
            target,
            text: "raw instruction".into(),
            context: ContextSnapshot::default(),
            return_to: None,
            language: "en".into(),
            timings: JobTimings {
                total_started: Instant::now(),
                queue_ms: 0,
                audio_ms: 1000,
                prepare_ms: 0,
                inference_ms: 0,
            },
            recording: None,
        }
    }

    #[test]
    fn ordinary_processing_snapshots_modes_before_stage_and_keeps_transformations() {
        use crate::dictation::processor::{Profile, Profiles};
        use std::cell::Cell;

        for transformations in [vec![], vec!["fixture-transform".to_string()]] {
            let loaded = Cell::new(false);
            let stage_sent = Cell::new(false);
            let processed = process_job_text(
                &processing_job(TranscriptionTarget::Paste),
                || {
                    assert!(!stage_sent.get());
                    loaded.set(true);
                    Profiles::new(
                        Profile::new("Global", "").transformations(transformations.clone()),
                    )
                },
                || {
                    assert!(loaded.get());
                    stage_sent.set(true);
                },
            );
            assert!(loaded.get());
            assert_eq!(stage_sent.get(), !transformations.is_empty());
            assert_eq!(processed.text, "raw instruction");
            assert_eq!(processed.transformations, transformations);
        }
    }

    #[test]
    fn meeting_delta_coalesces_turns_in_time_order() {
        let entry = |source, start_ms, end_ms, text: &str| TranscriptEntry {
            source,
            start_ms,
            end_ms,
            text: text.into(),
        };
        let entries = [
            entry(MeetingSource::Microphone, 0, 1_000, "First."),
            entry(MeetingSource::Microphone, 1_100, 1_400, "Still me."),
            entry(MeetingSource::System, 1_500, 2_500, "Reply."),
            entry(MeetingSource::Microphone, 2_600, 3_000, "Second."),
        ];
        assert_eq!(
            format_meeting_delta(&entries).unwrap(),
            "You: First. Still me.\n\nThem: Reply.\n\nYou: Second."
        );
        assert!(format_meeting_delta(&[]).is_err());
    }

    #[test]
    fn a_salvaged_dictation_is_copied_and_kept_for_paste_last_and_history() {
        // A normal salvage is processed before it is copied; a raw one is
        // copied as heard and recorded as raw history. Neither is pasted.
        for (target, kind) in [
            (TranscriptionTarget::CopyOnlyPaste, HistoryKind::Dictation),
            (TranscriptionTarget::RawCopyOnlyPaste, HistoryKind::Raw),
        ] {
            let directory = std::env::temp_dir().join(format!(
                "hex-parakeet-salvage-{}-{target:?}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&directory);
            std::fs::create_dir_all(&directory).unwrap();
            let history = History::new(crate::history::HistoryStore::open(
                directory.join("history.json"),
                crate::history::HistoryRetention::Week,
                crate::history::now_ms(),
            ));
            let text = "the part of the dictation the microphone heard";
            let mut pasted = None;
            let mut last_transcript = None;
            let event = finish_output(
                OutputJob::Completed {
                    job_id: DictationJobId(1),
                    control: Arc::new(JobControl::default()),
                    target,
                    result: Box::new(Ok(CompletedTranscript {
                        text: text.into(),
                        raw: text.into(),
                        application: Some("Call".into()),
                        return_to: None,
                        timings: JobTimings {
                            total_started: Instant::now(),
                            queue_ms: 0,
                            audio_ms: 4_000,
                            prepare_ms: 0,
                            inference_ms: 0,
                        },
                        processing: None,
                        recording: None,
                    })),
                },
                &mut |pasted_text, mode, commit| {
                    assert!(commit());
                    pasted = Some((pasted_text.to_string(), mode));
                    Ok(())
                },
                &mut last_transcript,
                Some(&history),
            );

            assert!(matches!(
                event,
                WorkerEvent::Completed {
                    result: Ok(ref completed),
                    ..
                } if completed.as_str() == text
            ));
            let (pasted_text, mode) = pasted.expect("the transcript reaches the clipboard");
            assert_eq!(pasted_text, text);
            assert!(
                matches!(mode, PasteMode::CopyOnly),
                "{target:?}: an interrupted dictation is copied, never pasted"
            );
            assert_eq!(last_transcript.as_deref(), Some(text));
            let entries = history.recent(1);
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].kind, kind, "{target:?}");
            assert_eq!(entries[0].final_text, text);
            assert_eq!(entries[0].application.as_deref(), Some("Call"));
            let _ = std::fs::remove_dir_all(&directory);
        }
    }

    #[test]
    fn parallel_processing_results_are_released_in_submission_order() {
        let mut outputs = OrderedOutputs::default();
        let job = |sequence| OutputJob::Paste {
            sequence,
            kind: PasteKind::LastTranscript,
        };

        assert!(outputs.push(job(1)).is_empty());
        assert_eq!(
            outputs
                .push(job(0))
                .into_iter()
                .map(|job| job.sequence())
                .collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(
            outputs
                .push(job(2))
                .into_iter()
                .map(|job| job.sequence())
                .collect::<Vec<_>>(),
            [2]
        );
    }

    #[test]
    fn ordered_waiting_outputs_remain_bounded_after_channel_drain() {
        let (output_sender, output_receiver) = mpsc::sync_channel(1);
        let (event_sender, events) = mpsc::channel();
        let worker = DictationWorker {
            engine: None,
            inference_jobs: None,
            output_jobs: Some(output_sender),
            events,
            event_sender,
            state: Arc::new(Mutex::new(WorkerState {
                next_sequence: 1,
                jobs: BTreeMap::from([(DictationJobId(0), Arc::new(JobControl::default()))]),
                pending_pastes: 0,
                return_to: None,
            })),
            inference_worker: None,
            processor_workers: Vec::new(),
            output_worker: None,
        };
        let mut ordered = OrderedOutputs::default();
        for _ in 1..MAX_PENDING_OUTPUTS {
            worker.paste_last().unwrap();
            assert!(ordered.push(output_receiver.try_recv().unwrap()).is_empty());
        }
        assert_eq!(worker.paste_last(), Err("dictation queue is full"));
        assert_eq!(worker.paste_meeting(), Err("dictation queue is full"));
        assert_eq!(ordered.waiting.len(), MAX_PENDING_OUTPUTS - 1);
        assert_eq!(
            worker.state.lock().unwrap().next_sequence,
            MAX_PENDING_OUTPUTS as u64
        );

        worker.state.lock().unwrap().jobs.remove(&DictationJobId(0));
        worker.paste_last().unwrap();
        assert_eq!(
            output_receiver.try_recv().unwrap().sequence(),
            MAX_PENDING_OUTPUTS as u64
        );
    }

    #[test]
    fn cancellation_replaces_waiting_output_and_ignores_late_completion() {
        let mut outputs = OrderedOutputs::default();
        let completed = |sequence| OutputJob::Completed {
            job_id: DictationJobId(sequence),
            control: Arc::new(JobControl::default()),
            target: TranscriptionTarget::Paste,
            result: Box::new(Err("late result".into())),
        };

        assert!(outputs.push(completed(1)).is_empty());
        assert!(
            outputs
                .push(OutputJob::Cancelled {
                    job_id: DictationJobId(1),
                })
                .is_empty()
        );
        let ready = outputs.push(OutputJob::Paste {
            sequence: 0,
            kind: PasteKind::LastTranscript,
        });
        assert_eq!(ready.len(), 2);
        assert!(matches!(ready[1], OutputJob::Cancelled { .. }));
        assert!(outputs.push(completed(1)).is_empty());
    }

    #[cfg(hex_capture)]
    #[test]
    fn chunked_word_transcription_keeps_word_times_across_chunks() {
        use crate::transcription::{Transcript, TranscriptSegment};
        let span = |start_ms, end_ms, text: &str| TranscriptSegment {
            start_ms,
            end_ms,
            text: text.into(),
        };
        let mut calls = Vec::new();
        let merged = transcribe_in_chunks(vec![0.0; 16_000 * 25], 16_000 * 10, |chunk| {
            calls.push(chunk.len());
            let chunk_index = calls.len();
            Ok(Transcript {
                text: format!(" part {chunk_index} "),
                segments: vec![span(0, 900, " part ")],
                words: vec![
                    span(-5, 400, " part"),
                    span(450, 300, "two"),
                    span(500, 600, "  "),
                ],
            })
        })
        .unwrap();
        assert_eq!(calls, [160_000, 160_000, 80_000]);
        assert_eq!(merged.text, "part 1 part 2 part 3");
        let words = merged
            .words
            .iter()
            .map(|word| (word.start_ms, word.end_ms, word.text.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            words,
            [
                (0, 400, "part"),
                (450, 450, "two"),
                (10_000, 10_400, "part"),
                (10_450, 10_450, "two"),
                (20_000, 20_400, "part"),
                (20_450, 20_450, "two"),
            ]
        );
        assert_eq!(merged.segments[2].start_ms, 20_000);
    }

    #[cfg(hex_capture)]
    #[test]
    fn word_transcription_is_a_foreground_request_on_the_shared_engine() {
        use crate::transcription::engine::Command as EngineCommand;
        let (engine, commands) = TranscriptionEngine::for_test();
        let engine = Arc::new(engine);
        let weak = Arc::downgrade(&engine);
        assert!(
            transcribe_with_words(&weak, &Default::default(), Vec::new())
                .unwrap()
                .words
                .is_empty()
        );
        assert!(
            commands.try_recv().is_err(),
            "silence never reaches the model"
        );

        // A busy dictation holds the chunk back until it is idle.
        let busy = DictationInference::begin();
        let request = {
            let weak = weak.clone();
            thread::spawn(move || {
                transcribe_with_words(
                    &weak,
                    &Default::default(),
                    vec![0.1; WORD_CHUNK_SAMPLES + 1_600],
                )
            })
        };
        assert!(
            commands
                .recv_timeout(std::time::Duration::from_millis(200))
                .is_err()
        );
        drop(busy);
        let command = commands
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        // A foreground request: after dictation already sent, counted as a
        // request so meeting chunks yield to it, and never inside a stream.
        assert!(matches!(
            command,
            EngineCommand::Foreground { ref clip, .. } if clip.len() == WORD_CHUNK_SAMPLES
        ));
        // The next chunk waits for this one, so a dictation sent meanwhile
        // runs first.
        assert!(
            commands
                .recv_timeout(std::time::Duration::from_millis(100))
                .is_err()
        );
        drop(command);
        assert!(request.join().unwrap().is_err());
        // Only a weak reference is held: once the engine is gone, a capture
        // fails instead of keeping it alive.
        drop(engine);
        assert!(transcribe_with_words(&weak, &Default::default(), vec![0.1; 16]).is_err());
    }
}
