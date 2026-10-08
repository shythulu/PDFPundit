//! Job runner over the `Engine` trait (T-02b types, T-15 runner). Wall-clock
//! drives progress display and cancellation here only, never an artefact.
//!
//! The UI thread owns the app state, the single `Receiver<AppEvent>` and the
//! [`JobRunner`]; job threads return data only by sending
//! `AppEvent::Job(id, event)` (TD §3), and the UI hands every job event back to
//! [`JobRunner::on_job_event`] so the runner can move jobs along.
#![allow(clippy::disallowed_types)]
// The shell (T-23a) is the runner's only caller, and it has not landed.
// TODO(T-23a): remove this allow once the shell drives the runner.
#![allow(dead_code)]

use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::num::NonZeroUsize;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::engine::{
    AnalysisResult, AnalysisStateUse, AnalyzeOptions, Cancelled, CorruptionClass, Engine, FileMeta,
    Finding, FontDb, FontSlot, Interact, InteractionReply, InteractionRequest, LogLevel,
    OutcomeStatus, Progress, RepairOptions, RepairReport, StateHandle,
};
use crate::panic_guard;
use crate::place::{self, BatchInputs, READ_ONLY_DESTINATION, TempFile};

/// A job's id, unique for the life of the runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JobId(pub u64);

/// What a job does. A Repair job receives the Analyze job's state handle, so
/// the engine never re-salvages on the normal path (eng-r2-q2).
#[derive(Debug, Clone, PartialEq)]
pub enum JobKind {
    Analyze,
    Repair {
        /// `None`: the planner decides.
        passes: Option<Vec<CorruptionClass>>,
        state: StateHandle,
    },
    ExportMarkdown,
}

/// What the UI's event source yields. `I` is the terminal input event, which
/// the input layer defines (T-23a/b); the runner never looks at it.
#[derive(Debug)]
pub enum AppEvent<I> {
    Input(I),
    Job(JobId, JobEvent),
    Tick,
}

/// What a job reports, in order (TD §3).
#[derive(Debug, Clone)]
pub enum JobEvent {
    Started {
        kind: JobKind,
        /// The file name, as shown.
        name: String,
        /// `None` for an input with no durable path (D-039), as in
        /// [`QueueEntry::path`].
        file: Option<PathBuf>,
    },
    /// "carving", "diagnosing C4", …
    Phase {
        name: &'static str,
        index: u32,
        total: u32,
    },
    Progress {
        done: u64,
        total: Option<u64>,
    },
    /// Streamed as discovered.
    Finding(Finding),
    Log(LogLevel, String),
    /// The job is blocked on a question. Answer it only through
    /// [`JobRunner::reply`], never by sending on `reply` yourself: the runner
    /// keeps its own clone of the sender, records each answer for replay after
    /// an eviction, and counts the job as parked until `JobRunner::reply`
    /// says otherwise. An answer sent straight on `reply` bypasses all of that.
    /// The runner would still count the job in `WaitingForYou`, and it could
    /// later evict a job that is really repairing; that worker would end
    /// silently and leave its row on "Repairing".
    ///
    /// Pass the event to [`JobRunner::on_job_event`], copy the `request` into
    /// the row, and drop the event. The job is cancelled when every clone of
    /// `reply` is gone without an answer (TD:219-221), so a clone kept in UI
    /// state stops the runner from ending the wait (on eviction or quit).
    NeedsInteraction {
        request: InteractionRequest,
        reply: SyncSender<InteractionReply>,
    },
    AnalyzeDone(Box<AnalysisResult>),
    RepairDone(Box<RepairRun>),
    ExportDone {
        path: PathBuf,
    },
    /// The job is waiting on the user and the next job has started.
    Parked,
    Resumed,
    /// A parked job's analysis state was dropped to stay under the memory cap
    /// (D-005); it re-analyses on reply.
    Evicted {
        id: JobId,
    },
    /// Status only, for the "waiting for you on N files" line; never a stop.
    WaitingForYou {
        parked: usize,
    },
    Failed {
        error: String,
        panicked: bool,
    },
    Cancelled,
}

/// A finished repair: the report and the run facts the run record keeps
/// beside it (D-073).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairRun {
    pub report: RepairReport,
    pub status: OutcomeStatus,
    pub output_path: Option<PathBuf>,
    pub placed: Option<Placed>,
    pub analysis_state: AnalysisStateUse,
}

/// One dropped file and where it is in the batch: the TD §7 view data
/// (`path, status, meta, findings, font_resolutions, job`).
///
/// The entry never holds the analysis state: the runner keeps the
/// [`StateHandle`] beside the job, so evicting a parked job's state (D-005)
/// never touches what the UI shows. `meta`, `findings` and `font_resolutions`
/// are copied out of the [`AnalysisResult`] and survive eviction.
#[derive(Debug, Clone, PartialEq)]
pub struct QueueEntry {
    /// The entry's current or last job.
    pub job: JobId,
    /// The file name, as shown.
    pub name: String,
    /// `None` when the input has no durable path (a file promise read into
    /// memory, D-039).
    pub path: Option<PathBuf>,
    pub bytes: u64,
    /// The TD's `status` (NotStarted | InProgress | Done | Error), split finer.
    pub state: EntryState,
    /// `AnalysisResult.meta`; `None` until the analysis finishes.
    pub meta: Option<FileMeta>,
    /// Streamed by [`JobEvent::Finding`] during analysis, then replaced by
    /// `AnalysisResult.findings` on [`JobEvent::AnalyzeDone`]. A repair's
    /// before and after lists are in `run`'s report.
    pub findings: Vec<Finding>,
    /// `AnalysisResult.font_slots`: each damaged slot with its resolution, if
    /// one is known.
    pub font_resolutions: Vec<FontSlot>,
    /// The finished repair, from [`JobEvent::RepairDone`]: the outcome, the
    /// report (whose `c9_summary` gives the C9 count line) and the run facts.
    pub run: Option<RepairRun>,
}

impl QueueEntry {
    /// A dropped file before any job has run on it.
    pub fn queued(job: JobId, name: String, path: Option<PathBuf>, bytes: u64) -> Self {
        QueueEntry {
            job,
            name,
            path,
            bytes,
            state: EntryState::Queued,
            meta: None,
            findings: Vec::new(),
            font_resolutions: Vec::new(),
            run: None,
        }
    }

    /// The repair's outcome, once there is one.
    pub fn outcome(&self) -> Option<&OutcomeStatus> {
        self.run.as_ref().map(|run| &run.status)
    }
}

/// Where an entry is. The in-progress states carry the job's latest
/// [`JobEvent::Progress`] (`done` of `total`, in the job's own units).
#[derive(Debug, Clone, PartialEq)]
pub enum EntryState {
    Queued,
    Analyzing {
        phase: Option<&'static str>,
        done: u64,
        total: Option<u64>,
    },
    Repairing {
        phase: Option<&'static str>,
        done: u64,
        total: Option<u64>,
    },
    Exporting {
        done: u64,
        total: Option<u64>,
    },
    /// Parked on a question; the batch carries on (GG §1). The question stays
    /// here when the job's state is evicted.
    WaitingOnUser(InteractionRequest),
    /// The job finished; the repair's outcome is `QueueEntry::run`.
    Done,
    Failed {
        error: String,
        panicked: bool,
    },
    Cancelled,
}

