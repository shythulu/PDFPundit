//! The loop's tests (T-23a): a scripted run on a test backend, the pre-frame
//! drop, the widget's resize request, the queue rows from job events, the
//! run record, the theme chooser and the panic hook's wiring.

use std::collections::BTreeSet;
use std::panic;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};

use crossterm::event::{KeyEventState, MouseButton, MouseEvent, MouseEventKind};

use super::*;
use crate::engine::{AnalysisStateUse, InteractionRequest, LogLevel};
use crate::jobs::{FakeEngine, JobInput, QueueEntry};
use crate::library::RecentStatus;
use crate::panic_guard::{self, SharedWriter};
use crate::place::ScratchDir;
use crate::ui::director::CellPos;
use crate::ui::goldens;
use crate::ui::layout::browse::BrowseState;
use crate::ui::term::TestScreen;

/// How far the fake clock moves per event: a tick, or on Windows the
/// collector's gap, so a typed key is a key by the next event.
#[cfg(not(windows))]
const STEP: Duration = TICK;
#[cfg(windows)]
const STEP: Duration = crate::ui::input::collector::BURST_GAP;

/// Time moves one step per event; the wall clock reads 11:38 in minute
/// `unix / 60`, and counts how often it is read.
struct FakeClock {
    t: Duration,
    unix: u64,
    reads: usize,
}

impl FakeClock {
    fn new() -> FakeClock {
        FakeClock {
            t: Duration::ZERO,
            unix: 1_790_000_000,
            reads: 0,
        }
    }
}

impl Clock for FakeClock {
    fn elapsed(&mut self) -> Duration {
        self.t += STEP;
        self.t
    }

    fn unix_secs(&mut self) -> u64 {
        self.unix
    }

    fn local_hm(&mut self, _unix: u64) -> (u8, u8) {
        self.reads += 1;
        (11, 38)
    }
}

/// One input, then time enough for the Windows collector to give a typed
/// key back as a key (on Unix the tick does nothing).
fn send(app: &mut App, input: Input) {
    app.on_input(input, Duration::ZERO);
    app.tick(Duration::from_secs(1));
}

fn key(c: char) -> Input {
    code(KeyCode::Char(c))
}

