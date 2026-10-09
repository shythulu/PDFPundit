//! Font questions from the UI (F-01): a parked job's question opens with
//! `i`, each answer goes through the runner, `esc` leaves the job parked,
//! "best for all" answers every parked job, `prompt_unresolved = false`
//! answers without asking, and one family asks once.

use super::*;
use crate::engine::{
    FontCandidate, FontPickRequest, FontSlot, InteractionReply, InteractionRequestId, Ratio,
    ToUnicodeState,
};
use crate::jobs::FakeStep;

fn candidate(id: &str, family: &str) -> FontCandidate {
    FontCandidate {
        font_id: id.into(),
        family: family.into(),
        language: "en".into(),
        score: Ratio { num: 1, den: 2 },
        confidence: Ratio { num: 1, den: 4 },
        preview: format!("{family} reads like this"),
    }
}

fn question(slot: &str, candidates: Vec<FontCandidate>) -> InteractionRequest {
    InteractionRequest::FontPick(FontPickRequest {
        id: InteractionRequestId(1),
        page: 0,
        slot: slot.into(),
        sample_codes: Vec::new(),
        candidates,
        preview: String::new(),
    })
}

fn lost_slot(slot: &str, base: &str) -> FontSlot {
    FontSlot {
        page: 0,
        slot: slot.into(),
        base_font: Some(base.into()),
        subtype: Some("/Type0".into()),
        embedded: false,
        tounicode: ToUnicodeState::Missing,
        glyph_count: 12,
        resolution: None,
    }
}

/// A runner over `engine`, its events on the returned receiver.
fn runner_on(engine: &Arc<FakeEngine>) -> (JobRunner<FakeEngine>, Receiver<AppEvent<Input>>) {
    let (tx, rx) = mpsc::channel();
    let runner = JobRunner::new(Arc::clone(engine), tx, RunnerOptions::default());
    (runner, rx)
}

/// Each key followed by a tick, so on Windows the collector gives a typed
/// key back as a key before the next one.
fn keys(inputs: &[Input]) -> Vec<Input> {
    inputs
        .iter()
        .flat_map(|i| [i.clone(), Input::Tick])
        .collect()
}

/// Runs `inputs` through the loop with `runner`, then the runner's events,
/// until `n` events `stop` picks have been handed to the app (or none came
/// for 20 seconds). How many `NeedsInteraction` events the app was given.
fn drive(
    app: &mut App,
    screen: &mut TestScreen,
    runner: &mut JobRunner<FakeEngine>,
    rx: &Receiver<AppEvent<Input>>,
    inputs: Vec<Input>,
    stop: fn(&JobEvent) -> bool,
    n: usize,
) -> usize {
    let mut inputs = inputs.into_iter();
    let (mut seen, mut asked) = (0, 0);
    let mut next = || {
        if let Some(input) = inputs.next() {
            return Some(AppEvent::Input(input));
        }
        if seen >= n {
            return None;
        }
        let ev = rx.recv_timeout(Duration::from_secs(20)).ok()?;
        if let AppEvent::Job(_, job) = &ev {
            if matches!(job, JobEvent::NeedsInteraction { .. }) {
                asked += 1;
            }
            if stop(job) {
                seen += 1;
            }
        }
        Some(ev)
    };
    let mut clock = FakeClock::new();
    event_loop(app, screen, &mut next, &mut clock, Some(runner), None).unwrap();
    asked
}

fn asks(ev: &JobEvent) -> bool {
    matches!(ev, JobEvent::NeedsInteraction { .. })
}

fn repaired(ev: &JobEvent) -> bool {
    matches!(ev, JobEvent::RepairDone(_) | JobEvent::Failed { .. })
}

fn parked(app: &App, i: usize) -> bool {
    matches!(
        app.state.batch.entries[i].state,
        EntryState::WaitingOnUser(_)
    )
}

