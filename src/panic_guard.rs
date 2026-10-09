//! Framework-free, UI-thread-scoped panic hook and the job-log sink (T-15).
//!
//! A Rust panic hook runs on the panicking thread before it unwinds, including
//! for panics that `catch_unwind` later catches (the job runner and the text
//! extractor catch engine panics). So the hook looks at the current thread:
//! on the UI thread it writes the terminal-restore bytes once and chains to the
//! previous hook, which prints the message on a usable terminal; on any other
//! thread it hands the payload and location to that thread's job sink and
//! writes nothing to the terminal, so an engine panic never tears down the
//! screen (D-051 amended, eng-r2-q3). T-23a only wires it. The hook and the
//! terminal guard share one restore: whichever goes first gives the terminal
//! back, and the other then writes nothing ([`claim_restore`]).
#![allow(clippy::disallowed_types)]

use std::cell::RefCell;
use std::io::Write;
use std::panic::{self, PanicHookInfo};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, ThreadId};

type Hook = Box<dyn Fn(&PanicHookInfo<'_>) + Sync + Send + 'static>;

/// Where a non-UI thread's panic message goes.
pub type JobSink = Box<dyn FnMut(String)>;

struct Terminal {
    ui_thread: ThreadId,
    restore: Vec<u8>,
    writer: Box<dyn Write + Send>,
    restored: bool,
}

static TERMINAL: Mutex<Option<Terminal>> = Mutex::new(None);
/// The hook `install` replaced; `Some` once installed.
static PREVIOUS: Mutex<Option<Arc<Hook>>> = Mutex::new(None);

thread_local! {
    static JOB_SINK: RefCell<Option<JobSink>> = const { RefCell::new(None) };
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic inside the hook must not turn into a second one here.
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Installs the hook. `restore` is written to `writer` the first time the UI
/// thread panics, never again and never for another thread. Calling it again
/// replaces the thread, bytes and writer and keeps one hook.
pub fn install(ui_thread: ThreadId, restore: Vec<u8>, writer: Box<dyn Write + Send>) {
    *lock(&TERMINAL) = Some(Terminal {
        ui_thread,
        restore,
        writer,
        restored: false,
    });
    let mut previous = lock(&PREVIOUS);
    if previous.is_some() {
        return;
    }
    let chained: Arc<Hook> = Arc::new(panic::take_hook());
    *previous = Some(Arc::clone(&chained));
    panic::set_hook(Box::new(move |info| {
        if on_ui_thread() {
            restore_terminal();
            chained(info);
        } else {
            to_job_sink(describe(info));
        }
    }));
}

/// Sets this thread's job sink: where its panic message goes once the hook is
/// installed. The runner sets one on every worker it spawns.
pub fn set_job_sink(sink: JobSink) {
    JOB_SINK.with(|s| *s.borrow_mut() = Some(sink));
}

fn on_ui_thread() -> bool {
    lock(&TERMINAL)
        .as_ref()
        .is_some_and(|t| t.ui_thread == thread::current().id())
}

fn restore_terminal() {
    let mut terminal = lock(&TERMINAL);
    if let Some(t) = terminal.as_mut().filter(|t| !t.restored) {
        t.restored = true;
        let _ = t.writer.write_all(&t.restore);
        let _ = t.writer.flush();
    }
}

/// Claims the one restore of the terminal for the caller (the terminal guard):
/// true when nothing has given it back yet, and from then on the hook writes
/// nothing; false when the hook already has. True when no hook is installed.
pub fn claim_restore() -> bool {
    match lock(&TERMINAL).as_mut() {
        Some(t) => !std::mem::replace(&mut t.restored, true),
        None => true,
    }
}

fn to_job_sink(message: String) {
    // `try_with`: a thread being torn down has no thread-locals left; a sink
    // that is already running (it panicked) is not entered again.
    let _ = JOB_SINK.try_with(|s| {
        if let Ok(mut sink) = s.try_borrow_mut()
            && let Some(sink) = sink.as_mut()
        {
            sink(message);
        }
    });
}

/// `panicked at <file>:<line>:<col>: <payload>`.
fn describe(info: &PanicHookInfo<'_>) -> String {
    let payload = payload_text(info.payload());
    match info.location() {
        Some(at) => format!(
            "panicked at {}:{}:{}: {payload}",
            at.file(),
            at.line(),
            at.column()
        ),
        None => format!("panicked: {payload}"),
    }
}

/// The text of a panic payload, for messages built after `catch_unwind`.
pub fn payload_text(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s
    } else {
        "a panic without a message"
    }
}

/// Serialises the tests that touch the process-wide hook.
#[cfg(test)]
pub(crate) fn test_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    lock(&LOCK)
}

/// Puts back the hook `install` replaced and forgets the terminal.
#[cfg(test)]
pub(crate) fn uninstall() {
    *lock(&TERMINAL) = None;
    if let Some(previous) = lock(&PREVIOUS).take() {
        panic::set_hook(Box::new(move |info| previous(info)));
    }
}

