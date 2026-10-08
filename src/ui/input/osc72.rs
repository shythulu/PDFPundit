//! kitty's drag and drop protocol, OSC 72 (T-31; D-033 amended, D-039,
//! D-057).
//!
//! The one source is the protocol as kitty publishes it,
//! <https://sw.kovidgoyal.net/kitty/dnd-protocol/> ("added in 0.47.0");
//! nothing here comes from kitty's code. The page prints OSC as
//! `0x1b 0x5b`: it is `ESC ]`, 0x1b 0x5d. Drops need kitty 0.49 or later,
//! the release with the fixes its changelog lists as CVEs.
//!
//! Every escape code is `ESC ] 72 ; metadata ; payload ST` (or `BEL`), the
//! metadata `:`-separated `key=value` pairs in any order. The pieces:
//!
//! - [`Handshake`]: start-up's query, `t=q` then DA1. A `t=q` reply before
//!   the DA1 reply means kitty; every other byte read in the window is
//!   discarded and counted (D-033 amended). `term::handshake` does the I/O.
//! - [`Splitter`]: on kitty the app owns stdin, and this splits it into OSC 72
//!   events and everything else, which `keys.rs` decodes. Any OSC that is not
//!   an OSC 72 frame is dropped: no OSC byte is ever a key.
//! - The reassembler under it: a payload over 4,096 encoded bytes comes in
//!   chunks, `m=1` on all but the last, and only the first chunk is sure to
//!   carry metadata beyond `m`, so a continuation (a bare `m=1;…`, or `m=0`
//!   with no `t` or `x`) belongs to the open transfer and takes its metadata
//!   from the first chunk. Data (`t=r`) ends with an empty payload and `m=0`;
//!   its chunks are joined, then decoded once, padding optional. Caps: 4,096
//!   bytes of payload per frame, 1 MiB per transfer.
//! - [`DndSession`]: the protocol on the UI thread, which writes every reply:
//!
//! | from kitty | the app |
//! |---|---|
//! | `t=m:x=…:y=…;MIME list` (the list on the first move and on a change) | `DragAt(cell)`; `t=m:o=1;text/uri-list` over the cat, else `t=m:o=0`, each time that changes |
//! | `t=m:x=-1:y=-1` (left; any negative cell) | `DragAt(None)` |
//! | `t=M…;full MIME list` | `t=r:x=<1-based index of text/uri-list>`, or `t=r:o=0` |
//! | `t=r:x=…;base64…` … `m=0` and an empty payload | the `file://` list: every file read into memory, then `t=r:o=1` (`o=0` when none) |
//! | `t=R:x=…;ENAME[:description]` | the drop ended: `t=r:o=0` and a hint (D-057) |
//!
//! Only copies are accepted: a drag that offers only a move (`o=2`) is
//! declined, so the source never deletes an original. kitty's own framing
//! beyond the page (`m=0` on every move and drop, a space after each MIME
//! type, 3,072-byte raw chunks, `x=` on every data chunk and on the end, the
//! `t=q` reply's exact form) is accepted and never relied on (FR-r1-b §B).

use std::collections::BTreeMap;

use base64::Engine;
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

#[cfg_attr(not(unix), allow(unused_imports))]
use super::Input;
#[cfg_attr(not(unix), allow(unused_imports))]
use super::keys::{Decoder, Token};
use super::paste::{self, PathCandidate, Style, is_pdf};
use crate::ui::director::{CatEvent, CellPos};
use crate::ui::strings;
#[cfg_attr(not(unix), allow(unused_imports))]
use crate::ui::term::HandshakeResult;

/// The most payload one frame may carry, in encoded bytes.
pub(crate) const PAYLOAD_CAP: usize = 4096;
/// The most one transfer (a MIME list, a drop's data) may carry, encoded.
pub(crate) const TRANSFER_CAP: usize = 1 << 20;
/// The longest metadata read.
const META_CAP: usize = 1024;
/// How much of a drop's files is read into memory before it completes.
pub(crate) const HOLD_LIMIT: u64 = 512 << 20;