#[test]
fn i_opens_the_question_and_a_pick_finishes_the_row() {
    let dir = ScratchDir::new("app-font-pick");
    let a = pdf(&dir, "a.pdf");
    let ask = question(
        "F1",
        vec![
            candidate("noto-sans", "Noto Sans"),
            candidate("noto-serif", "Noto Serif"),
        ],
    );
    let engine = Arc::new(FakeEngine::new().on_repair(vec![FakeStep::Ask(ask)]));
    let (mut runner, rx) = runner_on(&engine);
    let (mut app, mut screen) = started(112, 38, &config::Ui::default());

    drive(
        &mut app,
        &mut screen,
        &mut runner,
        &rx,
        vec![paste_of(&[&a])],
        asks,
        1,
    );
    assert!(parked(&app, 0), "{:?}", app.state.batch.entries[0].state);
    assert_eq!(app.state.screen, Screen::Main, "nothing opens by itself");
    assert_eq!(app.vm.needs_you, Some((1, "a.pdf".into())));

    drive(
        &mut app,
        &mut screen,
        &mut runner,
        &rx,
        keys(&[key('i')]),
        asks,
        0,
    );
    assert_eq!(
        app.state.screen,
        Screen::FontPick {
            entry: 0,
            selected: 0
        }
    );
    let rows = screen.rows();
    assert!(rows[3].contains("PiCK A FONT » a.pdf"), "{}", rows[3]);
    assert!(rows[3].contains(" 1 of 1 "), "{}", rows[3]);
    assert!(rows[37].contains("resolving a.pdf"), "{}", rows[37]);
    assert!(
        rows[30].contains("esc later (file stays parked)"),
        "one question: no best for all: {}",
        rows[30]
    );

    let pick = keys(&[code(KeyCode::Down), code(KeyCode::Enter)]);
    drive(&mut app, &mut screen, &mut runner, &rx, pick, repaired, 1);
    assert_eq!(
        engine.replies(),
        [InteractionReply::Pick("noto-serif".into())],
        "the runner passed the pick on"
    );
    assert_eq!(app.state.batch.entries[0].state, EntryState::Done);
    assert!(app.state.batch.entries[0].run.is_some());
    assert_eq!(app.state.screen, Screen::Main, "no question left");
    assert!(runner.shutdown(Duration::from_secs(5)));
}

#[test]
fn esc_and_l_leave_the_job_parked() {
    let dir = ScratchDir::new("app-font-later");
    let a = pdf(&dir, "a.pdf");
    let engine = Arc::new(FakeEngine::new().always_asks());
    let (mut runner, rx) = runner_on(&engine);
    let (mut app, mut screen) = started(112, 38, &config::Ui::default());
    drive(
        &mut app,
        &mut screen,
        &mut runner,
        &rx,
        vec![paste_of(&[&a])],
        asks,
        1,
    );

    let later = keys(&[key('i'), code(KeyCode::Esc), key('l')]);
    drive(&mut app, &mut screen, &mut runner, &rx, later, asks, 0);
    assert_eq!(app.state.screen, Screen::Main);
    assert!(parked(&app, 0));
    assert_eq!(runner.parked().count(), 1, "still waiting on the runner");
    assert!(engine.replies().is_empty(), "nothing was answered");
    assert!(runner.shutdown(Duration::from_secs(5)));
}

#[test]
fn best_for_all_answers_every_parked_job() {
    let dir = ScratchDir::new("app-font-all");
    let (a, b) = (pdf(&dir, "a.pdf"), pdf(&dir, "b.pdf"));
    let engine = Arc::new(FakeEngine::new().always_asks());
    let (mut runner, rx) = runner_on(&engine);
    let (mut app, mut screen) = started(112, 38, &config::Ui::default());
    drive(
        &mut app,
        &mut screen,
        &mut runner,
        &rx,
        vec![paste_of(&[&a, &b])],
        asks,
        2,
    );
    assert!(parked(&app, 0) && parked(&app, 1));

    drive(
        &mut app,
        &mut screen,
        &mut runner,
        &rx,
        keys(&[key('i')]),
        asks,
        0,
    );
    let rows = screen.rows();
    assert!(rows[3].contains(" 1 of 2 "), "{}", rows[3]);
    assert!(rows[30].contains("a best for all"), "{}", rows[30]);

    drive(
        &mut app,
        &mut screen,
        &mut runner,
        &rx,
        keys(&[key('a')]),
        repaired,
        2,
    );
    assert_eq!(
        engine.replies(),
        [InteractionReply::UseBest, InteractionReply::UseBest]
    );
    for row in &app.state.batch.entries {
        assert_eq!(row.state, EntryState::Done, "{}", row.name);
    }
    assert_eq!(app.state.screen, Screen::Main);
    assert!(runner.shutdown(Duration::from_secs(5)));
}

