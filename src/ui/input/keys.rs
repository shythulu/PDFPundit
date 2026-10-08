//! Key decoding for the typed-path collector (T-23b): which key events are
//! text.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// The character `k` types, if it types one: a `Char` key with no Ctrl or
/// Alt, or with both (AltGr on Windows reports as Ctrl+Alt, and it types the
/// `\` of several European layouts).
pub(crate) fn printable(k: &KeyEvent) -> Option<char> {
    let KeyCode::Char(c) = k.code else {
        return None;
    };
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    (ctrl == alt && !c.is_control()).then_some(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn characters_are_text_and_shortcuts_are_not() {
        let none = KeyModifiers::NONE;
        let shift = KeyModifiers::SHIFT;
        let altgr = KeyModifiers::CONTROL | KeyModifiers::ALT;
        assert_eq!(printable(&key(KeyCode::Char('a'), none)), Some('a'));
        assert_eq!(printable(&key(KeyCode::Char('"'), shift)), Some('"'));
        assert_eq!(printable(&key(KeyCode::Char(' '), none)), Some(' '));
        assert_eq!(printable(&key(KeyCode::Char('\\'), altgr)), Some('\\'));
        assert_eq!(printable(&key(KeyCode::Char('é'), none)), Some('é'));
        assert_eq!(
            printable(&key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(printable(&key(KeyCode::Char('x'), KeyModifiers::ALT)), None);
        assert_eq!(printable(&key(KeyCode::Char('\u{7}'), none)), None);
        assert_eq!(printable(&key(KeyCode::Enter, none)), None);
        assert_eq!(printable(&key(KeyCode::Up, none)), None);
    }
}