const URI_LIST: &str = "text/uri-list";

/// Start-up's question: does the terminal speak OSC 72?
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) const QUERY: &[u8] = b"\x1b]72;t=q\x1b\\";
/// Primary device attributes: every terminal answers it.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) const DA1: &[u8] = b"\x1b[c";
/// Drops wanted, files only.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) const OPT_IN: &[u8] = b"\x1b]72;t=a;text/uri-list\x1b\\";
/// No more drops: sent on every exit path once [`OPT_IN`] was.
pub(crate) const OPT_OUT: &[u8] = b"\x1b]72;t=A\x1b\\";
const ACCEPT: &[u8] = b"\x1b]72;t=m:o=1;text/uri-list\x1b\\";
const DECLINE: &[u8] = b"\x1b]72;t=m:o=0\x1b\\";
const DONE: &[u8] = b"\x1b]72;t=r:o=1\x1b\\";
const CANCELLED: &[u8] = b"\x1b]72;t=r:o=0\x1b\\";

/// Base64 that takes the last chunk with or without its padding.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// One OSC 72 message from the terminal, its chunks joined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DndEvent {
    /// `t=m`: a drag over the window at `cell`; `None` when it left. `copy`
    /// when the drag allows a copy; `mimes` when the terminal sent the list
    /// (the first move, and whenever it changes).
    Move {
        cell: Option<CellPos>,
        copy: bool,
        mimes: Option<Vec<String>>,
    },
    /// `t=M`: dropped at `cell`, with every MIME type the drop holds.
    Drop {
        cell: Option<CellPos>,
        copy: bool,
        mimes: Vec<String>,
    },
    /// `t=r`: the data for the MIME type at 1-based `idx`, decoded.
    Data {
        idx: u32,
        data: Result<Vec<u8>, DataError>,
    },
    /// `t=R`: the terminal ended the drop. `name` is a POSIX error name
    /// (`EUNKNOWN` when it was not one); `description` is there only when it
    /// was safe text.
    Error {
        name: String,
        description: Option<String>,
    },
}

/// Why a drop's data could not be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataError {
    /// Over a cap: a frame over 4,096 bytes, a transfer over 1 MiB.
    TooBig,
    /// Not base64.
    Garbled,
}

// ── frames ──────────────────────────────────────────────────────────────

/// A frame's metadata: `key=value` pairs, in any order. Pairs without `=`
/// are skipped; the last of a repeated key wins.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Meta(BTreeMap<String, String>);

impl Meta {
    fn parse(bytes: &[u8]) -> Option<Meta> {
        if bytes.len() > META_CAP {
            return None;
        }
        let text = std::str::from_utf8(bytes).ok()?;
        let pairs = text
            .split(':')
            .filter_map(|pair| pair.split_once('='))
            .filter(|(k, _)| !k.is_empty())
            .map(|(k, v)| (k.to_owned(), v.to_owned()));
        Some(Meta(pairs.collect()))
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    /// `t`, the event type.
    fn t(&self) -> Option<char> {
        let mut c = self.get("t")?.chars();
        let t = c.next()?;
        c.next().is_none().then_some(t)
    }

    /// An integer key: 32-bit, signed or unsigned, in decimal.
    fn int(&self, key: &str) -> Option<i64> {
        let n: i64 = self.get(key)?.parse().ok()?;
        (i64::from(i32::MIN)..=i64::from(u32::MAX))
            .contains(&n)
            .then_some(n)
    }

    /// `m=1`: more chunks follow. Absent is 0.
    fn more(&self) -> bool {
        self.int("m") == Some(1)
    }

    /// The cell `x`, `y` name; `None` for a negative one, which means the
    /// drag left. Absent keys are 0.
    fn cell(&self) -> Option<CellPos> {
        let (x, y) = (self.int("x").unwrap_or(0), self.int("y").unwrap_or(0));
        if x < 0 || y < 0 {
            return None;
        }
        let clamp = |n: i64| u16::try_from(n).unwrap_or(u16::MAX);
        Some(CellPos {
            x: clamp(x),
            y: clamp(y),
        })
    }

    /// Whether the drag allows a copy: `o` is 1 or 3, or not given.
    fn copy(&self) -> bool {
        self.int("o").is_none_or(|o| o & 1 == 1)
    }
}

/// One escape code.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Frame {
    meta: Meta,
    payload: Vec<u8>,
    /// The payload was over [`PAYLOAD_CAP`] and is not kept.
    oversize: bool,
}