impl EntryState {
    /// `(done, total)` while a job is working on the entry.
    pub fn progress(&self) -> Option<(u64, Option<u64>)> {
        match self {
            EntryState::Analyzing { done, total, .. }
            | EntryState::Repairing { done, total, .. }
            | EntryState::Exporting { done, total } => Some((*done, *total)),
            EntryState::Queued
            | EntryState::WaitingOnUser(_)
            | EntryState::Done
            | EntryState::Failed { .. }
            | EntryState::Cancelled => None,
        }
    }

    /// Done, failed or cancelled: nothing more will happen to the entry
    /// unless the user starts it again.
    pub fn is_finished(&self) -> bool {
        matches!(
            self,
            EntryState::Done | EntryState::Failed { .. } | EntryState::Cancelled
        )
    }
}

/// The batch, in drop order: the TD's `BatchState { current,
/// current_progress, completed, total }`. `current` is stored, because only the
/// runner knows which job is active when a resumed parked job runs beside it;
/// the other three are derived from `entries` (see the methods), so they can
/// never disagree with the rows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BatchState {
    pub entries: Vec<QueueEntry>,
    /// Index into `entries` of the runner's active job; `None` when idle.
    pub current: Option<usize>,
}

impl BatchState {
    pub fn current_entry(&self) -> Option<&QueueEntry> {
        self.current.and_then(|i| self.entries.get(i))
    }

    /// The current entry's `(done, total)`.
    pub fn current_progress(&self) -> Option<(u64, Option<u64>)> {
        self.current_entry().and_then(|e| e.state.progress())
    }

    /// Entries that are done, failed or cancelled.
    pub fn completed(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.state.is_finished())
            .count()
    }

    pub fn total(&self) -> usize {
        self.entries.len()
    }
}

/// Shared cancellation flag, polled by every job loop.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    pub fn check(&self) -> Result<(), Cancelled> {
        if self.is_cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Which no-replace guarantee held when an output was placed (D-044). A run
/// fact: it depends on the destination filesystem, so it is in the run record,
/// never the report (D-073).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Placed {
    /// A no-replace rename.
    Atomic,
    /// A hard link, then the temp file removed.
    Linked,
    /// The name claimed with `create_new`, then our placeholder replaced.
    ClaimedThenRenamed,
}

// ── the runner (T-15) ────────────────────────────────────────────────────

/// What a dropped file gives the runner: a path, or bytes with no durable path
/// (a macOS file promise read into memory, D-039).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobInput {
    File(PathBuf),
    Memory { name: String, bytes: Vec<u8> },
}

/// How the runner runs its jobs.
#[derive(Debug, Clone)]
pub struct RunnerOptions {
    pub analyze: AnalyzeOptions,
    pub repair: RepairOptions,
    pub fonts: Arc<FontDb>,
    /// `[general] output_dir`; `None` writes beside the input (D-061).
    pub output_dir: Option<PathBuf>,
    /// Bytes parked jobs may hold (their inputs plus their analysis states)
    /// before the runner reclaims memory from them (D-005). Reaching it never
    /// refuses or delays a job.
    pub parked_cap: u64,
}

/// The default [`RunnerOptions::parked_cap`]: 1 GiB.
pub const PARKED_CAP: u64 = 1 << 30;

impl Default for RunnerOptions {
    fn default() -> Self {
        // `threads` and `scratch_cap` change wall-clock and peak memory only;
        // they reach progress events, never an artefact (D-066, D-072).
        let analyze = AnalyzeOptions {
            threads: salvage_threads(),
            scratch_cap: 1 << 30,
            ..AnalyzeOptions::default()
        };
        RunnerOptions {
            repair: RepairOptions {
                analyze: analyze.clone(),
                ..RepairOptions::default()
            },
            analyze,
            fonts: FontDb::bundled(),
            output_dir: None,
            parked_cap: PARKED_CAP,
        }
    }
}

/// The thread count for `salvage_all`: `available_parallelism()` clamped to
/// 1..=16 (D-066).
pub fn salvage_threads() -> NonZeroUsize {
    let n = thread::available_parallelism()
        .map_or(1, NonZeroUsize::get)
        .clamp(1, 16);
    NonZeroUsize::new(n).unwrap_or(NonZeroUsize::MIN)
}

/// Why a parked job that was evicted re-analyses on reply; the run record
/// carries it (D-073).
pub const EVICTED_WHILE_PARKED: &str = "evicted while parked";
/// How a parked job ends when its released input no longer hashes the same.
pub const INPUT_CHANGED: &str = "input changed while waiting";

type Emit = Arc<dyn Fn(JobId, JobEvent) + Send + Sync>;
/// A job's input bytes, shared by the runner (which may release them, D-005)
/// and the job's worker (which fills the slot once it has read the file).
type InputSlot = Arc<Mutex<Option<Arc<Vec<u8>>>>>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs each dropped file through Analyze then Repair, one file at a time.
///
/// A job that asks a question parks: its worker thread stays blocked on the
/// question and the next queued file starts at once, so questions never stop
/// the batch (GG §1). Answer a question only with [`JobRunner::reply`], never
/// on the [`JobEvent::NeedsInteraction`] event's own sender (see there).
///
/// A reply resumes the parked job immediately, beside the active one. Resumed
/// jobs are not limited in number: each one starts when the user answers it,
/// and the user answers one question at a time, so they overlap the active job
/// only as fast as someone types. Queueing them instead would turn "a reply
/// resumes immediately" into "a reply waits for an earlier reply's job". A
/// resumed job counts against `parked_cap` again only if it parks again.
///
/// When parked jobs hold more than `parked_cap` bytes the runner
/// reclaims memory, never refusing a job (D-005, the recommended default): it
/// first evicts parked jobs' analysis states, cheapest to rebuild first
/// (lowest `salvage_work_total`), cancelling their threads; then, if their
/// inputs alone are still over the cap, it releases the bytes of inputs it can
/// re-read, largest first. An evicted job re-runs on reply with every earlier
/// reply replayed.
///
/// One [`JobId`] covers a file's whole pipeline, re-runs included.
/// `AnalyzeDone` and `Started { kind: Repair { state } }` carry the analysis
/// state's handle: a UI that keeps those events keeps the state alive after
/// the runner evicts it, so copy what the row needs and drop them.
pub struct JobRunner<E: Engine> {
    engine: Arc<E>,
    emit: Emit,
    opts: Arc<RunnerOptions>,
    inputs: Arc<Mutex<BatchInputs>>,
    jobs: BTreeMap<JobId, Job>,
    pending: VecDeque<JobId>,
    /// The one job the sequential batch is running; resumed jobs run beside it.
    active: Option<JobId>,
    /// Threads that were cancelled or finished, kept for `shutdown` to join.
    retired: Vec<JoinHandle<()>>,
    next_id: u64,
    /// The parked count last sent as `WaitingForYou`.
    waiting_reported: usize,
}

