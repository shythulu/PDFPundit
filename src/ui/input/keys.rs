//! Key input.
//!
//! - [`printable`]: which key events are text, for the Windows typed-path
//!   collector (T-23b).
//! - [`Decoder`]: the raw terminal bytes kitty sends once the app owns stdin
//!   (T-31, D-033) as events: kitty's legacy key encoding (the xterm-style
//!   one it uses until a program asks for its keyboard protocol), SGR mouse
//!   reports and bracketed pastes. OSC strings and device-attribute replies
//!   come out as their own tokens, never as keys: `osc72.rs` frames the
//!   OSC 72 ones and drops every other.
//!
//! | bytes | event |
//! |---|---|
//! | UTF-8 text | `Char`, Shift when upper case |
//! | `CR`, `LF` / `HT` / `DEL`, `BS` | Enter / Tab / Backspace |
//! | `0x01`–`0x1a`, `0x00`, `0x1c`–`0x1f` | Ctrl+letter, Ctrl+Space, Ctrl+4–7 |
//! | `ESC` then a key | that key with Alt |
//! | `ESC` alone, then nothing for a while ([`Decoder::idle`]) | Esc |
//! | `CSI A`–`D`, `H`, `F`, `Z`, `SS3 A`–`D`, `H`, `F`, `P`–`S` | arrows, Home, End, BackTab, F1–F4 |
//! | `CSI n ~` | Home, Insert, Delete, End, PageUp, PageDown, F1–F12 |
//! | `CSI 1 ; m X`, `CSI n ; m ~` | the same with modifiers (`m - 1`: 1 Shift, 2 Alt, 4 Ctrl, 8 Super, 16 Hyper, 32 Meta) |
//! | `CSI < b ; x ; y M` / `m` | an SGR mouse press, drag, move, wheel / release |
//! | `CSI 200 ~` … `CSI 201 ~` | one paste, everything between taken as text |
//! | `CSI ? … c` | [`Token::DeviceAttributes`] |
//! | `ESC ] digits ;` … `BEL` or `ESC \` | [`Token::Osc`] |
//! | `ESC P`, `ESC _`, `ESC ^`, `ESC X` … `ESC \` (DCS, APC, PM, SOS) | nothing |
//! | any other CSI | nothing |
//!
//! `ESC ]` is also what Alt+] types, so an OSC is only one once its number
//! (at most [`OSC_DIGITS`] digits) and `;` have come; `ESC ]` followed by
//! anything else, or by nothing for a while, is Alt+] and the keys after it.
//! Likewise `ESC P` (Alt+P) and the other string introducers: a string that
//! ends in `ESC \` is dropped whole, and one still open when nothing comes
//! for a while ([`Decoder::idle`]), or broken off by another escape, was Alt
//! and the keys after it (one over [`STR_CAP`] bytes is never typing). A
//! reply split by a pause is the price; the app sends no query that draws one
//! today.
//!
//! A bracketed paste over [`PASTE_CAP`] bytes is read to its end and dropped,
//! with a debug-log note.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
#[cfg_attr(not(unix), allow(unused_imports))]
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

#[cfg_attr(not(unix), allow(unused_imports))]
use super::Input;

/// The longest OSC body kept: the `72;` prefix, the metadata and a full
/// 4,096-byte payload with room to spare. A longer one comes out truncated.
pub(crate) const OSC_CAP: usize = 8 * 1024;
/// The longest CSI kept; a longer one is consumed and dropped.
const CSI_CAP: usize = 64;
/// The most digits an OSC number may have before it is Alt+] and typing.
pub(crate) const OSC_DIGITS: usize = 8;
/// The longest DCS, APC, PM or SOS string kept for replay as keys; a longer
/// one is never typing and is dropped whatever ends it.
const STR_CAP: usize = 4096;
/// The largest bracketed paste delivered; a larger one is dropped.
pub(crate) const PASTE_CAP: usize = 1 << 20;
const PASTE_END: &[u8] = b"\x1b[201~";

/// The character `k` types, if it types one: a `Char` key with no Ctrl or
/// Alt, or with both (AltGr on Windows reports as Ctrl+Alt, and it types the
/// `\` of several European layouts).
#[cfg(any(windows, test))]
pub(crate) fn printable(k: &KeyEvent) -> Option<char> {
    let KeyCode::Char(c) = k.code else {
        return None;
    };
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    (ctrl == alt && !c.is_control()).then_some(c)
}