impl Frame {
    /// The frame in an OSC body, when it is an OSC 72 one with readable
    /// metadata. With no second `;` the payload is empty.
    fn parse(body: &[u8], truncated: bool) -> Option<Frame> {
        let rest = body.strip_prefix(b"72;")?;
        let (meta, payload) = match memchr::memchr(b';', rest) {
            Some(i) => (&rest[..i], &rest[i + 1..]),
            None => (rest, &[][..]),
        };
        let meta = Meta::parse(meta)?;
        let oversize = truncated || payload.len() > PAYLOAD_CAP;
        Some(Frame {
            meta,
            payload: if oversize {
                Vec::new()
            } else {
                payload.to_vec()
            },
            oversize,
        })
    }
}

/// A chunked transfer in progress.
#[derive(Debug)]
struct Transfer {
    /// The first chunk's metadata, which the whole transfer goes by.
    meta: Meta,
    t: char,
    buf: Vec<u8>,
    /// A cap was passed; the rest is read and dropped.
    over: bool,
}

impl Transfer {
    fn add(&mut self, f: &Frame) {
        if f.oversize || self.buf.len() + f.payload.len() > TRANSFER_CAP {
            self.over = true;
            self.buf = Vec::new();
        }
        if !self.over {
            self.buf.extend_from_slice(&f.payload);
        }
    }

    fn finish(self) -> Option<DndEvent> {
        let mimes = |buf: &[u8]| -> Vec<String> {
            String::from_utf8_lossy(buf)
                .split_whitespace()
                .map(str::to_owned)
                .collect()
        };
        let m = &self.meta;
        Some(match self.t {
            'm' => DndEvent::Move {
                cell: m.cell(),
                copy: m.copy(),
                mimes: match (self.over, self.buf.is_empty()) {
                    (true, _) => Some(Vec::new()),
                    (false, true) => None,
                    (false, false) => Some(mimes(&self.buf)),
                },
            },
            'M' => DndEvent::Drop {
                cell: m.cell(),
                copy: m.copy(),
                mimes: if self.over {
                    Vec::new()
                } else {
                    mimes(&self.buf)
                },
            },
            'r' => DndEvent::Data {
                idx: m.int("x").and_then(|x| u32::try_from(x).ok()).unwrap_or(0),
                data: if self.over {
                    Err(DataError::TooBig)
                } else {
                    BASE64.decode(&self.buf).map_err(|_| DataError::Garbled)
                },
            },
            'R' => error_event(&self.buf),
            _ => return None,
        })
    }
}

/// `ENAME[:description]` as an [`DndEvent::Error`].
fn error_event(payload: &[u8]) -> DndEvent {
    let (name, description) = match memchr::memchr(b':', payload) {
        Some(i) => (&payload[..i], Some(&payload[i + 1..])),
        None => (payload, None),
    };
    let is_name = !name.is_empty()
        && name.len() <= 32
        && name.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_');
    let name = if is_name {
        String::from_utf8_lossy(name).into_owned()
    } else {
        "EUNKNOWN".to_owned()
    };
    DndEvent::Error {
        name,
        description: description.and_then(safe_text),
    }
}