struct Job {
    name: String,
    path: Option<PathBuf>,
    input: InputSlot,
    input_sha256: Option<[u8; 32]>,
    /// The runner's own handle on the analysis state, dropped on eviction.
    state: StateHandle,
    salvage_work_total: u64,
    /// Every reply the user gave this entry, in order (replayed on a re-run).
    replies: Vec<InteractionReply>,
    /// True once the state was evicted: the run record says so.
    rebuilt: bool,
    /// The live worker; `None` before it starts and after eviction.
    run: Option<Run>,
    stage: Stage,
}

enum Stage {
    Pending,
    Running,
    Parked {
        request: InteractionRequest,
        reply: Option<SyncSender<InteractionReply>>,
    },
    /// Cancelled; waiting for the worker to report `Cancelled`.
    Stopping,
}

struct Run {
    cancel: CancelToken,
    /// Set on eviction: the worker ends without a word, so the row keeps its
    /// question.
    quiet: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl Job {
    fn is_parked(&self) -> bool {
        matches!(self.stage, Stage::Parked { .. })
    }

    fn held_input(&self) -> u64 {
        lock(&self.input).as_ref().map_or(0, |b| b.len() as u64)
    }

    fn state_bytes(&self) -> u64 {
        self.state.0.as_ref().map_or(0, |s| s.heap_bytes())
    }
}

impl<E: Engine + 'static> JobRunner<E> {
    /// A runner that reports on `tx`, the UI's merged channel.
    pub fn new<I: Send + 'static>(
        engine: Arc<E>,
        tx: Sender<AppEvent<I>>,
        opts: RunnerOptions,
    ) -> Self {
        let emit: Emit = Arc::new(move |id, event| {
            // A closed channel means the UI has gone; the job's own
            // cancellation (the reply channel, the token) ends it.
            let _ = tx.send(AppEvent::Job(id, event));
        });
        JobRunner {
            engine,
            emit,
            opts: Arc::new(opts),
            inputs: Arc::new(Mutex::new(BatchInputs::new())),
            jobs: BTreeMap::new(),
            pending: VecDeque::new(),
            active: None,
            retired: Vec::new(),
            next_id: 1,
            waiting_reported: 0,
        }
    }

    /// Queues a file for Analyze then Repair (the autonomous flow, GG §1) and
    /// starts it when nothing else is running.
    pub fn submit(&mut self, input: JobInput) -> JobId {
        let id = JobId(self.next_id);
        self.next_id += 1;
        let (name, path, bytes) = match input {
            JobInput::File(path) => {
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                (name, Some(path), None)
            }
            JobInput::Memory { name, bytes } => (name, None, Some(Arc::new(bytes))),
        };
        if let Some(path) = &path {
            lock(&self.inputs).insert(path);
        }
        self.jobs.insert(
            id,
            Job {
                name,
                path,
                input: Arc::new(Mutex::new(bytes)),
                input_sha256: None,
                state: StateHandle::default(),
                salvage_work_total: 0,
                replies: Vec::new(),
                rebuilt: false,
                run: None,
                stage: Stage::Pending,
            },
        );
        self.pending.push_back(id);
        self.start_next();
        id
    }

    /// Cancels a job: a queued one leaves the queue at once; a running or
    /// parked one stops at its next check and reports `Cancelled`.
    pub fn cancel(&mut self, id: JobId) {
        let Some(job) = self.jobs.get_mut(&id) else {
            return;
        };
        match job.run.as_ref() {
            Some(run) => {
                run.cancel.cancel();
                // Dropping a parked job's reply sender ends its wait now.
                job.stage = Stage::Stopping;
            }
            None => {
                self.jobs.remove(&id);
                self.pending.retain(|p| *p != id);
                self.send(id, JobEvent::Cancelled);
            }
        }
        self.report_waiting(id);
    }

    /// Answers a parked job's question. A job whose thread is still waiting
    /// gets the reply at once; an evicted one re-runs Analyze and Repair with
    /// every reply so far replayed. False when the job is not parked.
    pub fn reply(&mut self, id: JobId, reply: InteractionReply) -> bool {
        let Some(job) = self.jobs.get_mut(&id) else {
            return false;
        };
        let Stage::Parked { reply: sender, .. } = &mut job.stage else {
            return false;
        };
        let sender = sender.take();
        job.replies.push(reply.clone());
        job.stage = Stage::Running;
        let delivered = job.run.is_some() && sender.is_some_and(|tx| tx.try_send(reply).is_ok());
        if !delivered {
            if let Some(run) = job.run.take() {
                run.quiet.store(true, Ordering::Release);
                run.cancel.cancel();
                self.retired.push(run.thread);
            }
            job.rebuilt = true;
            job.state = StateHandle::default();
            self.spawn(id);
        }
        self.send(id, JobEvent::Resumed);
        self.report_waiting(id);
        true
    }

    /// The runner's half of the event loop: the UI passes it every job event
    /// it receives, in order.
    pub fn on_job_event(&mut self, id: JobId, event: &JobEvent) {
        match event {
            JobEvent::AnalyzeDone(result) => {
                if let Some(job) = self.jobs.get_mut(&id) {
                    job.state = result.state.clone();
                    job.salvage_work_total = result.stats.salvage_work_total;
                    job.input_sha256 = Some(result.input_sha256);
                }
            }
            JobEvent::NeedsInteraction { request, reply } => {
                self.park(id, request.clone(), reply.clone());
            }
            JobEvent::RepairDone(_)
            | JobEvent::ExportDone { .. }
            | JobEvent::Failed { .. }
            | JobEvent::Cancelled => self.finish(id),
            JobEvent::Started { .. }
            | JobEvent::Phase { .. }
            | JobEvent::Progress { .. }
            | JobEvent::Finding(_)
            | JobEvent::Log(..)
            | JobEvent::Parked
            | JobEvent::Resumed
            | JobEvent::Evicted { .. }
            | JobEvent::WaitingForYou { .. } => {}
        }
    }

    /// Parked jobs and their questions, in id order.
    pub fn parked(&self) -> impl Iterator<Item = (JobId, &InteractionRequest)> {
        self.jobs.iter().filter_map(|(id, job)| match &job.stage {
            Stage::Parked { request, .. } => Some((*id, request)),
            _ => None,
        })
    }

    /// Bytes the parked jobs hold now, as the cap counts them: each one's
    /// input, if held, plus its analysis state's heap while it has one.
    pub fn retained_bytes(&self) -> u64 {
        self.jobs
            .values()
            .filter(|job| job.is_parked())
            .map(|job| {
                let state = if job.run.is_some() {
                    job.state_bytes()
                } else {
                    0
                };
                job.held_input() + state
            })
            .sum()
    }

    /// Cancels everything and waits up to `grace` for the workers to stop.
    /// True when every worker stopped in time.
    pub fn shutdown(mut self, grace: Duration) -> bool {
        let deadline = Instant::now() + grace;
        self.pending.clear();
        let mut threads = std::mem::take(&mut self.retired);
        for job in self.jobs.values_mut() {
            if let Some(run) = job.run.take() {
                run.cancel.cancel();
                threads.push(run.thread);
            }
            job.stage = Stage::Stopping;
        }
        while threads.iter().any(|t| !t.is_finished()) {
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(5));
        }
        for t in threads {
            let _ = t.join();
        }
        true
    }

    fn send(&self, id: JobId, event: JobEvent) {
        (self.emit)(id, event);
    }

    /// Waits for every retired worker (cancelled or finished) to end, so a
    /// test sees the files an evicted worker removes on its way out.
    #[cfg(test)]
    pub(crate) fn join_retired(&mut self) {
        for t in self.retired.drain(..) {
            let _ = t.join();
        }
    }

    fn start_next(&mut self) {
        if self.active.is_some() {
            return;
        }
        while let Some(id) = self.pending.pop_front() {
            if self.jobs.contains_key(&id) {
                self.active = Some(id);
                self.spawn(id);
                return;
            }
        }
    }

    fn spawn(&mut self, id: JobId) {
        let Some(job) = self.jobs.get_mut(&id) else {
            return;
        };
        let cancel = CancelToken::new();
        let quiet = Arc::new(AtomicBool::new(false));
        let worker = Worker {
            id,
            engine: Arc::clone(&self.engine),
            emit: Arc::clone(&self.emit),
            opts: Arc::clone(&self.opts),
            inputs: Arc::clone(&self.inputs),
            name: job.name.clone(),
            path: job.path.clone(),
            input: Arc::clone(&job.input),
            expect_sha256: job.input_sha256,
            replay: job.replies.iter().cloned().collect(),
            rebuilt: job.rebuilt,
            cancel: cancel.clone(),
            quiet: Arc::clone(&quiet),
        };
        job.stage = Stage::Running;
        match thread::Builder::new()
            .name(format!("job-{}", id.0))
            .spawn(move || worker.run())
        {
            Ok(thread) => {
                job.run = Some(Run {
                    cancel,
                    quiet,
                    thread,
                });
            }
            Err(e) => self.send(
                id,
                JobEvent::Failed {
                    error: format!("can't start a worker thread: {e}"),
                    panicked: false,
                },
            ),
        }
    }

    fn park(
        &mut self,
        id: JobId,
        request: InteractionRequest,
        reply: SyncSender<InteractionReply>,
    ) {
        let Some(job) = self.jobs.get_mut(&id) else {
            return;
        };
        if !matches!(job.stage, Stage::Running) {
            return;
        }
        job.stage = Stage::Parked {
            request,
            reply: Some(reply),
        };
        if self.active == Some(id) {
            self.active = None;
        }
        self.send(id, JobEvent::Parked);
        self.report_waiting(id);
        self.reclaim();
        self.start_next();
    }

    fn finish(&mut self, id: JobId) {
        if let Some(job) = self.jobs.remove(&id)
            && let Some(run) = job.run
        {
            self.retired.push(run.thread);
        }
        if self.active == Some(id) {
            self.active = None;
        }
        self.retired.retain(|t| !t.is_finished());
        self.report_waiting(id);
        self.start_next();
    }

    /// Brings the parked jobs back under `parked_cap` (D-005, lead-r4-fr1).
    /// This is the one branch D-005 decides: the alternative is to stop
    /// starting jobs here instead.
    fn reclaim(&mut self) {
        while self.retained_bytes() > self.opts.parked_cap {
            // (1) Evict a state, cheapest to rebuild first. A pinned input's
            // state goes too: it is the only reclaim such a job allows.
            let cheapest = self
                .jobs
                .iter()
                .filter(|(_, job)| job.is_parked() && job.run.is_some())
                .min_by_key(|(id, job)| (job.salvage_work_total, **id))
                .map(|(id, _)| *id);
            if let Some(id) = cheapest {
                self.evict(id);
                continue;
            }
            // (2) Release a re-readable input, largest first. The thread that
            // borrowed it was cancelled at step (1).
            let largest = self
                .jobs
                .iter()
                .filter(|(_, job)| job.is_parked() && job.path.is_some() && job.held_input() > 0)
                .max_by_key(|(id, job)| (job.held_input(), Reverse(**id)))
                .map(|(id, _)| *id);
            match largest {
                Some(id) => *lock(&self.jobs[&id].input) = None,
                // Only pinned inputs are left: they stay, and the batch goes on.
                None => return,
            }
        }
    }

    fn evict(&mut self, id: JobId) {
        let Some(job) = self.jobs.get_mut(&id) else {
            return;
        };
        if let Some(run) = job.run.take() {
            run.quiet.store(true, Ordering::Release);
            run.cancel.cancel();
            self.retired.push(run.thread);
        }
        if let Stage::Parked { reply, .. } = &mut job.stage {
            // The worker sees the channel close and ends at once.
            *reply = None;
        }
        job.state = StateHandle::default();
        self.send(id, JobEvent::Evicted { id });
    }

    fn report_waiting(&mut self, id: JobId) {
        let parked = self.jobs.values().filter(|job| job.is_parked()).count();
        if parked != self.waiting_reported {
            self.waiting_reported = parked;
            self.send(id, JobEvent::WaitingForYou { parked });
        }
    }
}