fn code(code: KeyCode) -> Input {
    Input::Key(KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

/// An app on a `w × h` test screen with the first frame drawn and the input
/// armed: start-up steps 4 and 5.
fn started(w: u16, h: u16, ui: &config::Ui) -> (App, TestScreen) {
    let mut screen = TestScreen::test(w, h, ColorCaps::TrueColor);
    let mut app = App::new(ui, ColorCaps::TrueColor, PathBuf::from("config.toml"));
    let (w, h) = screen.size().unwrap();
    app.resize(w, h);
    app.refresh(Duration::ZERO);
    app.render(&mut screen, Duration::ZERO).unwrap();
    app.arm();
    (app, screen)
}

/// Runs the loop over `script` with no runner and no store.
fn script(app: &mut App, screen: &mut TestScreen, clock: &mut FakeClock, events: Vec<Input>) {
    let mut events = events.into_iter().map(AppEvent::Input);
    event_loop::<FakeEngine>(app, screen, &mut || events.next(), clock, None, None).unwrap();
}

fn ticks(n: usize) -> Vec<Input> {
    vec![Input::Tick; n]
}

#[test]
fn a_scripted_run_of_ten_ticks() {
    let (mut app, mut screen) = started(112, 38, &config::Ui::default());
    let mut clock = FakeClock::new();
    let rows = screen.rows();
    assert!(rows[37].contains(strings::APP_NAME), "{}", rows[37]);
    assert!(rows[37].contains("112×38"), "{}", rows[37]);

    // Three ticks, `H`, two ticks: the full layout's hint row says "not yet".
    let mut first = ticks(3);
    first.push(key('H'));
    first.extend(ticks(2));
    script(&mut app, &mut screen, &mut clock, first);
    assert_eq!(app.state.clock, Some((11, 38)));
    let rows = screen.rows();
    assert!(rows[35].contains(strings::NOT_YET), "{}", rows[35]);
    assert!(rows[37].contains("11:38"), "{}", rows[37]);

    // A shrink to the widget, Enter, three ticks: the widget is drawn and
    // Enter asked the terminal to grow, once.
    let mut second = vec![Input::Resize(32, 16), code(KeyCode::Enter)];
    second.extend(ticks(3));
    script(&mut app, &mut screen, &mut clock, second);
    assert_eq!(app.state.term_size, (32, 16));
    assert_eq!(screen.raw(), GROW_TO_FULL);
    let rows = screen.rows();
    assert!(
        rows[15].starts_with(&format!(" {}", strings::APP_NAME)),
        "{}",
        rows[15]
    );
    assert!(
        rows[0].chars().skip(32).all(|c| c == ' '),
        "nothing past the tile: {}",
        rows[0]
    );

    // Two more ticks, `q`, and an event that is never reached.
    let mut last = ticks(2);
    last.extend([key('q'), key('H')]);
    script(&mut app, &mut screen, &mut clock, last);
    assert!(app.quit);
    // Ten ticks, `H`, the resize, Enter and `q`: fourteen events. Windows
    // holds a typed key for the collector's gap, so there `q` runs when the
    // next event comes, and that event is the last one taken.
    let taken = if cfg!(windows) { 15 } else { 14 };
    assert_eq!(clock.t, STEP * taken, "the event after q was never taken");
    assert_eq!(clock.reads, 1, "the wall clock is read once a minute");
    assert_eq!(app.state.hint, None, "the key after q never ran");
}

#[test]
fn inputs_before_the_first_frame_are_dropped_and_counted() {
    let mut app = App::new(&config::Ui::default(), ColorCaps::TrueColor, PathBuf::new());
    app.resize(112, 38);
    send(&mut app, Input::Paste("/tmp/a.pdf".into()));
    send(&mut app, key('q'));
    send(&mut app, key('H'));
    assert_eq!(app.pre_frame_dropped, 3);
    assert!(!app.quit && app.state.hint.is_none(), "nothing was handled");
    app.arm();
    assert!(app.debug_log.iter().any(|l| l.contains("dropped 3 inputs")));
    send(&mut app, Input::Paste("/tmp/a.pdf".into()));
    send(&mut app, key('H'));
    assert_eq!(app.pre_frame_dropped, 3, "armed: nothing more is dropped");
    assert_eq!(app.state.hint, Some(strings::NOT_YET));
}

fn needs_you_request() -> InteractionRequest {
    let batch = AppState::mockup_batch();
    match &batch.batch.entries[2].state {
        EntryState::WaitingOnUser(r) => r.clone(),
        other => panic!("frame 03's thesis waits on the user, not {other:?}"),
    }
}

fn ask(app: &mut App, id: u64) {
    let (reply, _) = mpsc::sync_channel(1);
    let ev = JobEvent::NeedsInteraction {
        request: needs_you_request(),
        reply,
    };
    app.on_job(JobId(id), ev, 0, None);
    app.refresh(Duration::ZERO);
}

fn queue(app: &mut App, n: u64) {
    for id in 1..=n {
        let name = format!("f{id}.pdf");
        app.state
            .batch
            .entries
            .push(QueueEntry::queued(JobId(id), name, None, 10));
    }
}

#[test]
fn the_widget_asks_to_grow_once_when_a_file_first_needs_you() {
    let (mut app, _) = started(32, 16, &config::Ui::default());
    queue(&mut app, 2);
    ask(&mut app, 1);
    assert_eq!(app.out, GROW_TO_FULL);
    ask(&mut app, 2);
    assert_eq!(app.out, GROW_TO_FULL, "still needs you: no second request");

    // Answered, then asked again: a new request.
    app.state.batch.entries[0].state = EntryState::Done;
    app.state.batch.entries[1].state = EntryState::Done;
    app.refresh(Duration::ZERO);
    ask(&mut app, 1);
    assert_eq!(app.out, [GROW_TO_FULL, GROW_TO_FULL].concat());

    let quiet = config::Ui {
        request_resize: false,
        ..config::Ui::default()
    };
    let (mut app, _) = started(32, 16, &quiet);
    queue(&mut app, 1);
    ask(&mut app, 1);
    send(&mut app, code(KeyCode::Enter));
    assert!(app.out.is_empty(), "request_resize is off");

    let (mut app, _) = started(112, 38, &config::Ui::default());
    queue(&mut app, 1);
    ask(&mut app, 1);
    send(&mut app, code(KeyCode::Enter));
    assert!(
        app.out.is_empty(),
        "the full layout already shows the question"
    );
}

#[test]
fn job_events_fill_the_queue_rows() {
    let mut app = App::new(&config::Ui::default(), ColorCaps::TrueColor, PathBuf::new());
    queue(&mut app, 2);
    let started = |kind| JobEvent::Started {
        kind,
        name: "f1.pdf".into(),
        file: None,
    };
    let one = JobId(1);
    app.on_job(one, started(JobKind::Analyze), 5, None);
    assert_eq!(app.state.batch.current, Some(0));
    app.on_job(
        one,
        JobEvent::Phase {
            name: "carving",
            index: 1,
            total: 3,
        },
        5,
        None,
    );
    app.on_job(
        one,
        JobEvent::Progress {
            done: 7,
            total: Some(9),
        },
        5,
        None,
    );
    assert_eq!(
        app.state.batch.entries[0].state,
        EntryState::Analyzing {
            phase: Some("carving"),
            done: 7,
            total: Some(9)
        }
    );
    app.on_job(
        one,
        JobEvent::Log(LogLevel::Warn, "odd xref".into()),
        5,
        None,
    );
    assert!(app.debug_log.iter().any(|l| l.contains("odd xref")));

    let thesis = AppState::mockup_result().batch.entries[2].clone();
    let finding = thesis.findings[0].clone();
    app.on_job(one, JobEvent::Finding(finding.clone()), 5, None);
    assert_eq!(app.state.batch.entries[0].findings, [finding]);

    ask(&mut app, 1);
    assert!(matches!(
        app.state.batch.entries[0].state,
        EntryState::WaitingOnUser(_)
    ));
    app.on_job(one, JobEvent::Parked, 5, None);
    assert_eq!(app.state.batch.current, None);
    app.on_job(one, JobEvent::Resumed, 5, None);
    assert!(matches!(
        app.state.batch.entries[0].state,
        EntryState::Repairing { .. }
    ));
    let run = thesis.run.clone().unwrap();
    app.on_job(one, JobEvent::RepairDone(Box::new(run.clone())), 5, None);
    assert_eq!(app.state.batch.entries[0].state, EntryState::Done);
    assert_eq!(app.state.batch.entries[0].run, Some(run));

    let two = JobId(2);
    app.on_job(two, started(JobKind::Analyze), 5, None);
    assert_eq!(app.state.batch.current, Some(1));
    app.on_job(
        two,
        JobEvent::Failed {
            error: "boom".into(),
            panicked: true,
        },
        5,
        None,
    );
    assert_eq!(
        app.state.batch.entries[1].state,
        EntryState::Failed {
            error: "boom".into(),
            panicked: true
        }
    );
    assert_eq!(app.state.batch.current, None);
    // An event for a job with no row changes nothing.
    let before = app.state.clone();
    app.on_job(JobId(9), JobEvent::Cancelled, 5, None);
    assert_eq!(app.state, before);
}

#[test]
fn a_finished_repair_is_recorded_and_the_history_read_again() {
    let dir = ScratchDir::new("app-history");
    let mut store = JsonStore::open_at(dir.join("history")).unwrap();
    let mut app = App::new(
        &config::Ui::default(),
        ColorCaps::TrueColor,
        PathBuf::from("/cfg/config.toml"),
    );
    let mut thesis = AppState::mockup_result().batch.entries[2].clone();
    let run = thesis.run.take().unwrap();
    thesis.state = EntryState::Queued;
    thesis.job = JobId(1);
    app.state.batch.entries.push(thesis);

    let repair = JobEvent::Started {
        kind: JobKind::Repair {
            passes: None,
            state: Default::default(),
        },
        name: "thesis_ar.pdf".into(),
        file: None,
    };
    app.on_job(JobId(1), repair, 100, Some(&mut store));
    app.on_job(
        JobId(1),
        JobEvent::RepairDone(Box::new(run.clone())),
        160,
        Some(&mut store),
    );

    assert_eq!(app.state.history, store.summary());
    assert_eq!((app.state.history.files, app.state.history.runs), (1, 1));
    assert_eq!(app.state.history.recent[0].name, "thesis_ar.pdf");
    assert_eq!(app.state.history.recent[0].status, RecentStatus::Repaired);
    let runs = store.runs_for(&run.report.input_sha256).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].started_at, 100, "when the repair started");
    assert_eq!(runs[0].config_path, PathBuf::from("/cfg/config.toml"));
    assert_eq!(runs[0].report, run.report);
    assert_eq!(runs[0].analysis_state, AnalysisStateUse::Reused);
}

