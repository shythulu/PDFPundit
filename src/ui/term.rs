//! The terminal (T-23a): the guard, raw mode and the alternate screen, the
//! colour probe, the Unix kitty handshake and raw stdin (T-31), the input
//! flush, the restore bytes the panic hook writes, and the one blit from a
//! [`Canvas`] to the screen.
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

use super::canvas::{Canvas, printable};
use super::color::{ColorCaps, Rgb, Snap, xterm256};
use super::input::osc72;
use super::theme::Theme;
use crate::panic_guard;

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
const KITTY_OPT_OUT: &[u8] = osc72::OPT_OUT;

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
    /// kitty drops were asked for: the opt-out goes with the rest.
    kitty: bool,
}

impl TermGuard {
    /// Step 2 of start-up. On an error the terminal is given back before it
    /// returns.
    pub fn enter() -> io::Result<TermGuard> {
        terminal::enable_raw_mode()?;
        let mut guard = TermGuard {
            restored: false,
            kitty: false,
        };
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

    /// The handshake opted in to kitty drops: giving the terminal back opts
    /// out.
    pub fn kitty_armed(&mut self) {
        self.kitty = true;
    }

    /// Gives the terminal back; later calls do nothing, and neither does
    /// this one when the panic hook already gave it back (the drop as a
    /// UI-thread panic unwinds).
    pub fn restore(&mut self) {
        if self.give_back(&mut io::stdout()) {
            let _ = terminal::disable_raw_mode();
        }
    }

    /// Writes the restore bytes to `out`, once between this guard and the
    /// panic hook ([`panic_guard::claim_restore`]). Whether it wrote them.
    fn give_back(&mut self, out: &mut impl Write) -> bool {
        if std::mem::replace(&mut self.restored, true) || !panic_guard::claim_restore() {
            return false;
        }
        if self.kitty {
            let _ = out.write_all(KITTY_OPT_OUT).and_then(|()| out.flush());
        }
        #[cfg(unix)]
        let _ = execute!(out, crossterm::event::DisableBracketedPaste);
        let _ = execute!(out, cursor::Show, terminal::LeaveAlternateScreen);
        true
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

/// How long the handshake reads (D-033).
#[cfg(unix)]
const HANDSHAKE_WINDOW: std::time::Duration = std::time::Duration::from_millis(300);

/// Step 3 of start-up, Unix only: the OSC 72 handshake (T-31). Writes `t=q`
/// then DA1 and reads stdin raw for at most 300 ms, or until the DA1 reply.
/// A `t=q` reply before it is kitty: `t=a` is sent and the raw splitter will
/// own stdin. Every other byte read is discarded and counted, on either
/// branch (D-033 amended). The caller must have raw mode on.
#[cfg(unix)]
pub fn handshake() -> HandshakeResult {
    let mut out = io::stdout();
    let asked = out
        .write_all(&[osc72::QUERY, osc72::DA1].concat())
        .and_then(|()| out.flush());
    let mut h = osc72::Handshake::new();
    let Ok(mut stdin) = asked.and_then(|()| RawStdin::open(false)) else {
        return h.result();
    };
    let start = std::time::Instant::now();
    while !h.done() {
        let left = HANDSHAKE_WINDOW.saturating_sub(start.elapsed());
        if left.is_zero() {
            break;
        }
        match stdin.wait(left) {
            Ok(Wake::Bytes(b)) => h.feed(b),
            Ok(Wake::Idle | Wake::Resize) => {}
            Ok(Wake::Closed) | Err(_) => break,
        }
    }
    let result = h.result();
    if result.kitty {
        let _ = out.write_all(osc72::OPT_IN).and_then(|()| out.flush());
    }
    result
}

/// Windows never runs the handshake: kitty does not run there (D-033).
#[cfg(not(unix))]
pub fn handshake() -> HandshakeResult {
    HandshakeResult::default()
}

/// What [`RawStdin::wait`] saw.
#[cfg(unix)]
pub enum Wake<'a> {
    /// Bytes read from stdin.
    Bytes(&'a [u8]),
    /// Nothing within the timeout.
    Idle,
    /// The window changed size (SIGWINCH).
    Resize,
    /// stdin hung up.
    Closed,
}

/// stdin read raw (T-31): the handshake's window and, on kitty, the session
/// reader, which also wants SIGWINCH. Nothing here parses.
#[cfg(unix)]
pub struct RawStdin {
    /// The read end of the pipe signal-hook writes a byte to on SIGWINCH.
    winch: Option<(std::os::unix::net::UnixStream, signal_hook::SigId)>,
    buf: Vec<u8>,
}

#[cfg(unix)]
impl RawStdin {
    pub fn open(watch_resize: bool) -> io::Result<RawStdin> {
        let winch = if watch_resize {
            let (read, write) = std::os::unix::net::UnixStream::pair()?;
            read.set_nonblocking(true)?;
            let id = signal_hook::low_level::pipe::register(signal_hook::consts::SIGWINCH, write)?;
            Some((read, id))
        } else {
            None
        };
        Ok(RawStdin {
            winch,
            buf: vec![0; 64 * 1024],
        })
    }

    /// Waits up to `timeout` for input or a resize. Bytes come first; a
    /// resize waits for the next call.
    pub fn wait(&mut self, timeout: std::time::Duration) -> io::Result<Wake<'_>> {
        use rustix::event::{PollFd, PollFlags, Timespec, poll};
        use rustix::io::Errno;
        let stdin = io::stdin();
        let ts = Timespec {
            tv_sec: i64::try_from(timeout.as_secs()).unwrap_or(i64::MAX),
            tv_nsec: timeout.subsec_nanos() as _,
        };
        let (input, resized) = loop {
            let mut fds = vec![PollFd::new(&stdin, PollFlags::IN)];
            if let Some((pipe, _)) = &self.winch {
                fds.push(PollFd::new(pipe, PollFlags::IN));
            }
            match poll(&mut fds, Some(&ts)) {
                Ok(_) => {}
                // A signal (SIGWINCH among them): poll again; the pipe says
                // whether it was a resize.
                Err(Errno::INTR) => continue,
                Err(e) => return Err(e.into()),
            }
            break (
                fds[0].revents(),
                fds.get(1)
                    .is_some_and(|f| f.revents().contains(PollFlags::IN)),
            );
        };
        if input.contains(PollFlags::IN) {
            return match rustix::io::read(&stdin, &mut self.buf[..]) {
                Ok(0) => Ok(Wake::Closed),
                Ok(n) => Ok(Wake::Bytes(&self.buf[..n])),
                Err(Errno::INTR | Errno::AGAIN) => Ok(Wake::Bytes(&self.buf[..0])),
                Err(e) => Err(e.into()),
            };
        }
        if input.intersects(PollFlags::HUP | PollFlags::ERR | PollFlags::NVAL) {
            return Ok(Wake::Closed);
        }
        if resized {
            if let Some((pipe, _)) = &mut self.winch {
                let mut sink = [0u8; 64];
                while matches!(io::Read::read(pipe, &mut sink), Ok(n) if n > 0) {}
            }
            return Ok(Wake::Resize);
        }
        Ok(Wake::Idle)
    }
}

#[cfg(unix)]
impl Drop for RawStdin {
    fn drop(&mut self) {
        if let Some((_, id)) = self.winch.take() {
            signal_hook::low_level::unregister(id);
        }
    }
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
/// `buf` are dropped. A control or bidi control character in a cell is shown
/// as `�` ([`printable`]): [`Canvas::text`] already replaces them, and this is
/// the second guard for a cell written some other way.
pub fn blit(canvas: &Canvas, buf: &mut Buffer, palette: &mut Palette) {
    let mut sym = [0u8; 4];
    for y in 0..canvas.h {
        for x in 0..canvas.w {
            let (Some(cell), Some(out)) = (canvas.get(x, y), buf.cell_mut((x, y))) else {
                continue;
            };
            out.set_symbol(printable(cell.ch).encode_utf8(&mut sym));
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

    /// The T-23a review's major: after the panic hook wrote the restore
    /// bytes, the guard's drop as the panic unwinds writes nothing more; and
    /// once the guard gave the terminal back, a later panic writes nothing.
    #[test]
    fn the_guard_and_the_panic_hook_restore_once_between_them() {
        use crate::panic_guard::{self, SharedWriter};
        let _serial = panic_guard::test_lock();
        let original = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let ui = std::thread::current().id();

        let hook = SharedWriter::default();
        panic_guard::install(ui, restore_sequence(true), Box::new(hook.clone()));
        let unwound = std::panic::catch_unwind(|| {
            let _guard = TermGuard {
                restored: false,
                kitty: true,
            };
            panic!("ui boom");
        });
        let mut late = TermGuard {
            restored: false,
            kitty: true,
        };
        let mut after_hook = Vec::new();
        let gave_after_hook = late.give_back(&mut after_hook);

        let hook_again = SharedWriter::default();
        panic_guard::install(ui, restore_sequence(true), Box::new(hook_again.clone()));
        let mut guard = TermGuard {
            restored: false,
            kitty: true,
        };
        let mut first = Vec::new();
        let gave_first = guard.give_back(&mut first);
        let panicked = std::panic::catch_unwind(|| panic!("ui boom"));
        panic_guard::uninstall();
        std::panic::set_hook(original);

        assert!(unwound.is_err() && panicked.is_err());
        assert_eq!(hook.bytes(), restore_sequence(true), "the hook's, once");
        assert!(
            !gave_after_hook && after_hook.is_empty(),
            "the drop adds nothing"
        );
        assert!(gave_first);
        assert!(first.ends_with(b"\x1b[?1049l"), "{first:?}");
        assert!(
            first
                .windows(KITTY_OPT_OUT.len())
                .any(|w| w == KITTY_OPT_OUT),
            "kitty opted out"
        );
        assert!(
            hook_again.bytes().is_empty(),
            "the panic after adds nothing"
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

    /// `bytes` with every CSI sequence (`ESC [`, parameter and intermediate
    /// bytes, a final byte) taken out; panics on any other byte below 0x20
    /// or DEL.
    fn outside_csi(bytes: &[u8]) -> String {
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b == 0x1b {
                assert_eq!(bytes.get(i + 1), Some(&b'['), "a bare ESC at byte {i}");
                let mut j = i + 2;
                while bytes.get(j).is_some_and(|b| (0x30..=0x3f).contains(b)) {
                    j += 1;
                }
                while bytes.get(j).is_some_and(|b| (0x20..=0x2f).contains(b)) {
                    j += 1;
                }
                assert!(
                    bytes.get(j).is_some_and(|b| (0x40..=0x7e).contains(b)),
                    "an unfinished CSI at byte {i}"
                );
                i = j + 1;
                continue;
            }
            assert!(b >= 0x20 && b != 0x7f, "byte {b:#04x} at {i}");
            out.push(b);
            i += 1;
        }
        String::from_utf8(out).expect("UTF-8")
    }

    /// The blit is the second guard: a control or bidi character that reached
    /// a cell some other way ([`Canvas::put`] takes any character) is shown
    /// as `�`. The only bytes below 0x20 that reach the terminal are the
    /// escape sequences the blit itself has written, at every colour depth.
    #[test]
    fn no_control_reaches_the_terminal_but_the_blits_own_sequences() {
        let t = theme();
        let hostile: Vec<char> = (0u32..0x20)
            .chain(0x7f..0xa0)
            .chain([0x200e, 0x200f])
            .chain(0x202a..=0x202e)
            .chain(0x2066..=0x2069)
            .filter_map(char::from_u32)
            .collect();
        let mut c = Canvas::new(40, 3, t);
        for (i, &ch) in hostile.iter().enumerate() {
            let i = i32::try_from(i).expect("small");
            c.put(i % 40, i / 40, ch, Some(t.roles.file), None);
        }
        c.set_blink(0, 0);
        c.text(0, 2, "ok {R}\u{1b}[2J", t.roles.file, None);
        for caps in [ColorCaps::TrueColor, ColorCaps::Ansi256, ColorCaps::Ansi16] {
            let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, 40, 3));
            blit(&c, &mut buf, &mut Palette::new(caps, t));
            let mut bytes = Vec::new();
            let cells = buf.content.iter().enumerate().map(|(i, cell)| {
                let (x, y) = buf.pos_of(i);
                (x, y, cell)
            });
            CrosstermBackend::new(&mut bytes)
                .draw(cells)
                .expect("drawn");
            let shown = outside_csi(&bytes);
            for ch in shown.chars() {
                assert_eq!(
                    crate::ui::canvas::printable(ch),
                    ch,
                    "U+{:04X} reached the terminal ({caps:?})",
                    u32::from(ch)
                );
            }
            let replaced = shown.chars().filter(|&ch| ch == '\u{fffd}').count();
            assert_eq!(replaced, hostile.len() + 1, "{caps:?}");
            assert!(shown.contains("ok {R}\u{fffd}[2J"), "{shown:?}");
        }
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
