//! The terminal (T-23a): the guard, raw mode and the alternate screen, the
//! colour probe, the Unix handshake slot, the input flush, the restore bytes
//! the panic hook writes, and the one blit from a [`Canvas`] to the screen.
//!
//! ratatui's types appear here and nowhere else (D-045: everything above
//! draws into the framework-free canvas). D-004 is the user's open choice of
//! framework; picking another one replaces [`blit`], [`Screen`] and the
//! crossterm reader in `input/mod.rs`, nothing more.

use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Write};

use crossterm::{cursor, execute, terminal};
use ratatui::Terminal;
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

use super::canvas::Canvas;
use super::color::{ColorCaps, Rgb, Snap, xterm256};
use super::theme::Theme;

/// The guard (D-043): stdin and stdout are both terminals. std's meaning:
/// `isatty` on Unix; on Windows a console handle, or a pipe named
/// `msys-*`/`cygwin-*` holding `-pty` (mintty, Git Bash). A pipe, a file and
/// `NUL` never are. It reads no byte of either stream.
pub fn stdio_is_terminal() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

/// What the terminal can show, from `COLORTERM` and `TERM` (the two variables
/// the probe may read, plan §3.1). A Windows console with no `TERM` takes
/// 24-bit colour, as every Windows 10 console does.
pub fn probe_color_caps() -> ColorCaps {
    let colorterm = std::env::var_os("COLORTERM");
    let term = std::env::var_os("TERM");
    color_caps(
        colorterm.as_deref().and_then(|s| s.to_str()),
        term.as_deref().and_then(|s| s.to_str()),
        cfg!(windows),
    )
}

fn color_caps(colorterm: Option<&str>, term: Option<&str>, windows: bool) -> ColorCaps {
    let colorterm = colorterm.unwrap_or("").to_ascii_lowercase();
    let term = term.map(str::to_ascii_lowercase);
    if colorterm == "truecolor" || colorterm == "24bit" {
        return ColorCaps::TrueColor;
    }
    match term.as_deref() {
        Some(t) if t.ends_with("-direct") => ColorCaps::TrueColor,
        Some(t) if t.contains("256color") => ColorCaps::Ansi256,
        None if windows => ColorCaps::TrueColor,
        _ => ColorCaps::Ansi16,
    }
}

/// kitty's drag-and-drop opt-out, sent on every exit path once `t=a` was sent
/// (T-31).
const KITTY_OPT_OUT: &[u8] = b"\x1b]72;t=A\x1b\\";

/// The bytes that give the terminal back: attributes reset, kitty's opt-out
/// when it was armed, bracketed paste off, the cursor shown, the alternate
/// screen left. Raw mode is not a byte sequence: [`PanicWriter`] and
/// [`TermGuard`] turn it off before writing these.
pub fn restore_sequence(kitty_armed: bool) -> Vec<u8> {
    let mut b = b"\x1b[0m".to_vec();
    if kitty_armed {
        b.extend_from_slice(KITTY_OPT_OUT);
    }
    if cfg!(unix) {
        b.extend_from_slice(b"\x1b[?2004l");
    }
    b.extend_from_slice(b"\x1b[?25h\x1b[?1049l");
    b
}

/// Raw mode, the alternate screen, a hidden cursor and (on Unix) bracketed
/// paste, for as long as it lives; dropping it gives the terminal back.
pub struct TermGuard {
    restored: bool,
}

impl TermGuard {
    /// Step 2 of start-up. On an error the terminal is given back before it
    /// returns.
    pub fn enter() -> io::Result<TermGuard> {
        terminal::enable_raw_mode()?;
        let mut guard = TermGuard { restored: false };
        let mut out = io::stdout();
        let entered = execute!(out, terminal::EnterAlternateScreen, cursor::Hide).and_then(|()| {
            // crossterm parses a paste with or without this; turning it on
            // makes the terminal mark one. Windows never reports pastes.
            #[cfg(unix)]
            execute!(out, crossterm::event::EnableBracketedPaste)?;
            Ok(())
        });
        match entered {
            Ok(()) => Ok(guard),
            Err(e) => {
                guard.restore();
                Err(e)
            }
        }
    }