#[test]
fn the_loop_drives_the_runner_to_a_finished_row() {
    let dir = ScratchDir::new("app-runner");
    let input = dir.join("a.pdf");
    std::fs::write(&input, b"%PDF-1.7\n%%EOF\n").unwrap();
    let (mut app, mut screen) = started(112, 38, &config::Ui::default());
    let (tx, rx) = mpsc::channel();
    let mut runner = JobRunner::new(Arc::new(FakeEngine::new()), tx, RunnerOptions::default());
    let id = runner.submit(JobInput::File(input.clone()));
    app.state
        .batch
        .entries
        .push(QueueEntry::queued(id, "a.pdf".into(), Some(input), 15));

    let mut finished = false;
    let mut next = || {
        if finished {
            return None;
        }
        let ev = rx.recv_timeout(Duration::from_secs(20)).ok()?;
        finished = matches!(
            ev,
            AppEvent::Job(_, JobEvent::RepairDone(_) | JobEvent::Failed { .. })
        );
        Some(ev)
    };
    let mut clock = FakeClock::new();
    event_loop(
        &mut app,
        &mut screen,
        &mut next,
        &mut clock,
        Some(&mut runner),
        None,
    )
    .unwrap();
    assert_eq!(app.state.batch.entries[0].state, EntryState::Done);
    assert!(app.state.batch.entries[0].run.is_some());
    assert!(runner.shutdown(Duration::from_secs(5)));
}

#[test]
fn keys_quit_open_the_chooser_and_move_the_cursor() {
    let (mut app, _) = started(112, 38, &config::Ui::default());
    let first = app.theme.name;
    send(&mut app, key('T'));
    assert_eq!(app.state.screen, Screen::Themes { selected: 0 });
    send(&mut app, code(KeyCode::Down));
    assert_eq!(app.state.screen, Screen::Themes { selected: 1 });
    assert_eq!(app.theme.name, Theme::all()[1].name, "tried on the spot");
    send(&mut app, code(KeyCode::Esc));
    assert_eq!(app.state.screen, Screen::Main);
    assert_eq!(app.theme.name, first, "Esc goes back");
    send(&mut app, key('T'));
    send(&mut app, code(KeyCode::Down));
    send(&mut app, code(KeyCode::Down));
    send(&mut app, key('q'));
    assert!(!app.quit, "q is not a chooser key");
    send(&mut app, code(KeyCode::Up));
    send(&mut app, code(KeyCode::Enter));
    assert_eq!(app.theme.name, Theme::all()[1].name, "Enter keeps it");
    assert_eq!(app.state.screen, Screen::Main);

    for (k, hint) in [('S', Some(strings::NOT_YET)), ('?', Some(strings::NOT_YET))] {
        send(&mut app, key(k));
        assert_eq!(app.state.hint, hint);
    }
    send(&mut app, code(KeyCode::Left));
    assert_eq!(app.state.hint, None, "a one-off hint lasts one key");
    send(&mut app, key('b'));
    assert!(matches!(app.state.screen, Screen::Browse(_)));
    send(&mut app, key('q'));
    assert!(!app.quit, "q is not a picker key");
    send(&mut app, code(KeyCode::Esc));
    assert_eq!(app.state.screen, Screen::Main);

    queue(&mut app, 3);
    send(&mut app, code(KeyCode::Down));
    send(&mut app, code(KeyCode::Down));
    send(&mut app, code(KeyCode::Down));
    assert_eq!(app.state.selected, Some(2));
    send(&mut app, code(KeyCode::Up));
    assert_eq!(app.state.selected, Some(1));
    let click = Input::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 1,
        modifiers: KeyModifiers::NONE,
    });
    send(&mut app, click);
    assert!(!app.quit);

    let mut ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    ctrl_c.kind = KeyEventKind::Press;
    send(&mut app, Input::Key(ctrl_c));
    assert!(app.quit);
}

#[test]
fn a_ui_thread_panic_writes_the_restore_sequence_once() {
    let _serial = panic_guard::test_lock();
    let original = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let writer = SharedWriter::default();
    // What `shell` installs, with the fake writer for the terminal.
    panic_guard::install(
        thread::current().id(),
        term::restore_sequence(false),
        Box::new(writer.clone()),
    );
    for _ in 0..2 {
        assert!(panic::catch_unwind(|| panic!("ui boom")).is_err());
    }
    panic_guard::uninstall();
    panic::set_hook(original);

    let want = term::restore_sequence(false);
    assert_eq!(writer.bytes(), want);
    for part in [&b"\x1b[?1049l"[..], b"\x1b[?25h"] {
        assert_eq!(
            want.windows(part.len()).filter(|w| *w == part).count(),
            1,
            "{part:?}"
        );
    }
    if cfg!(unix) {
        assert!(want.windows(8).any(|w| w == b"\x1b[?2004l"));
    }
}

#[test]
fn the_runner_options_follow_the_config() {
    let mut config = Config::default();
    config.repair.salvage_work = 123;
    config.general.output_dir = OutputDir::Dir(PathBuf::from("/out"));
    let opts = runner_options(&config);
    assert_eq!(opts.analyze.salvage_budget.work, 123);
    assert_eq!(opts.repair.analyze.salvage_budget.work, 123);
    assert_eq!(opts.output_dir, Some(PathBuf::from("/out")));
    let defaults = RunnerOptions::default();
    assert_eq!(opts.analyze.threads, defaults.analyze.threads);
    assert_eq!(opts.parked_cap, defaults.parked_cap);
    assert_eq!(runner_options(&Config::default()).output_dir, None);
}

#[test]
fn every_layout_kind_draws() {
    let mut seen = BTreeSet::new();
    for (w, h) in [(112, 38), (32, 16), (20, 1)] {
        let (app, screen) = started(w, h, &config::Ui::default());
        seen.insert(format!("{:?}", app.kind));
        assert!(screen.rows().iter().any(|r| r.trim() != ""), "{w}×{h}");
    }
    assert_eq!(seen.len(), 3);
}