/// Text that is safe to show: UTF-8 with no C0 or C1 control and no DEL.
fn safe_text(bytes: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(bytes).ok()?;
    let safe = !s.is_empty()
        && !s
            .chars()
            .any(|c| c.is_control() || ('\u{80}'..='\u{9f}').contains(&c));
    safe.then(|| s.to_owned())
}

/// Joins chunked frames into [`DndEvent`]s.
#[derive(Debug, Default)]
struct Reassembler {
    open: Option<Transfer>,
}

impl Reassembler {
    fn push(&mut self, f: Frame) -> Option<DndEvent> {
        let t = f.meta.t();
        // A query reply may come mid-transfer; after start-up it means
        // nothing.
        if t == Some('q') {
            return None;
        }
        if let Some(open) = self.open.as_mut() {
            let same_x = match (f.meta.get("x"), open.meta.get("x")) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            };
            if t.is_none_or(|t| t == open.t) && same_x {
                open.add(&f);
                let more = f.meta.more();
                let ends = if open.t == 'r' {
                    !more && f.payload.is_empty() && !f.oversize
                } else {
                    !more
                };
                return if ends {
                    self.open.take().and_then(Transfer::finish)
                } else {
                    None
                };
            }
            // Anything else while a transfer is open breaks the protocol:
            // the transfer is dropped and the new frame starts afresh.
            self.open = None;
        }
        // A continuation with nothing to continue.
        let t = t?;
        let more = f.meta.more();
        let empty_end = !more && f.payload.is_empty() && !f.oversize;
        let mut transfer = Transfer {
            meta: f.meta.clone(),
            t,
            buf: Vec::new(),
            over: false,
        };
        transfer.add(&f);
        let ends = if t == 'r' { empty_end } else { !more };
        if ends {
            transfer.finish()
        } else {
            self.open = Some(transfer);
            None
        }
    }
}

// ── the session reader (on kitty the app owns stdin) ────────────────────

/// Splits raw stdin into OSC 72 events and decoded keys, mouse reports and
/// pastes, across any split of the stream.
#[derive(Debug, Default)]
pub(crate) struct Splitter {
    keys: Decoder,
    chunks: Reassembler,
}

#[cfg_attr(not(unix), allow(dead_code))]
impl Splitter {
    pub(crate) fn new() -> Splitter {
        Splitter::default()
    }

    /// The inputs `bytes` completes.
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Vec<Input> {
        let tokens = self.keys.feed(bytes);
        self.route(tokens)
    }

    /// Nothing came for a while: a lone `ESC` is the Esc key.
    pub(crate) fn idle(&mut self) -> Vec<Input> {
        let tokens = self.keys.idle();
        self.route(tokens)
    }

    fn route(&mut self, tokens: Vec<Token>) -> Vec<Input> {
        let mut out = Vec::new();
        for token in tokens {
            match token {
                Token::Input(input) => out.push(input),
                Token::Osc {
                    body, truncated, ..
                } => {
                    if let Some(ev) =
                        Frame::parse(&body, truncated).and_then(|f| self.chunks.push(f))
                    {
                        out.push(Input::Dnd(ev));
                    }
                }
                // A late answer to start-up's DA1.
                Token::DeviceAttributes { .. } => {}
            }
        }
        out
    }
}

// ── the handshake ─────────────────────────────────────────────────────────

/// Start-up's window: what came back after [`QUERY`] and [`DA1`].
#[derive(Debug, Default)]
pub(crate) struct Handshake {
    keys: Decoder,
    read: usize,
    replies: usize,
    kitty: bool,
    da1: bool,
}

#[cfg_attr(not(unix), allow(dead_code))]
impl Handshake {
    pub(crate) fn new() -> Handshake {
        Handshake::default()
    }

