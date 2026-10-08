//! The shell (T-23a): [`run`]'s start-up order and the event loop.
//!
//! Start-up (D-043 amended, eng-r3-q3):
//! 1. stdin and stdout must both be terminals, else one line on stderr and
//!    exit 2, before any byte of input is read or anything is parsed; argv is
//!    never looked at;
//! 2. raw mode, the alternate screen and the colour probe; a failure exits
//!    non-zero with one line (a Git Bash pty passes step 1 and can fail here);
//! 3. on Unix the kitty handshake (T-31), whose discarded byte count goes to
//!    the debug log; on kitty, drops are asked for and every exit path,
//!    the panic hook's included, opts out again;
//! 4. the first frame;
//! 5. the pending terminal input flushed, then the input source armed.
//!
//! Nothing typed, pasted or dropped before step 5 reaches the loop: the
//! terminal's queue is flushed, and an input that somehow reaches an unarmed
//! app is dropped and counted.
//!
//! Drops (T-23b): a paste (on Windows, a burst of typed keys, D-034) is split
//! into path candidates, each candidate passes the drop gate, and the files it
//! lets in make the cat chomp and go to the runner, which analyses then
//! repairs each one. The one-line fallback takes no drops (D-064): it refuses
//! them with a hint and asks the terminal to grow. The browse picker (`b`,
//! T-37) is the fallback for a terminal that does not paste on drop; the
//! files it picks take the same road, gate and chomp included.
//!
//! kitty drops (T-31): the cat watches a drag over the window, and the
//! terminal is told the drop is wanted only over the cat ([`Layout::drop_zone`]).
//! A drop's files pass the same gate, and each one it lets in is read into
//! memory before the drop completes, since a macOS file promise is gone after
//! that (D-039): at most [`HOLD_LIMIT`] bytes a drop.
//!
//! Export (T-32b, D-048): Enter opens the per-file menu over the selected
//! queue row and its `Export → Markdown` item, or `e` straight from the batch
//! and result views, asks the runner for that file's Markdown export. It is
//! never automatic: asked while the file is still queued or working, the
//! export follows its repair (the "repair → export" one-shot); asked later,
//! the runner runs it on its own.
//!
//! The loop owns the [`AppState`], the [`Director`], the runner and the history
//! store. It hands every job event to the runner first and then to the state,
//! records each finished repair in the store and refills the history summary
//! after it, keeps `term_size` from every resize and the clock from the wall
//! clock once a minute, and redraws at most once a tick unless a key or a
//! resize asks sooner.

mod clock;

use std::collections::{BTreeMap, VecDeque};
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::canvas::Canvas;
use super::color::ColorCaps;
use super::director::{CatEvent, CatFrame, CellPos, Director, Mood, Stage};
#[cfg(windows)]
use super::input::collector::{Collector, Flush};
use super::input::gate::{Admitted, gate};
use super::input::osc72::{DndAction, DndSession, HOLD_LIMIT};
use super::input::paste::{PathCandidate, paths_from_paste};
use super::input::{DndEvent, Input, InputSource};
use super::layout::browse::{self, BrowseAction, BrowseKey, BrowseState};
use super::layout::full::{self, FileAction, FullLayout, MenuKey};
use super::layout::modals::{ModalKey, ThemeAction, ThemeChooser};
use super::layout::widget::{OneLine, WidgetLayout};
use super::layout::{FULL_SIZE, Layout, LayoutKind, LayoutPin, choose};
use super::state::{AppState, Screen};
use super::strings;
use super::term::{self, Surface};
use super::theme::Theme;
use super::view::{self, ViewModel};
use super::widgets::lightbar;
use crate::appdirs::AppDirs;
use crate::config::{self, Config, OutputDir};
use crate::engine::{AnalyzeOptions, Engine, Pdfpundit, RepairOptions};
use crate::jobs::{
    AppEvent, EntryState, JobEvent, JobId, JobInput, JobKind, JobRunner, QueueEntry, RunnerOptions,
};
use crate::library::{FileEntry, HistoryStore, JsonStore, RunRecord};
use crate::panic_guard;

/// The loop's tick: the cat animates at about 60 frames a second.
const TICK: Duration = Duration::from_millis(16);
/// How long quitting waits for the job threads.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);
/// The exit code for a non-terminal stdin or stdout (D-043).
const NOT_A_TERMINAL_EXIT: u8 = 2;
/// The exit code when the terminal cannot be set up or used.
const TERMINAL_EXIT: u8 = 1;
/// `CSI 8;38;112 t`: asks the terminal to grow to the full layout's size.
const GROW_TO_FULL: &[u8] = b"\x1b[8;38;112t";
/// `CSI 8;16;32 t`: asks the terminal to grow to the widget's size (D-064).
const GROW_TO_WIDGET: &[u8] = b"\x1b[8;16;32t";
/// Lines the debug log keeps.
const LOG_LINES: usize = 256;