#[test]
fn the_theme_chooser_is_drawn_over_the_full_layout() {
    let mut app = App::new(&config::Ui::default(), ColorCaps::TrueColor, PathBuf::new());
    app.state = AppState::mockup_idle();
    let (w, h) = app.state.term_size;
    app.resize(w, h);
    app.refresh(Duration::ZERO);
    app.arm();
    let frame = |app: &App| app.draw(&app.shown_at(Duration::ZERO));
    frame(&app).assert_matches(&goldens::load("01-idle"));
    send(&mut app, key('T'));
    app.refresh(Duration::ZERO);
    frame(&app).assert_matches(&goldens::load("06-theme-chooser"));

    // The widget has no room for it: `T` opens nothing there, and a shrink
    // closes an open chooser as Esc would.
    send(&mut app, code(KeyCode::Down));
    assert_ne!(app.theme_index, app.theme_before);
    send(&mut app, Input::Resize(32, 16));
    assert_eq!(app.state.screen, Screen::Main);
    assert_eq!(app.theme_index, app.theme_before);
    send(&mut app, key('T'));
    assert_eq!(app.state.screen, Screen::Main);
}

#[test]
fn browse_is_inert_in_the_one_line_fallback() {
    let (mut app, _) = started(20, 1, &config::Ui::default());
    send(&mut app, key('b'));
    assert_eq!(app.state.screen, Screen::Main, "b at 20×1 opens nothing");
    assert_eq!(app.state.hint, None);
    for (w, h) in [(32, 16), (112, 38)] {
        let (mut app, _) = started(w, h, &config::Ui::default());
        send(&mut app, key('B'));
        assert!(matches!(app.state.screen, Screen::Browse(_)), "{w}×{h}");
    }
}

// ── the browse picker (T-37) ────────────────────────────────────────────

/// An app on `w × h` whose picker opens on `dir`.
fn browsing_in(dir: &Path, w: u16, h: u16) -> App {
    let (mut app, _) = started(w, h, &config::Ui::default());
    app.browse_from = dir.to_path_buf();
    app
}

/// The ticket's fixture: `a.pdf`, `b.PDF`, `c.txt`, `sub/`.
fn browse_fixture(label: &str) -> ScratchDir {
    let dir = ScratchDir::new(label);
    pdf(&dir, "a.pdf");
    pdf(&dir, "b.PDF");
    std::fs::write(dir.join("c.txt"), b"%PDF-1.7\n").unwrap();
    std::fs::create_dir(dir.join("sub")).unwrap();
    dir
}

#[test]
fn confirming_two_picked_files_chomps_once_and_submits_both() {
    let dir = browse_fixture("app-browse");
    let mut app = browsing_in(dir.path(), 112, 38);
    send(&mut app, key('b'));
    let Screen::Browse(picker) = &app.state.screen else {
        panic!("b opens the picker");
    };
    let shown: Vec<String> = picker.entries.iter().map(|e| e.display()).collect();
    assert_eq!(shown, ["sub", "a.pdf", "b.PDF"], "two files and one folder");

    // `a` picks both, three downs reach the button, Enter feeds the cat.
    send(&mut app, key('a'));
    for _ in 0..3 {
        send(&mut app, code(KeyCode::Down));
    }
    assert!(app.admitted.is_empty(), "nothing goes in before the button");
    send(&mut app, code(KeyCode::Enter));
    assert_eq!(app.state.screen, Screen::Main, "confirming closes it");
    assert_eq!(app.state.hint, None);

    let mut submitted = Vec::new();
    let now = Duration::from_secs(3);
    app.submit_drops(now, &mut |input| {
        submitted.push(input);
        JobId(submitted.len() as u64)
    });
    let want = [dir.join("a.pdf"), dir.join("b.PDF")].map(JobInput::File);
    assert_eq!(submitted, want, "two submits, in name order");
    let frame = app.director.frame(now);
    assert_eq!(
        (frame.n, frame.caption),
        (2, "the drop: plop"),
        "one Dropped{{2}}"
    );
    let names: Vec<&str> = app
        .state
        .batch
        .entries
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(names, ["a.pdf", "b.PDF"]);
    app.submit_drops(now, &mut |_| panic!("submitted twice"));
}

/// The picker's files take the paste's road: a file gone by the time the
/// button is pressed is refused by the gate and named on the hint row.
#[test]
fn picked_files_pass_the_drop_gate() {
    let dir = browse_fixture("app-browse-gate");
    let mut app = browsing_in(dir.path(), 112, 38);
    send(&mut app, key('b'));
    send(&mut app, code(KeyCode::Down));
    send(&mut app, code(KeyCode::Enter));
    std::fs::remove_file(dir.join("a.pdf")).unwrap();
    send(&mut app, code(KeyCode::Enter));
    assert_eq!(app.state.screen, Screen::Main);
    assert!(app.admitted.is_empty());
    assert_eq!(app.state.hint, Some(strings::DROP_UNREADABLE));
}

#[test]
fn the_picker_closes_on_esc_and_on_a_shrink_to_one_line() {
    let dir = browse_fixture("app-browse-close");
    let mut app = browsing_in(dir.path(), 32, 16);
    send(&mut app, key('b'));
    send(&mut app, code(KeyCode::Enter));
    send(&mut app, code(KeyCode::Esc));
    assert_eq!(app.state.screen, Screen::Main);
    assert!(app.admitted.is_empty(), "Esc feeds nothing");
    // It opens again where it was closed.
    send(&mut app, key('b'));
    let Screen::Browse(picker) = &app.state.screen else {
        panic!("open");
    };
    assert_eq!(picker.cwd, dir.join("sub"));
    // Full and widget keep it; the one-line fallback closes it.
    send(&mut app, Input::Resize(112, 38));
    assert!(matches!(app.state.screen, Screen::Browse(_)));
    send(&mut app, Input::Resize(20, 1));
    assert_eq!(app.state.screen, Screen::Main);
    send(&mut app, key('b'));
    assert_eq!(app.state.screen, Screen::Main);
}

/// A path pasted while the picker is open is a drop: it goes through the
/// gate and the picker stays as it was, its picks untouched.
#[test]
fn a_paste_in_the_picker_is_a_drop_and_leaves_the_picks_alone() {
    let dir = browse_fixture("app-browse-paste");
    let mut app = browsing_in(dir.path(), 112, 38);
    send(&mut app, key('b'));
    send(&mut app, paste_of(&[&dir.join("a.pdf")]));
    assert_eq!(app.admitted.len(), 1);
    let Screen::Browse(picker) = &app.state.screen else {
        panic!("the picker stays open");
    };
    assert!(picker.selected.is_empty());
    assert_eq!((picker.cursor, picker.hidden), (0, false));
}