    /// Bytes read in the window. A `t=q` reply (matched on its metadata;
    /// whatever its payload says is ignored) counts only before the DA1
    /// reply; inside a bracketed paste nothing is a reply.
    pub(crate) fn feed(&mut self, bytes: &[u8]) {
        self.read += bytes.len();
        for token in self.keys.feed(bytes) {
            match token {
                Token::DeviceAttributes { raw } if !self.da1 => {
                    self.da1 = true;
                    self.replies += raw;
                }
                Token::Osc {
                    body,
                    truncated,
                    raw,
                } if !self.da1
                    && !self.kitty
                    && Frame::parse(&body, truncated).is_some_and(|f| f.meta.t() == Some('q')) =>
                {
                    self.kitty = true;
                    self.replies += raw;
                }
                _ => {}
            }
        }
    }

    /// The DA1 reply came: nothing later changes the answer.
    pub(crate) fn done(&self) -> bool {
        self.da1
    }

    /// The answer, with every byte that was neither reply counted.
    pub(crate) fn result(&self) -> HandshakeResult {
        HandshakeResult {
            kitty: self.kitty,
            discarded: self.read - self.replies,
        }
    }
}

// ── the protocol, on the UI thread ──────────────────────────────────────

/// What the app does for a [`DndEvent`], in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DndAction {
    /// Bytes for the terminal.
    Reply(Vec<u8>),
    Cat(CatEvent),
    /// The drop's files: read each one into memory, then call
    /// [`DndSession::finish`] with how many were taken.
    Files(Vec<PathCandidate>),
    /// The drop ended without files: the hint row's word, if any, and a
    /// debug-log line.
    Ended {
        hint: Option<&'static str>,
        log: String,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Idle,
    /// `t=r:x=idx` was sent.
    Requested(u32),
    /// The files are being read; [`DndSession::finish`] completes the drop.
    Reading,
}

/// The drag-and-drop session: replies, the cat's drag events and the drop's
/// files.
#[derive(Debug, Default)]
pub(crate) struct DndSession {
    /// The MIME types the drag offers.
    offered: Vec<String>,
    /// The last `t=m` answer sent for this drag.
    accepting: Option<bool>,
    phase: Phase,
}

impl DndSession {
    /// What `ev` asks of the app. `over_cat` says whether a cell is on the
    /// cat, where drops go.
    pub(crate) fn on(
        &mut self,
        ev: DndEvent,
        over_cat: &dyn Fn(CellPos) -> bool,
    ) -> Vec<DndAction> {
        match ev {
            DndEvent::Move { cell: None, .. } => {
                self.offered.clear();
                self.accepting = None;
                vec![DndAction::Cat(CatEvent::DragAt(None))]
            }
            DndEvent::Move {
                cell: Some(cell),
                copy,
                mimes,
            } => {
                if let Some(mimes) = mimes {
                    self.offered = mimes;
                }
                let accept = copy && over_cat(cell) && self.offered.iter().any(|m| m == URI_LIST);
                let mut out = vec![DndAction::Cat(CatEvent::DragAt(Some(cell)))];
                if self.accepting != Some(accept) {
                    self.accepting = Some(accept);
                    out.push(DndAction::Reply(
                        if accept { ACCEPT } else { DECLINE }.to_vec(),
                    ));
                }
                out
            }
            DndEvent::Drop { cell, copy, mimes } => {
                self.offered.clear();
                self.accepting = None;
                let at = mimes.iter().position(|m| m == URI_LIST);
                match at {
                    Some(i) if copy && cell.is_some_and(over_cat) => {
                        let idx = u32::try_from(i + 1).unwrap_or(u32::MAX);
                        self.phase = Phase::Requested(idx);
                        vec![DndAction::Reply(
                            format!("\x1b]72;t=r:x={idx}\x1b\\").into_bytes(),
                        )]
                    }
                    Some(_) => self.end(None, "drop: not on the cat, or not a copy".into()),
                    None => self.end(
                        Some(strings::DROP_NOT_FILES),
                        format!("drop: no {URI_LIST} in {}", mimes.join(" ")),
                    ),
                }
            }
            DndEvent::Data { idx, data } => {
                if self.phase != Phase::Requested(idx) {
                    return Vec::new();
                }
                match data {
                    Err(e) => {
                        let hint = match e {
                            DataError::TooBig => strings::DROP_TOO_MANY,
                            DataError::Garbled => strings::DROP_FAILED,
                        };
                        self.end(
                            Some(hint),
                            format!("drop: the file list was unusable: {e:?}"),
                        )
                    }
                    Ok(bytes) => {
                        let files = uri_list(&String::from_utf8_lossy(&bytes));
                        if files.is_empty() {
                            return self.end(
                                Some(strings::DROP_NOT_FILES),
                                "drop: the file list was empty".into(),
                            );
                        }
                        self.phase = Phase::Reading;
                        vec![DndAction::Files(files)]
                    }
                }
            }
            DndEvent::Error { name, description } => {
                let log = format!(
                    "drop: the terminal ended it: {name}{}",
                    description.map(|d| format!(": {d}")).unwrap_or_default()
                );
                if matches!(self.phase, Phase::Requested(_)) {
                    self.end(Some(error_hint(&name)), log)
                } else {
                    // No drop in progress: nothing ends (the page's rule for
                    // EPERM), and nothing is sent.
                    vec![DndAction::Ended { hint: None, log }]
                }
            }
        }
    }