/// One thing the decoder found in the bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Token {
    Input(Input),
    /// An OSC string's body, between `ESC ]` and its terminator. `truncated`
    /// when it was longer than [`OSC_CAP`] (the rest was read and dropped);
    /// `raw` is every byte it took, introducer and terminator included.
    Osc {
        body: Vec<u8>,
        truncated: bool,
        raw: usize,
    },
    /// A primary device attributes reply (`CSI ? … c`), `raw` bytes long.
    DeviceAttributes {
        raw: usize,
    },
}

#[derive(Debug, Default)]
enum State {
    #[default]
    Ground,
    /// Part of a UTF-8 character; `alt` when an `ESC` came before it.
    Utf8 {
        bytes: Vec<u8>,
        need: usize,
        alt: bool,
    },
    /// `ESC`.
    Esc,
    /// `ESC [` and what followed it.
    Csi { buf: Vec<u8>, overflow: bool },
    /// `ESC O`.
    Ss3,
    /// `ESC ]` and its body. `committed` once its number and `;` came;
    /// `esc` when the last byte was an `ESC` (half of an `ESC \`).
    Osc {
        body: Vec<u8>,
        truncated: bool,
        raw: usize,
        committed: bool,
        esc: bool,
    },
    /// `ESC` and `intro` (`P`, `_`, `^` or `X`): a control string, or Alt and
    /// the keys after it. `esc` when the last byte was an `ESC`.
    Str {
        intro: u8,
        buf: Vec<u8>,
        overflow: bool,
        esc: bool,
    },
    /// Inside a bracketed paste; `over` once it passed [`PASTE_CAP`] (only
    /// the tail that may hold the end marker is kept).
    Paste { buf: Vec<u8>, over: bool },
}

/// Terminal input bytes into [`Token`]s, across any split of the stream.
#[derive(Debug, Default)]
pub(crate) struct Decoder {
    state: State,
}

#[cfg_attr(not(unix), allow(dead_code))]
impl Decoder {
    /// Everything `bytes` completes. A sequence cut off at the end waits for
    /// the next call.
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Vec<Token> {
        let mut out = Vec::new();
        for &b in bytes {
            self.byte(b, &mut out);
        }
        out
    }

    /// Nothing came for a while: what was waiting on the next byte is a key
    /// after all. A lone `ESC` is Esc; `ESC [`, `ESC O` and an `ESC ]` whose
    /// number has not ended are Alt+`[`, Alt+`O` and Alt+`]` with the keys
    /// after it. A committed OSC, a started CSI and a paste keep waiting.
    pub(crate) fn idle(&mut self) -> Vec<Token> {
        let mut out = Vec::new();
        match std::mem::take(&mut self.state) {
            State::Esc => out.push(key(KeyCode::Esc, KeyModifiers::NONE)),
            State::Ss3 => out.push(alt_char('O')),
            State::Csi { buf, .. } if buf.is_empty() => out.push(alt_char('[')),
            State::Osc {
                body,
                committed: false,
                ..
            } => {
                out.push(alt_char(']'));
                for b in body {
                    self.byte(b, &mut out);
                }
            }
            State::Str {
                intro,
                buf,
                overflow: false,
                esc,
            } => {
                out.push(alt_char(char::from(intro)));
                for b in buf {
                    self.byte(b, &mut out);
                }
                if esc {
                    out.push(key(KeyCode::Esc, KeyModifiers::NONE));
                }
            }
            State::Str { .. } | State::Utf8 { .. } => {}
            other => self.state = other,
        }
        out
    }

