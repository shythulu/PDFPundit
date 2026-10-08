//! The runner against `FakeEngine` (T-15): batch order, cancellation, panics,
//! parked jobs and the memory cap (D-005), destinations (D-061) and no-clobber
//! outputs (D-044).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::Receiver;

use super::*;
use crate::engine::*;
use crate::panic_guard::{self, SharedWriter};
use crate::pdf::fixtures;
use crate::place::ScratchDir;

/// What the test saw, without the reply senders.
#[derive(Debug, Clone, PartialEq)]
enum Seen {
    Started(&'static str),
    AnalyzeDone,
    Asked(InteractionRequest),
    Done(Box<RepairRun>),
    Parked,
    Resumed,
    Evicted,
    Waiting(usize),
    Failed(String, bool),
    Cancelled,
    Exported(PathBuf),
    Other,
}

/// The UI side of the loop: receives every event, hands it to the runner,
/// and keeps what the assertions need.
struct Harness {
    runner: Option<JobRunner<FakeEngine>>,
    rx: Receiver<AppEvent<()>>,
    engine: Arc<FakeEngine>,
    seen: Vec<(JobId, Seen)>,
    /// The rows a UI would show, reduced from the events alone.
    rows: BTreeMap<JobId, EntryState>,
    analyzed_states: BTreeMap<JobId, StateHandle>,
    repair_states: BTreeMap<JobId, StateHandle>,
    work: BTreeMap<JobId, u64>,
    /// Parked jobs that still hold their state, with their W.
    holding: BTreeMap<JobId, u64>,
    /// Each eviction and how many jobs held a state just before it.
    evictions: Vec<(JobId, usize)>,
    /// Every `Log` event.
    logs: Vec<(JobId, LogLevel, String)>,
}

impl Harness {
    fn new(engine: FakeEngine, opts: RunnerOptions) -> Self {
        let (tx, rx) = mpsc::channel();
        let engine = Arc::new(engine);
        Harness {
            runner: Some(JobRunner::new(Arc::clone(&engine), tx, opts)),
            rx,
            engine,
            seen: Vec::new(),
            rows: BTreeMap::new(),
            analyzed_states: BTreeMap::new(),
            repair_states: BTreeMap::new(),
            work: BTreeMap::new(),
            holding: BTreeMap::new(),
            evictions: Vec::new(),
            logs: Vec::new(),
        }
    }

    fn runner(&mut self) -> &mut JobRunner<FakeEngine> {
        self.runner.as_mut().expect("runner")
    }

    fn submit(&mut self, path: &Path) -> JobId {
        self.runner().submit(JobInput::File(path.to_path_buf()))
    }

    /// Runs the loop until `done` holds, failing after 20 s.
    fn pump_until(&mut self, what: &str, done: impl Fn(&Harness) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done(self) {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(event) => self.handle(event),
                Err(e) => panic!("waiting for {what}: {e:?}; saw {:#?}", self.seen),
            }
        }
    }

    /// Handles every event already queued, including the ones the runner
    /// emitted while handling the earlier ones.
    fn drain(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            self.handle(event);
        }
    }

    fn handle(&mut self, event: AppEvent<()>) {
        if let AppEvent::Job(id, event) = event {
            if let Some(runner) = self.runner.as_mut() {
                runner.on_job_event(id, &event);
            }
            self.record(id, &event);
        }
    }

    fn record(&mut self, id: JobId, event: &JobEvent) {
        let seen = match event {
            JobEvent::Started { kind, .. } => match kind {
                JobKind::Analyze => {
                    self.rows.insert(
                        id,
                        EntryState::Analyzing {
                            phase: None,
                            done: 0,
                            total: None,
                        },
                    );
                    Seen::Started("analyze")
                }
                JobKind::Repair { state, .. } => {
                    self.repair_states.insert(id, state.clone());
                    self.rows.insert(
                        id,
                        EntryState::Repairing {
                            phase: None,
                            done: 0,
                            total: None,
                        },
                    );
                    Seen::Started("repair")
                }
                JobKind::ExportMarkdown => Seen::Started("export"),
            },
            JobEvent::AnalyzeDone(result) => {
                self.analyzed_states.insert(id, result.state.clone());
                self.work.insert(id, result.stats.salvage_work_total);
                Seen::AnalyzeDone
            }
            JobEvent::NeedsInteraction { request, .. } => {
                self.rows
                    .insert(id, EntryState::WaitingOnUser(request.clone()));
                Seen::Asked(request.clone())
            }
            JobEvent::RepairDone(run) => {
                self.rows.insert(id, EntryState::Done);
                Seen::Done(run.clone())
            }
            JobEvent::Parked => {
                self.holding.insert(id, self.work[&id]);
                Seen::Parked
            }
            JobEvent::Resumed => {
                self.holding.remove(&id);
                Seen::Resumed
            }
            JobEvent::Evicted { id: evicted } => {
                assert_eq!(*evicted, id);
                let w = self.holding[&id];
                let cheapest = self.holding.values().min().copied();
                assert_eq!(
                    Some(w),
                    cheapest,
                    "{id:?} (W {w}) evicted while a cheaper state remained: {:?}",
                    self.holding
                );
                self.evictions.push((id, self.holding.len()));
                self.holding.remove(&id);
                Seen::Evicted
            }
            JobEvent::WaitingForYou { parked } => Seen::Waiting(*parked),
            JobEvent::ExportDone { path } => {
                self.rows.insert(id, EntryState::Done);
                Seen::Exported(path.clone())
            }
            JobEvent::Log(level, msg) => {
                self.logs.push((id, *level, msg.clone()));
                Seen::Other
            }
            JobEvent::Failed { error, panicked } => {
                self.holding.remove(&id);
                self.rows.insert(
                    id,
                    EntryState::Failed {
                        error: error.clone(),
                        panicked: *panicked,
                    },
                );
                Seen::Failed(error.clone(), *panicked)
            }
            JobEvent::Cancelled => {
                self.holding.remove(&id);
                self.rows.insert(id, EntryState::Cancelled);
                Seen::Cancelled
            }
            _ => Seen::Other,
        };
        self.seen.push((id, seen));
    }

    fn count(&self, f: impl Fn(&Seen) -> bool) -> usize {
        self.seen.iter().filter(|(_, s)| f(s)).count()
    }

    fn count_of(&self, id: JobId, f: impl Fn(&Seen) -> bool) -> usize {
        self.seen.iter().filter(|(i, s)| *i == id && f(s)).count()
    }

    fn position(&self, f: impl Fn(&(JobId, Seen)) -> bool) -> Option<usize> {
        self.seen.iter().position(f)
    }

    fn done(&self, id: JobId) -> Option<&RepairRun> {
        self.seen.iter().find_map(|(i, s)| match s {
            Seen::Done(run) if *i == id => Some(&**run),
            _ => None,
        })
    }

    fn finished(&self, id: JobId) -> bool {
        self.rows.get(&id).is_some_and(EntryState::is_finished)
    }
}