/// Windows: a path dropped as a burst of keys while the picker is open goes
/// through the collector like on the main screen, so its `a`s, spaces and
/// `.`s are never read as picker keys.
#[cfg(windows)]
#[test]
fn a_typed_burst_in_the_picker_is_a_drop_on_windows() {
    let dir = browse_fixture("app-browse-burst");
    let dropped = pdf(&dir, "a b.pdf");
    let mut app = browsing_in(dir.path(), 112, 38);
    send(&mut app, key('b'));
    let text = format!("\"{}\"", dropped.display());
    for (i, c) in text.chars().enumerate() {
        app.on_input(
            key(c),
            Duration::from_secs(10) + Duration::from_millis(i as u64),
        );
    }
    app.tick(Duration::from_secs(20));
    assert_eq!(app.admitted.len(), 1);
    assert_eq!(app.admitted[0].path, dropped);
    let Screen::Browse(picker) = &app.state.screen else {
        panic!("the picker stays open");
    };
    assert!(picker.selected.is_empty(), "no `a` picked anything");
    assert_eq!((picker.cursor, picker.hidden), (0, false));
}

/// A sample picker over the mockup's names, independent of any disk.
fn sample_picker() -> BrowseState {
    use crate::ui::fs::Entry;
    let entry = |name: &str, is_dir: bool, size: u64| Entry {
        name: name.into(),
        is_dir,
        size,
    };
    let cwd = PathBuf::from("/evidence/case-2291");
    BrowseState {
        entries: vec![
            entry("2024-q3", true, 0),
            entry("scans", true, 0),
            entry("contract_signed.pdf", false, 245_000),
            entry("invoice_scan.pdf", false, 1_100_000),
            entry("minutes_q3.pdf", false, 180_000),
            entry("report_2024.pdf", false, 512_000),
            entry("thesis_ar.pdf", false, 393_000),
        ],
        cursor: 5,
        selected: [cwd.join("invoice_scan.pdf"), cwd.join("thesis_ar.pdf")]
            .into_iter()
            .collect(),
        cwd,
        ..BrowseState::default()
    }
}

/// The picker's look, over the idle screen and across the widget, against
/// self-goldens (D-048: no mockup frame exists for it yet).
#[test]
fn the_picker_matches_its_self_goldens() {
    for (name, size) in [("browse-full", (112, 38)), ("browse-widget", (32, 16))] {
        let mut app = App::new(&config::Ui::default(), ColorCaps::TrueColor, PathBuf::new());
        app.state = AppState::mockup_idle();
        app.resize(size.0, size.1);
        app.refresh(Duration::ZERO);
        app.state.screen = Screen::Browse(sample_picker());
        let c = app.draw(&app.shown_at(Duration::ZERO));
        goldens::assert_self_golden(&c, name);
    }
}

#[test]
fn the_full_status_bar_says_browsing() {
    let mut app = App::new(&config::Ui::default(), ColorCaps::TrueColor, PathBuf::new());
    app.state = AppState::mockup_idle();
    app.resize(112, 38);
    app.refresh(Duration::ZERO);
    app.state.screen = Screen::Browse(sample_picker());
    let c = app.draw(&app.shown_at(Duration::ZERO));
    let bar: String = (0..112).map(|x| c.get(x, 37).expect("cell").ch).collect();
    assert!(bar.contains(strings::BROWSING), "{bar}");
}

// ── drops (T-23b) ───────────────────────────────────────────────────────

/// Writes a small PDF named `name` into `dir`.
fn pdf(dir: &ScratchDir, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, b"%PDF-1.7\n%%EOF\n").unwrap();
    path
}

/// A paste of `paths`, each double-quoted as a terminal would. Double
/// quotes, not single: the Windows style reads `'` as a name character.
/// Scratch paths hold no `"`, `$` or `` ` ``, and a `\` in one is a Windows
/// path, where it is a name character too.
fn paste_of(paths: &[&Path]) -> Input {
    let items: Vec<String> = paths
        .iter()
        .map(|p| format!("\"{}\"", p.display()))
        .collect();
    Input::Paste(items.join(" "))
}

/// Runs `events` through the loop with `runner`, then the runner's events
/// until every job that started has finished (and, when none has, until
/// nothing comes for a second).
fn script_with_runner(
    app: &mut App,
    screen: &mut TestScreen,
    runner: &mut JobRunner<FakeEngine>,
    rx: &Receiver<AppEvent<Input>>,
    events: Vec<Input>,
) {
    let mut events = events.into_iter();
    let (mut started, mut finished) = (0, 0);
    let mut next = || {
        if let Some(input) = events.next() {
            return Some(AppEvent::Input(input));
        }
        let wait = if started > finished {
            Duration::from_secs(20)
        } else {
            Duration::from_secs(1)
        };
        let ev = rx.recv_timeout(wait).ok()?;
        match &ev {
            AppEvent::Job(
                _,
                JobEvent::Started {
                    kind: JobKind::Analyze | JobKind::ExportMarkdown,
                    ..
                },
            ) => started += 1,
            AppEvent::Job(
                _,
                JobEvent::RepairDone(_)
                | JobEvent::ExportDone { .. }
                | JobEvent::Failed { .. }
                | JobEvent::Cancelled,
            ) => finished += 1,
            _ => {}
        }
        Some(ev)
    };
    let mut clock = FakeClock::new();
    event_loop(app, screen, &mut next, &mut clock, Some(runner), None).unwrap();
}

#[test]
fn a_pasted_pdf_is_chomped_analysed_and_repaired() {
    let dir = ScratchDir::new("app-paste");
    let a = pdf(&dir, "a b.pdf");
    let b = pdf(&dir, "B.PDF");
    let (mut app, mut screen) = started(112, 38, &config::Ui::default());
    let (tx, rx) = mpsc::channel();
    let mut runner = JobRunner::new(Arc::new(FakeEngine::new()), tx, RunnerOptions::default());

    let paste = paste_of(&[&a, &b, Path::new("/nowhere/notes.txt")]);
    script_with_runner(&mut app, &mut screen, &mut runner, &rx, vec![paste]);

    let rows = &app.state.batch.entries;
    let names: Vec<&str> = rows.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["a b.pdf", "B.PDF"], "in paste order");
    assert_eq!(rows[0].path.as_deref(), Some(a.as_path()));
    for row in rows {
        assert_eq!(row.state, EntryState::Done, "{}", row.name);
        assert!(row.run.is_some(), "analysed, then repaired: {}", row.name);
    }
    assert_eq!(
        app.state.hint,
        Some(strings::DROP_NOT_A_PDF),
        "the refused item is named on the hint row"
    );
    assert!(runner.shutdown(Duration::from_secs(5)));
}