/// Starts PDFPundit: the guard, then the cat. Arguments are never read.
pub fn run() -> ExitCode {
    // (1) The guard, before any input is read or anything parsed.
    if !term::stdio_is_terminal() {
        let _ = writeln!(io::stderr(), "{}", strings::NOT_A_TERMINAL);
        return ExitCode::from(NOT_A_TERMINAL_EXIT);
    }
    match shell() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let line = strings::TERMINAL_FAILED.replace("{e}", &e.to_string());
            let _ = writeln!(io::stderr(), "{line}");
            ExitCode::from(TERMINAL_EXIT)
        }
    }
}

/// Steps 2 to 5 and the loop. The terminal is given back before it returns,
/// on every path.
fn shell() -> io::Result<()> {
    let dirs = AppDirs::resolve();
    let (config, warnings, config_path) = match &dirs {
        Some(d) => Config::load(d),
        None => (Config::default(), Vec::new(), PathBuf::new()),
    };
    let opened = dirs.as_ref().map(JsonStore::open);

    // (2) Raw mode, the alternate screen, the colour probe; and the panic
    // hook, which gives the terminal back if the UI thread panics (D-051).
    let caps = term::probe_color_caps();
    let mut guard = term::TermGuard::enter()?;
    panic_guard::install(
        thread::current().id(),
        term::restore_sequence(false),
        Box::new(term::PanicWriter),
    );

    // (3) The handshake. Once kitty drops are asked for, every way out opts
    // out again, the panic hook's included.
    let handshake = term::handshake();
    if handshake.kitty {
        guard.kitty_armed();
        panic_guard::install(
            thread::current().id(),
            term::restore_sequence(true),
            Box::new(term::PanicWriter),
        );
    }

    let mut app = App::new(&config.ui, caps, config_path);
    app.warn_above = runner_options(&config).analyze.max_file_bytes;
    app.log(format!(
        "handshake: kitty {}, {} bytes discarded",
        handshake.kitty, handshake.discarded
    ));
    for w in &warnings {
        app.log(w.to_string());
    }
    let mut store = match opened {
        Some(Ok(store)) => Some(store),
        Some(Err(e)) => {
            app.log(format!("history: could not open the store: {e}"));
            None
        }
        None => {
            app.log("history: no home directory, so no history".into());
            None
        }
    };
    let mut screen = term::Tty::tty(caps)?;
    let (w, h) = screen.size()?;
    app.resize(w, h);
    if let Some(s) = &store {
        app.state.history = s.summary();
    }
    let mut clock = SystemClock::new();
    let unix = clock.unix_secs();
    app.minute = Some(unix / 60);
    app.state.clock = Some(clock.local_hm(unix));

    // (4) The first frame.
    app.refresh(Duration::ZERO);
    app.render(&mut screen, Duration::ZERO)?;

    // (5) Flush what the terminal queued before now, then arm the input.
    term::flush_input()?;
    let inputs = InputSource::spawn(&config.ui, handshake);
    app.arm();

    // A debug build can be told to panic on the UI thread here, to watch the
    // panic hook give the terminal back. Never compiled into a release
    // (D-051; CI checks the release binary for the name).
    #[cfg(debug_assertions)]
    if std::env::var_os("PDFPUNDIT_PANIC").is_some() {
        panic!("PDFPUNDIT_PANIC: a UI-thread panic on request");
    }

    let (tx, rx) = mpsc::channel();
    forward(inputs, tx.clone());
    let mut runner = JobRunner::new(Arc::new(Pdfpundit), tx, runner_options(&config));
    let result = event_loop(
        &mut app,
        &mut screen,
        &mut || next_event(&rx),
        &mut clock,
        Some(&mut runner),
        store.as_mut().map(|s| s as &mut dyn HistoryStore),
    );
    // The terminal first: the wait for the job threads happens on the user's
    // own screen, not on a frozen cat.
    guard.restore();
    runner.shutdown(SHUTDOWN_GRACE);
    result
}

/// The runner's options under `config`: its salvage budget and output
/// directory, with the thread count and scratch cap the runner picks.
fn runner_options(config: &Config) -> RunnerOptions {
    let defaults = RunnerOptions::default();
    let repair = config.repair_options();
    let analyze = AnalyzeOptions {
        salvage_budget: repair.analyze.salvage_budget,
        ..defaults.analyze.clone()
    };
    RunnerOptions {
        repair: RepairOptions {
            analyze: analyze.clone(),
            ..repair
        },
        analyze,
        output_dir: match &config.general.output_dir {
            OutputDir::Beside => None,
            OutputDir::Dir(d) => Some(d.clone()),
        },
        ..defaults
    }
}

/// Moves terminal inputs onto the loop's merged channel.
fn forward(inputs: Receiver<Input>, tx: Sender<AppEvent<Input>>) {
    let _ = thread::Builder::new()
        .name("input-forward".into())
        .spawn(move || {
            for input in inputs {
                if tx.send(AppEvent::Input(input)).is_err() {
                    return;
                }
            }
        });
}

/// The next event, or a tick when none came within one.
fn next_event(rx: &Receiver<AppEvent<Input>>) -> Option<AppEvent<Input>> {
    match rx.recv_timeout(TICK) {
        Ok(ev) => Some(ev),
        Err(RecvTimeoutError::Timeout) => Some(AppEvent::Tick),
        Err(RecvTimeoutError::Disconnected) => None,
    }
}

