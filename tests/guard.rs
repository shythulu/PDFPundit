//! The terminal guard (D-043; test shape from FR-r1-g) and the start-up input
//! flush (eng-r3-q3, lead-r4-fr2), against the built binary.
//!
//! The false case gates on every OS: with stdin or stdout a pipe, a file or
//! the null device the binary prints one line to stderr and exits 2, having
//! read nothing and written nothing else. CI runs this file under
//! `cargo test --release` too, so the shipped profile is the one proven
//! (D-051).
//!
//! The true case and the flush need a terminal, which a hosted runner step
//! does not have, so they open a pty (rustix's `pty` feature, a dev-only
//! dependency). They run on Unix only: a ConPTY variant waits on a run that
//! shows ConPTY delivers output on windows-latest (actions/runner#3168).
//! The guard times its child, so the time types are allowed here.
#![allow(clippy::disallowed_types)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const LINE: &str = "PDFPundit runs in a terminal; drop PDFs on the cat.";

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pdfpundit"))
}

/// A fresh directory under the system temp dir, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Scratch {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "pdfpundit-guard-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.0)
            .expect("read dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn assert_refused(out: &std::process::Output) {
    assert_eq!(
        out.status.code(),
        Some(2),
        "exit status {:?}, stderr {:?}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stderr), format!("{LINE}\n"));
}

#[test]
fn piped_stdin_and_a_file_for_stdout_exit_2() {
    let dir = Scratch::new("piped");
    let fixture = dir.path().join("a.pdf");
    std::fs::write(&fixture, b"%PDF-1.4\n%%EOF\n").expect("fixture");
    let stdout = std::fs::File::create(dir.path().join("stdout.txt")).expect("stdout file");
    // argv is never read: a path there changes nothing.
    let mut child = bin()
        .arg(&fixture)
        .stdin(Stdio::piped())
        .stdout(stdout)
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    // The child may already have exited: a broken pipe here is not a failure.
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(fixture.to_string_lossy().as_bytes());
    }
    let out = child.wait_with_output().expect("wait");
    assert_refused(&out);
    let written = std::fs::read(dir.path().join("stdout.txt")).expect("read stdout file");
    assert!(written.is_empty(), "stdout got {written:?}");
    assert_eq!(dir.names(), ["a.pdf", "stdout.txt"]);
    assert_eq!(
        std::fs::read(&fixture).expect("fixture"),
        b"%PDF-1.4\n%%EOF\n"
    );
}

#[test]
fn everything_piped_exits_2() {
    let out = bin()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run");
    assert_refused(&out);
    assert!(out.stdout.is_empty());
}

#[test]
fn null_stdin_exits_2() {
    let out = bin()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run");
    assert_refused(&out);
    assert!(out.stdout.is_empty());
}

#[cfg(unix)]
mod pty {
    //! The binary on a pseudo-terminal. Every byte the test writes to the
    //! master before the child arms its input must be discarded; a `q` that
    //! arrived would quit it, so "still running" is the proof that nothing
    //! was delivered, and a `q` sent later quitting it is the proof that live
    //! input still arrives.

    use std::fs::File;
    use std::io::{Read, Write};
    use std::process::{Child, Stdio};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
    use rustix::termios::{Winsize, tcsetwinsize};

    use super::{Scratch, bin};

    /// The status bar's word every layout from the widget up draws.
    const FRAME_MARK: &[u8] = b"offline";
    /// Entering the alternate screen: step 2, before the first frame.
    const ALT_SCREEN: &[u8] = b"\x1b[?1049h";
    /// `ab` and a bracketed paste, then a `q` that would quit the app.
    const STALE: &[u8] = b"ab\x1b[200~hi\x1b[201~q";

    struct Session {
        child: Child,
        master: File,
        out: Arc<Mutex<Vec<u8>>>,
        /// Told once when the reader stops at its pause mark.
        paused: Receiver<()>,
        resume: Sender<()>,
        _home: Scratch,
    }

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    /// Starts the binary on a `cols × rows` pty. `before` is written to the
    /// master first, before the child exists. The reader stops reading the
    /// child's output once it has seen `pause_at` and waits for `resume`.
    fn start(cols: u16, rows: u16, before: &[u8], pause_at: Option<&'static [u8]>) -> Session {
        open(cols, rows, before, pause_at, &[])
    }

    /// [`start`] with extra environment variables.
    #[cfg(debug_assertions)]
    fn start_with(cols: u16, rows: u16, env: &[(&str, &str)]) -> Session {
        open(cols, rows, b"", None, env)
    }

    fn open(
        cols: u16,
        rows: u16,
        before: &[u8],
        pause_at: Option<&'static [u8]>,
        env: &[(&str, &str)],
    ) -> Session {
        let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).expect("openpt");
        grantpt(&master).expect("grantpt");
        unlockpt(&master).expect("unlockpt");
        let name = ptsname(&master, Vec::new()).expect("ptsname");
        let slave = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(name.to_str().expect("utf-8 pty name"))
            .expect("open the pty");
        let size = Winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        tcsetwinsize(&slave, size).expect("set the pty size");
        let mut master = File::from(master);
        master.write_all(before).expect("write to the master");

        // The app's config and history land in a scratch home, never the
        // developer's.
        let home = Scratch::new("pty-home");
        let child = bin()
            .stdin(Stdio::from(slave.try_clone().expect("clone")))
            .stdout(Stdio::from(slave.try_clone().expect("clone")))
            .stderr(Stdio::from(slave))
            .env("HOME", home.path())
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_DATA_HOME")
            .env_remove("XDG_CACHE_HOME")
            .envs(env.iter().copied())
            .spawn()
            .expect("spawn on the pty");