#[test]
fn a_drop_makes_the_cat_chomp_n_files() {
    let dir = ScratchDir::new("app-chomp");
    let paths = [pdf(&dir, "1.pdf"), pdf(&dir, "2.pdf"), pdf(&dir, "3.pdf")];
    let (mut app, _) = started(112, 38, &config::Ui::default());
    let refs: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
    send(&mut app, paste_of(&refs));
    let mut submitted = Vec::new();
    let now = Duration::from_secs(3);
    app.submit_drops(now, &mut |input| {
        submitted.push(input);
        JobId(submitted.len() as u64)
    });
    let want: Vec<JobInput> = paths.iter().cloned().map(JobInput::File).collect();
    assert_eq!(submitted, want);
    let frame = app.director.frame(now);
    assert_eq!(frame.n, 3);
    assert_eq!(frame.caption, "the drop: plop");
    let ids: Vec<JobId> = app.state.batch.entries.iter().map(|e| e.job).collect();
    assert_eq!(ids, [JobId(1), JobId(2), JobId(3)]);

    // Nothing more was let in: nothing more is submitted.
    app.submit_drops(now, &mut |_| panic!("submitted twice"));
}

#[test]
fn a_clean_drop_clears_the_last_refusal() {
    let dir = ScratchDir::new("app-hint");
    let a = pdf(&dir, "a.pdf");
    let (mut app, _) = started(112, 38, &config::Ui::default());
    send(&mut app, paste_of(&[Path::new("/nowhere/notes.txt")]));
    assert_eq!(app.state.hint, Some(strings::DROP_NOT_A_PDF));
    send(&mut app, paste_of(&[&a]));
    assert_eq!(
        app.state.hint, None,
        "the good drop is not shown as refused"
    );
    assert_eq!(app.admitted.len(), 1);
}

#[test]
fn a_paste_in_the_one_line_layout_is_refused_until_the_widget_fits() {
    let dir = ScratchDir::new("app-oneline");
    let a = pdf(&dir, "a.pdf");
    let (mut app, mut screen) = started(20, 1, &config::Ui::default());
    let (tx, rx) = mpsc::channel();
    let mut runner = JobRunner::new(Arc::new(FakeEngine::new()), tx, RunnerOptions::default());

    script_with_runner(
        &mut app,
        &mut screen,
        &mut runner,
        &rx,
        vec![paste_of(&[&a])],
    );
    assert!(app.state.batch.entries.is_empty(), "nothing submitted");
    assert_eq!(app.refused_too_small, 1, "counted as refused");
    assert_eq!(app.state.hint, Some(strings::TOO_SMALL_TO_EAT));
    assert_eq!(screen.raw(), GROW_TO_WIDGET, "one resize request");

    let events = vec![Input::Resize(32, 16), paste_of(&[&a])];
    script_with_runner(&mut app, &mut screen, &mut runner, &rx, events);
    assert_eq!(app.state.batch.entries.len(), 1, "the widget takes it");
    assert_eq!(app.state.batch.entries[0].state, EntryState::Done);
    assert_eq!(app.refused_too_small, 1);
    assert_eq!(screen.raw(), GROW_TO_WIDGET, "no second request");
    assert!(runner.shutdown(Duration::from_secs(5)));

    // With request_resize off the refusal asks for nothing.
    let quiet = config::Ui {
        request_resize: false,
        ..config::Ui::default()
    };
    let (mut app, _) = started(20, 1, &quiet);
    send(&mut app, paste_of(&[&a, &a]));
    assert_eq!(app.refused_too_small, 2);
    assert!(app.out.is_empty());
    assert!(app.admitted.is_empty());
}

/// Unix only: NTFS allocates what `set_len` asks for, and the gate's own
/// test already makes these two files on every target.
#[cfg(unix)]
#[test]
fn big_files_are_let_in_with_a_warning_and_huge_ones_refused() {
    let dir = ScratchDir::new("app-size");
    let sparse = |name: &str, len: u64| {
        let path = pdf(&dir, name);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(len)
            .unwrap();
        path
    };
    let huge = sparse("huge.pdf", 5 << 30);
    let big = sparse("big.pdf", 600 << 20);
    let (mut app, _) = started(112, 38, &config::Ui::default());

    send(&mut app, paste_of(&[&huge]));
    assert!(app.admitted.is_empty());
    assert_eq!(app.state.hint, Some(strings::DROP_TOO_BIG));

    send(&mut app, paste_of(&[&big]));
    assert_eq!(app.admitted.len(), 1);
    assert_eq!(app.state.hint, Some(strings::DROP_BIG));
    assert!(
        app.debug_log.iter().any(|l| l.contains("600 MiB")),
        "the log names the size"
    );
    assert!(
        app.debug_log
            .iter()
            .any(|l| l.contains("%PDF- in the first KiB: true"))
    );
}

#[cfg(unix)]
#[test]
fn typed_keys_never_make_a_path_on_unix() {
    let dir = ScratchDir::new("app-typed");
    let a = pdf(&dir, "a.pdf");
    let (mut app, _) = started(32, 16, &config::Ui::default());
    let text = a.display().to_string();
    for c in text.chars() {
        app.on_input(key(c), Duration::ZERO);
    }
    app.on_input(code(KeyCode::Enter), Duration::ZERO);
    app.tick(Duration::from_secs(1));
    assert!(app.admitted.is_empty(), "only a bracketed paste is a paste");
}

#[cfg(windows)]
#[test]
fn a_typed_burst_is_a_paste_on_windows() {
    let dir = ScratchDir::new("app-typed");
    let a = pdf(&dir, "a b.pdf");
    let (mut app, _) = started(32, 16, &config::Ui::default());
    let text = format!("\"{}\"", a.display());
    for (i, c) in text.chars().enumerate() {
        app.on_input(key(c), Duration::from_millis(i as u64));
    }
    assert!(app.admitted.is_empty(), "still coming");
    app.tick(Duration::from_secs(5));
    assert_eq!(app.admitted.len(), 1);
    assert_eq!(app.admitted[0].0.path, a);
    assert!(!app.quit);
}

// ── kitty drops (T-31) ──────────────────────────────────────────────────

const ACCEPT: &[u8] = b"\x1b]72;t=m:o=1;text/uri-list\x1b\\";
const DECLINE: &[u8] = b"\x1b]72;t=m:o=0\x1b\\";
const REQUEST: &[u8] = b"\x1b]72;t=r:x=1\x1b\\";
const DONE: &[u8] = b"\x1b]72;t=r:o=1\x1b\\";
const CANCELLED: &[u8] = b"\x1b]72;t=r:o=0\x1b\\";