#[test]
fn an_answer_moves_on_to_the_next_parked_question() {
    let dir = ScratchDir::new("app-font-next");
    let (a, b) = (pdf(&dir, "a.pdf"), pdf(&dir, "b.pdf"));
    let engine = Arc::new(FakeEngine::new().always_asks());
    let (mut runner, rx) = runner_on(&engine);
    let (mut app, mut screen) = started(112, 38, &config::Ui::default());
    drive(
        &mut app,
        &mut screen,
        &mut runner,
        &rx,
        vec![paste_of(&[&a, &b])],
        asks,
        2,
    );

    drive(
        &mut app,
        &mut screen,
        &mut runner,
        &rx,
        keys(&[key('i'), key('s')]),
        repaired,
        1,
    );
    assert_eq!(engine.replies(), [InteractionReply::Skip]);
    assert_eq!(
        app.state.screen,
        Screen::FontPick {
            entry: 1,
            selected: 0
        },
        "the next parked question"
    );
    assert!(screen.rows()[3].contains("b.pdf"), "{}", screen.rows()[3]);
    drive(
        &mut app,
        &mut screen,
        &mut runner,
        &rx,
        keys(&[key('b')]),
        repaired,
        1,
    );
    assert_eq!(
        engine.replies(),
        [InteractionReply::Skip, InteractionReply::UseBest]
    );
    assert_eq!(app.state.screen, Screen::Main);
    assert!(runner.shutdown(Duration::from_secs(5)));
}

#[test]
fn prompt_unresolved_false_never_opens_the_modal() {
    let dir = ScratchDir::new("app-font-silent");
    let a = pdf(&dir, "a.pdf");
    let engine = Arc::new(FakeEngine::new().always_asks());
    let (mut runner, rx) = runner_on(&engine);
    let (mut app, mut screen) = started(112, 38, &config::Ui::default());
    let mut config = Config::default();
    config.fonts.prompt_unresolved = false;
    app.configure(&config);
    assert!(!app.prompt_unresolved, "taken from [fonts]");

    let asked = drive(
        &mut app,
        &mut screen,
        &mut runner,
        &rx,
        vec![paste_of(&[&a])],
        asks,
        1,
    );
    assert_eq!(asked, 1);
    assert!(!parked(&app, 0), "answered as it came");
    drive(
        &mut app,
        &mut screen,
        &mut runner,
        &rx,
        keys(&[key('i')]),
        repaired,
        1,
    );
    assert_eq!(app.state.screen, Screen::Main, "nothing to resolve");
    assert_eq!(engine.replies(), [InteractionReply::UseBest]);
    assert_eq!(app.state.batch.entries[0].state, EntryState::Done);
    assert!(
        app.debug_log
            .iter()
            .any(|l| l.contains("without asking") && l.contains("prompt_unresolved")),
        "the policy answer is logged"
    );
    assert!(runner.shutdown(Duration::from_secs(5)));
}

