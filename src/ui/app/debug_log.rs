//! The debug log on disk (D-132): every line of the loop's in-memory log is
//! also appended to `<cache dir>/debug.log`. Once the file would pass
//! [`CAP`] bytes it is rotated: moved, without replacing anything, to the
//! first free `debug.<n>.log` beside it (the no-replace move outputs use,
//! D-044), and a new `debug.log` is begun. Nothing here reaches an artefact.

use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

use crate::place::place_no_replace;

/// The log's name inside the cache directory.
pub(crate) const FILE: &str = "debug.log";
/// The size a log file is rotated at.
pub(crate) const CAP: u64 = 1 << 20;
/// The highest `debug.<n>.log` tried before a rotation gives up.
const MAX_ROTATED: u32 = 10_000;

/// `debug.log` in one directory, open for appending.
pub(crate) struct DebugLog {
    dir: PathBuf,
    file: File,
    /// The file's length as far as this process knows.
    len: u64,
    cap: u64,
}

impl DebugLog {
    /// Opens (creating the directory and the file as needed) `debug.log` in
    /// `dir`, rotating it first when it is already at the cap, and writes a
    /// line that says a session started.
    pub(crate) fn open(dir: &Path) -> io::Result<DebugLog> {
        DebugLog::with_cap(dir, CAP)
    }

    fn with_cap(dir: &Path, cap: u64) -> io::Result<DebugLog> {
        fs::create_dir_all(dir)?;
        let (file, len) = append_to(&dir.join(FILE))?;
        let mut log = DebugLog {
            dir: dir.to_path_buf(),
            file,
            len,
            cap,
        };
        if log.len >= log.cap {
            log.rotate()?;
        }
        log.write(&format!(
            "--- pdfpundit {} started, pid {} ---",
            env!("CARGO_PKG_VERSION"),
            std::process::id()
        ))?;
        Ok(log)
    }

    /// Appends `line` and a newline, rotating first when the file would pass
    /// the cap. A control character in the line (a file name's newline) is
    /// written escaped, so one entry is always one line.
    pub(crate) fn write(&mut self, line: &str) -> io::Result<()> {
        let mut text = String::with_capacity(line.len() + 1);
        for ch in line.chars() {
            if ch.is_control() {
                text.extend(ch.escape_default());
            } else {
                text.push(ch);
            }
        }
        text.push('\n');
        let n = text.len() as u64;
        if self.len > 0 && self.len + n > self.cap {
            self.rotate()?;
        }
        self.file.write_all(text.as_bytes())?;
        self.len += n;
        Ok(())
    }

    /// Moves `debug.log` to the first free `debug.<n>.log`, never over an
    /// existing file, and begins a new one.
    fn rotate(&mut self) -> io::Result<()> {
        self.file.flush()?;
        let from = self.dir.join(FILE);
        let mut moved = false;
        for n in 1..=MAX_ROTATED {
            let to = self.dir.join(format!("debug.{n}.log"));
            match place_no_replace(&from, &to) {
                Ok(_) => {
                    moved = true;
                    break;
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        if !moved {
            return Err(io::Error::new(
                ErrorKind::AlreadyExists,
                format!("every name up to debug.{MAX_ROTATED}.log is taken"),
            ));
        }
        (self.file, self.len) = append_to(&from)?;
        Ok(())
    }
}

/// `path` opened for appending, created if absent, and its length.
fn append_to(path: &Path) -> io::Result<(File, u64)> {
    let file = OpenOptions::new().append(true).create(true).open(path)?;
    let len = file.metadata()?.len();
    Ok((file, len))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::place::ScratchDir;

    fn read(dir: &ScratchDir, name: &str) -> String {
        fs::read_to_string(dir.join(name)).expect("log")
    }

    #[test]
    fn lines_are_appended_after_a_session_line() {
        let dir = ScratchDir::new("debug-log");
        let cache = dir.join("cache");
        let mut log = DebugLog::open(&cache).expect("open");
        log.write("drop refused: a.txt: not a pdf").expect("write");
        log.write("name\nwith a newline\u{1b}[2J").expect("write");
        drop(log);
        DebugLog::open(&cache).expect("again");
        let text = fs::read_to_string(cache.join(FILE)).expect("log");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 4, "{text}");
        assert!(lines[0].starts_with("--- pdfpundit ") && lines[0].ends_with(" ---"));
        assert_eq!(lines[1], "drop refused: a.txt: not a pdf");
        assert_eq!(lines[2], "name\\nwith a newline\\u{1b}[2J");
        assert!(
            lines[3].starts_with("--- pdfpundit "),
            "appended, not replaced"
        );
    }

    #[test]
    fn a_full_log_is_rotated_onto_a_free_name_and_nothing_is_replaced() {
        let dir = ScratchDir::new("debug-log-rotate");
        fs::write(dir.join("debug.1.log"), b"an older log").expect("write");
        let mut log = DebugLog::with_cap(dir.path(), 100).expect("open");
        let session = read(&dir, FILE);
        let line = "x".repeat(40);
        log.write(&line).expect("write");
        assert_eq!(dir.names(), ["debug.1.log", "debug.log"], "under the cap");

        // The next line would pass 100 bytes: the file moves to debug.2.log,
        // the first free name, and the line begins a new debug.log.
        log.write(&line).expect("write");
        assert_eq!(dir.names(), ["debug.1.log", "debug.2.log", "debug.log"]);
        assert_eq!(read(&dir, "debug.1.log"), "an older log");
        assert_eq!(read(&dir, "debug.2.log"), format!("{session}{line}\n"));
        assert_eq!(read(&dir, FILE), format!("{line}\n"));

        // A log already at the cap is rotated when it is opened.
        drop(log);
        fs::write(dir.join(FILE), "y".repeat(100)).expect("write");
        DebugLog::with_cap(dir.path(), 100).expect("open");
        assert_eq!(read(&dir, "debug.3.log"), "y".repeat(100));
        assert!(read(&dir, FILE).starts_with("--- pdfpundit "));
    }

    #[test]
    fn an_unwritable_cache_directory_is_an_error() {
        let dir = ScratchDir::new("debug-log-blocked");
        fs::write(dir.join("cache"), b"a file, not a folder").expect("write");
        assert!(DebugLog::open(&dir.join("cache")).is_err());
    }
}