fn is_done(s: &Seen) -> bool {
    matches!(s, Seen::Done(_))
}
fn is_asked(s: &Seen) -> bool {
    matches!(s, Seen::Asked(_))
}
fn is_evicted(s: &Seen) -> bool {
    matches!(s, Seen::Evicted)
}

fn write(dir: &ScratchDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, bytes).expect("write input");
    path
}

fn opts() -> RunnerOptions {
    RunnerOptions::default()
}

fn rebuilt() -> AnalysisStateUse {
    AnalysisStateUse::Rebuilt {
        reason: EVICTED_WHILE_PARKED.into(),
    }
}

/// Uninstalls the panic guard when the test ends, passing or not.
struct Guarded(#[allow(dead_code)] MutexGuard<'static, ()>);

impl Guarded {
    fn install(writer: &SharedWriter) -> Self {
        let lock = panic_guard::test_lock();
        panic_guard::install(
            thread::current().id(),
            b"RESTORE".to_vec(),
            Box::new(writer.clone()),
        );
        Guarded(lock)
    }
}

impl Drop for Guarded {
    fn drop(&mut self) {
        panic_guard::uninstall();
    }
}

#[test]
fn a_batch_runs_one_file_at_a_time_in_drop_order() {
    let dir = ScratchDir::new("jobs-order");
    let paths: Vec<PathBuf> = (0..3)
        .map(|i| {
            write(
                &dir,
                &format!("f{i}.pdf"),
                format!("%PDF-1.4 file {i}").as_bytes(),
            )
        })
        .collect();
    let engine = FakeEngine::new().on_analyze(vec![FakeStep::Slow {
        steps: 3,
        pause_ms: 2,
    }]);
    let mut h = Harness::new(engine, opts());
    let ids: Vec<JobId> = paths.iter().map(|p| h.submit(p)).collect();
    h.pump_until("three repairs", |h| h.count(is_done) == 3);

    let starts: Vec<(JobId, Seen)> = h
        .seen
        .iter()
        .filter(|(_, s)| matches!(s, Seen::Started(_)))
        .cloned()
        .collect();
    let expected: Vec<(JobId, Seen)> = ids
        .iter()
        .flat_map(|id| {
            [
                (*id, Seen::Started("analyze")),
                (*id, Seen::Started("repair")),
            ]
        })
        .collect();
    assert_eq!(starts, expected);
    for pair in ids.windows(2) {
        let done = h
            .position(|(i, s)| *i == pair[0] && is_done(s))
            .expect("done");
        let next = h.position(|(i, _)| *i == pair[1]).expect("next");
        assert!(
            done < next,
            "{:?} started before {:?} finished",
            pair[1],
            pair[0]
        );
    }
    for (i, id) in ids.iter().enumerate() {
        let run = h.done(*id).expect("run");
        let out = dir.join(&format!("f{i}.repaired.pdf"));
        assert_eq!(run.output_path.as_ref(), Some(&out));
        assert_eq!(run.placed, Some(Placed::Atomic));
        assert_eq!(run.analysis_state, AnalysisStateUse::Reused);
        assert_eq!(run.status, OutcomeStatus::Ok);
        assert_eq!(
            fs::read(&out).expect("output"),
            fs::read(&paths[i]).expect("input")
        );
    }
    assert_eq!(
        dir.names().len(),
        6,
        "inputs and outputs only: {:?}",
        dir.names()
    );
}

#[test]
fn cancel_removes_a_queued_job_and_stops_a_running_one() {
    let dir = ScratchDir::new("jobs-cancel");
    let engine = FakeEngine::new().on_analyze(vec![FakeStep::Slow {
        steps: 40,
        pause_ms: 5,
    }]);
    let mut h = Harness::new(engine, opts());
    let ids: Vec<JobId> = (0..3)
        .map(|i| {
            let p = write(&dir, &format!("c{i}.pdf"), b"x");
            h.submit(&p)
        })
        .collect();
    h.runner().cancel(ids[1]);
    h.pump_until("the queued job's Cancelled", |h| h.finished(ids[1]));
    h.pump_until("the others", |h| h.finished(ids[0]) && h.finished(ids[2]));
    assert_eq!(h.rows[&ids[1]], EntryState::Cancelled);
    assert_eq!(h.count_of(ids[1], |s| matches!(s, Seen::Started(_))), 0);
    assert!(h.done(ids[0]).is_some() && h.done(ids[2]).is_some());

    let p = write(&dir, "c3.pdf", b"x");
    let running = h.submit(&p);
    h.pump_until("it starts", |h| {
        h.count_of(running, |s| *s == Seen::Started("analyze")) == 1
    });
    h.runner().cancel(running);
    h.pump_until("it stops", |h| h.finished(running));
    assert_eq!(h.rows[&running], EntryState::Cancelled);
    assert!(!dir.join("c3.repaired.pdf").exists());
}

#[test]
fn a_worker_panic_fails_its_job_and_the_batch_goes_on() {
    let writer = SharedWriter::default();
    let _guard = Guarded::install(&writer);
    let dir = ScratchDir::new("jobs-panic");
    let bad = write(&dir, "bad.pdf", b"bad input");
    let good = write(&dir, "good.pdf", b"good input");
    let engine = FakeEngine::new().panics_when(|bytes| bytes.starts_with(b"bad"));
    let mut h = Harness::new(engine, opts());
    let bad = h.submit(&bad);
    let good = h.submit(&good);
    h.pump_until("both finish", |h| h.finished(bad) && h.finished(good));

    let EntryState::Failed { error, panicked } = &h.rows[&bad] else {
        panic!("not failed: {:?}", h.rows[&bad]);
    };
    assert!(*panicked);
    // The hook put the payload and its location in the job's sink.
    let error = error.replace('\\', "/");
    assert!(error.starts_with("panicked at src/jobs.rs:"), "{error}");
    assert!(error.ends_with("fake engine panic"), "{error}");
    assert!(h.done(good).is_some(), "the next job ran to the end");
    assert!(writer.bytes().is_empty(), "nothing went to the terminal");
}

#[test]
fn quitting_while_a_job_asks_cancels_it_within_400_ms() {
    let dir = ScratchDir::new("jobs-quit");
    let p = write(&dir, "q.pdf", b"x");
    let mut h = Harness::new(FakeEngine::new().always_asks(), opts());
    let id = h.submit(&p);
    h.pump_until("the question", |h| h.count(is_asked) == 1);

    // The UI goes: the runner (holding the only reply sender) is dropped.
    let quit = Instant::now();
    drop(h.runner.take());
    loop {
        let left = Duration::from_millis(400).saturating_sub(quit.elapsed());
        match h.rx.recv_timeout(left) {
            Ok(AppEvent::Job(i, JobEvent::Cancelled)) if i == id => break,
            Ok(_) => {}
            Err(e) => panic!("no Cancelled within 400 ms: {e:?}"),
        }
    }
    assert!(quit.elapsed() < Duration::from_millis(400));
    assert!(!dir.join("q.repaired.pdf").exists());
    assert_eq!(dir.names(), ["q.pdf"], "the temp file is gone");
}

#[test]
fn a_parked_job_does_not_block_the_next_and_a_reply_resumes_it() {
    let dir = ScratchDir::new("jobs-park");
    let a = write(&dir, "a.pdf", b"first");
    let b = write(&dir, "b.pdf", b"second");
    let mut h = Harness::new(FakeEngine::new().always_asks(), opts());
    let a = h.submit(&a);
    let b = h.submit(&b);
    h.pump_until("both waiting", |h| {
        h.seen.iter().any(|(_, s)| *s == Seen::Waiting(2))
    });

    let parked = h
        .position(|(i, s)| *i == a && *s == Seen::Parked)
        .expect("parked");
    let next = h
        .position(|(i, s)| *i == b && *s == Seen::Started("analyze"))
        .expect("next started");
    assert!(parked < next, "the next file starts once the first parks");
    assert_eq!(
        h.runner().parked().map(|(id, _)| id).collect::<Vec<_>>(),
        [a, b]
    );

    assert!(h.runner().reply(a, InteractionReply::Pick("font-a".into())));
    h.pump_until("a finishes", |h| h.finished(a));
    let run = h.done(a).expect("a done");
    assert_eq!(
        run.analysis_state,
        AnalysisStateUse::Reused,
        "its thread kept the state"
    );
    assert_eq!(
        h.engine.replies(),
        [InteractionReply::Pick("font-a".into())]
    );
    assert!(!h.finished(b), "b is still waiting");

    assert!(
        !h.runner().reply(a, InteractionReply::Skip),
        "a is no longer parked"
    );
    assert!(h.runner().reply(b, InteractionReply::TextOnly));
    h.pump_until("b finishes", |h| h.finished(b));
    assert!(h.done(b).is_some());
    assert_eq!(
        h.seen.iter().rev().find_map(|(_, s)| match s {
            Seen::Waiting(n) => Some(*n),
            _ => None,
        }),
        Some(0)
    );
}

const SIZE: usize = 1_000;
const C9_WORK: u64 = 10_700_000_000;

/// File `i` of the draining batch; file 0 has a C9-sized state.
fn draining_input(i: usize) -> Vec<u8> {
    let tag = if i == 0 { "C9" } else { "C7" };
    let mut bytes = format!("{tag} file {i:02} ").into_bytes();
    bytes.resize(SIZE, b'.');
    bytes
}

fn draining_profile(bytes: &[u8]) -> FakeProfile {
    let i: u64 = std::str::from_utf8(&bytes[8..10])
        .expect("ascii")
        .parse()
        .expect("index");
    FakeProfile {
        salvage_work_total: if bytes.starts_with(b"C9") {
            C9_WORK
        } else {
            (i * 37) % 23 * 1_000 + i
        },
        heap_bytes: 250,
    }
}

#[test]
fn the_queue_keeps_draining_when_every_file_asks() {
    let dir = ScratchDir::new("jobs-drain");
    let engine = FakeEngine::new().always_asks().profile(draining_profile);
    let mut h = Harness::new(
        engine,
        RunnerOptions {
            parked_cap: 3 * SIZE as u64,
            ..opts()
        },
    );
    let ids: Vec<JobId> = (0..20)
        .map(|i| {
            let p = write(&dir, &format!("f{i:02}.pdf"), &draining_input(i));
            h.submit(&p)
        })
        .collect();
    h.pump_until("all twenty waiting", |h| {
        h.count(is_asked) == 20 && h.seen.iter().any(|(_, s)| *s == Seen::Waiting(20))
    });
    // The runner's own events for the last park (Evicted) follow it.
    h.drain();

    assert_eq!(
        h.count(|s| *s == Seen::AnalyzeDone),
        20,
        "every file was analysed"
    );
    for id in &ids {
        assert!(
            matches!(h.rows[id], EntryState::WaitingOnUser(_)),
            "{id:?}'s question is still showing: {:?}",
            h.rows[id]
        );
    }
    assert_eq!(
        h.count(|s| matches!(s, Seen::Failed(..) | Seen::Cancelled)),
        0
    );
    assert_eq!(h.runner().parked().count(), 20);
    assert!(h.runner().retained_bytes() <= 3 * SIZE as u64);
    // Eviction went cheapest first (checked at every Evicted as it arrived);
    // the C9 state went only when no cheaper state was left.
    let c9 = ids[0];
    let (_, holders) = h
        .evictions
        .iter()
        .find(|(id, _)| *id == c9)
        .expect("the C9 state was evicted");
    assert_eq!(*holders, 1);
    let c9_at = h
        .evictions
        .iter()
        .position(|(id, _)| *id == c9)
        .expect("c9");
    assert!(
        c9_at >= 2,
        "two cheaper states went first: {:?}",
        h.evictions
    );
    let evicted: BTreeSet<JobId> = h.evictions.iter().map(|(id, _)| *id).collect();
    assert_eq!(
        evicted.len(),
        20,
        "inputs alone exceed the cap, so every state went"
    );

    for (i, id) in ids.iter().enumerate() {
        assert!(
            h.runner()
                .reply(*id, InteractionReply::Pick(format!("font-{i}")))
        );
    }
    h.pump_until("all twenty repaired", |h| h.count(is_done) == 20);
    assert_eq!(
        h.count(is_asked),
        20,
        "the replayed reply answered the re-run"
    );
    for (i, id) in ids.iter().enumerate() {
        let run = h.done(*id).expect("done");
        assert_eq!(run.analysis_state, rebuilt());
        let out = dir.join(&format!("f{i:02}.repaired.pdf"));
        assert_eq!(fs::read(&out).expect("output"), draining_input(i));
    }
    let mut replies = h.engine.replies();
    replies.sort_by_key(|r| format!("{r:?}"));
    let mut expected: Vec<InteractionReply> = (0..20)
        .map(|i| InteractionReply::Pick(format!("font-{i}")))
        .collect();
    expected.sort_by_key(|r| format!("{r:?}"));
    assert_eq!(replies, expected, "each reply reached the engine once");
}

#[test]
fn a_re_run_replays_every_earlier_reply() {
    let dir = ScratchDir::new("jobs-replay");
    let p = write(&dir, "a.pdf", b"two questions");
    let first = InteractionRequest::FontUnreproducible(UnreproducibleRequest {
        id: InteractionRequestId(9),
        family: "Garamond".into(),
        slots: vec![(0, "F2".into())],
        reason: "no font".into(),
        options: Vec::new(),
    });
    let engine = FakeEngine::new()
        .on_repair(vec![FakeStep::Ask(first.clone())])
        .always_asks();
    let mut h = Harness::new(
        engine,
        RunnerOptions {
            parked_cap: 0,
            ..opts()
        },
    );
    let id = h.submit(&p);
    h.pump_until("the first question, evicted", |h| h.count(is_evicted) == 1);
    assert_eq!(
        h.runner().retained_bytes(),
        0,
        "state evicted and input released"
    );

    assert!(h.runner().reply(id, InteractionReply::TextOnly));
    h.pump_until("the second question, evicted", |h| h.count(is_evicted) == 2);
    let asked: Vec<InteractionRequest> = h
        .seen
        .iter()
        .filter_map(|(_, s)| match s {
            Seen::Asked(r) => Some(r.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        asked,
        [first, FakeEngine::standard_question()],
        "q1 was not asked again"
    );

    assert!(h.runner().reply(id, InteractionReply::Pick("x".into())));
    h.pump_until("done", |h| h.finished(id));
    assert_eq!(h.done(id).expect("done").analysis_state, rebuilt());
    assert_eq!(
        h.engine.replies(),
        [
            InteractionReply::TextOnly,
            InteractionReply::TextOnly,
            InteractionReply::Pick("x".into())
        ],
        "the second run got the first reply replayed"
    );
}

#[test]
fn a_released_input_that_changed_on_disk_is_refused() {
    let dir = ScratchDir::new("jobs-changed");
    let p = write(&dir, "a.pdf", b"original bytes");
    let mut h = Harness::new(
        FakeEngine::new().always_asks(),
        RunnerOptions {
            parked_cap: 0,
            ..opts()
        },
    );
    let id = h.submit(&p);
    h.pump_until("evicted", |h| h.count(is_evicted) == 1);
    fs::write(&p, b"edited while waiting").expect("edit");
    assert!(h.runner().reply(id, InteractionReply::UseBest));
    h.pump_until("the end", |h| h.finished(id));
    assert_eq!(
        h.rows[&id],
        EntryState::Failed {
            error: INPUT_CHANGED.into(),
            panicked: false
        }
    );
    // The evicted worker removes its temp file on its way out, silently and
    // possibly after the re-run has failed.
    h.runner().join_retired();
    assert_eq!(dir.names(), ["a.pdf"], "nothing was written");
}

#[test]
fn a_promised_input_is_pinned_and_the_batch_still_goes_on() {
    let dir = ScratchDir::new("jobs-pinned");
    let out = ScratchDir::new("jobs-pinned-out");
    let file = write(&dir, "b.pdf", &[b'b'; 100]);
    let mut h = Harness::new(
        FakeEngine::new().always_asks(),
        RunnerOptions {
            parked_cap: 1_000,
            output_dir: Some(out.path().to_path_buf()),
            ..opts()
        },
    );
    let promise = h.runner().submit(JobInput::Memory {
        name: "promise.pdf".into(),
        bytes: vec![b'p'; 2_000],
    });
    let file = h.submit(&file);
    h.pump_until("both parked", |h| h.count(is_asked) == 2);

    assert!(
        h.evictions.iter().any(|(id, _)| *id == promise),
        "its state was evicted"
    );
    let started = h
        .position(|(i, s)| *i == file && *s == Seen::Started("analyze"))
        .expect("the next job started");
    let parked = h
        .position(|(i, s)| *i == promise && *s == Seen::Parked)
        .expect("parked");
    assert!(parked < started);
    // The file's input was released at step (2); the promise's never is.
    assert_eq!(h.runner().retained_bytes(), 2_000);

    assert!(h.runner().reply(promise, InteractionReply::UseBest));
    h.pump_until("the promise is repaired", |h| h.finished(promise));
    let run = h.done(promise).expect("done");
    assert_eq!(run.analysis_state, rebuilt());
    assert_eq!(run.output_path, Some(out.join("promise.repaired.pdf")));
    assert_eq!(
        fs::read(out.join("promise.repaired.pdf")).expect("out"),
        vec![b'p'; 2_000]
    );
}

/// How many jobs had parked when the first `Evicted` came, for 8 parked
/// 1 MB inputs under a 6 MB cap with `heap_bytes() = factor × input`.
fn parked_before_first_eviction(factor: u64) -> usize {
    const MB: usize = 1 << 20;
    let out = ScratchDir::new("jobs-heap");
    let engine = FakeEngine::new()
        .always_asks()
        .profile(move |bytes| FakeProfile {
            salvage_work_total: 0,
            heap_bytes: factor * bytes.len() as u64,
        });
    let mut h = Harness::new(
        engine,
        RunnerOptions {
            parked_cap: 6 * MB as u64,
            output_dir: Some(out.path().to_path_buf()),
            ..opts()
        },
    );
    for i in 0..8u8 {
        h.runner().submit(JobInput::Memory {
            name: format!("m{i}.pdf"),
            bytes: vec![i; MB],
        });
    }
    h.pump_until("eight parked", |h| h.count(|s| *s == Seen::Parked) == 8);
    let first = h.position(|(_, s)| is_evicted(s)).expect("an eviction");
    h.seen[..first]
        .iter()
        .filter(|(_, s)| *s == Seen::Parked)
        .count()
}

#[test]
fn the_cap_counts_the_analysis_state_not_just_the_input() {
    assert_eq!(parked_before_first_eviction(3), 2, "4 MB, then 8 MB > 6 MB");
    assert_eq!(
        parked_before_first_eviction(0),
        7,
        "only when inputs alone pass 6 MB"
    );
}

#[test]
fn the_repair_stage_gets_the_analyze_stages_state() {
    let dir = ScratchDir::new("jobs-state");
    let p = write(&dir, "s.pdf", b"state");
    let mut h = Harness::new(FakeEngine::new(), opts());
    let id = h.submit(&p);
    h.pump_until("done", |h| h.finished(id));
    let analyzed = h.analyzed_states[&id].0.clone().expect("analysis state");
    let handed = h.repair_states[&id].0.clone().expect("repair kind's state");
    let received = h.engine.repaired_states()[0]
        .0
        .clone()
        .expect("engine's state");
    assert!(Arc::ptr_eq(&analyzed, &handed));
    assert!(Arc::ptr_eq(&analyzed, &received));
    assert_eq!(
        h.done(id).expect("done").analysis_state,
        AnalysisStateUse::Reused
    );
}

#[test]
fn an_existing_output_name_is_never_replaced() {
    let dir = ScratchDir::new("jobs-clobber");
    let input = write(&dir, "a.pdf", b"input a");
    write(&dir, "a.repaired.pdf", b"the examiner's earlier output");
    let mut h = Harness::new(FakeEngine::new(), opts());
    let id = h.submit(&input);
    h.pump_until("done", |h| h.finished(id));
    let run = h.done(id).expect("done");
    assert_eq!(run.output_path, Some(dir.join("a.repaired (2).pdf")));
    assert_eq!(
        fs::read(dir.join("a.repaired.pdf")).expect("kept"),
        b"the examiner's earlier output"
    );
    assert_eq!(
        dir.names(),
        ["a.pdf", "a.repaired (2).pdf", "a.repaired.pdf"]
    );
}

#[test]
fn an_input_and_its_namesake_output_in_one_batch_both_survive() {
    let dir = ScratchDir::new("jobs-namesake");
    let a = write(&dir, "a.pdf", b"input a");
    let ar = write(&dir, "a.repaired.pdf", b"input a.repaired");
    let mut h = Harness::new(FakeEngine::new(), opts());
    let ids = [h.submit(&a), h.submit(&ar)];
    h.pump_until("done", |h| ids.iter().all(|id| h.finished(*id)));
    let outs: Vec<PathBuf> = ids
        .iter()
        .map(|id| {
            h.done(*id)
                .expect("done")
                .output_path
                .clone()
                .expect("path")
        })
        .collect();
    assert_eq!(
        outs,
        [
            dir.join("a.repaired (2).pdf"),
            dir.join("a.repaired.repaired.pdf")
        ]
    );
    for out in &outs {
        assert!(*out != a && *out != ar, "an output landed on an input");
    }
    assert_eq!(fs::read(&a).expect("a"), b"input a");
    assert_eq!(fs::read(&ar).expect("ar"), b"input a.repaired");
}

#[test]
fn the_temp_file_is_gone_after_a_failed_or_cancelled_repair() {
    let writer = SharedWriter::default();
    let _guard = Guarded::install(&writer);
    let dir = ScratchDir::new("jobs-temp");
    let p = write(&dir, "a.pdf", b"x");
    let engine = FakeEngine::new().on_repair(vec![FakeStep::Panic("repair boom".into())]);
    let mut h = Harness::new(engine, opts());
    let id = h.submit(&p);
    h.pump_until("failed", |h| h.finished(id));
    assert!(matches!(
        h.rows[&id],
        EntryState::Failed { panicked: true, .. }
    ));
    assert_eq!(dir.names(), ["a.pdf"]);

    let engine = FakeEngine::new().on_repair(vec![FakeStep::Slow {
        steps: 200,
        pause_ms: 5,
    }]);
    let mut h = Harness::new(engine, opts());
    let id = h.submit(&p);
    h.pump_until("repairing", |h| {
        h.count_of(id, |s| *s == Seen::Started("repair")) == 1
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while dir.names().len() < 2 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(2));
    }
    let names = dir.names();
    assert_eq!(names.len(), 2, "the temp file is opened before engine work");
    assert!(
        names[0].ends_with(".tmp") || names[1].ends_with(".tmp"),
        "{names:?}"
    );
    h.runner().cancel(id);
    h.pump_until("cancelled", |h| h.finished(id));
    assert_eq!(h.rows[&id], EntryState::Cancelled);
    assert_eq!(dir.names(), ["a.pdf"]);
    assert!(writer.bytes().is_empty());
}

/// Makes `dir` unwritable for this user; false when that cannot be enforced
/// (running as root).
#[cfg(unix)]
fn make_read_only(dir: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o555)).expect("chmod");
    let probe = dir.join("probe");
    if fs::write(&probe, b"").is_ok() {
        let _ = fs::remove_file(probe);
        return false;
    }
    true
}

#[cfg(unix)]
fn make_writable(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).expect("chmod");
}