fn drag(x: u16, y: u16) -> Input {
    Input::Dnd(DndEvent::Move {
        cell: Some(CellPos { x, y }),
        copy: true,
        mimes: Some(vec!["text/uri-list".into()]),
    })
}

fn drop_on(x: u16, y: u16) -> Input {
    Input::Dnd(DndEvent::Drop {
        cell: Some(CellPos { x, y }),
        copy: true,
        mimes: vec!["text/uri-list".into(), "text/plain".into()],
    })
}

/// The `text/uri-list` naming `paths`.
fn uri_list(paths: &[&Path]) -> Input {
    use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
    const KEEP: &percent_encoding::AsciiSet = &NON_ALPHANUMERIC
        .remove(b'/')
        .remove(b'.')
        .remove(b'-')
        .remove(b'_');
    let list: String = paths
        .iter()
        .map(|p| {
            let p = p.to_str().expect("a UTF-8 scratch path");
            format!("file://{}\r\n", utf8_percent_encode(p, KEEP))
        })
        .collect();
    Input::Dnd(DndEvent::Data {
        idx: Some(1),
        data: Ok(list.into_bytes()),
    })
}

fn at_rest(app: &App, now: Duration) -> bool {
    app.director.frame(now) == Director::new().frame(now)
}

/// The mock terminal: every byte the loop wrote, as the terminal reads it.
fn wire(screen: &TestScreen) -> Vec<u8> {
    screen.raw().to_vec()
}

#[test]
fn a_kitty_drop_on_the_cat_is_read_before_it_completes() {
    let dir = ScratchDir::new("app-kitty");
    let a = pdf(&dir, "a b.pdf");
    let (mut app, mut screen) = started(112, 38, &config::Ui::default());
    let mut clock = FakeClock::new();

    // The first move over the cat is answered; the cat watches the drag.
    script(&mut app, &mut screen, &mut clock, vec![drag(50, 20)]);
    assert_eq!(wire(&screen), ACCEPT);
    assert!(!at_rest(&app, Duration::from_secs(1)), "the cat watches");

    // The drop asks for the list; the list's file is read, then completed.
    script(
        &mut app,
        &mut screen,
        &mut clock,
        vec![drop_on(50, 20), uri_list(&[&a])],
    );
    assert_eq!(wire(&screen), [ACCEPT, REQUEST, DONE].concat());
    assert_eq!(app.admitted.len(), 1);
    assert_eq!(app.admitted[0].0.path, a);
    let want = std::fs::read(&a).unwrap();
    assert_eq!(app.admitted[0].1.as_deref(), Some(want.as_slice()));

    // The file goes away when the drop ends (a file promise); its bytes
    // were taken while it was there. The runner pins them and, with the path
    // gone, never writes beside it (D-039, D-061; runner_tests.rs).
    std::fs::remove_file(&a).unwrap();
    let mut submitted = Vec::new();
    app.submit_drops(Duration::from_secs(2), &mut |input| {
        submitted.push(input);
        JobId(1)
    });
    assert_eq!(
        submitted,
        [JobInput::Dropped {
            path: a.clone(),
            bytes: want
        }]
    );
    assert_eq!(app.director.frame(Duration::from_secs(2)).n, 1);
}

#[test]
fn off_the_cat_is_declined_and_a_drop_there_cancelled() {
    let dir = ScratchDir::new("app-kitty-off");
    let a = pdf(&dir, "a.pdf");
    let (mut app, mut screen) = started(112, 38, &config::Ui::default());
    let mut clock = FakeClock::new();
    script(
        &mut app,
        &mut screen,
        &mut clock,
        vec![drag(5, 5), drag(6, 5), drag(50, 20), drag(100, 20)],
    );
    assert_eq!(wire(&screen), [DECLINE, ACCEPT, DECLINE].concat());
    script(
        &mut app,
        &mut screen,
        &mut clock,
        vec![drop_on(100, 20), uri_list(&[&a])],
    );
    assert_eq!(
        wire(&screen),
        [DECLINE, ACCEPT, DECLINE, CANCELLED].concat(),
        "no request, and the unasked-for list is ignored"
    );
    assert!(app.admitted.is_empty());
    assert!(at_rest(&app, Duration::from_secs(5)));

    // The widget takes drops over its cat; the one-line fallback nowhere.
    let (mut app, mut screen) = started(32, 16, &config::Ui::default());
    script(
        &mut app,
        &mut screen,
        &mut clock,
        vec![drag(10, 5), drag(10, 15)],
    );
    assert_eq!(wire(&screen), [ACCEPT, DECLINE].concat());
    let (mut app, mut screen) = started(20, 1, &config::Ui::default());
    script(&mut app, &mut screen, &mut clock, vec![drag(0, 0)]);
    assert_eq!(wire(&screen), DECLINE);
}

#[test]
fn a_drop_the_terminal_ends_still_gets_its_completion() {
    let (mut app, mut screen) = started(112, 38, &config::Ui::default());
    let mut clock = FakeClock::new();
    let refused = Input::Dnd(DndEvent::Error {
        name: "EPERM".into(),
        description: Some("the drag started in this window".into()),
    });
    script(
        &mut app,
        &mut screen,
        &mut clock,
        vec![drag(50, 20), drop_on(50, 20), refused],
    );
    assert_eq!(wire(&screen), [ACCEPT, REQUEST, CANCELLED].concat());
    assert_eq!(app.state.hint, Some(strings::DROP_REFUSED));
    assert!(
        app.debug_log
            .iter()
            .any(|l| l.contains("EPERM: the drag started in this window")),
        "the description goes to the debug log"
    );
    assert!(at_rest(&app, Duration::from_secs(5)));
    assert!(app.admitted.is_empty());
}

#[test]
fn a_drop_with_nothing_to_take_is_cancelled() {
    let dir = ScratchDir::new("app-kitty-none");
    let txt = dir.join("notes.txt");
    std::fs::write(&txt, b"text").unwrap();
    let (mut app, mut screen) = started(112, 38, &config::Ui::default());
    let mut clock = FakeClock::new();
    script(
        &mut app,
        &mut screen,
        &mut clock,
        vec![drag(50, 20), drop_on(50, 20), uri_list(&[&txt])],
    );
    assert_eq!(wire(&screen), [ACCEPT, REQUEST, CANCELLED].concat());
    assert_eq!(app.state.hint, Some(strings::DROP_NOT_A_PDF));
    assert!(at_rest(&app, Duration::from_secs(5)));
}