/// A `Write` whose bytes a test can read back.
#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct SharedWriter(pub(crate) Arc<Mutex<Vec<u8>>>);

#[cfg(test)]
impl SharedWriter {
    pub(crate) fn bytes(&self) -> Vec<u8> {
        lock(&self.0).clone()
    }
}

#[cfg(test)]
impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        lock(&self.0).extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const RESTORE: &[u8] = b"\x1b[?1049l\x1b[?2004l";

    /// A stand-in for the previous hook that counts only this test's own
    /// panics. Other tests run at the same time, and a panic of theirs can
    /// reach this hook between `uninstall` and the test restoring the original.
    fn counting_hook(count: &'static AtomicUsize, ours: &'static [&'static str]) -> Hook {
        Box::new(move |info| {
            if ours.contains(&payload_text(info.payload())) {
                count.fetch_add(1, Ordering::SeqCst);
            }
        })
    }

    #[test]
    fn a_ui_thread_panic_restores_the_terminal_once_and_chains() {
        let _serial = test_lock();
        let original = panic::take_hook();
        static CHAINED: AtomicUsize = AtomicUsize::new(0);
        panic::set_hook(counting_hook(&CHAINED, &["ui boom"]));
        let writer = SharedWriter::default();
        install(
            thread::current().id(),
            RESTORE.to_vec(),
            Box::new(writer.clone()),
        );

        for _ in 0..2 {
            let r = panic::catch_unwind(|| panic!("ui boom"));
            assert!(r.is_err());
        }
        uninstall();
        panic::set_hook(original);

        assert_eq!(writer.bytes(), RESTORE, "written once, for the first panic");
        assert_eq!(
            CHAINED.load(Ordering::SeqCst),
            2,
            "the previous hook ran each time"
        );
    }

    #[test]
    fn a_worker_panic_goes_to_its_sink_and_never_to_the_terminal() {
        let _serial = test_lock();
        let original = panic::take_hook();
        static CHAINED: AtomicUsize = AtomicUsize::new(0);
        panic::set_hook(counting_hook(&CHAINED, &["engine boom", "nobody listens"]));
        let writer = SharedWriter::default();
        install(
            thread::current().id(),
            RESTORE.to_vec(),
            Box::new(writer.clone()),
        );

        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink_seen = Arc::clone(&seen);
        let with_sink = thread::spawn(move || {
            set_job_sink(Box::new(move |msg| lock(&sink_seen).push(msg)));
            panic!("engine boom");
        })
        .join();
        let without_sink = thread::spawn(|| panic!("nobody listens")).join();
        uninstall();
        panic::set_hook(original);

        assert!(with_sink.is_err() && without_sink.is_err());
        assert!(writer.bytes().is_empty(), "nothing reached the terminal");
        assert_eq!(
            CHAINED.load(Ordering::SeqCst),
            0,
            "the default hook never ran"
        );
        let seen = lock(&seen).clone();
        assert_eq!(seen.len(), 1);
        assert!(
            seen[0]
                .replace('\\', "/")
                .starts_with("panicked at src/panic_guard.rs:"),
            "{}",
            seen[0]
        );
        assert!(seen[0].ends_with(": engine boom"), "{}", seen[0]);
    }

    #[test]
    fn the_terminal_is_restored_once_by_the_hook_or_the_guard() {
        let _serial = test_lock();
        let original = panic::take_hook();
        panic::set_hook(Box::new(|_| {}));
        let writer = SharedWriter::default();
        let ui = thread::current().id();

        // The hook went first: the guard's claim is refused.
        install(ui, RESTORE.to_vec(), Box::new(writer.clone()));
        assert!(panic::catch_unwind(|| panic!("ui boom")).is_err());
        let after_hook = claim_restore();

        // The guard went first: a later panic writes nothing.
        let quiet = SharedWriter::default();
        install(ui, RESTORE.to_vec(), Box::new(quiet.clone()));
        let first = claim_restore();
        let second = claim_restore();
        assert!(panic::catch_unwind(|| panic!("ui boom")).is_err());
        uninstall();
        panic::set_hook(original);

        assert_eq!(writer.bytes(), RESTORE);
        assert!(!after_hook, "the hook already gave the terminal back");
        assert!(first && !second, "one claim");
        assert!(quiet.bytes().is_empty(), "the guard already gave it back");
        assert!(claim_restore(), "with no hook installed the guard restores");
    }

    #[test]
    fn payload_text_reads_both_string_kinds() {
        let s: Box<dyn std::any::Any + Send> = Box::new("static");
        assert_eq!(payload_text(&*s), "static");
        let s: Box<dyn std::any::Any + Send> = Box::new(String::from("owned"));
        assert_eq!(payload_text(&*s), "owned");
        let s: Box<dyn std::any::Any + Send> = Box::new(7u8);
        assert_eq!(payload_text(&*s), "a panic without a message");
    }
}