impl<E: Engine> Drop for JobRunner<E> {
    /// The UI is going: every worker stops at its next check, and a parked one
    /// at once, because its reply channel closes with the runner.
    fn drop(&mut self) {
        for job in self.jobs.values() {
            if let Some(run) = &job.run {
                run.cancel.cancel();
            }
        }
    }
}

/// How a pipeline stopped short.
enum Stop {
    Cancelled,
    Failed(String),
}

impl From<Cancelled> for Stop {
    fn from(_: Cancelled) -> Self {
        Stop::Cancelled
    }
}

/// One run of one file's pipeline, on its own thread.
struct Worker<E> {
    id: JobId,
    engine: Arc<E>,
    emit: Emit,
    opts: Arc<RunnerOptions>,
    inputs: Arc<Mutex<BatchInputs>>,
    name: String,
    path: Option<PathBuf>,
    input: InputSlot,
    /// The hash a re-read input must still have.
    expect_sha256: Option<[u8; 32]>,
    /// Replies answered without asking again (a re-run after eviction).
    replay: VecDeque<InteractionReply>,
    rebuilt: bool,
    cancel: CancelToken,
    quiet: Arc<AtomicBool>,
}

impl<E: Engine> Worker<E> {
    fn run(self) {
        // The panic hook hands this thread's panic message here, with its
        // location, and writes nothing to the terminal (D-051).
        let message: Rc<RefCell<Option<String>>> = Rc::default();
        let slot = Rc::clone(&message);
        panic_guard::set_job_sink(Box::new(move |msg| *slot.borrow_mut() = Some(msg)));

        let result = panic::catch_unwind(AssertUnwindSafe(|| self.pipeline()));
        if self.quiet.load(Ordering::Acquire) {
            return;
        }
        let end = match result {
            Ok(Ok(())) => return,
            Ok(Err(Stop::Cancelled)) => JobEvent::Cancelled,
            Ok(Err(Stop::Failed(error))) => JobEvent::Failed {
                error,
                panicked: false,
            },
            Err(payload) => {
                let error = message.borrow_mut().take().unwrap_or_else(|| {
                    format!("panicked: {}", panic_guard::payload_text(&*payload))
                });
                self.send(JobEvent::Log(LogLevel::Error, error.clone()));
                JobEvent::Failed {
                    error,
                    panicked: true,
                }
            }
        };
        self.send(end);
    }