    fn byte(&mut self, b: u8, out: &mut Vec<Token>) {
        match std::mem::take(&mut self.state) {
            State::Ground => self.ground(b, false, out),
            State::Utf8 {
                mut bytes,
                need,
                alt,
            } => {
                if b & 0xc0 != 0x80 {
                    // Not a continuation: the character is dropped and `b`
                    // starts afresh.
                    self.ground(b, false, out);
                    return;
                }
                bytes.push(b);
                if bytes.len() < need {
                    self.state = State::Utf8 { bytes, need, alt };
                    return;
                }
                if let Some(c) = std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(|s| s.chars().next())
                {
                    out.push(char_key(c, alt));
                }
            }
            State::Esc => match b {
                b'[' => {
                    self.state = State::Csi {
                        buf: Vec::new(),
                        overflow: false,
                    }
                }
                b']' => {
                    self.state = State::Osc {
                        body: Vec::new(),
                        truncated: false,
                        raw: 2,
                        committed: false,
                        esc: false,
                    }
                }
                b'O' => self.state = State::Ss3,
                b'P' | b'_' | b'^' | b'X' => {
                    self.state = State::Str {
                        intro: b,
                        buf: Vec::new(),
                        overflow: false,
                        esc: false,
                    }
                }
                0x1b => {
                    out.push(key(KeyCode::Esc, KeyModifiers::NONE));
                    self.state = State::Esc;
                }
                _ => self.ground(b, true, out),
            },
            State::Ss3 => {
                let code = match b {
                    b'A' => Some(KeyCode::Up),
                    b'B' => Some(KeyCode::Down),
                    b'C' => Some(KeyCode::Right),
                    b'D' => Some(KeyCode::Left),
                    b'H' => Some(KeyCode::Home),
                    b'F' => Some(KeyCode::End),
                    b'P' => Some(KeyCode::F(1)),
                    b'Q' => Some(KeyCode::F(2)),
                    b'R' => Some(KeyCode::F(3)),
                    b'S' => Some(KeyCode::F(4)),
                    _ => None,
                };
                match code {
                    Some(code) => out.push(key(code, KeyModifiers::NONE)),
                    None => {
                        out.push(alt_char('O'));
                        self.byte(b, out);
                    }
                }
            }
            State::Csi { mut buf, overflow } => match b {
                0x40..=0x7e => {
                    if !overflow {
                        self.csi(&buf, b, out);
                    }
                }
                0x20..=0x3f => {
                    let overflow = overflow || buf.len() >= CSI_CAP;
                    if !overflow {
                        buf.push(b);
                    }
                    self.state = State::Csi { buf, overflow };
                }
                // Anything else ends the sequence unfinished; the byte starts
                // afresh.
                _ => self.byte(b, out),
            },
            State::Osc {
                mut body,
                mut truncated,
                mut raw,
                mut committed,
                esc,
            } => {
                raw += 1;
                if esc {
                    if b == b'\\' {
                        out.push(Token::Osc {
                            body,
                            truncated,
                            raw,
                        });
                        return;
                    }
                    // `ESC` and not ST: the string is broken off and dropped,
                    // and the `ESC` starts what comes next.
                    self.state = State::Esc;
                    self.byte(b, out);
                    return;
                }
                match b {
                    0x07 if committed => {
                        out.push(Token::Osc {
                            body,
                            truncated,
                            raw,
                        });
                        return;
                    }
                    0x1b if !committed => {
                        // Alt+] and the digits after it, then a new `ESC`.
                        out.push(alt_char(']'));
                        for d in body {
                            self.byte(d, out);
                        }
                        self.state = State::Esc;
                        return;
                    }
                    0x1b => {
                        self.state = State::Osc {
                            body,
                            truncated,
                            raw,
                            committed,
                            esc: true,
                        };
                        return;
                    }
                    b'0'..=b'9' if !committed && body.len() < OSC_DIGITS => body.push(b),
                    b';' if !committed && !body.is_empty() => {
                        body.push(b);
                        committed = true;
                    }
                    _ if !committed => {
                        // Not an OSC: Alt+], then these bytes as typed.
                        out.push(alt_char(']'));
                        for d in body {
                            self.byte(d, out);
                        }
                        self.byte(b, out);
                        return;
                    }
                    _ if body.len() < OSC_CAP => body.push(b),
                    _ => truncated = true,
                }
                self.state = State::Osc {
                    body,
                    truncated,
                    raw,
                    committed,
                    esc: false,
                };
            }
            State::Str {
                intro,
                mut buf,
                mut overflow,
                esc,
            } => {
                if esc {
                    // `ESC \` ends the string, dropped whole. Any other byte
                    // means it was typing after all (a reply always ends in
                    // ST): Alt and the keys, then the `ESC` starts afresh. An
                    // overlong one is never typing and is dropped.
                    if b != b'\\' {
                        if !overflow {
                            out.push(alt_char(char::from(intro)));
                            for d in buf {
                                self.byte(d, out);
                            }
                        }
                        self.state = State::Esc;
                        self.byte(b, out);
                    }
                    return;
                }
                let esc = b == 0x1b;
                if !esc {
                    overflow = overflow || buf.len() >= STR_CAP;
                    if !overflow {
                        buf.push(b);
                    }
                }
                self.state = State::Str {
                    intro,
                    buf,
                    overflow,
                    esc,
                };
            }
            State::Paste { mut buf, mut over } => {
                buf.push(b);
                if buf.ends_with(PASTE_END) {
                    buf.truncate(buf.len() - PASTE_END.len());
                    out.push(Token::Input(if over {
                        Input::Note(format!("paste: over {} MiB, dropped", PASTE_CAP >> 20))
                    } else {
                        Input::Paste(String::from_utf8_lossy(&buf).into_owned())
                    }));
                    return;
                }
                // The kept bytes may end in all but the last of the marker.
                if buf.len() > PASTE_CAP + PASTE_END.len() - 1 {
                    over = true;
                    buf.drain(..buf.len() - (PASTE_END.len() - 1));
                }
                self.state = State::Paste { buf, over };
            }
        }
    }