#[cfg(windows)]
fn make_read_only(dir: &Path) -> bool {
    let user = std::env::var("USERNAME").expect("USERNAME");
    let ok = std::process::Command::new("icacls")
        .arg(dir)
        .arg("/deny")
        .arg(format!("{user}:(WD,AD)"))
        .status()
        .expect("icacls")
        .success();
    assert!(ok, "icacls /deny");
    let probe = dir.join("probe");
    if fs::write(&probe, b"").is_ok() {
        let _ = fs::remove_file(probe);
        return false;
    }
    true
}

#[cfg(windows)]
fn make_writable(dir: &Path) {
    let user = std::env::var("USERNAME").expect("USERNAME");
    let _ = std::process::Command::new("icacls")
        .arg(dir)
        .arg("/remove:d")
        .arg(user)
        .status();
}

#[test]
fn a_read_only_folder_fails_the_repair_and_nothing_is_written() {
    let root = ScratchDir::new("jobs-readonly");
    let evidence = root.join("evidence");
    fs::create_dir(&evidence).expect("evidence dir");
    let input = evidence.join("a.pdf");
    fs::write(&input, b"evidence").expect("input");
    if !make_read_only(&evidence) {
        make_writable(&evidence);
        eprintln!("skipped: this user can write to a read-only directory");
        return;
    }
    let mut h = Harness::new(FakeEngine::new(), opts());
    let id = h.submit(&input);
    h.pump_until("the end", |h| h.finished(id));
    let listing = crate::place::names_in(&evidence);
    make_writable(&evidence);

    assert_eq!(
        h.rows[&id],
        EntryState::Failed {
            error: READ_ONLY_DESTINATION.into(),
            panicked: false
        }
    );
    assert_eq!(
        h.count_of(id, |s| *s == Seen::AnalyzeDone),
        1,
        "findings still show"
    );
    assert_eq!(listing, ["a.pdf"], "the folder is untouched");
    assert_eq!(fs::read(&input).expect("input"), b"evidence");
    assert_eq!(
        root.names(),
        ["evidence"],
        "nothing was written anywhere else"
    );
}