    /// Completes the drop once its files are read: `t=r:o=1` when any was
    /// taken, else `t=r:o=0`. `None` when no drop was waiting on its files.
    pub(crate) fn finish(&mut self, took: usize) -> Option<Vec<u8>> {
        if self.phase != Phase::Reading {
            return None;
        }
        self.phase = Phase::Idle;
        Some(if took > 0 { DONE } else { CANCELLED }.to_vec())
    }

    /// Ends the drop with nothing taken: `t=r:o=0`, even after the
    /// terminal's own error (D-057), and the cat stops watching.
    fn end(&mut self, hint: Option<&'static str>, log: String) -> Vec<DndAction> {
        self.phase = Phase::Idle;
        vec![
            DndAction::Reply(CANCELLED.to_vec()),
            DndAction::Cat(CatEvent::DragAt(None)),
            DndAction::Ended { hint, log },
        ]
    }
}

/// The hint for a `t=R` error name; any name not listed is a plain failure.
fn error_hint(name: &str) -> &'static str {
    match name {
        "EPERM" => strings::DROP_REFUSED,
        "ENOENT" => strings::DROP_GONE,
        "EMFILE" | "ENOMEM" => strings::DROP_TOO_MANY,
        // EIO, EINVAL, EUNKNOWN and anything else.
        _ => strings::DROP_FAILED,
    }
}

/// The path candidates in a `text/uri-list` (RFC 2483): one URI a line,
/// blank lines and `#` comments skipped. Untrusted: only `file://` URIs,
/// with an empty or `localhost` host, percent-decoded, no NUL, ending in
/// `.pdf`.
pub(crate) fn uri_list(text: &str) -> Vec<PathCandidate> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|uri| {
            let file = uri
                .get(.."file://".len())
                .is_some_and(|s| s.eq_ignore_ascii_case("file://"));
            let path = if file {
                paste::from_file_uri(uri, Style::NATIVE)
            } else {
                Err(strings::DROP_NOT_FILES)
            }
            .and_then(|p| {
                if is_pdf(&p) {
                    Ok(p)
                } else {
                    Err(strings::DROP_NOT_A_PDF)
                }
            });
            let (path, reason) = match path {
                Ok(p) => (Some(p), None),
                Err(why) => (None, Some(why)),
            };
            PathCandidate {
                raw: uri.to_owned(),
                path,
                reason,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