/// Unix only, as the size test above: NTFS allocates what `set_len` asks.
#[cfg(unix)]
#[test]
fn a_drop_holds_at_most_512_mib() {
    let dir = ScratchDir::new("app-kitty-hold");
    let small = pdf(&dir, "small.pdf");
    let big = pdf(&dir, "big.pdf");
    std::fs::File::options()
        .write(true)
        .open(&big)
        .unwrap()
        .set_len(HOLD_LIMIT)
        .unwrap();
    let (mut app, mut screen) = started(112, 38, &config::Ui::default());
    let mut clock = FakeClock::new();
    // The small file first: the big one would take the drop past the limit.
    script(
        &mut app,
        &mut screen,
        &mut clock,
        vec![drag(50, 20), drop_on(50, 20), uri_list(&[&small, &big])],
    );
    assert_eq!(wire(&screen), [ACCEPT, REQUEST, DONE].concat());
    assert_eq!(app.admitted.len(), 1);
    assert_eq!(app.admitted[0].0.path, small);
    assert_eq!(app.state.hint, Some(strings::DROP_HOLD_LIMIT));
}

// ── the export action (T-32b, D-048) ────────────────────────────────────

/// A queue of `n` finished files on the full layout.
fn finished_queue(app: &mut App, n: u64) {
    queue(app, n);
    for e in &mut app.state.batch.entries {
        e.state = EntryState::Done;
    }
    app.refresh(Duration::ZERO);
}

/// The exports the loop would ask the runner for now; each is accepted.
fn asked_exports(app: &mut App) -> Vec<JobId> {
    let mut asked = Vec::new();
    app.request_exports(&mut |id| {
        asked.push(id);
        true
    });
    asked
}

#[test]
fn enter_opens_the_file_menu_and_its_export_item_asks_for_the_export() {
    let (mut app, _) = started(112, 38, &config::Ui::default());
    send(&mut app, code(KeyCode::Enter));
    assert_eq!(app.state.file_menu, None, "no menu without a file");

    finished_queue(&mut app, 3);
    send(&mut app, code(KeyCode::Down));
    send(&mut app, code(KeyCode::Down));
    send(&mut app, code(KeyCode::Enter));
    assert_eq!(app.state.file_menu, Some(0));
    assert_eq!(
        app.state.selected,
        Some(2),
        "the menu acts on the cursor's file"
    );
    send(&mut app, code(KeyCode::Up));
    assert_eq!(app.state.file_menu, Some(0), "the top item stays put");
    send(&mut app, code(KeyCode::Down));
    send(&mut app, code(KeyCode::Down));
    assert_eq!(
        strings::FILE_MENU_ITEMS[app.state.file_menu.expect("open")],
        Some((strings::EXPORT_MARKDOWN, 'e'))
    );
    assert!(
        asked_exports(&mut app).is_empty(),
        "moving asks for nothing"
    );
    send(&mut app, code(KeyCode::Enter));
    assert_eq!(app.state.file_menu, None, "choosing closes the menu");
    assert_eq!(asked_exports(&mut app), [JobId(3)]);
    assert!(asked_exports(&mut app).is_empty(), "asked once");
}

#[test]
fn the_menu_skips_its_separator_and_answers_other_items_not_yet() {
    let (mut app, _) = started(112, 38, &config::Ui::default());
    finished_queue(&mut app, 1);
    send(&mut app, code(KeyCode::Enter));
    for _ in 0..4 {
        send(&mut app, code(KeyCode::Down));
    }
    let at = app.state.file_menu.expect("open");
    assert!(
        strings::FILE_MENU_ITEMS[at].is_some(),
        "never on the separator"
    );
    assert_eq!(at, 5);
    send(&mut app, key('o'));
    assert_eq!(app.state.file_menu, None);
    assert_eq!(app.state.hint, Some(strings::NOT_YET));
    assert!(asked_exports(&mut app).is_empty());

    send(&mut app, code(KeyCode::Enter));
    send(&mut app, code(KeyCode::Esc));
    assert_eq!(app.state.file_menu, None);
    send(&mut app, code(KeyCode::Enter));
    send(&mut app, key('e'));
    assert_eq!(asked_exports(&mut app), [JobId(1)], "an item's own key");

    send(&mut app, code(KeyCode::Enter));
    app.resize(80, 24);
    assert_eq!(app.state.file_menu, None, "the widget has no file menu");
}

#[test]
fn e_exports_the_selected_file_and_a_refusal_is_on_the_hint_row() {
    let (mut app, _) = started(112, 38, &config::Ui::default());
    send(&mut app, key('e'));
    assert!(asked_exports(&mut app).is_empty(), "nothing to export");
    finished_queue(&mut app, 2);
    send(&mut app, key('e'));
    assert_eq!(
        asked_exports(&mut app),
        [JobId(1)],
        "the first file by default"
    );
    send(&mut app, code(KeyCode::Down));
    send(&mut app, code(KeyCode::Down));
    send(&mut app, key('e'));
    app.request_exports(&mut |_| false);
    assert_eq!(app.state.hint, Some(strings::EXPORT_UNAVAILABLE));
}

#[test]
fn e_on_a_pasted_pdf_writes_its_markdown_beside_it() {
    let dir = ScratchDir::new("app-export");
    let input = dir.join("memo.pdf");
    std::fs::write(&input, crate::pdf::fixtures::golden_pdf()).unwrap();
    let (mut app, mut screen) = started(112, 38, &config::Ui::default());
    let (tx, rx) = mpsc::channel();
    let mut runner = JobRunner::new(Arc::new(FakeEngine::new()), tx, RunnerOptions::default());

    let events = vec![paste_of(&[&input]), key('e')];
    script_with_runner(&mut app, &mut screen, &mut runner, &rx, events);

    let row = &app.state.batch.entries[0];
    assert_eq!(row.state, EntryState::Done);
    assert!(row.run.is_some(), "repaired first");
    let md = std::fs::read_to_string(dir.join("memo.md")).expect("exported");
    assert!(md.contains(crate::pdf::fixtures::GOLDEN_TEXT[0][0]), "{md}");
    assert_eq!(dir.names(), ["memo.md", "memo.pdf", "memo.repaired.pdf"]);
    assert!(runner.shutdown(Duration::from_secs(5)));
}