#[test]
fn output_dir_collects_outputs_and_suffixes_shared_stems() {
    let root = ScratchDir::new("jobs-outdir");
    fs::create_dir(root.join("x")).expect("x");
    fs::create_dir(root.join("y")).expect("y");
    let one = write(&root, "x/a.pdf", b"from x");
    let two = write(&root, "y/a.pdf", b"from y");
    let out = root.join("out");
    let mut h = Harness::new(
        FakeEngine::new(),
        RunnerOptions {
            output_dir: Some(out.clone()),
            ..opts()
        },
    );
    let ids = [h.submit(&one), h.submit(&two)];
    h.pump_until("done", |h| ids.iter().all(|id| h.finished(*id)));
    assert_eq!(
        h.done(ids[0]).expect("x").output_path,
        Some(out.join("a.repaired.pdf"))
    );
    assert_eq!(
        h.done(ids[1]).expect("y").output_path,
        Some(out.join("a.repaired (2).pdf"))
    );
    assert_eq!(
        fs::read(out.join("a.repaired (2).pdf")).expect("y out"),
        b"from y"
    );
    assert_eq!(crate::place::names_in(&root.join("x")), ["a.pdf"]);
}

#[test]
fn an_unusable_output_dir_is_named_in_the_failure() {
    let root = ScratchDir::new("jobs-badout");
    let input = write(&root, "a.pdf", b"x");
    // A regular file where output_dir's parent should be: create_dir_all fails.
    write(&root, "blocker", b"not a directory");
    let out = root.join("blocker").join("out");
    let mut h = Harness::new(
        FakeEngine::new(),
        RunnerOptions {
            output_dir: Some(out.clone()),
            ..opts()
        },
    );
    let id = h.submit(&input);
    h.pump_until("the end", |h| h.finished(id));
    let EntryState::Failed { error, panicked } = &h.rows[&id] else {
        panic!("not failed: {:?}", h.rows[&id]);
    };
    assert!(!panicked);
    let expected = format!("can't write to the output_dir ({}): ", out.display());
    assert!(error.starts_with(&expected), "{error}");
    assert_ne!(error, READ_ONLY_DESTINATION);
    assert_eq!(root.names(), ["a.pdf", "blocker"], "nothing was written");
}