    /// Gives the terminal back; later calls do nothing.
    pub fn restore(&mut self) {
        if std::mem::replace(&mut self.restored, true) {
            return;
        }
        let mut out = io::stdout();
        // TODO(T-31): send `KITTY_OPT_OUT` here when the handshake armed it.
        #[cfg(unix)]
        let _ = execute!(out, crossterm::event::DisableBracketedPaste);
        let _ = execute!(out, cursor::Show, terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

impl Drop for TermGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

/// The writer the panic hook gets: it turns raw mode off, then writes the
/// restore bytes to stdout, so the message the previous hook prints lands on a
/// usable terminal.
pub struct PanicWriter;

impl Write for PanicWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let _ = terminal::disable_raw_mode();
        io::stdout().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stdout().flush()
    }
}

/// What the Unix kitty handshake found (T-31 runs it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HandshakeResult {
    /// kitty answered `t=q` before DA1: the raw splitter owns stdin.
    pub kitty: bool,
    /// Bytes read in the window that were neither reply: discarded, never
    /// delivered, and counted for the debug log.
    pub discarded: usize,
}

/// Step 3 of start-up, Unix only: the OSC 72 handshake (T-31). Until T-31
/// lands it reads nothing and finds no kitty. Windows never runs one.
pub fn handshake() -> HandshakeResult {
    // TODO(T-31): write `t=q` and DA1, read for at most 300 ms, discard and
    // count every byte that is neither reply.
    HandshakeResult::default()
}

/// Step 5 of start-up: discards everything the terminal has queued for us,
/// typed, pasted or dropped before the first frame. Call it after raw mode is
/// on and immediately before arming the input source: crossterm reads nothing
/// until its first poll, so bytes queued before then would otherwise arrive
/// as keys and pastes (lead-r4-fr2).
#[cfg(unix)]
pub fn flush_input() -> io::Result<()> {
    use rustix::termios::{QueueSelector, tcflush};
    // crossterm reads stdin when it is a terminal, else /dev/tty.
    let stdin = io::stdin();
    if stdin.is_terminal() {
        tcflush(&stdin, QueueSelector::IFlush)?;
    } else {
        let tty = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")?;
        tcflush(&tty, QueueSelector::IFlush)?;
    }
    Ok(())
}

