//! The typed-path collector (Windows only, D-034). crossterm never reports a
//! paste on Windows: a path dropped on Windows Terminal arrives as one key
//! event per character. The collector tells such a burst from typing by
//! speed: printable keys less than [`BURST_GAP`] apart are one burst, and a
//! burst of two or more characters is pasted text. The app hands that text to
//! `paths_from_paste`.
//!
//! | what arrives | what the collector gives back |
//! |---|---|
//! | a printable key | nothing yet: it is held |
//! | a printable key [`BURST_GAP`] or more after the last | the held burst, then holds the new key |
//! | no key for [`BURST_GAP`] (`idle`) | the held burst |
//! | Enter inside a burst | the burst as text; Enter is part of the paste |
//! | any other key | the held burst, then the key |
//!
//! A burst of one key is a key, given back as it came, so hotkeys still work
//! (they land [`BURST_GAP`] late). The app arms the collector with the rest of
//! its input, after the first frame. Built on Windows only; its tests run on
//! every target.

use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent};

use super::keys::printable;

/// Keys closer together than this are one burst.
pub(crate) const BURST_GAP: Duration = Duration::from_millis(150);

/// What the collector gives back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Flush {
    /// A key to handle as a key.
    Key(KeyEvent),
    /// A burst's text, for `paths_from_paste`.
    Text(String),
}

/// The held burst.
#[derive(Debug, Default)]
pub(crate) struct Collector {
    keys: Vec<KeyEvent>,
    /// When the last held key came.
    last: Duration,
}

impl Collector {
    /// `k`, arriving at `now` (the app's clock since start-up).
    pub(crate) fn key(&mut self, k: KeyEvent, now: Duration) -> Vec<Flush> {
        let mut out = Vec::new();
        let late = self.idle_at(now);
        if printable(&k).is_some() {
            out.extend(late);
            self.keys.push(k);
            self.last = now;
            return out;
        }
        if let KeyCode::Modifier(_) = k.code {
            // A lone Shift is neither text nor a reason to end the burst.
            out.extend(late);
            out.push(Flush::Key(k));
            return out;
        }
        match (late, self.take()) {
            (Some(done), _) => {
                out.push(done);
                out.push(Flush::Key(k));
            }
            (None, Some(Flush::Text(text))) if k.code == KeyCode::Enter => {
                out.push(Flush::Text(text));
            }
            (None, held) => {
                out.extend(held);
                out.push(Flush::Key(k));
            }
        }
        out
    }

    /// The held burst, once nothing has come for [`BURST_GAP`].
    pub(crate) fn idle(&mut self, now: Duration) -> Option<Flush> {
        self.idle_at(now)
    }

    fn idle_at(&mut self, now: Duration) -> Option<Flush> {
        if now.saturating_sub(self.last) >= BURST_GAP {
            self.take()
        } else {
            None
        }
    }