#[test]
fn a_promised_input_without_output_dir_fails_with_the_fixed_message() {
    let mut h = Harness::new(FakeEngine::new(), opts());
    let id = h.runner().submit(JobInput::Memory {
        name: "promise.pdf".into(),
        bytes: b"bytes".to_vec(),
    });
    h.pump_until("the end", |h| h.finished(id));
    assert_eq!(
        h.rows[&id],
        EntryState::Failed {
            error: READ_ONLY_DESTINATION.into(),
            panicked: false
        }
    );
    assert_eq!(h.count_of(id, |s| *s == Seen::AnalyzeDone), 1);
}

/// A kitty drop's file (T-31): read while the drop was active, so the job
/// works on those bytes, and while its path is still there the output goes
/// beside it.
#[test]
fn a_dropped_input_works_on_its_bytes_and_lands_beside_its_path() {
    let dir = ScratchDir::new("jobs-dropped");
    let path = write(&dir, "d.pdf", b"on disk now");
    let mut h = Harness::new(FakeEngine::new(), opts());
    let id = h.runner().submit(JobInput::Dropped {
        path: path.clone(),
        bytes: b"dropped".to_vec(),
    });
    h.pump_until("done", |h| h.finished(id));
    let run = h.done(id).expect("repaired");
    let out = dir.join("d.repaired.pdf");
    assert_eq!(run.output_path, Some(out.clone()));
    assert_eq!(fs::read(&out).expect("out"), b"dropped", "the held bytes");
    assert_eq!(
        fs::read(&path).expect("in"),
        b"on disk now",
        "never written"
    );
}