        let out = Arc::new(Mutex::new(Vec::new()));
        let (paused_tx, paused) = mpsc::channel();
        let (resume, resume_rx) = mpsc::channel::<()>();
        let mut reader = master.try_clone().expect("clone the master");
        let sink = Arc::clone(&out);
        thread::spawn(move || {
            let mut pause_at = pause_at;
            let mut chunk = [0u8; 4096];
            loop {
                let n = match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => n,
                };
                let seen = {
                    let mut all = sink.lock().expect("output");
                    all.extend_from_slice(&chunk[..n]);
                    pause_at.is_some_and(|mark| contains(&all, mark))
                };
                if seen {
                    pause_at = None;
                    let _ = paused_tx.send(());
                    let _ = resume_rx.recv();
                }
            }
        });
        Session {
            child,
            master,
            out,
            paused,
            resume,
            _home: home,
        }
    }

    impl Session {
        fn output(&self) -> Vec<u8> {
            self.out.lock().expect("output").clone()
        }

        /// Waits up to `limit` for `mark` in the child's output.
        fn wait_for(&mut self, mark: &[u8], limit: Duration) {
            let deadline = Instant::now() + limit;
            while !contains(&self.output(), mark) {
                if let Some(status) = self.child.try_wait().expect("try_wait") {
                    panic!("exited with {status} before {mark:?}: {:?}", self.text());
                }
                assert!(
                    Instant::now() < deadline,
                    "no {mark:?} in {:?}",
                    self.text()
                );
                thread::sleep(Duration::from_millis(10));
            }
        }

        /// Still running after `d`.
        fn alive_after(&mut self, d: Duration) {
            let deadline = Instant::now() + d;
            while Instant::now() < deadline {
                if let Some(status) = self.child.try_wait().expect("try_wait") {
                    panic!("exited with {status}: {:?}", self.text());
                }
                thread::sleep(Duration::from_millis(20));
            }
        }

        /// Types `q` every 100 ms until the app quits; its exit code.
        fn quit(&mut self) -> Option<i32> {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(status) = self.child.try_wait().expect("try_wait") {
                    return status.code();
                }
                assert!(Instant::now() < deadline, "q never quit: {:?}", self.text());
                self.master.write_all(b"q").expect("type q");
                thread::sleep(Duration::from_millis(100));
            }
        }

        fn text(&self) -> String {
            let out = self.output();
            let tail = out.len().saturating_sub(400);
            String::from_utf8_lossy(&out[tail..]).into_owned()
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            let _ = self.resume.send(());
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// The panic hook as wired, end to end: a UI-thread panic (a debug
    /// build's `PDFPUNDIT_PANIC`) leaves the alternate screen and bracketed
    /// paste before the message, and exits 101.
    #[cfg(debug_assertions)]
    #[test]
    fn a_ui_thread_panic_gives_the_terminal_back() {
        let mut s = start_with(112, 38, &[("PDFPUNDIT_PANIC", "1")]);
        let deadline = Instant::now() + Duration::from_secs(20);
        let status = loop {
            if let Some(status) = s.child.try_wait().expect("try_wait") {
                break status;
            }
            assert!(Instant::now() < deadline, "no panic: {:?}", s.text());
            thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(status.code(), Some(101), "{:?}", s.text());
        // The reader may still be copying the last bytes.
        let restored = b"\x1b[?2004l\x1b[?25h\x1b[?1049l";
        let deadline = Instant::now() + Duration::from_secs(5);
        while !contains(&s.output(), b"PDFPUNDIT_PANIC: a UI-thread panic") {
            assert!(Instant::now() < deadline, "no message: {:?}", s.text());
            thread::sleep(Duration::from_millis(20));
        }
        let out = s.output();
        let at = |needle: &[u8]| out.windows(needle.len()).position(|w| w == needle);
        let restore = at(restored).expect("the restore sequence");
        let message = at(b"PDFPUNDIT_PANIC: a UI-thread panic").expect("the message");
        assert!(restore < message, "restored before the message");
        assert!(
            at(FRAME_MARK).is_some_and(|f| f < restore),
            "after the first frame"
        );
    }

    #[test]
    fn on_a_terminal_it_runs_instead_of_refusing() {
        let mut s = start(112, 38, b"", None);
        s.wait_for(ALT_SCREEN, Duration::from_secs(20));
        s.alive_after(Duration::from_millis(500));
    }

    /// lead-r4-fr2's probe: `ab` and a bracketed paste on the master before
    /// the child starts arrive as nothing; a key typed after the flush still
    /// arrives.
    #[test]
    fn bytes_queued_before_start_are_flushed_and_later_keys_arrive() {
        let mut s = start(112, 38, STALE, None);
        s.wait_for(FRAME_MARK, Duration::from_secs(20));
        s.alive_after(Duration::from_millis(500));
        assert_eq!(s.quit(), Some(0));
    }

    /// A paste landing between the terminal setup and the first frame (the
    /// handshake window) submits nothing and the first frame still draws.
    /// The reader stops at the alternate screen, so the child blocks writing
    /// its first frame (an 800 × 250 frame is over 200 KB, more than a pty holds)
    /// and is still before the flush when the paste is written.
    #[test]
    fn a_paste_before_the_first_frame_is_never_delivered() {
        let mut s = start(800, 250, b"", Some(ALT_SCREEN));
        s.paused
            .recv_timeout(Duration::from_secs(20))
            .expect("the alternate screen");
        s.master.write_all(STALE).expect("paste");
        thread::sleep(Duration::from_millis(200));
        assert!(
            !contains(&s.output(), FRAME_MARK),
            "the first frame was already out"
        );
        let _ = s.resume.send(());
        s.wait_for(FRAME_MARK, Duration::from_secs(20));
        s.alive_after(Duration::from_millis(500));
        assert_eq!(s.quit(), Some(0));
    }
}