    fn send(&self, event: JobEvent) {
        (self.emit)(self.id, event);
    }

    fn pipeline(&self) -> Result<(), Stop> {
        self.send(JobEvent::Started {
            kind: JobKind::Analyze,
            name: self.name.clone(),
            file: self.path.clone(),
        });
        let bytes = self.load()?;
        if bytes.len() as u64 > self.opts.analyze.max_file_bytes {
            self.send(JobEvent::Log(
                LogLevel::Warn,
                format!(
                    "{} is {} MiB and is held in memory while it is worked on",
                    self.name,
                    bytes.len() >> 20
                ),
            ));
        }
        let mut progress = JobProgress {
            id: self.id,
            emit: Arc::clone(&self.emit),
            cancel: self.cancel.clone(),
        };
        let analysis = self
            .engine
            .analyze(&bytes, &self.opts.analyze, &mut progress)?;
        self.send(JobEvent::AnalyzeDone(Box::new(analysis.clone())));

        // Repair receives the Analyze stage's state, so the engine never
        // re-salvages on the normal path (eng-r2-q2).
        self.send(JobEvent::Started {
            kind: JobKind::Repair {
                passes: self.opts.repair.passes.clone(),
                state: analysis.state.clone(),
            },
            name: self.name.clone(),
            file: self.path.clone(),
        });
        // The only write probe, before any engine work; the input is never
        // touched (D-061).
        let unwritable = |e: std::io::Error| {
            Stop::Failed(match &self.opts.output_dir {
                // The fixed message points at output_dir, so it only fits
                // when the examiner has not set one.
                None if place::is_unwritable(&e) => READ_ONLY_DESTINATION.to_owned(),
                None => format!("can't write to the destination: {e}"),
                Some(dir) => format!("can't write to the output_dir ({}): {e}", dir.display()),
            })
        };
        let dest = place::destination_for(self.path.as_deref(), self.opts.output_dir.as_deref())
            .map_err(unwritable)?;
        let temp = TempFile::create(&dest).map_err(unwritable)?;

        let plan = self.engine.plan(&analysis, &self.opts.repair);
        let mut ask = JobInteract {
            id: self.id,
            emit: Arc::clone(&self.emit),
            cancel: self.cancel.clone(),
            replay: self.replay.clone(),
        };
        let outcome = self.engine.repair(
            &bytes,
            &analysis,
            &plan,
            &self.opts.repair,
            &self.opts.fonts,
            &mut ask,
            &mut progress,
        )?;
        let (output_path, placed) = match &outcome.output {
            Some(output) => {
                let stem = format!("{}.repaired", file_stem(&self.name));
                let inputs = lock(&self.inputs).clone();
                let (path, placed) = temp
                    .place(&stem, "pdf", output, &inputs)
                    .map_err(|e| Stop::Failed(format!("can't write the repaired file: {e}")))?;
                (Some(path), Some(placed))
            }
            // Nothing to write: dropping the temp file removes it.
            None => (None, None),
        };
        let analysis_state = if self.rebuilt {
            AnalysisStateUse::Rebuilt {
                reason: EVICTED_WHILE_PARKED.to_owned(),
            }
        } else {
            outcome.analysis_state
        };
        self.send(JobEvent::RepairDone(Box::new(RepairRun {
            report: outcome.report,
            status: outcome.status,
            output_path,
            placed,
            analysis_state,
        })));
        Ok(())
    }

    /// The input bytes: held ones, or read from the file. A re-read after the
    /// runner released the bytes must hash as before (D-005 step 2).
    fn load(&self) -> Result<Arc<Vec<u8>>, Stop> {
        if let Some(bytes) = lock(&self.input).clone() {
            return Ok(bytes);
        }
        let Some(path) = &self.path else {
            return Err(Stop::Failed(format!(
                "{} is no longer in memory",
                self.name
            )));
        };
        let bytes =
            fs::read(path).map_err(|e| Stop::Failed(format!("can't read {}: {e}", self.name)))?;
        if let Some(expected) = self.expect_sha256
            && <[u8; 32]>::from(Sha256::digest(&bytes)) != expected
        {
            return Err(Stop::Failed(INPUT_CHANGED.to_owned()));
        }
        let bytes = Arc::new(bytes);
        *lock(&self.input) = Some(Arc::clone(&bytes));
        Ok(bytes)
    }
}

/// `memo` for `memo.pdf`; the name itself when it has no stem.
fn file_stem(name: &str) -> String {
    Path::new(name)
        .file_stem()
        .map_or_else(|| name.to_owned(), |s| s.to_string_lossy().into_owned())
}

/// Streams engine progress to the UI and reports the job's cancellation.
struct JobProgress {
    id: JobId,
    emit: Emit,
    cancel: CancelToken,
}

impl Progress for JobProgress {
    fn phase(&mut self, name: &'static str, index: u32, total: u32) {
        (self.emit)(self.id, JobEvent::Phase { name, index, total });
    }
    fn progress(&mut self, done: u64, total: Option<u64>) {
        (self.emit)(self.id, JobEvent::Progress { done, total });
    }
    fn finding(&mut self, f: &Finding) {
        (self.emit)(self.id, JobEvent::Finding(f.clone()));
    }
    fn log(&mut self, level: LogLevel, msg: String) {
        (self.emit)(self.id, JobEvent::Log(level, msg));
    }
    fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

/// The engine's questions go to the UI as `NeedsInteraction` and the worker
/// blocks for the answer, polling its token every 200 ms (TD:202-220). A
/// closed reply channel (the UI quit, or the job was evicted) is `Cancelled`.
struct JobInteract {
    id: JobId,
    emit: Emit,
    cancel: CancelToken,
    replay: VecDeque<InteractionReply>,
}

impl Interact for JobInteract {
    fn ask(&mut self, request: InteractionRequest) -> Result<InteractionReply, Cancelled> {
        if let Some(reply) = self.replay.pop_front() {
            return Ok(reply);
        }
        let (tx, rx) = mpsc::sync_channel(1);
        (self.emit)(self.id, JobEvent::NeedsInteraction { request, reply: tx });
        loop {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(reply) => return Ok(reply),
                Err(RecvTimeoutError::Timeout) => self.cancel.check()?,
                Err(RecvTimeoutError::Disconnected) => return Err(Cancelled),
            }
        }
    }
}

#[cfg(test)]
pub(crate) use fake::{FakeEngine, FakeProfile, FakeStep};

/// A scripted engine for the runner's tests (T-15): it replays steps instead of
/// reading PDFs.
#[cfg(test)]
mod fake {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use sha2::{Digest, Sha256};

    use crate::engine::*;

    /// One scripted step.
    #[derive(Debug, Clone)]
    pub(crate) enum FakeStep {
        Phase(&'static str, u32, u32),
        Finding(Finding),
        Log(LogLevel, String),
        /// Repair only: ask, and record the reply.
        Ask(InteractionRequest),
        /// `steps` progress ticks `pause_ms` apart; `Err(Cancelled)` as soon as
        /// the sink reports cancellation.
        Slow {
            steps: u64,
            pause_ms: u64,
        },
        /// Panic on the calling (worker) thread.
        Panic(String),
    }