/// A dropped file that is gone by the D-061 probe (a macOS file promise,
/// D-039) has no durable "beside": with no `output_dir` the job fails with the
/// fixed message and writes nothing beside the vanished path.
#[test]
fn a_dropped_input_whose_file_is_gone_fails_without_output_dir() {
    let dir = ScratchDir::new("jobs-dropped-gone");
    let mut h = Harness::new(FakeEngine::new(), opts());
    let id = h.runner().submit(JobInput::Dropped {
        path: dir.join("promise.pdf"),
        bytes: b"promised".to_vec(),
    });
    h.pump_until("the end", |h| h.finished(id));
    assert_eq!(
        h.rows[&id],
        EntryState::Failed {
            error: READ_ONLY_DESTINATION.into(),
            panicked: false
        }
    );
    assert_eq!(h.count_of(id, |s| *s == Seen::AnalyzeDone), 1);
    assert!(dir.names().is_empty(), "nothing beside the vanished path");
}

/// The same input with `output_dir` set: the output goes there.
#[test]
fn a_dropped_input_whose_file_is_gone_goes_to_output_dir() {
    let dir = ScratchDir::new("jobs-dropped-gone-dir");
    let out = ScratchDir::new("jobs-dropped-gone-out");
    let mut h = Harness::new(
        FakeEngine::new(),
        RunnerOptions {
            output_dir: Some(out.path().to_path_buf()),
            ..opts()
        },
    );
    let id = h.runner().submit(JobInput::Dropped {
        path: dir.join("promise.pdf"),
        bytes: b"promised".to_vec(),
    });
    h.pump_until("done", |h| h.finished(id));
    let run = h.done(id).expect("repaired");
    assert_eq!(run.output_path, Some(out.join("promise.repaired.pdf")));
    assert_eq!(
        fs::read(out.join("promise.repaired.pdf")).expect("out"),
        b"promised"
    );
    assert!(dir.names().is_empty(), "nothing beside the vanished path");
}

