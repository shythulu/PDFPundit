//! Job runner over the `Engine` trait (T-02b types, T-15 runner). Wall-clock
//! drives progress display and cancellation here only, never an artefact.
//!
//! The UI thread owns the app state and the single `Receiver<AppEvent>`; job
//! threads return data only by sending `AppEvent::Job(id, event)` (TD §3).
#![allow(clippy::disallowed_types)]
// The runner (T-15) and the shell (T-23a) consume these types.
// TODO(T-15): remove this allow once the runner uses them.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;

use serde::{Deserialize, Serialize};

use crate::engine::{
    AnalysisResult, AnalysisStateUse, Cancelled, CorruptionClass, FileMeta, Finding, FontSlot,
    InteractionReply, InteractionRequest, LogLevel, OutcomeStatus, RepairReport, StateHandle,
};

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
    /// The job is blocked until `reply` receives an answer. The job is
    /// cancelled when every clone of `reply` is dropped without an answer
    /// (TD:219-221), so never keep a clone of this event in UI state: a stored
    /// clone keeps the sender alive and the job can then never be cancelled
    /// that way. Move `reply` out, answer once, and drop it.
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

    pub(crate) struct FakeEngine {
        analyze_steps: Vec<FakeStep>,
        repair_steps: Vec<FakeStep>,
        always_asks: bool,
        profile: Arc<ProfileFn>,
        replies: Mutex<Vec<InteractionReply>>,
    }

    impl Default for FakeEngine {
        fn default() -> Self {
            FakeEngine {
                analyze_steps: Vec::new(),
                repair_steps: Vec::new(),
                always_asks: false,
                profile: Arc::new(|_| FakeProfile::default()),
                replies: Mutex::new(Vec::new()),
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