/// Step 5 of start-up on Windows: `FlushConsoleInputBuffer` on `CONIN$`
/// opened read and write, the handle crossterm reads (the call needs
/// `GENERIC_WRITE`, which the standard input handle may lack). Compile-checked
/// only until the first windows-latest run (D-058).
#[cfg(windows)]
pub fn flush_input() -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Console::FlushConsoleInputBuffer;
    let conin = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("CONIN$")?;
    // SAFETY: the handle is open for the whole call and owned by `conin`.
    let ok = unsafe { FlushConsoleInputBuffer(conin.as_raw_handle()) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Turns canvas colours into terminal colours: 24-bit as they are; otherwise
/// the nearest xterm colour 16-255, or the nearest of the theme's 16 ANSI
/// colours, as an index.
pub struct Palette {
    caps: ColorCaps,
    entries: Vec<Rgb>,
    snap: Snap,
    /// The first index `entries` stands for.
    base: u8,
    seen: BTreeMap<Rgb, u8>,
}

impl Palette {
    pub fn new(caps: ColorCaps, theme: &Theme) -> Palette {
        let (entries, base) = match caps {
            ColorCaps::TrueColor => (Vec::new(), 0),
            ColorCaps::Ansi256 => (xterm256().to_vec(), 16),
            ColorCaps::Ansi16 => (theme.ansi.to_vec(), 0),
        };
        Palette {
            caps,
            snap: Snap::new(&entries),
            entries,
            base,
            seen: BTreeMap::new(),
        }
    }

    fn color(&mut self, c: Rgb) -> Color {
        if self.caps == ColorCaps::TrueColor {
            let [r, g, b] = c.0;
            return Color::Rgb(r, g, b);
        }
        let i = match self.seen.get(&c) {
            Some(&i) => i,
            None => {
                let near = self.snap.nearest(c);
                let at = self.entries.iter().position(|&e| e == near).unwrap_or(0);
                let i = self.base + u8::try_from(at).unwrap_or(0);
                self.seen.insert(c, i);
                i
            }
        };
        Color::Indexed(i)
    }
}

/// The blit: every canvas cell into `buf` (`set_symbol`, the colours through
/// `palette`, a blinking cell as `SLOW_BLINK`). Cells past either edge of
/// `buf` are dropped.
pub fn blit(canvas: &Canvas, buf: &mut Buffer, palette: &mut Palette) {
    let mut sym = [0u8; 4];
    for y in 0..canvas.h {
        for x in 0..canvas.w {
            let (Some(cell), Some(out)) = (canvas.get(x, y), buf.cell_mut((x, y))) else {
                continue;
            };
            out.set_symbol(cell.ch.encode_utf8(&mut sym));
            out.set_fg(palette.color(cell.fg));
            out.set_bg(palette.color(cell.bg));
            out.modifier = if canvas.blink.contains(&(x, y)) {
                Modifier::SLOW_BLINK
            } else {
                Modifier::empty()
            };
        }
    }
}

/// Where the loop draws: a size, a canvas to show in a theme's colours, and
/// raw bytes to send (the widget's resize request).
pub trait Surface {
    fn size(&mut self) -> io::Result<(u16, u16)>;
    fn present(&mut self, canvas: &Canvas, theme: &Theme) -> io::Result<()>;
    fn write_raw(&mut self, bytes: &[u8]) -> io::Result<()>;
}

/// A ratatui terminal and the stream raw bytes go to.
pub struct Screen<B: Backend, W: Write> {
    terminal: Terminal<B>,
    raw: W,
    caps: ColorCaps,
    /// The palette and the theme it was made for.
    palette: Option<(&'static str, Palette)>,
}

/// The real terminal.
pub type Tty = Screen<CrosstermBackend<io::Stdout>, io::Stdout>;

impl Tty {
    pub fn tty(caps: ColorCaps) -> io::Result<Tty> {
        Ok(Screen {
            terminal: Terminal::new(CrosstermBackend::new(io::stdout()))?,
            raw: io::stdout(),
            caps,
            palette: None,
        })
    }
}

impl<B: Backend, W: Write> Surface for Screen<B, W> {
    fn size(&mut self) -> io::Result<(u16, u16)> {
        let s = self.terminal.size().map_err(other)?;
        Ok((s.width, s.height))
    }

    fn present(&mut self, canvas: &Canvas, theme: &Theme) -> io::Result<()> {
        if self
            .palette
            .as_ref()
            .is_none_or(|(name, _)| *name != theme.name)
        {
            self.palette = Some((theme.name, Palette::new(self.caps, theme)));
        }
        let Some((_, palette)) = self.palette.as_mut() else {
            return Ok(());
        };
        self.terminal
            .draw(|f| blit(canvas, f.buffer_mut(), palette))
            .map_err(other)?;
        Ok(())
    }

    fn write_raw(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.raw.write_all(bytes)?;
        self.raw.flush()
    }
}

fn other(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

/// An in-memory screen for the loop's tests.
#[cfg(test)]
pub type TestScreen = Screen<ratatui::backend::TestBackend, Vec<u8>>;

#[cfg(test)]
impl TestScreen {
    pub fn test(w: u16, h: u16, caps: ColorCaps) -> TestScreen {
        Screen {
            terminal: Terminal::new(ratatui::backend::TestBackend::new(w, h))
                .unwrap_or_else(|e| match e {}),
            raw: Vec::new(),
            caps,
            palette: None,
        }
    }

    /// Resizes the fake terminal, as a window drag would.
    pub fn resize(&mut self, w: u16, h: u16) {
        self.terminal.backend_mut().resize(w, h);
    }

    /// Each row's symbols.
    pub fn rows(&self) -> Vec<String> {
        let buf = self.terminal.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect()
            })
            .collect()
    }

    /// The raw bytes written so far.
    pub fn raw(&self) -> &[u8] {
        &self.raw
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::canvas::HALF;

    fn theme() -> &'static Theme {
        Theme::default_theme()
    }

    #[test]
    fn the_probe_reads_colorterm_then_term() {
        use ColorCaps::*;
        for (colorterm, term, windows, want) in [
            (Some("truecolor"), Some("xterm-256color"), false, TrueColor),
            (Some("24bit"), None, false, TrueColor),
            (Some("TrueColor"), Some("xterm"), false, TrueColor),
            (None, Some("xterm-direct"), false, TrueColor),
            (None, Some("xterm-256color"), false, Ansi256),
            (Some(""), Some("screen-256color"), false, Ansi256),
            (None, Some("xterm"), false, Ansi16),
            (None, Some("dumb"), false, Ansi16),
            (None, None, false, Ansi16),
            (None, None, true, TrueColor),
            (None, Some("xterm"), true, Ansi16),
        ] {
            assert_eq!(
                color_caps(colorterm, term, windows),
                want,
                "{colorterm:?} {term:?} {windows}"
            );
        }
    }

    #[test]
    fn the_restore_sequence_gives_everything_back() {
        let plain = restore_sequence(false);
        let kitty = restore_sequence(true);
        let paste_off: &[u8] = if cfg!(unix) { b"\x1b[?2004l" } else { b"" };
        let want = [b"\x1b[0m".as_slice(), paste_off, b"\x1b[?25h\x1b[?1049l"].concat();
        assert_eq!(plain, want);
        assert_eq!(
            kitty,
            [b"\x1b[0m".as_slice(), KITTY_OPT_OUT, &want[4..]].concat()
        );
    }

    #[test]
    fn the_blit_copies_symbols_colours_and_blink() {
        let t = theme();
        let mut c = Canvas::new(4, 2, t);
        let red = Rgb::from_u32(0xe0445c);
        let blue = Rgb::from_u32(0x2040ff);
        c.put(0, 0, 'A', Some(red), Some(blue));
        c.pix(1, 0, &[[Some(red)], [Some(blue)]]);
        c.put(3, 1, '‼', Some(red), None);
        c.set_blink(3, 1);
        let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, 3, 2));
        blit(&c, &mut buf, &mut Palette::new(ColorCaps::TrueColor, t));

        let a = &buf[(0, 0)];
        assert_eq!(a.symbol(), "A");
        assert_eq!(
            (a.fg, a.bg),
            (Color::Rgb(0xe0, 0x44, 0x5c), Color::Rgb(0x20, 0x40, 0xff))
        );
        assert_eq!(a.modifier, Modifier::empty());
        let p = &buf[(1, 0)];
        assert_eq!(p.symbol(), HALF.to_string());
        assert_eq!(
            (p.fg, p.bg),
            (Color::Rgb(0xe0, 0x44, 0x5c), Color::Rgb(0x20, 0x40, 0xff))
        );
        let [r, g, b] = t.roles.bg.0;
        assert_eq!(buf[(2, 1)].bg, Color::Rgb(r, g, b));
        // (3, 1) is past the buffer's edge: dropped, no panic.
        let mut wide = Buffer::empty(ratatui::layout::Rect::new(0, 0, 4, 2));
        blit(&c, &mut wide, &mut Palette::new(ColorCaps::TrueColor, t));
        assert_eq!(wide[(3, 1)].symbol(), "‼");
        assert_eq!(wide[(3, 1)].modifier, Modifier::SLOW_BLINK);
        assert_eq!(wide[(2, 1)].modifier, Modifier::empty());
    }

    #[test]
    fn fewer_colours_blit_as_indices() {
        let t = theme();
        let mut c = Canvas::new(2, 1, t);
        c.put(
            0,
            0,
            'x',
            Some(Rgb([0xff, 0, 0])),
            Some(Rgb([0x80, 0x80, 0x80])),
        );
        c.put(1, 0, 'y', Some(t.ansi[12]), Some(t.ansi[0]));
        let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, 2, 1));
        blit(&c, &mut buf, &mut Palette::new(ColorCaps::Ansi256, t));
        // Pure red is cube colour (5, 0, 0) = 16 + 180; grey 0x80 is the ramp's 244.
        assert_eq!(buf[(0, 0)].fg, Color::Indexed(196));
        assert_eq!(buf[(0, 0)].bg, Color::Indexed(244));

        blit(&c, &mut buf, &mut Palette::new(ColorCaps::Ansi16, t));
        let Color::Indexed(i) = buf[(1, 0)].fg else {
            panic!("an index");
        };
        assert_eq!(t.ansi[usize::from(i)], t.ansi[12]);
        let Color::Indexed(i) = buf[(1, 0)].bg else {
            panic!("an index");
        };
        assert_eq!(t.ansi[usize::from(i)], t.ansi[0]);
    }

    #[test]
    fn a_test_screen_shows_what_was_presented() {
        let t = theme();
        let mut s = TestScreen::test(3, 1, ColorCaps::TrueColor);
        let mut c = Canvas::new(3, 1, t);
        c.text(0, 0, "cat", t.roles.body, None);
        s.present(&c, t).unwrap();
        s.write_raw(b"\x1b[8;38;112t").unwrap();
        assert_eq!(s.rows(), ["cat"]);
        assert_eq!(s.raw(), b"\x1b[8;38;112t");
        assert_eq!(s.size().unwrap(), (3, 1));
        s.resize(5, 2);
        assert_eq!(s.size().unwrap(), (5, 2));
    }
}