/// A parked kitty drop over the cap keeps its bytes (D-039, D-005 step 2):
/// its path may have been a promise that no longer re-reads. Only its state
/// is evicted, and the reply resumes from the held bytes.
#[test]
fn a_parked_dropped_input_over_the_cap_keeps_its_bytes() {
    let dir = ScratchDir::new("jobs-dropped-pinned");
    let out = ScratchDir::new("jobs-dropped-pinned-out");
    let path = write(&dir, "d.pdf", b"x");
    let mut h = Harness::new(
        FakeEngine::new().always_asks(),
        RunnerOptions {
            parked_cap: 0,
            output_dir: Some(out.path().to_path_buf()),
            ..opts()
        },
    );
    let id = h.runner().submit(JobInput::Dropped {
        path: path.clone(),
        bytes: vec![b'd'; 2_000],
    });
    h.pump_until("parked and evicted", |h| h.count(is_evicted) == 1);
    assert_eq!(h.runner().retained_bytes(), 2_000, "the bytes are pinned");

    // The promise is gone by the time the examiner answers.
    fs::remove_file(&path).expect("remove");
    assert!(h.runner().reply(id, InteractionReply::UseBest));
    h.pump_until("resumed from the held bytes", |h| h.finished(id));
    let run = h.done(id).expect("done");
    assert_eq!(run.analysis_state, rebuilt());
    assert_eq!(
        fs::read(out.join("d.repaired.pdf")).expect("out"),
        vec![b'd'; 2_000]
    );
}

#[test]
fn shutdown_stops_every_worker() {
    let dir = ScratchDir::new("jobs-shutdown");
    let mut h = Harness::new(FakeEngine::new().always_asks(), opts());
    for i in 0..3 {
        let p = write(&dir, &format!("s{i}.pdf"), b"x");
        h.submit(&p);
    }
    h.pump_until("all asking", |h| h.count(is_asked) == 3);
    let runner = h.runner.take().expect("runner");
    assert!(runner.shutdown(Duration::from_secs(2)));
    assert_eq!(dir.names().len(), 3, "no temp file is left");
}

#[test]
fn the_salvage_threads_are_clamped_and_reach_both_option_sets() {
    let o = RunnerOptions::default();
    assert!((1..=16).contains(&o.analyze.threads.get()));
    assert_eq!(o.analyze.scratch_cap, 1 << 30);
    assert_eq!(o.repair.analyze, o.analyze);
    assert_eq!(o.parked_cap, 1 << 30);
    assert_eq!(o.output_dir, None);
}

// ── Markdown export (T-32b) ─────────────────────────────────────────────

fn is_exported(s: &Seen) -> bool {
    matches!(s, Seen::Exported(_))
}

impl Harness {
    /// Where each of job `id`'s exports went, in order.
    fn exports(&self, id: JobId) -> Vec<PathBuf> {
        self.seen
            .iter()
            .filter_map(|(i, s)| match s {
                Seen::Exported(path) if *i == id => Some(path.clone()),
                _ => None,
            })
            .collect()
    }
}

/// An engine slow enough that an export asked right after `submit` reaches
/// the job before its repair is placed.
fn slow_engine() -> FakeEngine {
    FakeEngine::new().on_analyze(vec![FakeStep::Slow {
        steps: 5,
        pause_ms: 20,
    }])
}

/// The page-one image of the golden, as a repair extracts it.
fn golden_images() -> Vec<(String, Vec<u8>)> {
    vec![("p1-1.jpg".to_owned(), fixtures::TINY_JPEG.to_vec())]
}

fn images_dir(stem: &str, input: &[u8]) -> String {
    place::images_dir_name(stem, &Sha256::digest(input).into())
}

#[test]
fn the_one_shot_flow_repairs_then_exports_the_same_entry() {
    let dir = ScratchDir::new("jobs-oneshot");
    let input = write(&dir, "memo.pdf", &fixtures::golden_pdf());
    let mut h = Harness::new(slow_engine(), opts());
    let id = h.submit(&input);
    assert!(h.runner().export(id));
    h.pump_until("exported", |h| h.count(is_exported) == 1);
    h.drain();

    let steps: Vec<&Seen> = h
        .seen
        .iter()
        .filter(|(i, s)| *i == id && !matches!(s, Seen::Other | Seen::Waiting(_)))
        .map(|(_, s)| s)
        .collect();
    assert_eq!(steps.len(), 6, "{steps:#?}");
    assert_eq!(*steps[0], Seen::Started("analyze"));
    assert_eq!(*steps[1], Seen::AnalyzeDone);
    assert_eq!(*steps[2], Seen::Started("repair"));
    assert!(is_done(steps[3]));
    assert_eq!(*steps[4], Seen::Started("export"));
    assert_eq!(*steps[5], Seen::Exported(dir.join("memo.md")));

    let md = fs::read_to_string(dir.join("memo.md")).expect("md");
    for line in fixtures::GOLDEN_TEXT.iter().flat_map(|p| p.iter()) {
        assert!(md.contains(line), "{line:?} in {md}");
    }
    assert_eq!(dir.names(), ["memo.md", "memo.pdf", "memo.repaired.pdf"]);
}

#[test]
fn export_never_runs_unless_asked() {
    let dir = ScratchDir::new("jobs-noexport");
    let input = write(&dir, "memo.pdf", &fixtures::golden_pdf());
    let mut h = Harness::new(FakeEngine::new().with_images(golden_images()), opts());
    let id = h.submit(&input);
    h.pump_until("done", |h| h.finished(id));
    h.drain();
    assert_eq!(h.count(|s| *s == Seen::Started("export")), 0);
    assert_eq!(dir.names(), ["memo.pdf", "memo.repaired.pdf"]);
}

