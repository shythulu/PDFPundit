//! The shell (T-23a): [`run`]'s start-up order and the event loop.
//!
//! Start-up (D-043 amended, eng-r3-q3):
//! 1. stdin and stdout must both be terminals, else one line on stderr and
//!    exit 2, before any byte of input is read or anything is parsed; argv is
//!    never looked at;
//! 2. raw mode, the alternate screen and the colour probe; a failure exits
//!    non-zero with one line (a Git Bash pty passes step 1 and can fail here);
//! 3. on Unix the kitty handshake slot (T-31; nothing until then), whose
//!    discarded byte count goes to the debug log;
//! 4. the first frame;
//! 5. the pending terminal input flushed, then the input source armed.
//!
//! Nothing typed, pasted or dropped before step 5 reaches the loop: the
//! terminal's queue is flushed, and an input that somehow reaches an unarmed
//! app is dropped and counted.
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
use super::director::{CatEvent, CatFrame, Director, Mood, Stage};
use super::input::{Input, InputSource};
use super::layout::full::{self, FullLayout};
use super::layout::widget::{OneLine, WidgetLayout};
use super::layout::{Layout, LayoutKind, LayoutPin, choose};
use super::state::{AppState, Screen};
use super::strings;
use super::term::{self, Surface};
use super::theme::Theme;
use super::view::{self, ViewModel};
use crate::appdirs::AppDirs;
use crate::config::{self, Config, OutputDir};
use crate::engine::{AnalyzeOptions, Engine, Pdfpundit, RepairOptions};
use crate::jobs::{AppEvent, EntryState, JobEvent, JobId, JobKind, JobRunner, RunnerOptions};
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

    // (3) The handshake slot.
    let handshake = term::handshake();

    let mut app = App::new(&config.ui, caps, config_path);
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
    runner.shutdown(SHUTDOWN_GRACE);
    guard.restore();
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
                app.on_input(input);
                true
            }
            AppEvent::Job(id, ev) => {
                if let Some(r) = runner.as_deref_mut() {
                    r.on_job_event(id, &ev);
                }
                app.on_job(id, ev, unix, store.as_deref_mut());
                false
            }
            AppEvent::Tick => false,
        };
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
        self.director.set_stage(match self.kind {
            LayoutKind::Full => Stage::Full,
            LayoutKind::Widget | LayoutKind::OneLine => Stage::Widget,
        });
    }

    pub(crate) fn on_input(&mut self, input: Input) {
        if !self.armed {
            self.pre_frame_dropped += 1;
            return;
        }
        match input {
            Input::Key(k) => self.on_key(k),
            Input::Resize(w, h) => self.resize(w, h),
            // TODO(T-23b): pastes go through the paste parser and the drop gate.
            Input::Paste(_) | Input::Mouse(_) | Input::Tick => {}
            Input::Dnd(d) => match d {},
        }
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
        match k.code {
            KeyCode::Char('q' | 'Q') => self.quit = true,
            KeyCode::Char('T' | 't') => {
                self.theme_before = self.theme_index;
                self.state.screen = Screen::Themes {
                    selected: self.theme_index,
                };
            }
            // TODO(T-37): the browse picker.
            KeyCode::Char('b' | 'B') => self.state.hint = Some(strings::NOT_YET),
            KeyCode::Char(c) => {
                full::menu_key(&mut self.state, c);
            }
            KeyCode::Enter if self.kind == LayoutKind::Widget => self.ask_for_full_size(),
            KeyCode::Up if self.kind == LayoutKind::Full => self.move_cursor(false),
            KeyCode::Down if self.kind == LayoutKind::Full => self.move_cursor(true),
            _ => {}
        }
    }

    /// The theme chooser: up and down try each theme on the spot, Enter keeps
    /// it, Esc or `q` goes back to the one before.
    // TODO(T-24): `ThemeChooser::draw` shows the list over the main screen.
    fn chooser_key(&mut self, code: KeyCode, selected: usize) {
        let n = Theme::all().len();
        let pick = match code {
            KeyCode::Up | KeyCode::Char('k') => (selected + n - 1) % n,
            KeyCode::Down | KeyCode::Char('j') => (selected + 1) % n,
            KeyCode::Enter => {
                self.state.screen = Screen::Main;
                return;
            }
            KeyCode::Esc | KeyCode::Char('q' | 'Q') => {
                self.set_theme(self.theme_before);
                self.state.screen = Screen::Main;
                return;
            }
            _ => return,
        };
        self.set_theme(pick);
        self.state.screen = Screen::Themes { selected: pick };
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
            JobEvent::ExportDone { .. } => {
                entry.state = EntryState::Done;
                self.finish(i);
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
        let shown = Shown {
            vm: self.vm.clone(),
            cat: self.director.frame(now),
            size: self.state.term_size,
            theme: self.theme_index,
            kind: self.kind,
        };
        if self.shown.as_ref() == Some(&shown) {
            return Ok(());
        }
        let (w, h) = shown.size;
        let mut c = Canvas::new(w, h, &self.theme);
        layout(self.kind).draw(&mut c, &shown.vm, &shown.cat, &self.theme);
        surface.present(&c, &self.theme)?;
        self.shown = Some(shown);
        Ok(())
    }
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
