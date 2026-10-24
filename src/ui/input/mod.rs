//! Terminal input (D-033 amended): who owns stdin, and the one event type the
//! loop sees.
//!
//! - Windows: crossterm's reader always (console input records, never raw
//!   stdin), and never an OSC 72 handshake.
//! - Unix, when the handshake found kitty: the raw-stdin splitter (T-31) owns
//!   stdin for the session.
//! - Unix otherwise: crossterm's reader, with bracketed paste on (the terminal
//!   setup in `term.rs` turns it on, and every exit path turns it off).
//!
//! On Unix only two things become paths: a bracketed paste (crossterm's
//! `Event::Paste`, or the raw splitter's, which likewise exist only between
//! the markers) and, on kitty, an OSC 72 drop's `file://` list. Loose key
//! events never do. On Windows, where crossterm reports no paste, the
//! app runs keys through the typed-path collector (`collector.rs`, D-034).
//! Every path, from any source, passes the drop gate (`gate.rs`).
//!
//! The source is armed only after the first frame and the input flush
//! (`term::flush_input`), so nothing typed, pasted or dropped before the cat
//! is on screen reaches the queue (D-043). crossterm builds its event source
//! lazily on the first read, on the reader thread spawned here.

// The typed-path collector is built on Windows only (D-034); its tests run
// everywhere.
#[cfg(any(windows, test))]
pub(crate) mod collector;
pub(crate) mod gate;
pub(crate) mod keys;
pub(crate) mod osc72;
pub(crate) mod paste;

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use crossterm::event::{self, Event, KeyEvent, KeyEventKind, MouseEvent};

pub use osc72::DndEvent;

use super::term::HandshakeResult;
use crate::config;

/// One terminal input event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    Key(KeyEvent),
    Mouse(MouseEvent),
    /// A bracketed paste (Unix): one event per paste, however many lines.
    Paste(String),
    /// The terminal's new size, columns then rows.
    Resize(u16, u16),
    /// kitty drag and drop (T-31): built on Unix only, by the raw splitter.
    #[cfg_attr(not(unix), allow(dead_code))]
    Dnd(DndEvent),
    /// A line for the debug log from the reader thread (T-31: the raw
    /// reader runs without resize events).
    #[cfg_attr(not(unix), allow(dead_code))]
    Note(String),
    /// Time passes and nothing else happens: a scripted input's pause. The
    /// readers never send it; the loop ticks on its own.
    #[cfg_attr(not(test), allow(dead_code))]
    Tick,
}

/// The reader that feeds [`Input`]s to the loop.
pub struct InputSource;

impl InputSource {
    /// Arms the input source and returns its queue. Call it once, after the
    /// first frame and immediately after `term::flush_input`.
    pub fn spawn(_cfg: &config::Ui, handshake: HandshakeResult) -> Receiver<Input> {
        let (tx, rx) = mpsc::channel();
        // Detached: the thread blocks reading and ends with the process, or
        // when the loop drops the receiver.
        #[cfg(unix)]
        if handshake.kitty {
            let _ = thread::Builder::new()
                .name("input".into())
                .spawn(move || raw_reader(&tx));
            return rx;
        }
        let _ = handshake;
        let _ = thread::Builder::new()
            .name("input".into())
            .spawn(move || crossterm_reader(&tx));
        rx
    }
}

/// How long a lone `ESC` waits for the rest of a sequence before it is the
/// Esc key.
#[cfg(unix)]
const ESC_WAIT: std::time::Duration = std::time::Duration::from_millis(50);

/// kitty's branch (T-31): stdin read raw and split into OSC 72 events and
/// keys, mouse reports and pastes; a SIGWINCH is a resize.
#[cfg(unix)]
fn raw_reader(tx: &Sender<Input>) {
    use super::term::{RawStdin, Wake};
    // The raw reader is the only input on kitty: if the SIGWINCH pipe cannot
    // be set up, keys and drops still come, without resize events. Opening
    // without the pipe does no I/O and cannot fail.
    let mut stdin = match RawStdin::open(true) {
        Ok(stdin) => stdin,
        Err(e) => {
            let note = format!("input: no resize events ({e}); keys and drops still work");
            if tx.send(Input::Note(note)).is_err() {
                return;
            }
            match RawStdin::open(false) {
                Ok(stdin) => stdin,
                // Unreachable today; never leave the app with no keys.
                Err(_) => return crossterm_reader(tx),
            }
        }
    };
    let mut splitter = osc72::Splitter::new();
    loop {
        let inputs = match stdin.wait(ESC_WAIT) {
            Ok(Wake::Bytes(b)) => splitter.feed(b),
            Ok(Wake::Idle) => splitter.idle(),
            Ok(Wake::Resize) => match crossterm::terminal::size() {
                Ok((w, h)) => vec![Input::Resize(w, h)],
                Err(_) => Vec::new(),
            },
            Ok(Wake::Closed) | Err(_) => return,
        };
        for input in inputs {
            if tx.send(input).is_err() {
                return;
            }
        }
    }
}

fn crossterm_reader(tx: &Sender<Input>) {
    while let Ok(ev) = event::read() {
        if let Some(input) = from_crossterm(ev)
            && tx.send(input).is_err()
        {
            return;
        }
    }
}

/// A crossterm event as an [`Input`]; `None` for what the loop never needs
/// (focus changes, key releases, which only Windows reports).
pub fn from_crossterm(ev: Event) -> Option<Input> {
    match ev {
        Event::Key(k) if k.kind == KeyEventKind::Release => None,
        Event::Key(k) => Some(Input::Key(k)),
        Event::Mouse(m) => Some(Input::Mouse(m)),
        Event::Paste(s) => Some(Input::Paste(s)),
        Event::Resize(w, h) => Some(Input::Resize(w, h)),
        Event::FocusGained | Event::FocusLost => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEventState, KeyModifiers};

    fn key(kind: KeyEventKind) -> Event {
        Event::Key(KeyEvent {
            code: KeyCode::Char('Z'),
            modifiers: KeyModifiers::SHIFT,
            kind,
            state: KeyEventState::NONE,
        })
    }

    #[test]
    fn crossterm_events_map_onto_inputs() {
        let Some(Input::Key(k)) = from_crossterm(key(KeyEventKind::Press)) else {
            panic!("a press is a key");
        };
        assert_eq!(k.code, KeyCode::Char('Z'));
        assert!(from_crossterm(key(KeyEventKind::Repeat)).is_some());
        assert_eq!(from_crossterm(key(KeyEventKind::Release)), None);
        assert_eq!(
            from_crossterm(Event::Paste("a.pdf\nb.pdf".into())),
            Some(Input::Paste("a.pdf\nb.pdf".into()))
        );
        assert_eq!(
            from_crossterm(Event::Resize(112, 38)),
            Some(Input::Resize(112, 38))
        );
        assert_eq!(from_crossterm(Event::FocusGained), None);
        assert_eq!(from_crossterm(Event::FocusLost), None);
    }
}