/// Time for the loop: since start-up for the cat, and the wall clock for the
/// status bar and the run record. Display and records only; nothing here
/// reaches an artefact.
pub(crate) trait Clock {
    fn elapsed(&mut self) -> Duration;
    fn unix_secs(&mut self) -> u64;
    /// `(hour, minute)`, local time, at `unix` seconds.
    fn local_hm(&mut self, unix: u64) -> (u8, u8);
}

struct SystemClock {
    start: Instant,
}

impl SystemClock {
    fn new() -> SystemClock {
        SystemClock {
            start: Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn elapsed(&mut self) -> Duration {
        self.start.elapsed()
    }

    fn unix_secs(&mut self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }

    fn local_hm(&mut self, unix: u64) -> (u8, u8) {
        clock::local_hm(unix)
    }
}

/// Runs the loop until a quit key, or until `next` has nothing more.
fn event_loop<E: Engine + 'static>(
    app: &mut App,
    surface: &mut dyn Surface,
    next: &mut dyn FnMut() -> Option<AppEvent<Input>>,
    clock: &mut dyn Clock,
    mut runner: Option<&mut JobRunner<E>>,
    mut store: Option<&mut dyn HistoryStore>,
) -> io::Result<()> {
    let mut drawn_at: Option<Duration> = None;
    while !app.quit {
        let Some(ev) = next() else {
            break;
        };
        let now = clock.elapsed();
        let unix = clock.unix_secs();
        if app.minute != Some(unix / 60) {
            app.minute = Some(unix / 60);
            app.state.clock = Some(clock.local_hm(unix));
        }
        let urgent = match ev {
            AppEvent::Input(input) => {
                app.on_input(input, now);
                true
            }
            AppEvent::Job(id, ev) => {
                if let Some(r) = runner.as_deref_mut() {
                    r.on_job_event(id, &ev);
                }
                app.on_job(id, ev, unix, store.as_deref_mut());
                false
            }
            AppEvent::Tick => {
                app.tick(now);
                false
            }
        };
        if let Some(r) = runner.as_deref_mut() {
            app.submit_drops(now, &mut |input| r.submit(input));
            app.request_exports(&mut |id| r.export(id));
        }
        app.refresh(now);
        if app.quit {
            break;
        }
        if urgent || drawn_at.is_none_or(|t| now >= t + TICK) {
            app.render(surface, now)?;
            drawn_at = Some(now);
        }
        let out = std::mem::take(&mut app.out);
        if !out.is_empty() {
            surface.write_raw(&out)?;
        }
    }
    Ok(())
}

/// What a frame was drawn from: the same again draws nothing.
#[derive(Clone, Debug, PartialEq)]
struct Shown {
    vm: ViewModel,
    cat: CatFrame,
    size: (u16, u16),
    theme: usize,
    kind: LayoutKind,
    screen: Screen,
}

/// The UI's state machine: inputs and job events in, frames and terminal
/// bytes out.
pub(crate) struct App {
    pub(crate) state: AppState,
    director: Director,
    caps: ColorCaps,
    /// Into [`Theme::all`].
    theme_index: usize,
    /// That theme as the terminal can show it.
    theme: Theme,
    /// The theme to go back to when the chooser is closed without a pick.
    theme_before: usize,
    pin: LayoutPin,
    kind: LayoutKind,
    request_resize: bool,
    config_path: PathBuf,
    /// Set at step 5; inputs before it are dropped and counted.
    armed: bool,
    pre_frame_dropped: usize,
    /// Files above this size are let in with a warning (512 MiB).
    warn_above: u64,
    /// Files the drop gate let in, for [`App::submit_drops`]; a kitty drop's
    /// with their bytes, read while the drop was active (D-039).
    admitted: Vec<(Admitted, Option<Vec<u8>>)>,
    /// Jobs whose Markdown export was asked for, for
    /// [`App::request_exports`].
    exports: Vec<JobId>,
    /// The kitty drag-and-drop protocol (T-31).
    dnd: DndSession,
    /// Path candidates the one-line fallback refused (D-064).
    refused_too_small: usize,
    /// The folder the browse picker opens on: the working directory, then
    /// the folder it was last closed in.
    browse_from: PathBuf,
    /// Windows: typed keys, until they are known to be keys or a path.
    #[cfg(windows)]
    collector: Collector,
    vm: ViewModel,
    mood: Mood,
    needs_you: bool,
    /// The wall-clock minute `state.clock` was read in.
    minute: Option<u64>,
    quit: bool,
    /// Bytes for the terminal (the widget's resize request).
    out: Vec<u8>,
    /// When each repair started, for its run record.
    repair_started: BTreeMap<JobId, u64>,
    shown: Option<Shown>,
    debug_log: VecDeque<String>,
}

impl App {
    pub(crate) fn new(ui: &config::Ui, caps: ColorCaps, config_path: PathBuf) -> App {
        let theme_index = Theme::all()
            .iter()
            .position(|t| t.name == ui.theme)
            .unwrap_or(0);
        let state = AppState::default();
        let vm = view::view(&state);
        let pin = LayoutPin::from(ui.layout);
        App {
            director: Director::new(),
            caps,
            theme_index,
            theme: Theme::all()[theme_index].downgrade(caps),
            theme_before: theme_index,
            pin,
            kind: choose(state.term_size, pin),
            request_resize: ui.request_resize,
            config_path,
            armed: false,
            pre_frame_dropped: 0,
            warn_above: AnalyzeOptions::default().max_file_bytes,
            admitted: Vec::new(),
            exports: Vec::new(),
            dnd: DndSession::default(),
            refused_too_small: 0,
            browse_from: std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from(std::path::MAIN_SEPARATOR_STR)),
            #[cfg(windows)]
            collector: Collector::default(),
            mood: vm.mood,
            vm,
            state,
            needs_you: false,
            minute: None,
            quit: false,
            out: Vec::new(),
            repair_started: BTreeMap::new(),
            shown: None,
            debug_log: VecDeque::new(),
        }
    }