    /// Per-input numbers the eviction order depends on.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub(crate) struct FakeProfile {
        pub salvage_work_total: u64,
        pub heap_bytes: u64,
    }

    type ProfileFn = dyn Fn(&[u8]) -> FakeProfile + Send + Sync;
    type PanicFn = dyn Fn(&[u8]) -> bool + Send + Sync;

    pub(crate) struct FakeEngine {
        analyze_steps: Vec<FakeStep>,
        repair_steps: Vec<FakeStep>,
        always_asks: bool,
        profile: Arc<ProfileFn>,
        panics_when: Arc<PanicFn>,
        replies: Mutex<Vec<InteractionReply>>,
        repaired_states: Mutex<Vec<StateHandle>>,
    }

    impl Default for FakeEngine {
        fn default() -> Self {
            FakeEngine {
                analyze_steps: Vec::new(),
                repair_steps: Vec::new(),
                always_asks: false,
                profile: Arc::new(|_| FakeProfile::default()),
                panics_when: Arc::new(|_| false),
                replies: Mutex::new(Vec::new()),
                repaired_states: Mutex::new(Vec::new()),
            }
        }
    }

    impl FakeEngine {
        pub(crate) fn new() -> Self {
            Self::default()
        }

        pub(crate) fn on_analyze(mut self, steps: Vec<FakeStep>) -> Self {
            self.analyze_steps = steps;
            self
        }

        pub(crate) fn on_repair(mut self, steps: Vec<FakeStep>) -> Self {
            self.repair_steps = steps;
            self
        }

        /// Every repair asks a font question after its scripted steps.
        pub(crate) fn always_asks(mut self) -> Self {
            self.always_asks = true;
            self
        }

        /// `salvage_work_total` and the state's `heap_bytes()` per input.
        pub(crate) fn profile(
            mut self,
            f: impl Fn(&[u8]) -> FakeProfile + Send + Sync + 'static,
        ) -> Self {
            self.profile = Arc::new(f);
            self
        }

        /// Analysis panics, on the calling thread, for inputs `f` picks.
        pub(crate) fn panics_when(
            mut self,
            f: impl Fn(&[u8]) -> bool + Send + Sync + 'static,
        ) -> Self {
            self.panics_when = Arc::new(f);
            self
        }

        /// The state handle each `repair` call received, in call order.
        pub(crate) fn repaired_states(&self) -> Vec<StateHandle> {
            self.repaired_states.lock().expect("states lock").clone()
        }

        /// Every reply the fake has received, in order.
        pub(crate) fn replies(&self) -> Vec<InteractionReply> {
            self.replies.lock().expect("replies lock").clone()
        }

        /// The question `always_asks` puts.
        pub(crate) fn standard_question() -> InteractionRequest {
            InteractionRequest::FontPick(FontPickRequest {
                id: InteractionRequestId(1),
                page: 0,
                slot: "F1".into(),
                sample_codes: Vec::new(),
                candidates: Vec::new(),
                preview: String::new(),
            })
        }

        fn run(
            &self,
            steps: &[FakeStep],
            sink: &mut dyn Progress,
            mut ask: Option<&mut dyn Interact>,
        ) -> Result<Vec<Finding>, Cancelled> {
            let mut findings = Vec::new();
            for step in steps {
                if sink.cancelled() {
                    return Err(Cancelled);
                }
                match step {
                    FakeStep::Phase(name, index, total) => sink.phase(name, *index, *total),
                    FakeStep::Finding(f) => {
                        sink.finding(f);
                        findings.push(f.clone());
                    }
                    FakeStep::Log(level, msg) => sink.log(*level, msg.clone()),
                    FakeStep::Ask(req) => {
                        let ask = ask
                            .as_deref_mut()
                            .expect("the fake asks only during repair");
                        self.ask(ask, req.clone())?;
                    }
                    FakeStep::Slow { steps, pause_ms } => {
                        for done in 1..=*steps {
                            std::thread::sleep(Duration::from_millis(*pause_ms));
                            if sink.cancelled() {
                                return Err(Cancelled);
                            }
                            sink.progress(done, Some(*steps));
                        }
                    }
                    FakeStep::Panic(msg) => panic!("{msg}"),
                }
            }
            Ok(findings)
        }

        fn ask(&self, ask: &mut dyn Interact, req: InteractionRequest) -> Result<(), Cancelled> {
            let reply = ask.ask(req)?;
            self.replies.lock().expect("replies lock").push(reply);
            Ok(())
        }
    }

    impl Engine for FakeEngine {
        fn analyze(
            &self,
            bytes: &[u8],
            _opts: &AnalyzeOptions,
            sink: &mut dyn Progress,
        ) -> Result<AnalysisResult, Cancelled> {
            if (self.panics_when)(bytes) {
                panic!("fake engine panic");
            }
            let findings = self.run(&self.analyze_steps, sink, None)?;
            let profile = (self.profile)(bytes);
            let input_sha256: [u8; 32] = Sha256::digest(bytes).into();
            let mut state = AnalysisState::new(
                Default::default(),
                Default::default(),
                Default::default(),
                input_sha256,
            );
            state.scripted_heap_bytes = profile.heap_bytes;
            Ok(AnalysisResult {
                meta: FileMeta::default(),
                findings,
                carve: CarveSummary::default(),
                font_slots: Vec::new(),
                stats: AnalyzeStats {
                    bytes: bytes.len() as u64,
                    salvage_work_total: profile.salvage_work_total,
                    ..AnalyzeStats::default()
                },
                input_sha256,
                state: StateHandle::new(state),
            })
        }

        fn plan(&self, _analysis: &AnalysisResult, _opts: &RepairOptions) -> RepairPlan {
            RepairPlan {
                candidates: vec![Toolpath::Resave],
                prior: Toolpath::Resave,
                escalations: Vec::new(),
            }
        }