#[test]
fn one_family_asks_once() {
    let dir = ScratchDir::new("app-font-family");
    let a = pdf(&dir, "a.pdf");
    let regular = question("F1", vec![candidate("noto-sans-regular", "Noto Sans")]);
    let bold = question(
        "F2",
        vec![
            candidate("noto-serif-bold", "Noto Serif"),
            candidate("noto-sans-bold", "Noto Sans"),
        ],
    );
    let engine = Arc::new(
        FakeEngine::new()
            .with_font_slots(vec![
                lost_slot("F1", "/ABCDEF+NotoSans-Regular"),
                lost_slot("F2", "/NotoSans-Bold"),
            ])
            .on_repair(vec![FakeStep::Ask(regular), FakeStep::Ask(bold)]),
    );
    let (mut runner, rx) = runner_on(&engine);
    let (mut app, mut screen) = started(112, 38, &config::Ui::default());
    drive(
        &mut app,
        &mut screen,
        &mut runner,
        &rx,
        vec![paste_of(&[&a])],
        asks,
        1,
    );

    let pick = keys(&[key('i'), code(KeyCode::Enter)]);
    let asked = drive(&mut app, &mut screen, &mut runner, &rx, pick, repaired, 1);
    assert_eq!(asked, 1, "the bold face was asked of the app");
    assert_eq!(
        engine.replies(),
        [
            InteractionReply::Pick("noto-sans-regular".into()),
            InteractionReply::Pick("noto-sans-bold".into()),
        ],
        "and answered from the regular face's pick"
    );
    assert_eq!(app.state.batch.entries[0].state, EntryState::Done);
    assert_eq!(app.state.screen, Screen::Main, "asked once");
    // The engine records the carried answer as the user's: the log names it.
    let line = app.debug_log.iter().find(|l| l.contains("without asking"));
    let line = line.expect("the carried answer is logged");
    assert!(line.contains("a.pdf"), "{line}");
    assert!(line.contains("slot F2 on p.1"), "{line}");
    assert!(line.contains("slot F1 on p.1 (family notosans)"), "{line}");
    assert!(runner.shutdown(Duration::from_secs(5)));
}

#[test]
fn the_widget_never_shows_the_question() {
    let (mut app, _) = started(32, 16, &config::Ui::default());
    queue(&mut app, 1);
    ask(&mut app, 1);
    send(&mut app, key('i'));
    assert_eq!(
        app.state.screen,
        Screen::Main,
        "D5: no prompt in the widget"
    );

    let (mut app, _) = started(112, 38, &config::Ui::default());
    queue(&mut app, 1);
    ask(&mut app, 1);
    send(&mut app, key('i'));
    assert!(matches!(app.state.screen, Screen::FontPick { .. }));
    send(&mut app, Input::Resize(32, 16));
    assert_eq!(app.state.screen, Screen::Main, "a shrink closes it");
    assert!(parked(&app, 0), "and the job stays parked");
}

/// The frame 03 batch with the modal open on its parked file: drawn over
/// the batch view with the file's name on the status bar, its slot's
/// details from the analysis.
#[test]
fn the_modal_is_drawn_over_the_batch_view() {
    let mut app = App::new(&config::Ui::default(), ColorCaps::TrueColor, PathBuf::new());
    app.state = AppState::mockup_batch();
    let (w, h) = app.state.term_size;
    app.resize(w, h);
    app.refresh(Duration::ZERO);
    app.arm();
    send(&mut app, key('i'));
    app.refresh(Duration::ZERO);
    assert_eq!(
        app.state.screen,
        Screen::FontPick {
            entry: 2,
            selected: 0
        }
    );
    let c = app.draw(&app.shown_at(Duration::ZERO));
    let row = |y: u16| -> String { (0..c.w).map(|x| c.get(x, y).expect("cell").ch).collect() };
    assert!(row(3).contains("PiCK A FONT » thesis_ar.pdf"), "{}", row(3));
    assert!(row(5).contains("slot F3 (CIDFont+F1)"), "{}", row(5));
    assert!(row(37).contains("resolving thesis_ar.pdf"), "{}", row(37));
}

/// A file's name on the status bar is text, never markup.
#[test]
fn the_status_bar_draws_the_files_name_literally() {
    let mut app = App::new(&config::Ui::default(), ColorCaps::TrueColor, PathBuf::new());
    app.state = AppState::mockup_batch();
    app.state.batch.entries[2].name = "{R}x{/b}.pdf".into();
    let (w, h) = app.state.term_size;
    app.resize(w, h);
    app.arm();
    send(&mut app, key('i'));
    app.refresh(Duration::ZERO);
    let c = app.draw(&app.shown_at(Duration::ZERO));
    let bar: String = (0..c.w).map(|x| c.get(x, 37).expect("cell").ch).collect();
    assert!(bar.contains("resolving {R}x{/b}.pdf │"), "{bar}");
    let at = bar
        .find("{R}")
        .map(|b| bar[..b].chars().count())
        .expect("name");
    let cell = c.get(u16::try_from(at).unwrap(), 37).expect("cell");
    assert_eq!(cell.bg, app.theme.roles.lightbar);
}