    /// Step 5: from now on inputs are handled.
    pub(crate) fn arm(&mut self) {
        self.armed = true;
        if self.pre_frame_dropped > 0 {
            let n = self.pre_frame_dropped;
            self.log(format!(
                "dropped {n} inputs that came before the first frame"
            ));
        }
    }

    fn log(&mut self, line: String) {
        if self.debug_log.len() == LOG_LINES {
            self.debug_log.pop_front();
        }
        self.debug_log.push_back(line);
    }

    /// A new terminal size: the layout is chosen again.
    pub(crate) fn resize(&mut self, w: u16, h: u16) {
        self.state.term_size = (w, h);
        self.kind = choose((w, h), self.pin);
        // The theme chooser is drawn over the full layout only: a shrink
        // closes it as Esc would.
        if self.kind != LayoutKind::Full && matches!(self.state.screen, Screen::Themes { .. }) {
            self.set_theme(self.theme_before);
            self.state.screen = Screen::Main;
        }
        // The one-line fallback has no picker (D-064): a shrink closes it as
        // Esc would.
        if self.kind == LayoutKind::OneLine && matches!(self.state.screen, Screen::Browse(_)) {
            self.close_browse();
        }
        // The per-file menu is the full layout's: a shrink closes it.
        if self.kind != LayoutKind::Full {
            self.state.file_menu = None;
        }
        self.director.set_stage(match self.kind {
            LayoutKind::Full => Stage::Full,
            LayoutKind::Widget | LayoutKind::OneLine => Stage::Widget,
        });
    }

    /// One input at `now` (the loop's clock since start-up).
    pub(crate) fn on_input(&mut self, input: Input, now: Duration) {
        if !self.armed {
            self.pre_frame_dropped += 1;
            return;
        }
        match input {
            Input::Key(k) => self.typed(k, now),
            Input::Resize(w, h) => self.resize(w, h),
            Input::Paste(text) => self.pasted(&text),
            Input::Tick => self.tick(now),
            Input::Mouse(_) => {}
            Input::Dnd(d) => self.on_dnd(d, now),
            Input::Note(line) => self.log(line),
        }
    }

    /// Time passes: on Windows a typed burst that has gone quiet is flushed.
    pub(crate) fn tick(&mut self, _now: Duration) {
        #[cfg(windows)]
        if let Some(f) = self.collector.idle(_now) {
            self.collected(f);
        }
    }

    /// A key. On Windows the main screen's and the picker's keys go through
    /// the collector first; on Unix a key is only ever a key. In the picker,
    /// a path dropped as a burst of keys is a drop like a paste there (fed
    /// through the gate, the picks left alone), not a run of picker keys that
    /// would pick, toggle and move as it went.
    fn typed(&mut self, k: KeyEvent, _now: Duration) {
        if k.kind == KeyEventKind::Release {
            return;
        }
        #[cfg(windows)]
        if matches!(self.state.screen, Screen::Main | Screen::Browse(_)) {
            for f in self.collector.key(k, _now) {
                self.collected(f);
            }
            return;
        }
        self.on_key(k);
    }

    #[cfg(windows)]
    fn collected(&mut self, f: Flush) {
        match f {
            Flush::Key(k) => self.on_key(k),
            Flush::Text(text) => self.pasted(&text),
        }
    }

    /// Pasted text: its paths go to the drop gate.
    fn pasted(&mut self, text: &str) {
        self.dropped(paths_from_paste(text));
    }