        fn repair(
            &self,
            bytes: &[u8],
            analysis: &AnalysisResult,
            _plan: &RepairPlan,
            opts: &RepairOptions,
            fonts: &FontDb,
            ask: &mut dyn Interact,
            sink: &mut dyn Progress,
        ) -> Result<RepairOutcome, Cancelled> {
            self.repaired_states
                .lock()
                .expect("states lock")
                .push(analysis.state.clone());
            self.run(&self.repair_steps, sink, Some(&mut *ask))?;
            if self.always_asks {
                self.ask(ask, Self::standard_question())?;
            }
            let hash: [u8; 32] = Sha256::digest(bytes).into();
            let analysis_state = match &analysis.state.0 {
                Some(state) if state.input_sha256 == hash => AnalysisStateUse::Reused,
                Some(_) => AnalysisStateUse::Rebuilt {
                    reason: "input changed".into(),
                },
                None => AnalysisStateUse::Rebuilt {
                    reason: "no analysis state".into(),
                },
            };
            Ok(RepairOutcome {
                output: Some(bytes.to_vec()),
                report: RepairReport::default_for(analysis, opts, fonts),
                status: OutcomeStatus::Ok,
                images: Vec::new(),
                analysis_state,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::*;

    const fn send<T: Send>() {}
    const _: () = {
        send::<JobId>();
        send::<JobKind>();
        send::<JobEvent>();
        send::<AppEvent<()>>();
        send::<RepairRun>();
        send::<QueueEntry>();
        send::<BatchState>();
        send::<CancelToken>();
        send::<Placed>();
        send::<FakeEngine>();
    };

    /// Records every sink call as a line.
    #[derive(Default)]
    struct Recorder {
        lines: Vec<String>,
        cancel_after: Option<usize>,
    }

    impl Progress for Recorder {
        fn phase(&mut self, name: &'static str, index: u32, total: u32) {
            self.lines.push(format!("phase {name} {index}/{total}"));
        }
        fn progress(&mut self, done: u64, total: Option<u64>) {
            self.lines.push(format!("progress {done}/{total:?}"));
        }
        fn finding(&mut self, f: &Finding) {
            self.lines.push(format!("finding {}", f.id));
        }
        fn log(&mut self, level: LogLevel, msg: String) {
            self.lines.push(format!("log {level:?} {msg}"));
        }
        fn cancelled(&self) -> bool {
            self.cancel_after.is_some_and(|n| self.lines.len() >= n)
        }
    }

    /// Answers from a list, recording each request.
    struct Scripted {
        replies: Vec<InteractionReply>,
        asked: Vec<InteractionRequest>,
    }

    impl Interact for Scripted {
        fn ask(&mut self, req: InteractionRequest) -> Result<InteractionReply, Cancelled> {
            self.asked.push(req);
            if self.replies.is_empty() {
                Err(Cancelled)
            } else {
                Ok(self.replies.remove(0))
            }
        }
    }

    fn finding(id: &str) -> Finding {
        Finding {
            id: id.into(),
            class: FindingKind::Corruption(CorruptionClass::C3TrailerDamaged),
            severity: Severity::Warning,
            location: Location::File,
            summary: "trailer missing".into(),
            evidence: Vec::new(),
            repair: Repairability::Auto,
        }
    }

    fn analyzed(engine: &FakeEngine, bytes: &[u8]) -> AnalysisResult {
        engine
            .analyze(bytes, &AnalyzeOptions::default(), &mut NullProgress)
            .expect("not cancelled")
    }

    #[test]
    fn fake_engine_replays_its_analyze_script_in_order() {
        let engine = FakeEngine::new().on_analyze(vec![
            FakeStep::Phase("carving", 1, 3),
            FakeStep::Finding(finding("C3-001")),
            FakeStep::Log(LogLevel::Warn, "resync at byte 2,176".into()),
            FakeStep::Slow {
                steps: 2,
                pause_ms: 0,
            },
            FakeStep::Finding(finding("C3-002")),
        ]);
        let mut rec = Recorder::default();
        let result = engine
            .analyze(b"%PDF-1.4", &AnalyzeOptions::default(), &mut rec)
            .expect("not cancelled");
        assert_eq!(
            rec.lines,
            [
                "phase carving 1/3",
                "finding C3-001",
                "log Warn resync at byte 2,176",
                "progress 1/Some(2)",
                "progress 2/Some(2)",
                "finding C3-002",
            ]
        );
        let ids: Vec<&str> = result.findings.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, ["C3-001", "C3-002"]);
        assert_eq!(result.stats.bytes, 8);
        assert!(result.state.0.is_some());
    }

    #[test]
    fn slow_progress_stops_when_cancelled() {
        let engine = FakeEngine::new().on_analyze(vec![FakeStep::Slow {
            steps: 1_000,
            pause_ms: 1,
        }]);
        let mut rec = Recorder {
            cancel_after: Some(3),
            ..Recorder::default()
        };
        let r = engine.analyze(b"x", &AnalyzeOptions::default(), &mut rec);
        assert_eq!(r, Err(Cancelled));
        assert_eq!(rec.lines.len(), 3);
    }

    #[test]
    fn fake_engine_asks_and_records_the_replies() {
        let engine = FakeEngine::new()
            .on_repair(vec![FakeStep::Ask(InteractionRequest::FontUnreproducible(
                UnreproducibleRequest {
                    id: InteractionRequestId(7),
                    family: "Garamond".into(),
                    slots: vec![(0, "F2".into())],
                    reason: "no font".into(),
                    options: Vec::new(),
                },
            ))])
            .always_asks();
        let analysis = analyzed(&engine, b"%PDF-1.4");
        let mut ask = Scripted {
            replies: vec![
                InteractionReply::TextOnly,
                InteractionReply::Pick("x".into()),
            ],
            asked: Vec::new(),
        };
        let out = engine
            .repair(
                b"%PDF-1.4",
                &analysis,
                &engine.plan(&analysis, &RepairOptions::default()),
                &RepairOptions::default(),
                &FontDb::empty(),
                &mut ask,
                &mut NullProgress,
            )
            .expect("answered");
        assert_eq!(ask.asked.len(), 2);
        assert_eq!(ask.asked[1], FakeEngine::standard_question());
        assert_eq!(
            engine.replies(),
            [
                InteractionReply::TextOnly,
                InteractionReply::Pick("x".into())
            ]
        );
        assert_eq!(out.status, OutcomeStatus::Ok);
        assert_eq!(out.output.as_deref(), Some(&b"%PDF-1.4"[..]));
    }

    #[test]
    fn an_unanswered_question_cancels_the_repair() {
        let engine = FakeEngine::new().always_asks();
        let analysis = analyzed(&engine, b"a");
        let mut ask = Scripted {
            replies: Vec::new(),
            asked: Vec::new(),
        };
        let r = engine.repair(
            b"a",
            &analysis,
            &engine.plan(&analysis, &RepairOptions::default()),
            &RepairOptions::default(),
            &FontDb::empty(),
            &mut ask,
            &mut NullProgress,
        );
        assert_eq!(r, Err(Cancelled));
        assert!(engine.replies().is_empty());
    }

    #[test]
    fn a_scripted_panic_happens_on_the_calling_thread() {
        let engine = Arc::new(FakeEngine::new().on_analyze(vec![FakeStep::Panic("boom".into())]));
        let worker = Arc::clone(&engine);
        let joined = std::thread::spawn(move || {
            worker.analyze(b"x", &AnalyzeOptions::default(), &mut NullProgress)
        })
        .join();
        let payload = joined.expect_err("the worker panicked");
        assert_eq!(
            payload.downcast_ref::<String>().map(String::as_str),
            Some("boom")
        );
    }

    #[test]
    fn the_profile_scripts_eviction_numbers_per_input() {
        let engine = FakeEngine::new().profile(|bytes| FakeProfile {
            salvage_work_total: if bytes.starts_with(b"C9") {
                10_700_000_000
            } else {
                0
            },
            heap_bytes: 3 * bytes.len() as u64,
        });
        let cheap = analyzed(&engine, b"C7 file");
        let costly = analyzed(&engine, b"C9 file!");
        assert_eq!(cheap.stats.salvage_work_total, 0);
        assert_eq!(costly.stats.salvage_work_total, 10_700_000_000);
        let heap = |a: &AnalysisResult| a.state.0.as_ref().expect("state").heap_bytes();
        assert_eq!(heap(&cheap), 21);
        assert_eq!(heap(&costly), 24);
    }

    #[test]
    fn repair_reports_whether_the_state_was_reused() {
        let engine = FakeEngine::new();
        let opts = RepairOptions::default();
        let run = |bytes: &[u8], analysis: &AnalysisResult| {
            engine
                .repair(
                    bytes,
                    analysis,
                    &engine.plan(analysis, &opts),
                    &opts,
                    &FontDb::empty(),
                    &mut UseBest,
                    &mut NullProgress,
                )
                .expect("not cancelled")
                .analysis_state
        };
        let analysis = analyzed(&engine, b"one");
        assert_eq!(run(b"one", &analysis), AnalysisStateUse::Reused);
        assert!(matches!(
            run(b"two", &analysis),
            AnalysisStateUse::Rebuilt { .. }
        ));
        let mut from_history = analysis.clone();
        from_history.state = StateHandle::default();
        assert!(matches!(
            run(b"one", &from_history),
            AnalysisStateUse::Rebuilt { .. }
        ));
    }

    /// The batch T-20's view model reads: one file parked on a font question,
    /// one done with a C9 report, one being analysed.
    #[test]
    fn a_view_model_batch_is_built_from_these_types() {
        let engine = FakeEngine::new();
        let mut analysis = analyzed(&engine, b"%PDF-1.4 scan");
        analysis.meta.pages = 6;
        analysis.findings = vec![finding("C9-001")];
        analysis.font_slots = vec![FontSlot {
            page: 0,
            slot: "F1".into(),
            base_font: Some("/Garamond".into()),
            subtype: Some("/Type1".into()),
            embedded: false,
            tounicode: ToUnicodeState::Missing,
            glyph_count: 80,
            resolution: Some(FontResolution {
                kind: FontResolutionKind::TextOnly,
                provenance: vec!["no font reproduces the glyphs".into()],
            }),
        }];

        let mut parked = QueueEntry::queued(JobId(1), "memo.pdf".into(), None, 2_048);
        parked.state = EntryState::WaitingOnUser(FakeEngine::standard_question());
        parked.meta = Some(analysis.meta.clone());
        parked.findings = analysis.findings.clone();
        parked.font_resolutions = analysis.font_slots.clone();

        let mut report =
            RepairReport::default_for(&analysis, &RepairOptions::default(), &FontDb::empty());
        report.c9_summary = C9Summary {
            streams_damaged: 4,
            repaired: 3,
            exact: 2,
            accepted: 1,
            ambiguous: 0,
            unrecoverable: 1,
            unsearched: 0,
        };
        let mut done = QueueEntry::queued(
            JobId(2),
            "scan.pdf".into(),
            Some(PathBuf::from("scan.pdf")),
            1_153_024,
        );
        done.state = EntryState::Done;
        done.meta = Some(analysis.meta.clone());
        done.findings = analysis.findings.clone();
        done.font_resolutions = analysis.font_slots.clone();
        done.run = Some(RepairRun {
            report,
            status: OutcomeStatus::Partial(vec!["1 stream written as found".into()]),
            output_path: Some(PathBuf::from("scan.repaired.pdf")),
            placed: Some(Placed::Atomic),
            analysis_state: AnalysisStateUse::Reused,
        });

        let mut working = QueueEntry::queued(
            JobId(3),
            "ledger.pdf".into(),
            Some(PathBuf::from("ledger.pdf")),
            40_000,
        );
        working.state = EntryState::Analyzing {
            phase: Some("carving"),
            done: 10_000,
            total: Some(40_000),
        };

        let batch = BatchState {
            entries: vec![parked, done, working],
            current: Some(2),
        };

        assert_eq!(batch.total(), 3);
        assert_eq!(batch.completed(), 1);
        assert_eq!(batch.current_entry().map(|e| e.job), Some(JobId(3)));
        assert_eq!(batch.current_progress(), Some((10_000, Some(40_000))));

        let needs_you: Vec<&str> = batch
            .entries
            .iter()
            .filter(|e| matches!(e.state, EntryState::WaitingOnUser(_)))
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(needs_you, ["memo.pdf"]);
        let parked = &batch.entries[0];
        assert_eq!(parked.findings[0].id, "C9-001");
        assert_eq!(parked.meta.as_ref().map(|m| m.pages), Some(6));
        assert!(matches!(
            parked.font_resolutions[0]
                .resolution
                .as_ref()
                .map(|r| &r.kind),
            Some(FontResolutionKind::TextOnly)
        ));
        assert_eq!(parked.outcome(), None);

        let done = &batch.entries[1];
        assert!(matches!(done.outcome(), Some(OutcomeStatus::Partial(_))));
        let run = done.run.as_ref().expect("run");
        assert_eq!(run.report.c9_summary.repaired, 3);
        let c9_line = run
            .report
            .lines()
            .into_iter()
            .find(|l| l.contains("streams repaired"));
        assert_eq!(
            c9_line.as_deref(),
            Some(
                "3 streams repaired; 2 of them unique within the searched window; \
                 1 accepted without a uniqueness check; 0 ambiguous; \
                 1 unrecoverable, written as found"
            )
        );

        let idle = BatchState::default();
        assert_eq!((idle.total(), idle.completed()), (0, 0));
        assert_eq!(idle.current_progress(), None);
    }

    #[test]
    fn entry_progress_is_reported_only_while_working() {
        let working = [
            EntryState::Analyzing {
                phase: None,
                done: 1,
                total: None,
            },
            EntryState::Repairing {
                phase: Some("diagnosing C4"),
                done: 2,
                total: Some(5),
            },
            EntryState::Exporting {
                done: 3,
                total: Some(3),
            },
        ];
        for state in &working {
            assert!(state.progress().is_some() && !state.is_finished());
        }
        let finished = [
            EntryState::Done,
            EntryState::Failed {
                error: "x".into(),
                panicked: true,
            },
            EntryState::Cancelled,
        ];
        for state in &finished {
            assert!(state.progress().is_none() && state.is_finished());
        }
        for state in [
            EntryState::Queued,
            EntryState::WaitingOnUser(FakeEngine::standard_question()),
        ] {
            assert!(state.progress().is_none() && !state.is_finished());
        }
    }

    #[test]
    fn cancel_token_is_shared_between_clones() {
        let token = CancelToken::new();
        let seen = token.clone();
        assert_eq!(seen.check(), Ok(()));
        token.cancel();
        assert!(seen.is_cancelled());
        assert_eq!(seen.check(), Err(Cancelled));
    }

    #[test]
    fn the_repair_job_carries_the_analyze_jobs_state() {
        let engine = FakeEngine::new();
        let analysis = analyzed(&engine, b"x");
        let kind = JobKind::Repair {
            passes: None,
            state: analysis.state.clone(),
        };
        let JobKind::Repair { state, .. } = &kind else {
            unreachable!()
        };
        assert!(Arc::ptr_eq(
            state.0.as_ref().expect("state"),
            analysis.state.0.as_ref().expect("state")
        ));
    }
}

#[cfg(test)]
mod runner_tests;