#[test]
fn an_existing_report_md_survives_and_the_export_takes_2() {
    let dir = ScratchDir::new("jobs-report-md");
    let input = write(&dir, "report.pdf", &fixtures::golden_pdf());
    write(&dir, "report.md", b"the examiner's own notes");
    let mut h = Harness::new(slow_engine(), opts());
    let id = h.submit(&input);
    assert!(h.runner().export(id));
    h.pump_until("exported", |h| h.count(is_exported) == 1);
    assert_eq!(h.exports(id), [dir.join("report (2).md")]);
    assert_eq!(
        fs::read(dir.join("report.md")).expect("kept"),
        b"the examiner's own notes"
    );
}

#[test]
fn two_exports_of_one_input_are_identical_and_write_the_images_once() {
    let dir = ScratchDir::new("jobs-twice");
    let bytes = fixtures::golden_pdf();
    let input = write(&dir, "scan.pdf", &bytes);
    let images = images_dir("scan", &bytes);
    let engine = slow_engine().with_images(golden_images());
    let mut h = Harness::new(engine, opts());
    let id = h.submit(&input);
    assert!(h.runner().export(id));
    h.pump_until("first export", |h| h.count(is_exported) == 1);
    h.pump_until("job ended", |h| h.finished(id));
    let image = dir.join(&images).join("p1-1.jpg");
    let written = fs::metadata(&image)
        .and_then(|m| m.modified())
        .expect("mtime");

    // Asked again once the job has ended: an export-only run.
    assert!(h.runner().export(id));
    h.pump_until("second export", |h| h.count(is_exported) == 2);
    assert_eq!(
        h.exports(id),
        [dir.join("scan.md"), dir.join("scan (2).md")]
    );
    let first = fs::read(dir.join("scan.md")).expect("first");
    let second = fs::read(dir.join("scan (2).md")).expect("second");
    assert_eq!(first, second, "the .md bytes never depend on the disk");
    let link = format!("![]({images}/p1-1.jpg)");
    assert!(String::from_utf8_lossy(&first).contains(&link), "{link}");

    assert_eq!(
        dir.names(),
        [
            "scan (2).md".to_owned(),
            images.clone(),
            "scan.md".to_owned(),
            "scan.pdf".to_owned(),
            "scan.repaired.pdf".to_owned(),
        ],
        "one images directory, one repaired file"
    );
    assert_eq!(
        fs::metadata(&image)
            .and_then(|m| m.modified())
            .expect("mtime"),
        written,
        "the images directory was reused, not rewritten"
    );
    assert_eq!(fs::read(&image).expect("image"), fixtures::TINY_JPEG);
    // The export-only run repaired in memory: no second repaired file, no
    // second RepairDone.
    assert_eq!(h.count_of(id, is_done), 1);
}

#[test]
fn a_foreign_images_directory_never_captures_the_links() {
    let dir = ScratchDir::new("jobs-foreign");
    let bytes = fixtures::golden_pdf();
    let input = write(&dir, "scan.pdf", &bytes);
    let images = images_dir("scan", &bytes);
    fs::create_dir(dir.join(&images)).expect("foreign dir");
    fs::write(dir.join(&images).join("p1-1.jpg"), b"someone else's").expect("foreign file");

    let mut h = Harness::new(slow_engine().with_images(golden_images()), opts());
    let id = h.submit(&input);
    assert!(h.runner().export(id));
    h.pump_until("exported", |h| h.count(is_exported) == 1);

    let base = images.strip_suffix(".images").expect("suffix");
    let ours = format!("{base} (2).images");
    assert_eq!(
        fs::read(dir.join(&ours).join("p1-1.jpg")).expect("ours"),
        fixtures::TINY_JPEG
    );
    assert_eq!(
        fs::read(dir.join(&images).join("p1-1.jpg")).expect("theirs"),
        b"someone else's",
        "the foreign directory is left alone"
    );
    let md = fs::read_to_string(dir.join("scan.md")).expect("md");
    let encoded = ours
        .replace(' ', "%20")
        .replace('(', "%28")
        .replace(')', "%29");
    assert!(md.contains(&format!("![]({encoded}/p1-1.jpg)")), "{md}");
    assert!(!md.contains(&format!("]({images}/")), "{md}");
    let warned: Vec<&String> = h
        .logs
        .iter()
        .filter(|(i, level, _)| *i == id && *level == LogLevel::Warn)
        .map(|(_, _, msg)| msg)
        .collect();
    assert_eq!(warned.len(), 1, "{warned:?}");
    assert!(warned[0].contains(&ours) && warned[0].contains(&images));
}

#[test]
fn a_later_export_replays_the_answers_and_checks_the_input() {
    let dir = ScratchDir::new("jobs-export-later");
    let input = write(&dir, "memo.pdf", &fixtures::golden_pdf());
    let mut h = Harness::new(FakeEngine::new().always_asks(), opts());
    let id = h.submit(&input);
    h.pump_until("asked", |h| h.count(is_asked) == 1);
    assert!(h.runner().reply(id, InteractionReply::UseBest));
    h.pump_until("done", |h| h.finished(id));

    assert!(h.runner().export(id));
    h.pump_until("exported", |h| h.count(is_exported) == 1);
    assert_eq!(h.count(is_asked), 1, "the earlier answer was replayed");
    assert_eq!(
        h.engine.replies(),
        [InteractionReply::UseBest, InteractionReply::UseBest]
    );
    assert_eq!(h.exports(id), [dir.join("memo.md")]);

    // The file changed since: the export refuses it.
    fs::write(&input, b"%PDF-1.4 something else").expect("rewrite");
    assert!(h.runner().export(id));
    h.pump_until("failed", |h| {
        h.count(|s| matches!(s, Seen::Failed(e, false) if e == INPUT_CHANGED)) == 1
    });
}

#[test]
fn an_unknown_or_unstarted_job_cannot_be_exported() {
    let dir = ScratchDir::new("jobs-export-unknown");
    let mut h = Harness::new(slow_engine(), opts());
    assert!(!h.runner().export(JobId(99)));
    let first = h.submit(&write(&dir, "a.pdf", &fixtures::golden_pdf()));
    let queued = h.submit(&write(&dir, "b.pdf", &fixtures::golden_pdf()));
    h.runner().cancel(queued);
    assert!(!h.runner().export(queued));
    h.pump_until("done", |h| h.finished(first));
}