    /// The held keys: one key as a key, more as text.
    fn take(&mut self) -> Option<Flush> {
        match self.keys.len() {
            0 => None,
            1 => self.keys.pop().map(Flush::Key),
            _ => Some(Flush::Text(
                self.keys.drain(..).filter_map(|k| printable(&k)).collect(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers, ModifierKeyCode};

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn ch(c: char) -> KeyEvent {
        press(KeyCode::Char(c))
    }

    /// Types `text` from `start`, one key every `gap` ms; returns what came
    /// back and the time of the last key.
    fn type_out(c: &mut Collector, text: &str, start: u64, gap: u64) -> (Vec<Flush>, u64) {
        let mut out = Vec::new();
        let mut t = start;
        for (i, k) in text.chars().enumerate() {
            t = start + gap * i as u64;
            out.extend(c.key(ch(k), ms(t)));
        }
        (out, t)
    }

    #[test]
    fn a_fast_burst_flushes_as_text_when_idle() {
        let mut c = Collector::default();
        let (out, last) = type_out(&mut c, r#""C:\a b\x.pdf""#, 1000, 2);
        assert!(out.is_empty(), "held while it comes");
        assert_eq!(c.idle(ms(last + 149)), None, "149 ms is still the burst");
        assert_eq!(
            c.idle(ms(last + 150)),
            Some(Flush::Text(r#""C:\a b\x.pdf""#.into()))
        );
        assert_eq!(c.idle(ms(last + 1000)), None, "flushed once");
    }

    #[test]
    fn enter_ends_a_burst_and_is_eaten() {
        let mut c = Collector::default();
        let (_, last) = type_out(&mut c, "a.pdf", 0, 10);
        assert_eq!(
            c.key(press(KeyCode::Enter), ms(last + 5)),
            [Flush::Text("a.pdf".into())]
        );
        // The next Enter is a key again.
        assert_eq!(
            c.key(press(KeyCode::Enter), ms(last + 6)),
            [Flush::Key(press(KeyCode::Enter))]
        );
    }

    #[test]
    fn keys_149_ms_apart_are_a_burst_and_150_ms_apart_are_typing() {
        let mut c = Collector::default();
        let (out, last) = type_out(&mut c, "ab", 0, 149);
        assert!(out.is_empty());
        assert_eq!(c.idle(ms(last + 150)), Some(Flush::Text("ab".into())));

        let mut c = Collector::default();
        let (out, last) = type_out(&mut c, "qH", 0, 150);
        assert_eq!(out, [Flush::Key(ch('q'))], "q was typed, not pasted");
        assert_eq!(c.idle(ms(last + 150)), Some(Flush::Key(ch('H'))));
    }

    #[test]
    fn a_lone_key_comes_back_as_a_key() {
        let mut c = Collector::default();
        assert!(c.key(ch('q'), ms(500)).is_empty());
        assert_eq!(c.idle(ms(600)), None);
        assert_eq!(c.idle(ms(650)), Some(Flush::Key(ch('q'))));
        // Enter right after one key: the key, then Enter.
        assert!(c.key(ch('T'), ms(900)).is_empty());
        assert_eq!(
            c.key(press(KeyCode::Enter), ms(910)),
            [Flush::Key(ch('T')), Flush::Key(press(KeyCode::Enter))]
        );
    }

    #[test]
    fn another_key_ends_the_burst_and_passes() {
        let mut c = Collector::default();
        type_out(&mut c, "x.pdf", 0, 1);
        assert_eq!(
            c.key(press(KeyCode::Up), ms(10)),
            [Flush::Text("x.pdf".into()), Flush::Key(press(KeyCode::Up))]
        );
        // A late Enter after an unflushed burst: the burst, then Enter.
        type_out(&mut c, "y.pdf", 100, 1);
        assert_eq!(
            c.key(press(KeyCode::Enter), ms(400)),
            [
                Flush::Text("y.pdf".into()),
                Flush::Key(press(KeyCode::Enter))
            ]
        );
    }

    #[test]
    fn a_gap_starts_a_new_burst() {
        let mut c = Collector::default();
        type_out(&mut c, "a.pdf", 0, 5);
        let (out, last) = type_out(&mut c, "b.pdf", 400, 5);
        assert_eq!(out, [Flush::Text("a.pdf".into())]);
        assert_eq!(c.idle(ms(last + 150)), Some(Flush::Text("b.pdf".into())));
    }

    #[test]
    fn a_lone_modifier_neither_types_nor_ends_the_burst() {
        let mut c = Collector::default();
        type_out(&mut c, "a", 0, 1);
        let shift = press(KeyCode::Modifier(ModifierKeyCode::LeftShift));
        assert_eq!(c.key(shift, ms(2)), [Flush::Key(shift)]);
        assert!(c.key(ch('B'), ms(3)).is_empty());
        assert_eq!(c.idle(ms(200)), Some(Flush::Text("aB".into())));
    }
}