    /// Path candidates from any source (a paste, the collector, an OSC 72
    /// drop, the picker). Where the layout takes no input they are all
    /// refused (D-064); otherwise each passes the gate or is refused, and the
    /// hint row says why (the first refusal, else the size warning).
    pub(crate) fn dropped(&mut self, candidates: Vec<PathCandidate>) {
        if candidates.is_empty() {
            return;
        }
        if !layout(self.kind).accepts_input() {
            self.refused_too_small += candidates.len();
            self.log(format!(
                "drop: {} refused, the one-line layout takes no drops",
                candidates.len()
            ));
            self.state.hint = Some(strings::TOO_SMALL_TO_EAT);
            if self.request_resize {
                self.out.extend_from_slice(GROW_TO_WIDGET);
            }
            return;
        }
        let mut refusal = None;
        let mut warning = None;
        for c in candidates {
            let verdict = match (c.path, c.reason) {
                (Some(path), _) => gate(&path, self.warn_above),
                (None, why) => Err(why.unwrap_or(strings::DROP_GARBLED)),
            };
            match verdict {
                Ok(a) => {
                    self.log(format!(
                        "drop: {} ({} bytes), %PDF- in the first KiB: {}",
                        a.path.display(),
                        a.bytes,
                        a.drop_sniff
                    ));
                    if a.big {
                        warning = Some(strings::DROP_BIG);
                        self.log(format!(
                            "drop: {} is {} MiB and is held in memory while it is worked on",
                            a.path.display(),
                            a.bytes >> 20
                        ));
                    }
                    self.admitted.push((a, None));
                }
                Err(why) => {
                    self.log(format!("drop refused: {}: {why}", c.raw));
                    refusal.get_or_insert(why);
                }
            }
        }
        // A clean drop clears an earlier drop's hint, so a good file never
        // looks refused.
        self.state.hint = refusal.or(warning);
    }