    /// A byte with nothing pending; `alt` when an `ESC` came before it.
    fn ground(&mut self, b: u8, alt: bool, out: &mut Vec<Token>) {
        let with_alt = |mut k: KeyEvent| {
            if alt {
                k.modifiers |= KeyModifiers::ALT;
            }
            Token::Input(Input::Key(k))
        };
        match b {
            0x1b => self.state = State::Esc,
            0x00..=0x1f | 0x7f => out.push(with_alt(control(b))),
            0x20..=0x7e => out.push(char_key(char::from(b), alt)),
            0xc2..=0xf4 => {
                let need = match b {
                    0xc2..=0xdf => 2,
                    0xe0..=0xef => 3,
                    _ => 4,
                };
                self.state = State::Utf8 {
                    bytes: vec![b],
                    need,
                    alt,
                };
            }
            // A stray continuation byte or one that never starts UTF-8.
            _ => {}
        }
    }

    /// A finished CSI: `params` between `ESC [` and the final byte `fin`.
    fn csi(&mut self, params: &[u8], fin: u8, out: &mut Vec<Token>) {
        let raw = 2 + params.len() + 1;
        if params.first() == Some(&b'?') && fin == b'c' {
            out.push(Token::DeviceAttributes { raw });
            return;
        }
        if params.first() == Some(&b'<') && matches!(fin, b'M' | b'm') {
            if let Some(m) = sgr_mouse(&params[1..], fin == b'm') {
                out.push(Token::Input(Input::Mouse(m)));
            }
            return;
        }
        if params == b"200" && fin == b'~' {
            self.state = State::Paste {
                buf: Vec::new(),
                over: false,
            };
            return;
        }
        let Ok(text) = std::str::from_utf8(params) else {
            return;
        };
        let mut nums = text.split(';').map(|n| n.parse::<u16>().ok());
        let first = nums.next().flatten();
        let mods = nums.next().flatten().map_or(KeyModifiers::NONE, modifiers);
        let code = match (fin, first) {
            (b'A', _) => KeyCode::Up,
            (b'B', _) => KeyCode::Down,
            (b'C', _) => KeyCode::Right,
            (b'D', _) => KeyCode::Left,
            (b'H', _) => KeyCode::Home,
            (b'F', _) => KeyCode::End,
            (b'P', _) => KeyCode::F(1),
            (b'Q', _) => KeyCode::F(2),
            (b'R', _) => KeyCode::F(3),
            (b'S', _) => KeyCode::F(4),
            (b'Z', _) => {
                out.push(key(KeyCode::BackTab, KeyModifiers::SHIFT | mods));
                return;
            }
            (b'~', Some(n)) => match n {
                1 | 7 => KeyCode::Home,
                2 => KeyCode::Insert,
                3 => KeyCode::Delete,
                4 | 8 => KeyCode::End,
                5 => KeyCode::PageUp,
                6 => KeyCode::PageDown,
                11..=15 => KeyCode::F((n - 10) as u8),
                17..=21 => KeyCode::F((n - 11) as u8),
                23 | 24 => KeyCode::F((n - 12) as u8),
                _ => return,
            },
            // Focus reports, the end of a paste that never began, and every
            // sequence nobody asked for.
            _ => return,
        };
        out.push(key(code, mods));
    }
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> Token {
    Token::Input(Input::Key(KeyEvent::new(code, modifiers)))
}

fn alt_char(c: char) -> Token {
    char_key(c, true)
}

/// `c` typed, with Shift when it is upper case (as crossterm reports it).
fn char_key(c: char, alt: bool) -> Token {
    let mut m = if c.is_uppercase() {
        KeyModifiers::SHIFT
    } else {
        KeyModifiers::NONE
    };
    if alt {
        m |= KeyModifiers::ALT;
    }
    key(KeyCode::Char(c), m)
}

/// A C0 control byte or DEL as the key that types it.
fn control(b: u8) -> KeyEvent {
    match b {
        b'\r' | b'\n' => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        b'\t' => KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        0x7f | 0x08 => KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        0x00 => KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL),
        0x01..=0x1a => KeyEvent::new(
            KeyCode::Char(char::from(b'a' + b - 1)),
            KeyModifiers::CONTROL,
        ),
        _ => KeyEvent::new(
            KeyCode::Char(char::from(b'4' + (b - 0x1c))),
            KeyModifiers::CONTROL,
        ),
    }
}