    /// The files the gate let in since the last call: the cat chomps them
    /// (`Dropped{n}`), then each is submitted, in order, and gets its queue
    /// row. The runner analyses each one, then repairs it with the analysis
    /// state (GG §1).
    pub(crate) fn submit_drops(
        &mut self,
        now: Duration,
        submit: &mut dyn FnMut(JobInput) -> JobId,
    ) {
        if self.admitted.is_empty() {
            return;
        }
        let admitted = std::mem::take(&mut self.admitted);
        self.director
            .on(CatEvent::Dropped { n: admitted.len() }, now);
        for (a, bytes) in admitted {
            let id = submit(match bytes {
                Some(bytes) => JobInput::Dropped {
                    path: a.path.clone(),
                    bytes,
                },
                None => JobInput::File(a.path.clone()),
            });
            let name = a.path.file_name().map_or_else(
                || a.path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            self.state
                .batch
                .entries
                .push(QueueEntry::queued(id, name, Some(a.path), a.bytes));
        }
    }

    /// A kitty drag-and-drop event: the session's replies go out, the cat
    /// watches the drag, and a drop's files are taken before it completes.
    fn on_dnd(&mut self, ev: DndEvent, now: Duration) {
        let zone = layout(self.kind).drop_zone();
        let main = self.state.screen == Screen::Main;
        let over_cat = |c: CellPos| {
            main && zone
                .is_some_and(|(x, y, w, h)| (x..x + w).contains(&c.x) && (y..y + h).contains(&c.y))
        };
        for action in self.dnd.on(ev, &over_cat) {
            match action {
                DndAction::Reply(bytes) => self.out.extend_from_slice(&bytes),
                DndAction::Cat(ev) => self.director.on(ev, now),
                DndAction::Files(candidates) => {
                    let took = self.take_drop(candidates);
                    if let Some(done) = self.dnd.finish(took) {
                        self.out.extend_from_slice(&done);
                    }
                    if took == 0 {
                        self.director.on(CatEvent::DragAt(None), now);
                    }
                }
                DndAction::Ended { hint, log } => {
                    self.log(log);
                    if hint.is_some() {
                        self.state.hint = hint;
                    }
                }
            }
        }
    }

    /// A kitty drop's candidates through the gate, then each file it lets in
    /// read into memory, at most [`HOLD_LIMIT`] bytes for the drop. How many
    /// were taken.
    ///
    /// v1 reads on the UI thread: D-039 needs the bytes before `t=r:o=1`,
    /// not here, so a large drop from slow media freezes the cat (no frames,
    /// no keys) until the read ends. The better shape reads on a worker and
    /// calls [`DndSession::finish`] when it is done.
    fn take_drop(&mut self, candidates: Vec<PathCandidate>) -> usize {
        let before = self.admitted.len();
        self.dropped(candidates);
        let gated: Vec<_> = self.admitted.drain(before..).collect();
        let mut held = 0u64;
        let mut refusal = None;
        for (a, _) in gated {
            let read = if held + a.bytes > HOLD_LIMIT {
                Err(strings::DROP_HOLD_LIMIT)
            } else {
                read_at_most(&a.path, HOLD_LIMIT - held)
            };
            match read {
                Ok(bytes) => {
                    held += bytes.len() as u64;
                    self.admitted.push((a, Some(bytes)));
                }
                Err(why) => {
                    self.log(format!("drop refused: {}: {why}", a.path.display()));
                    refusal.get_or_insert(why);
                }
            }
        }
        if refusal.is_some() {
            self.state.hint = refusal;
        }
        self.admitted.len() - before
    }

    fn on_key(&mut self, k: KeyEvent) {
        if k.kind == KeyEventKind::Release {
            return;
        }
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if let Screen::Themes { selected } = self.state.screen {
            self.chooser_key(k.code, selected);
            return;
        }
        // A one-off hint lasts until the next key.
        self.state.hint = None;
        if matches!(self.state.screen, Screen::Browse(_)) {
            self.browse_key(k.code);
            return;
        }
        if self.state.file_menu.is_some() {
            self.file_menu_key(k.code);
            return;
        }
        match k.code {
            KeyCode::Char('q' | 'Q') => self.quit = true,
            KeyCode::Char('T' | 't') if self.kind == LayoutKind::Full => {
                self.theme_before = self.theme_index;
                self.state.screen = Screen::Themes {
                    selected: ThemeChooser::open(Theme::all(), &Theme::all()[self.theme_index]),
                };
            }
            // The browse picker. Inert where the layout takes no input (the
            // one-line fallback, D-064).
            KeyCode::Char('b' | 'B') if layout(self.kind).accepts_input() => {
                self.state.screen = Screen::Browse(BrowseState::open(&self.browse_from));
            }
            // Export the selected file (the result view's `e`, DA:472).
            KeyCode::Char('e' | 'E')
                if self.kind == LayoutKind::Full && full::menu_target(&self.state).is_some() =>
            {
                if let Some(i) = full::menu_target(&self.state) {
                    self.ask_export(i);
                }
            }
            KeyCode::Char(c) => {
                full::menu_key(&mut self.state, c);
            }
            KeyCode::Enter if self.kind == LayoutKind::Full => {
                full::open_file_menu(&mut self.state);
            }
            KeyCode::Enter if self.kind == LayoutKind::Widget => self.ask_for_full_size(),
            KeyCode::Up if self.kind == LayoutKind::Full => self.move_cursor(false),
            KeyCode::Down if self.kind == LayoutKind::Full => self.move_cursor(true),
            _ => {}
        }
    }

    /// The theme chooser (T-24's keyboard model): up and down try each theme
    /// on the spot, Enter keeps it, Esc goes back to the one before.
    fn chooser_key(&mut self, code: KeyCode, selected: usize) {
        let key = match code {
            KeyCode::Up => ModalKey::Up,
            KeyCode::Down => ModalKey::Down,
            KeyCode::Enter => ModalKey::Enter,
            KeyCode::Esc => ModalKey::Esc,
            KeyCode::Char(c) => ModalKey::Char(c),
            _ => return,
        };
        match ThemeChooser::key(Theme::all().len(), selected, key) {
            ThemeAction::Preview(i) => {
                self.set_theme(i);
                self.state.screen = Screen::Themes { selected: i };
            }
            ThemeAction::Apply(i) => {
                self.set_theme(i);
                self.state.screen = Screen::Main;
            }
            ThemeAction::Cancel => {
                self.set_theme(self.theme_before);
                self.state.screen = Screen::Main;
            }
            ThemeAction::Ignore => {}
        }
    }

    /// The browse picker's keys. Confirming closes it and sends the picked
    /// paths through [`App::dropped`], the drop gate and chomp a paste goes
    /// through.
    fn browse_key(&mut self, code: KeyCode) {
        let key = match code {
            KeyCode::Up => BrowseKey::Up,
            KeyCode::Down => BrowseKey::Down,
            KeyCode::Left => BrowseKey::Left,
            KeyCode::Right => BrowseKey::Right,
            KeyCode::Enter => BrowseKey::Enter,
            KeyCode::Backspace => BrowseKey::Backspace,
            KeyCode::Esc => BrowseKey::Esc,
            KeyCode::Char(c) => BrowseKey::Char(c),
            _ => return,
        };
        let Screen::Browse(picker) = &mut self.state.screen else {
            return;
        };
        match picker.key(key) {
            BrowseAction::Stay => {}
            BrowseAction::Cancel => self.close_browse(),
            BrowseAction::Feed(paths) => {
                self.close_browse();
                let candidates = paths
                    .into_iter()
                    .map(|path| PathCandidate {
                        raw: path.display().to_string(),
                        path: Some(path),
                        reason: None,
                    })
                    .collect();
                self.dropped(candidates);
            }
        }
    }

    /// The per-file menu's keys (T-22b's menu, T-32b's export item); `q`
    /// still quits, as the menu's hotkeys row says.
    fn file_menu_key(&mut self, code: KeyCode) {
        let key = match code {
            KeyCode::Char('q' | 'Q') => {
                self.quit = true;
                return;
            }
            KeyCode::Up => MenuKey::Up,
            KeyCode::Down => MenuKey::Down,
            KeyCode::Enter => MenuKey::Enter,
            KeyCode::Esc => MenuKey::Esc,
            KeyCode::Char(c) => MenuKey::Char(c),
            _ => return,
        };
        if let Some(FileAction::Export(i)) = full::file_menu_key(&mut self.state, key) {
            self.ask_export(i);
        }
    }

    /// Queue row `i`'s export, for the loop to ask the runner for.
    fn ask_export(&mut self, i: usize) {
        if let Some(e) = self.state.batch.entries.get(i) {
            self.exports.push(e.job);
        }
    }

    /// The exports asked for since the last call, each passed to `export`
    /// (the runner's [`JobRunner::export`]). One the runner cannot run (its
    /// file was cancelled before it started) is refused on the hint row.
    pub(crate) fn request_exports(&mut self, export: &mut dyn FnMut(JobId) -> bool) {
        for id in std::mem::take(&mut self.exports) {
            if !export(id) {
                self.log(format!("export: job {} is not known to the runner", id.0));
                self.state.hint = Some(strings::EXPORT_UNAVAILABLE);
            }
        }
    }

    /// Closes the picker; it opens on the same folder next time.
    fn close_browse(&mut self) {
        if let Screen::Browse(picker) = std::mem::take(&mut self.state.screen) {
            self.browse_from = picker.cwd;
        }
    }

    fn set_theme(&mut self, i: usize) {
        self.theme_index = i;
        self.theme = Theme::all()[i].downgrade(self.caps);
    }

    /// The queue cursor, one row up or down, kept on the queue.
    fn move_cursor(&mut self, down: bool) {
        let n = self.state.batch.entries.len();
        if n == 0 {
            return;
        }
        let at = self
            .state
            .selected
            .or(self.state.batch.current)
            .unwrap_or(0)
            .min(n - 1);
        self.state.selected = Some(if down {
            (at + 1).min(n - 1)
        } else {
            at.saturating_sub(1)
        });
    }

    /// The widget's request to grow to the full layout, when allowed.
    fn ask_for_full_size(&mut self) {
        if self.request_resize {
            self.out.extend_from_slice(GROW_TO_FULL);
        }
    }

    /// A job event into the queue row it belongs to. A finished repair is
    /// recorded in `store`, and the history summary is read again.
    fn on_job(
        &mut self,
        id: JobId,
        ev: JobEvent,
        unix: u64,
        store: Option<&mut (dyn HistoryStore + '_)>,
    ) {
        let batch = &mut self.state.batch;
        let Some(i) = batch.entries.iter().position(|e| e.job == id) else {
            return;
        };
        let entry = &mut batch.entries[i];
        match ev {
            JobEvent::Started { kind, .. } => {
                entry.state = match kind {
                    JobKind::Analyze => EntryState::Analyzing {
                        phase: None,
                        done: 0,
                        total: None,
                    },
                    JobKind::Repair { .. } => {
                        self.repair_started.insert(id, unix);
                        EntryState::Repairing {
                            phase: None,
                            done: 0,
                            total: None,
                        }
                    }
                    JobKind::ExportMarkdown => EntryState::Exporting {
                        done: 0,
                        total: None,
                    },
                };
                batch.current = Some(i);
            }
            JobEvent::Phase { name, .. } => match &mut entry.state {
                EntryState::Analyzing { phase, .. } | EntryState::Repairing { phase, .. } => {
                    *phase = Some(name);
                }
                _ => {}
            },
            JobEvent::Progress { done: d, total: t } => match &mut entry.state {
                EntryState::Analyzing { done, total, .. }
                | EntryState::Repairing { done, total, .. }
                | EntryState::Exporting { done, total } => {
                    (*done, *total) = (d, t);
                }
                _ => {}
            },
            JobEvent::Finding(f) => entry.findings.push(f),
            JobEvent::Log(level, msg) => {
                let line = format!("{}: {level:?}: {msg}", entry.name);
                self.log(line);
            }
            // The reply sender is dropped here: answers go through the
            // runner, and a kept clone would keep a cancelled job waiting.
            JobEvent::NeedsInteraction { request, .. } => {
                entry.state = EntryState::WaitingOnUser(request);
            }
            JobEvent::AnalyzeDone(result) => {
                let result = *result;
                entry.meta = Some(result.meta);
                entry.findings = result.findings;
                entry.font_resolutions = result.font_slots;
            }
            JobEvent::RepairDone(run) => {
                entry.run = Some(*run);
                entry.state = EntryState::Done;
                self.finish(i);
                self.record(id, i, unix, store);
            }
            JobEvent::ExportDone { path } => {
                entry.state = EntryState::Done;
                self.finish(i);
                self.log(format!("export: wrote {}", path.display()));
            }
            JobEvent::Parked => self.finish(i),
            JobEvent::Resumed => {
                entry.state = EntryState::Repairing {
                    phase: None,
                    done: 0,
                    total: None,
                };
            }
            JobEvent::Failed { error, panicked } => {
                entry.state = EntryState::Failed { error, panicked };
                self.finish(i);
            }
            JobEvent::Cancelled => {
                entry.state = EntryState::Cancelled;
                self.finish(i);
            }
            JobEvent::Evicted { .. } | JobEvent::WaitingForYou { .. } => {}
        }
    }

    /// Row `i` is no longer the runner's active job.
    fn finish(&mut self, i: usize) {
        if self.state.batch.current == Some(i) {
            self.state.batch.current = None;
        }
    }

    /// Row `i`'s finished repair into the history, then the summary again.
    fn record(
        &mut self,
        id: JobId,
        i: usize,
        unix: u64,
        store: Option<&mut (dyn HistoryStore + '_)>,
    ) {
        let started_at = self.repair_started.remove(&id).unwrap_or(unix);
        let Some(store) = store else {
            return;
        };
        let e = self.state.batch.entries[i].clone();
        let Some(run) = &e.run else {
            return;
        };
        let sha256 = run.report.input_sha256;
        let status = view::recent_status(&run.report, &run.status);
        let added_at = store
            .list_files(&e.name)
            .iter()
            .find(|f| f.sha256 == sha256)
            .map_or(unix, |f| f.added_at);
        let file = FileEntry {
            sha256,
            name: e.name.clone(),
            path: e.path.clone(),
            size: e.bytes,
            added_at,
            status,
        };
        let record = RunRecord {
            started_at,
            input_sha256: sha256,
            input_name: e.name.clone(),
            output_path: run.output_path.clone(),
            placed: run.placed,
            analysis_state: run.analysis_state.clone(),
            config_path: self.config_path.clone(),
            report: run.report.clone(),
        };
        let written = store
            .upsert_file(file)
            .and_then(|()| store.record_run(record, status));
        if let Err(err) = written {
            self.log(format!("history: could not record {}: {err}", e.name));
        }
        self.state.history = store.summary();
    }

    /// The view model again, and what follows from it: the cat's mood, and
    /// the widget's one resize request when a file first needs the user.
    pub(crate) fn refresh(&mut self, now: Duration) {
        self.vm = view::view(&self.state);
        if self.vm.mood != self.mood {
            self.mood = self.vm.mood;
            self.director.on(CatEvent::Mood(self.mood), now);
        }
        let needs = self.vm.needs_you.is_some();
        if needs && !self.needs_you && self.kind == LayoutKind::Widget {
            self.ask_for_full_size();
        }
        self.needs_you = needs;
    }

    /// Draws the screen at `now`, unless it would be the frame already shown.
    pub(crate) fn render(&mut self, surface: &mut dyn Surface, now: Duration) -> io::Result<()> {
        let shown = self.shown_at(now);
        if self.shown.as_ref() == Some(&shown) {
            return Ok(());
        }
        let c = self.draw(&shown);
        surface.present(&c, &self.theme)?;
        self.shown = Some(shown);
        Ok(())
    }

    /// What the screen shows at `now`.
    fn shown_at(&self, now: Duration) -> Shown {
        Shown {
            vm: self.vm.clone(),
            cat: self.director.frame(now),
            size: self.state.term_size,
            theme: self.theme_index,
            kind: self.kind,
            screen: self.state.screen.clone(),
        }
    }

    /// The canvas for `shown`: the layout, then the theme chooser over the
    /// full layout or the browse picker over either layout while it is open,
    /// with the full layout's status bar saying so.
    fn draw(&self, shown: &Shown) -> Canvas {
        let (w, h) = shown.size;
        let mut c = Canvas::new(w, h, &self.theme);
        layout(self.kind).draw(&mut c, &shown.vm, &shown.cat, &self.theme);
        match (shown.kind, &shown.screen) {
            (LayoutKind::Full, Screen::Themes { selected }) => {
                ThemeChooser::draw(&mut c, Theme::all(), *selected);
                modal_status(&mut c, &shown.vm, strings::CHOOSING_THEME, &self.theme);
            }
            (kind, Screen::Browse(picker)) => {
                browse::draw(&mut c, picker, kind, &self.theme);
                if kind == LayoutKind::Full {
                    modal_status(&mut c, &shown.vm, strings::BROWSING, &self.theme);
                }
            }
            _ => {}
        }
        c
    }
}

/// The full layout's status bar while a modal is up (T-24): the modal's state
/// takes the place of the batch's and the queue count.
fn modal_status(c: &mut Canvas, vm: &ViewModel, state: &str, theme: &Theme) {
    let sb = &vm.status_bar;
    let mut parts = vec![
        strings::NODE_N.replace("{n}", &sb.node.to_string()),
        format!("{{M}}{state}"),
    ];
    if sb.offline {
        parts.push(format!("{{G}}{}", strings::OFFLINE));
    }
    let mut right = format!(
        "{{M}}{} {{c}}│{{W}} {}×{}",
        theme.name, sb.size.0, sb.size.1
    );
    if let Some((h, m)) = sb.clock {
        right.push_str(&format!(" {{c}}│{{W}} {h:02}:{m:02}"));
    }
    let (w, h) = FULL_SIZE;
    lightbar::status_bar(c, i32::from(h) - 1, i32::from(w), &parts, &right, theme);
}

/// The whole of `path`, if it is at most `limit` bytes.
fn read_at_most(path: &std::path::Path, limit: u64) -> Result<Vec<u8>, &'static str> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(limit.saturating_add(1)).read_to_end(&mut bytes))
        .map_err(|_| strings::DROP_UNREADABLE)?;
    if bytes.len() as u64 > limit {
        return Err(strings::DROP_HOLD_LIMIT);
    }
    Ok(bytes)
}

fn layout(kind: LayoutKind) -> &'static dyn Layout {
    match kind {
        LayoutKind::Full => &FullLayout,
        LayoutKind::Widget => &WidgetLayout,
        LayoutKind::OneLine => &OneLine,
    }
}

#[cfg(test)]
mod tests;