/// The xterm modifier parameter: one more than a bit set.
fn modifiers(param: u16) -> KeyModifiers {
    let bits = param.saturating_sub(1);
    let mut m = KeyModifiers::NONE;
    for (bit, flag) in [
        (1, KeyModifiers::SHIFT),
        (2, KeyModifiers::ALT),
        (4, KeyModifiers::CONTROL),
        (8, KeyModifiers::SUPER),
        (16, KeyModifiers::HYPER),
        (32, KeyModifiers::META),
    ] {
        if bits & bit != 0 {
            m |= flag;
        }
    }
    m
}

/// `b;x;y` of an SGR mouse report, `release` for the `m` form.
fn sgr_mouse(params: &[u8], release: bool) -> Option<MouseEvent> {
    let text = std::str::from_utf8(params).ok()?;
    let mut nums = text.split(';').map(|n| n.parse::<u16>().ok());
    let (cb, x, y) = (nums.next()??, nums.next()??, nums.next()??);
    if nums.next().is_some() {
        return None;
    }
    let button = (cb & 0b11) | ((cb & 0b1100_0000) >> 4);
    let dragging = cb & 0b10_0000 != 0;
    let pick = |n| match n {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        _ => MouseButton::Right,
    };
    let kind = match (button, dragging) {
        (0..=2, false) if release => MouseEventKind::Up(pick(button)),
        (0..=2, false) => MouseEventKind::Down(pick(button)),
        (0..=2, true) => MouseEventKind::Drag(pick(button)),
        (3, false) => MouseEventKind::Up(MouseButton::Left),
        (3..=5, true) => MouseEventKind::Moved,
        (4, false) => MouseEventKind::ScrollUp,
        (5, false) => MouseEventKind::ScrollDown,
        (6, false) => MouseEventKind::ScrollLeft,
        (7, false) => MouseEventKind::ScrollRight,
        _ => return None,
    };
    let mut modifiers = KeyModifiers::NONE;
    for (bit, flag) in [
        (4, KeyModifiers::SHIFT),
        (8, KeyModifiers::ALT),
        (16, KeyModifiers::CONTROL),
    ] {
        if cb & bit != 0 {
            modifiers |= flag;
        }
    }
    Some(MouseEvent {
        kind,
        column: x.saturating_sub(1),
        row: y.saturating_sub(1),
        modifiers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    fn key_ev(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
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
        assert_eq!(printable(&key_ev(KeyCode::Char('a'), none)), Some('a'));
        assert_eq!(printable(&key_ev(KeyCode::Char('"'), shift)), Some('"'));
        assert_eq!(printable(&key_ev(KeyCode::Char(' '), none)), Some(' '));
        assert_eq!(printable(&key_ev(KeyCode::Char('\\'), altgr)), Some('\\'));
        assert_eq!(printable(&key_ev(KeyCode::Char('é'), none)), Some('é'));
        assert_eq!(
            printable(&key_ev(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(
            printable(&key_ev(KeyCode::Char('x'), KeyModifiers::ALT)),
            None
        );
        assert_eq!(printable(&key_ev(KeyCode::Char('\u{7}'), none)), None);
        assert_eq!(printable(&key_ev(KeyCode::Enter, none)), None);
        assert_eq!(printable(&key_ev(KeyCode::Up, none)), None);
    }

    fn k(code: KeyCode, m: KeyModifiers) -> Token {
        Token::Input(Input::Key(key_ev(code, m)))
    }

    fn c(ch: char) -> Token {
        k(KeyCode::Char(ch), KeyModifiers::NONE)
    }

    /// Every token of `bytes`, fed whole, then one byte at a time: the split
    /// must not matter.
    fn decode(bytes: &[u8]) -> Vec<Token> {
        let whole = Decoder::default().feed(bytes);
        let mut d = Decoder::default();
        let split: Vec<Token> = bytes.iter().flat_map(|&b| d.feed(&[b])).collect();
        assert_eq!(whole, split, "{bytes:?}");
        whole
    }

    #[test]
    fn legacy_text_and_control_keys() {
        use KeyModifiers as M;
        assert_eq!(
            decode("aQ é€😺".as_bytes()),
            [
                c('a'),
                k(KeyCode::Char('Q'), M::SHIFT),
                c(' '),
                c('é'),
                c('€'),
                c('😺'),
            ]
        );
        assert_eq!(
            decode(b"\r\n\t\x7f\x08\x00\x01\x03\x1a\x1c\x1f"),
            [
                k(KeyCode::Enter, M::NONE),
                k(KeyCode::Enter, M::NONE),
                k(KeyCode::Tab, M::NONE),
                k(KeyCode::Backspace, M::NONE),
                k(KeyCode::Backspace, M::NONE),
                k(KeyCode::Char(' '), M::CONTROL),
                k(KeyCode::Char('a'), M::CONTROL),
                k(KeyCode::Char('c'), M::CONTROL),
                k(KeyCode::Char('z'), M::CONTROL),
                k(KeyCode::Char('4'), M::CONTROL),
                k(KeyCode::Char('7'), M::CONTROL),
            ]
        );
        // Bytes that are never UTF-8, and a character cut by another byte,
        // are dropped.
        assert_eq!(decode(b"\xff\x80a\xc3b"), [c('a'), c('b')]);
    }

    #[test]
    fn escape_prefixes_alt_and_a_lone_escape_waits_for_idle() {
        use KeyModifiers as M;
        assert_eq!(
            decode(b"\x1bx\x1bX\x1b\r\x1b\x7f"),
            [
                k(KeyCode::Char('x'), M::ALT),
                k(KeyCode::Char('X'), M::ALT | M::SHIFT),
                k(KeyCode::Enter, M::ALT),
                k(KeyCode::Backspace, M::ALT),
            ]
        );
        assert_eq!(decode("\x1bé".as_bytes()), [k(KeyCode::Char('é'), M::ALT)]);
        let mut d = Decoder::default();
        assert_eq!(d.feed(b"\x1b"), []);
        assert_eq!(d.idle(), [k(KeyCode::Esc, M::NONE)]);
        assert_eq!(d.feed(b"\x1b\x1b"), [k(KeyCode::Esc, M::NONE)]);
        assert_eq!(d.idle(), [k(KeyCode::Esc, M::NONE)]);
        // A sequence split across reads is not two keys.
        assert_eq!(d.feed(b"\x1b"), []);
        assert_eq!(d.feed(b"[A"), [k(KeyCode::Up, M::NONE)]);
        // `ESC [` and `ESC O` with nothing after them are Alt keys.
        d.feed(b"\x1b[");
        assert_eq!(d.idle(), [k(KeyCode::Char('['), M::ALT)]);
        d.feed(b"\x1bO");
        assert_eq!(d.idle(), [k(KeyCode::Char('O'), M::ALT | M::SHIFT)]);
        assert_eq!(d.idle(), []);
    }

    #[test]
    fn cursor_function_and_editing_keys() {
        use KeyModifiers as M;
        assert_eq!(
            decode(b"\x1b[A\x1b[B\x1b[C\x1b[D\x1b[H\x1b[F\x1bOA\x1bOH\x1bOP\x1bOS\x1b[Z"),
            [
                k(KeyCode::Up, M::NONE),
                k(KeyCode::Down, M::NONE),
                k(KeyCode::Right, M::NONE),
                k(KeyCode::Left, M::NONE),
                k(KeyCode::Home, M::NONE),
                k(KeyCode::End, M::NONE),
                k(KeyCode::Up, M::NONE),
                k(KeyCode::Home, M::NONE),
                k(KeyCode::F(1), M::NONE),
                k(KeyCode::F(4), M::NONE),
                k(KeyCode::BackTab, M::SHIFT),
            ]
        );
        assert_eq!(
            decode(b"\x1b[1;5A\x1b[1;2D\x1b[1;3P\x1b[3~\x1b[5;5~\x1b[2~\x1b[6~\x1b[1~\x1b[4~"),
            [
                k(KeyCode::Up, M::CONTROL),
                k(KeyCode::Left, M::SHIFT),
                k(KeyCode::F(1), M::ALT),
                k(KeyCode::Delete, M::NONE),
                k(KeyCode::PageUp, M::CONTROL),
                k(KeyCode::Insert, M::NONE),
                k(KeyCode::PageDown, M::NONE),
                k(KeyCode::Home, M::NONE),
                k(KeyCode::End, M::NONE),
            ]
        );
        assert_eq!(
            decode(b"\x1b[15~\x1b[17~\x1b[21~\x1b[23~\x1b[24;8~"),
            [
                k(KeyCode::F(5), M::NONE),
                k(KeyCode::F(6), M::NONE),
                k(KeyCode::F(10), M::NONE),
                k(KeyCode::F(11), M::NONE),
                k(KeyCode::F(12), M::SHIFT | M::ALT | M::CONTROL),
            ]
        );
        // Focus reports, a lone paste end, unknown and overlong sequences:
        // nothing, and the next key still decodes.
        let long = [b"\x1b[".as_slice(), &[b'1'; 100], b"~x"].concat();
        assert_eq!(decode(&long), [c('x')]);
        assert_eq!(decode(b"\x1b[I\x1b[O\x1b[201~\x1b[99~\x1b[5n\x1bOzq"), {
            let mut v = vec![k(KeyCode::Char('O'), M::ALT | M::SHIFT)];
            v.extend([c('z'), c('q')]);
            v
        });
    }

    #[test]
    fn sgr_mouse_reports() {
        use MouseEventKind as K;
        let m = |kind, column, row, modifiers| {
            Token::Input(Input::Mouse(MouseEvent {
                kind,
                column,
                row,
                modifiers,
            }))
        };
        assert_eq!(
            decode(b"\x1b[<0;10;5M\x1b[<0;10;5m\x1b[<2;1;1M\x1b[<32;3;4M\x1b[<35;3;4M\x1b[<64;1;1M\x1b[<65;1;1M\x1b[<20;7;8M"),
            [
                m(K::Down(MouseButton::Left), 9, 4, KeyModifiers::NONE),
                m(K::Up(MouseButton::Left), 9, 4, KeyModifiers::NONE),
                m(K::Down(MouseButton::Right), 0, 0, KeyModifiers::NONE),
                m(K::Drag(MouseButton::Left), 2, 3, KeyModifiers::NONE),
                m(K::Moved, 2, 3, KeyModifiers::NONE),
                m(K::ScrollUp, 0, 0, KeyModifiers::NONE),
                m(K::ScrollDown, 0, 0, KeyModifiers::NONE),
                m(
                    K::Down(MouseButton::Left),
                    6,
                    7,
                    KeyModifiers::SHIFT | KeyModifiers::CONTROL
                ),
            ]
        );
        // A malformed report is nothing.
        assert_eq!(
            decode(b"\x1b[<0;1M\x1b[<0;1;1;1M\x1b[<99999;1;1M\x1b[<128;1;1M"),
            []
        );
    }

    #[test]
    fn a_bracketed_paste_is_one_event_whatever_it_holds() {
        let paste = b"\x1b[200~/tmp/a b.pdf\n\x1b]72;t=m;x\x07\x1b[A\x1b[201~z";
        assert_eq!(
            decode(paste),
            [
                Token::Input(Input::Paste("/tmp/a b.pdf\n\x1b]72;t=m;x\x07\x1b[A".into())),
                c('z'),
            ]
        );
        // An unfinished paste waits, through idle.
        let mut d = Decoder::default();
        assert_eq!(d.feed(b"\x1b[200~hi\x1b[20"), []);
        assert_eq!(d.idle(), []);
        assert_eq!(d.feed(b"1~"), [Token::Input(Input::Paste("hi".into()))]);
    }

    #[test]
    fn osc_strings_and_device_attributes_are_tokens_not_keys() {
        let osc = |body: &[u8], raw| Token::Osc {
            body: body.to_vec(),
            truncated: false,
            raw,
        };
        assert_eq!(
            decode(b"\x1b]72;t=q\x1b\\\x1b]72;t=q;\x07\x1b[?62;22c\x1b]10;rgb:0/0/0\x07"),
            [
                osc(b"72;t=q", 10),
                osc(b"72;t=q;", 10),
                Token::DeviceAttributes { raw: 9 },
                osc(b"10;rgb:0/0/0", 15),
            ]
        );
        // Longer than the cap: truncated, and every byte is still consumed.
        let long = [
            b"\x1b]72;".as_slice(),
            &vec![b'A'; OSC_CAP + 10],
            b"\x1b\\a",
        ]
        .concat();
        let t = decode(&long);
        let Token::Osc {
            body,
            truncated,
            raw,
        } = &t[0]
        else {
            panic!("{t:?}");
        };
        assert!(*truncated);
        assert_eq!(body.len(), OSC_CAP);
        assert_eq!(*raw, long.len() - 1);
        assert_eq!(t[1..], [c('a')]);
    }

    #[test]
    fn alt_bracket_is_a_key_until_an_osc_number_ends() {
        use KeyModifiers as M;
        let alt = k(KeyCode::Char(']'), M::ALT);
        assert_eq!(decode(b"\x1b]x"), [alt.clone(), c('x')]);
        assert_eq!(decode(b"\x1b]7a"), [alt.clone(), c('7'), c('a')]);
        assert_eq!(decode(b"\x1b];"), [alt.clone(), c(';')]);
        let mut d = Decoder::default();
        assert_eq!(d.feed(b"\x1b]72"), []);
        assert_eq!(d.idle(), [alt.clone(), c('7'), c('2')]);
        // Once committed, idle leaves it be; an ESC that is not ST breaks it
        // off, and nothing of it is a key.
        assert_eq!(d.feed(b"\x1b]72;t=m:x=1"), []);
        assert_eq!(d.idle(), []);
        assert_eq!(d.feed(b"\x1b[B"), [k(KeyCode::Down, M::NONE)]);
        // A number longer than an OSC's is typing.
        let many = [b"\x1b]".as_slice(), &[b'7'; OSC_DIGITS + 1], b";"].concat();
        let mut want = vec![alt.clone()];
        want.extend(std::iter::repeat_n(c('7'), OSC_DIGITS + 1));
        want.push(c(';'));
        assert_eq!(decode(&many), want);
    }

    #[test]
    fn control_strings_are_dropped_and_alt_letters_wait_for_idle() {
        use KeyModifiers as M;
        // DCS (an XTVERSION or XTGETTCAP reply), APC (a kitty graphics
        // reply), PM and SOS: nothing of them is a key, `q` included.
        assert_eq!(
            decode(b"\x1bP>|kitty(0.49)\x1b\\\x1b_Gi=1;OK\x1b\\\x1b^q\x1b\\\x1bXq\x1b\\z"),
            [c('z')]
        );
        // An overlong string is consumed whole.
        let long = [b"\x1bP".as_slice(), &[b'q'; 3 * STR_CAP], b"\x1b\\z"].concat();
        assert_eq!(decode(&long), [c('z')]);
        // Alt+P typed: a key once nothing more comes, with what followed it.
        let mut d = Decoder::default();
        assert_eq!(d.feed(b"\x1bPq"), []);
        assert_eq!(d.idle(), [k(KeyCode::Char('P'), M::ALT | M::SHIFT), c('q')]);
        // An ESC that is not ST: it was typing, and the ESC starts afresh.
        assert_eq!(
            decode(b"\x1b_x\x1b[A"),
            [
                k(KeyCode::Char('_'), M::ALT),
                c('x'),
                k(KeyCode::Up, M::NONE)
            ]
        );
        // An overlong string open at idle is never typing.
        let mut d = Decoder::default();
        assert_eq!(
            d.feed(&[b"\x1bP".as_slice(), &[b'q'; STR_CAP + 1]].concat()),
            []
        );
        assert_eq!(d.idle(), []);
    }

    #[test]
    fn a_paste_over_the_cap_is_dropped_with_a_note() {
        let mut d = Decoder::default();
        assert_eq!(d.feed(b"\x1b[200~"), []);
        let chunk = vec![b'a'; 64 * 1024];
        for _ in 0..(PASTE_CAP / chunk.len() + 1) {
            assert_eq!(d.feed(&chunk), []);
        }
        // The end marker split across reads is still found.
        assert_eq!(d.feed(b"\x1b[20"), []);
        let t = d.feed(b"1~z");
        assert!(matches!(&t[0], Token::Input(Input::Note(_))), "{t:?}");
        assert_eq!(t[1..], [c('z')]);
        // At the cap exactly, it is delivered.
        let mut d = Decoder::default();
        let paste = [
            b"\x1b[200~".as_slice(),
            &vec![b'b'; PASTE_CAP],
            b"\x1b[201~",
        ]
        .concat();
        assert_eq!(
            d.feed(&paste),
            [Token::Input(Input::Paste("b".repeat(PASTE_CAP)))]
        );
    }
}
