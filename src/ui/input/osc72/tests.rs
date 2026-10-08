//! Byte fixtures for the OSC 72 receiver (T-31), in the page's minimal
//! framing and in kitty's observed one (FR-r1-b §B), which is tolerated and
//! never required. FR-12's live capture joins these as one more fixture.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;

/// `ESC ] 72 ; meta [; payload] ST`; `bel` ends it with BEL instead.
fn osc(meta: &str, payload: Option<&[u8]>, bel: bool) -> Vec<u8> {
    let mut b = format!("\x1b]72;{meta}").into_bytes();
    if let Some(p) = payload {
        b.push(b';');
        b.extend_from_slice(p);
    }
    b.extend_from_slice(if bel { b"\x07" } else { b"\x1b\\" });
    b
}

fn st(meta: &str, payload: &[u8]) -> Vec<u8> {
    osc(meta, Some(payload), false)
}

/// Everything `bytes` gives, fed whole and then in every chunk size from 1
/// to 7 bytes: the split must not matter.
fn split(bytes: &[u8]) -> Vec<Input> {
    let whole = Splitter::new().feed(bytes);
    for size in 1..=7 {
        let mut s = Splitter::new();
        let parts: Vec<Input> = bytes.chunks(size).flat_map(|c| s.feed(c)).collect();
        assert_eq!(parts, whole, "split into {size}-byte reads");
    }
    whole
}

fn dnd(bytes: &[u8]) -> Vec<DndEvent> {
    split(bytes)
        .into_iter()
        .map(|i| match i {
            Input::Dnd(ev) => ev,
            other => panic!("not a drag event: {other:?}"),
        })
        .collect()
}

fn cell(x: u16, y: u16) -> Option<CellPos> {
    Some(CellPos { x, y })
}

fn mimes(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn key(c: char) -> Input {
    Input::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
}

/// `data` base64-encoded in `raw`-byte pieces, unpadded at the end.
fn b64_chunks(data: &[u8], raw: usize) -> Vec<String> {
    let unpadded = GeneralPurpose::new(
        &alphabet::STANDARD,
        GeneralPurposeConfig::new().with_encode_padding(false),
    );
    data.chunks(raw).map(|c| unpadded.encode(c)).collect()
}

// ── frames: move, leave, drop ────────────────────────────────────────────

#[test]
fn moves_in_the_minimal_and_the_observed_framing() {
    let first = DndEvent::Move {
        cell: cell(40, 12),
        copy: true,
        mimes: Some(mimes(&["text/uri-list", "text/plain"])),
    };
    // Minimal: keys in any order, no `m`, the list space-separated, ST.
    assert_eq!(
        dnd(&st(
            "y=12:t=m:o=1:x=40:X=400:Y=300",
            b"text/uri-list text/plain"
        )),
        std::slice::from_ref(&first)
    );
    // Observed: `m=0` on the move, a trailing space after each type, BEL.
    assert_eq!(
        dnd(&osc(
            "t=m:x=40:y=12:X=400:Y=300:o=1:m=0",
            Some(b"text/uri-list text/plain "),
            true
        )),
        [first]
    );
    // A later move with no list, with and without the second `;`.
    let later = DndEvent::Move {
        cell: cell(41, 12),
        copy: true,
        mimes: None,
    };
    assert_eq!(
        dnd(&st("t=m:x=41:y=12:o=1", b"")),
        std::slice::from_ref(&later)
    );
    assert_eq!(dnd(&osc("t=m:x=41:y=12:o=1", None, false)), [later]);
    // A move-only drag, and an `i=` from a multiplexer.
    assert_eq!(
        dnd(&osc("i=7:t=m:x=0:y=0:o=2", None, false)),
        [DndEvent::Move {
            cell: cell(0, 0),
            copy: false,
            mimes: None
        }]
    );
}

#[test]
fn a_leave_is_any_negative_cell() {
    let left = DndEvent::Move {
        cell: None,
        copy: true,
        mimes: None,
    };
    assert_eq!(dnd(&st("t=m:x=-1:y=-1", b"")), std::slice::from_ref(&left));
    assert_eq!(
        dnd(&osc("t=m:x=-1:y=-1:m=0", None, true)),
        std::slice::from_ref(&left)
    );
    assert_eq!(dnd(&st("t=m:x=-1:y=5:o=3", b"")), [left]);
}

#[test]
fn drops_in_both_framings() {
    let want = DndEvent::Drop {
        cell: cell(50, 20),
        copy: true,
        mimes: mimes(&["text/plain", "text/uri-list"]),
    };
    assert_eq!(
        dnd(&st(
            "t=M:x=50:y=20:X=1:Y=2:o=1",
            b"text/plain text/uri-list"
        )),
        std::slice::from_ref(&want)
    );
    assert_eq!(
        dnd(&osc(
            "o=3:m=0:Y=2:X=1:y=20:x=50:t=M",
            Some(b"text/plain  text/uri-list "),
            true
        )),
        [want]
    );
}

#[test]
fn a_chunked_mime_list_takes_the_first_chunks_metadata() {
    let names: Vec<String> = (0..300)
        .map(|i| format!("application/x-type-{i}"))
        .collect();
    let list = names.join(" ");
    let bytes = list.as_bytes();
    assert!(bytes.len() > PAYLOAD_CAP);
    let (a, b) = bytes.split_at(PAYLOAD_CAP);
    // Minimal: a bare `m=0` continuation with no `t` or `x`.
    let mut stream = st("t=M:x=3:y=4:m=1", a);
    stream.extend(st("m=0", b));
    let want = DndEvent::Drop {
        cell: cell(3, 4),
        copy: true,
        mimes: names.clone(),
    };
    assert_eq!(dnd(&stream), std::slice::from_ref(&want));
    // A chunked move list, the continuation with no metadata but `m`.
    let mut stream = st("t=m:x=3:y=4:m=1", a);
    stream.extend(osc("m=0", Some(b), true));
    assert_eq!(
        dnd(&stream),
        [DndEvent::Move {
            cell: cell(3, 4),
            copy: true,
            mimes: Some(names),
        }]
    );
    // A query reply may come in the middle of a transfer.
    let mut stream = st("t=M:x=3:y=4:m=1", a);
    stream.extend(osc("t=q", None, false));
    stream.extend(st("m=0", b));
    assert_eq!(dnd(&stream), [want]);
}

// ── data ────────────────────────────────────────────────────────────────

/// A `t=r` reply for `data`: chunks of `raw` bytes, `m=1` on each, then the
/// empty `m=0` end. `observed` puts `x=` on every chunk and on the end.
fn data_stream(data: &[u8], raw: usize, observed: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, chunk) in b64_chunks(data, raw).iter().enumerate() {
        let meta = match (i, observed) {
            (0, _) | (_, true) => "t=r:x=2:m=1",
            _ => "m=1",
        };
        out.extend(st(meta, chunk.as_bytes()));
    }
    out.extend(st(if observed { "t=r:x=2:m=0" } else { "m=0" }, b""));
    out
}

#[test]
fn data_chunks_are_joined_then_decoded_once() {
    let mut list = (0..300)
        .map(|i| format!("file:///home/u/scans/batch%20{i:03}/report-{i}.pdf\r\n"))
        .collect::<String>();
    while list.len() % 3 != 1 {
        list.push_str("#\n");
    }
    assert!(list.len() > 3 * 3072, "several chunks");
    let want = [DndEvent::Data {
        idx: Some(2),
        data: Ok(list.as_bytes().to_vec()),
    }];
    // kitty's observed 3,072-byte raw chunks (4,096 encoded), `x=` on every
    // chunk and the end.
    assert_eq!(dnd(&data_stream(list.as_bytes(), 3072, true)), want);
    // Minimal: bare continuations, an unpadded last chunk.
    assert_eq!(list.len() % 3, 1, "the last chunk needs padding it lacks");
    assert_eq!(dnd(&data_stream(list.as_bytes(), 3000, false)), want);
    // Padded, in one chunk, ended by `t=r:x=2` with no `m` and no payload.
    let mut one = st("t=r:x=2", BASE64.encode(b"file:///a.pdf").as_bytes());
    one.extend(osc("t=r:x=2", None, true));
    assert_eq!(
        dnd(&one),
        [DndEvent::Data {
            idx: Some(2),
            data: Ok(b"file:///a.pdf".to_vec())
        }]
    );
}

#[test]
fn data_split_across_reads_and_between_keys() {
    let mut stream = b"ab".to_vec();
    stream.extend(data_stream(b"file:///x/y.pdf\n", 3, false));
    stream.extend(b"c");
    let got = split(&stream);
    assert_eq!(
        got,
        [
            key('a'),
            key('b'),
            Input::Dnd(DndEvent::Data {
                idx: Some(2),
                data: Ok(b"file:///x/y.pdf\n".to_vec())
            }),
            key('c'),
        ]
    );
}

#[test]
fn caps_are_enforced() {
    // A frame over 4,096 bytes of payload is never kept.
    let big = vec![b'A'; PAYLOAD_CAP + 4];
    let mut stream = st("t=r:x=1:m=1", &big);
    stream.extend(st("m=0", b""));
    assert_eq!(
        dnd(&stream),
        [DndEvent::Data {
            idx: Some(1),
            data: Err(DataError::TooBig)
        }]
    );
    // A transfer over 1 MiB: dropped, read to its end, reported once.
    let chunk = vec![b'A'; PAYLOAD_CAP];
    let mut stream = st("t=r:x=1:m=1", &chunk);
    for _ in 0..(TRANSFER_CAP / PAYLOAD_CAP) {
        stream.extend(st("m=1", &chunk));
    }
    stream.extend(st("m=0", b""));
    stream.extend(b"k");
    let got = Splitter::new().feed(&stream);
    assert_eq!(
        got,
        [
            Input::Dnd(DndEvent::Data {
                idx: Some(1),
                data: Err(DataError::TooBig)
            }),
            key('k'),
        ]
    );
    // An over-long OSC is consumed whole: nothing of it is a key.
    let huge = [
        b"\x1b]72;t=m;".as_slice(),
        &vec![b'x'; 3 * keys_cap()],
        b"\x1b\\z",
    ]
    .concat();
    let got = Splitter::new().feed(&huge);
    assert_eq!(
        got,
        [
            Input::Dnd(DndEvent::Move {
                cell: cell(0, 0),
                copy: true,
                mimes: Some(Vec::new())
            }),
            key('z'),
        ]
    );
    // Bad base64 is garbled, not a crash.
    let mut stream = st("t=r:x=1", b"!!!!");
    stream.extend(st("t=r:x=1", b""));
    assert_eq!(
        dnd(&stream),
        [DndEvent::Data {
            idx: Some(1),
            data: Err(DataError::Garbled)
        }]
    );
}

/// A `t=r` chain another frame breaks into is reported as garbled, so the
/// drop it answers still ends; the new frame then counts as usual.
#[test]
fn an_abandoned_data_transfer_is_reported_garbled() {
    let a = BASE64.encode(b"file:///a.pdf");
    // Another `t`, mid-chain.
    let mut stream = st("t=r:x=2:m=1", a.as_bytes());
    stream.extend(st("t=m:x=-1:y=-1", b""));
    assert_eq!(
        dnd(&stream),
        [
            DndEvent::Data {
                idx: Some(2),
                data: Err(DataError::Garbled)
            },
            DndEvent::Move {
                cell: None,
                copy: true,
                mimes: None
            },
        ]
    );
    // Another `x`, mid-chain: the new transfer goes on.
    let mut stream = st("t=r:x=2:m=1", a.as_bytes());
    stream.extend(st("t=r:x=3", a.as_bytes()));
    stream.extend(st("t=r:x=3", b""));
    assert_eq!(
        dnd(&stream),
        [
            DndEvent::Data {
                idx: Some(2),
                data: Err(DataError::Garbled)
            },
            DndEvent::Data {
                idx: Some(3),
                data: Ok(b"file:///a.pdf".to_vec())
            },
        ]
    );
    // Any other transfer abandoned says nothing.
    let mut stream = st("t=M:m=1", b"text/uri-");
    stream.extend(st("t=m:x=1:y=1", b""));
    assert_eq!(
        dnd(&stream),
        [DndEvent::Move {
            cell: cell(1, 1),
            copy: true,
            mimes: None
        }]
    );
}

/// A first data chunk with no `x` has no index, rather than index 0.
#[test]
fn data_with_no_index_says_so() {
    let mut stream = st("t=r:m=1", BASE64.encode(b"file:///a.pdf").as_bytes());
    stream.extend(st("m=0", b""));
    assert_eq!(
        dnd(&stream),
        [DndEvent::Data {
            idx: None,
            data: Ok(b"file:///a.pdf".to_vec())
        }]
    );
}

fn keys_cap() -> usize {
    crate::ui::input::keys::OSC_CAP
}

// ── errors ──────────────────────────────────────────────────────────────

#[test]
fn every_error_name_and_an_unknown_one() {
    for name in [
        "EPERM", "ENOENT", "EIO", "EINVAL", "EMFILE", "ENOMEM", "EUNKNOWN", "EXDEV",
    ] {
        assert_eq!(
            dnd(&st("t=R:x=1", name.as_bytes())),
            [DndEvent::Error {
                name: name.into(),
                description: None
            }],
            "{name}"
        );
    }
    assert_eq!(
        dnd(&osc(
            "t=R:x=1:m=0",
            Some("EIO:disk said no: 5 ä".as_bytes()),
            true
        )),
        [DndEvent::Error {
            name: "EIO".into(),
            description: Some("disk said no: 5 ä".into())
        }]
    );
    // A description with a control character (ESC and BEL cannot be in a
    // payload at all: they end the frame), or not UTF-8, is dropped; a name
    // that is not one becomes EUNKNOWN.
    for (payload, name, description) in [
        (b"EIO:bad\x01text".as_slice(), "EIO", None),
        (b"EIO:del\x7f", "EIO", None),
        (b"EIO:\xff\xfe", "EIO", None),
        ("EIO:c1 \u{9b} here".as_bytes(), "EIO", None),
        (b"no such:x", "EUNKNOWN", Some("x")),
        (b"E\xc3\xa9:x", "EUNKNOWN", Some("x")),
        (b"", "EUNKNOWN", None),
    ] {
        assert_eq!(
            dnd(&st("t=R", payload)),
            [DndEvent::Error {
                name: name.into(),
                description: description.map(str::to_owned)
            }],
            "{payload:?}"
        );
    }
}

// ── the splitter: no OSC byte is ever a key ─────────────────────────────

#[test]
fn unparsed_osc_bytes_are_never_keys() {
    let mut stream = b"a".to_vec();
    stream.extend(b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\"); // another OSC
    stream.extend(b"\x1b]72\x07"); // not committed: Alt+] and keys? no: BEL
    stream.extend(osc("t=z:x=1", Some(b"payload"), false)); // unknown type
    stream.extend(b"\x1b]72;t=m:x=\xff;x\x07"); // metadata not UTF-8
    stream.extend(st("m=1", b"orphan continuation"));
    stream.extend(osc("t=q", Some(b"late=1"), false)); // a late query reply
    stream.extend(b"\x1b[?62;22c"); // a late DA1 reply
    stream.extend(b"b");
    let got = split(&stream);
    let keys: Vec<&Input> = got.iter().filter(|i| !matches!(i, Input::Dnd(_))).collect();
    // `ESC ] 72 BEL` never committed to an OSC: it is Alt+], 7, 2, Ctrl+G,
    // which is what those bytes type. Everything committed is no key.
    assert_eq!(
        keys,
        [
            &key('a'),
            &Input::Key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::ALT)),
            &key('7'),
            &key('2'),
            &Input::Key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL)),
            &key('b'),
        ]
    );
    assert!(got.iter().all(|i| !matches!(i, Input::Dnd(_))), "{got:?}");
}

#[test]
fn a_frame_interleaved_with_keys_and_a_paste() {
    let mut stream = b"q".to_vec();
    stream.extend(st("t=m:x=1:y=2:o=1", b"text/uri-list"));
    stream.extend(b"\x1b[A\x1b[200~/tmp/a.pdf\x1b[201~");
    stream.extend(osc("t=m:x=-1:y=-1", None, true));
    stream.extend(b"\x1b");
    let mut s = Splitter::new();
    let mut got = s.feed(&stream);
    got.extend(s.idle());
    assert_eq!(
        got,
        [
            key('q'),
            Input::Dnd(DndEvent::Move {
                cell: cell(1, 2),
                copy: true,
                mimes: Some(mimes(&["text/uri-list"]))
            }),
            Input::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            Input::Paste("/tmp/a.pdf".into()),
            Input::Dnd(DndEvent::Move {
                cell: None,
                copy: true,
                mimes: None
            }),
            Input::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        ]
    );
}

// ── the uri list ────────────────────────────────────────────────────────

#[test]
fn the_uri_list_keeps_local_pdfs_and_refuses_the_rest() {
    let list = "# a comment\r\n\
                file:///home/u/My%20Scans/r%C3%A9sum%C3%A9.pdf\r\n\
                \r\n\
                FILE://localhost/tmp/b.PDF\r\n\
                file://evil.example/share/c.pdf\r\n\
                file:///etc/passwd\r\n\
                file:///tmp/nul%00byte.pdf\r\n\
                https://example.com/d.pdf\r\n\
                file:/tmp/no-authority.pdf\r\n\
                file:///tmp/%FF.pdf\n";
    let got = uri_list(list);
    let outcome: Vec<(Option<PathBuf>, Option<&str>)> =
        got.iter().map(|c| (c.path.clone(), c.reason)).collect();
    let ok = |p: &str| (Some(PathBuf::from(p)), None);
    let no = |why: &'static str| (None, Some(why));
    #[cfg(unix)]
    let raw_ff = {
        use std::os::unix::ffi::OsStrExt;
        (
            Some(PathBuf::from(std::ffi::OsStr::from_bytes(b"/tmp/\xff.pdf"))),
            None,
        )
    };
    #[cfg(not(unix))]
    let raw_ff = no(strings::DROP_GARBLED);
    assert_eq!(
        outcome,
        [
            ok("/home/u/My Scans/résumé.pdf"),
            ok("/tmp/b.PDF"),
            no(strings::DROP_NOT_LOCAL),
            no(strings::DROP_NOT_A_PDF),
            no(strings::DROP_GARBLED),
            no(strings::DROP_NOT_FILES),
            no(strings::DROP_NOT_FILES),
            raw_ff,
        ]
    );
    assert_eq!(got[2].raw, "file://evil.example/share/c.pdf");
    assert!(uri_list("# only a comment\r\n\r\n").is_empty());
}

// ── the handshake ───────────────────────────────────────────────────────

fn handshake(reads: &[&[u8]]) -> (HandshakeResult, bool) {
    let mut h = Handshake::new();
    for r in reads {
        h.feed(r);
    }
    (h.result(), h.done())
}

const DA1_REPLY: &[u8] = b"\x1b[?62;22;52c";

#[test]
fn the_query_reply_before_da1_means_kitty() {
    let q = b"\x1b]72;t=q\x1b\\";
    let q_semi = b"\x1b]72;t=q;\x1b\\";
    let q_bel_flags = b"\x1b]72;i=0:t=q;future=1:more=2\x07";
    for reply in [q.as_slice(), q_semi, q_bel_flags] {
        let (r, done) = handshake(&[reply, DA1_REPLY]);
        assert!(done);
        assert_eq!(
            r,
            HandshakeResult {
                kitty: true,
                discarded: 0
            },
            "{reply:?}"
        );
        // Split byte by byte, the same.
        let all = [reply, DA1_REPLY].concat();
        let bytes: Vec<&[u8]> = all.chunks(1).collect();
        assert_eq!(handshake(&bytes).0, r);
    }
}

#[test]
fn da1_first_means_not_kitty() {
    let (r, done) = handshake(&[DA1_REPLY, b"\x1b]72;t=q\x1b\\"]);
    assert!(done);
    assert_eq!(
        r,
        HandshakeResult {
            kitty: false,
            discarded: 10
        },
        "a late query reply is discarded"
    );
    assert_eq!(
        handshake(&[DA1_REPLY]).0,
        HandshakeResult {
            kitty: false,
            discarded: 0
        }
    );
}

#[test]
fn a_timeout_decides_on_what_came() {
    // Nothing at all.
    let (r, done) = handshake(&[]);
    assert!(!done);
    assert_eq!(r, HandshakeResult::default());
    // The query reply and no DA1 within the window: kitty answered first.
    let (r, done) = handshake(&[b"\x1b]72;t=q\x1b\\"]);
    assert!(!done);
    assert!(r.kitty);
    // A reply cut off by the window is not a reply; its bytes are counted.
    let (r, _) = handshake(&[b"\x1b]72;t="]);
    assert_eq!(
        r,
        HandshakeResult {
            kitty: false,
            discarded: 7
        }
    );
}

#[test]
fn bytes_in_the_window_are_discarded_and_counted() {
    let paste: &[u8] = b"\x1b[200~/tmp/x.pdf\x1b]72;t=q\x1b\\\x1b[201~";
    let typed: &[u8] = b"ab\x1b[A";
    let other_osc: &[u8] = b"\x1b]11;rgb:0/0/0\x07";
    // kitty: everything but the two replies is counted, and a fake query
    // reply inside a paste is part of the paste.
    let (r, _) = handshake(&[
        typed,
        paste,
        b"\x1b]72;t=q\x1b\\",
        other_osc,
        DA1_REPLY,
        b"z",
    ]);
    assert_eq!(
        r,
        HandshakeResult {
            kitty: true,
            discarded: typed.len() + paste.len() + other_osc.len() + 1
        }
    );
    // Not kitty: the paste's fake reply does not make it kitty.
    let (r, _) = handshake(&[paste, DA1_REPLY]);
    assert_eq!(
        r,
        HandshakeResult {
            kitty: false,
            discarded: paste.len()
        }
    );
}

// ── the session: replies, the cat, the drop's end ──────────────────────

/// A mock terminal: the session's replies, in order, as text.
#[derive(Default)]
struct Mock {
    session: DndSession,
    wire: Vec<String>,
    cat: Vec<CatEvent>,
    hints: Vec<Option<&'static str>>,
    files: Vec<Vec<PathCandidate>>,
}

impl Mock {
    /// `ev` with the cat at columns 10–19; a drop's files are taken when
    /// `take` says how many.
    fn on(&mut self, ev: DndEvent, take: usize) {
        let over = |c: CellPos| (10..20).contains(&c.x);
        for a in self.session.on(ev, &over) {
            match a {
                DndAction::Reply(b) => self.wire.push(String::from_utf8(b).unwrap()),
                DndAction::Cat(c) => self.cat.push(c),
                DndAction::Ended { hint, .. } => self.hints.push(hint),
                DndAction::Files(f) => {
                    self.files.push(f);
                    let done = self
                        .session
                        .finish(take)
                        .expect("a drop waits on its files");
                    self.wire.push(String::from_utf8(done).unwrap());
                }
            }
        }
    }
}

fn mv(x: u16, list: Option<&[&str]>) -> DndEvent {
    DndEvent::Move {
        cell: cell(x, 5),
        copy: true,
        mimes: list.map(mimes),
    }
}

fn drop_at(x: u16) -> DndEvent {
    DndEvent::Drop {
        cell: cell(x, 5),
        copy: true,
        mimes: mimes(&["text/plain", "text/uri-list"]),
    }
}

fn data(list: &str) -> DndEvent {
    DndEvent::Data {
        idx: Some(2),
        data: Ok(list.as_bytes().to_vec()),
    }
}

const ACCEPT_S: &str = "\x1b]72;t=m:o=1;text/uri-list\x1b\\";
const DECLINE_S: &str = "\x1b]72;t=m:o=0\x1b\\";
const REQUEST_S: &str = "\x1b]72;t=r:x=2\x1b\\";
const DONE_S: &str = "\x1b]72;t=r:o=1\x1b\\";
const CANCEL_S: &str = "\x1b]72;t=r:o=0\x1b\\";

#[test]
fn moves_are_answered_when_the_answer_changes() {
    let mut m = Mock::default();
    let list: &[&str] = &["text/uri-list", "text/plain"];
    m.on(mv(2, Some(list)), 0); // off the cat: declined
    m.on(mv(3, None), 0); // same answer: nothing sent
    m.on(mv(12, None), 0); // onto the cat: accepted
    m.on(mv(13, None), 0);
    m.on(mv(25, None), 0); // off again
    m.on(mv(14, Some(&["text/plain"])), 0); // the list changed: no files
    assert_eq!(m.wire, [DECLINE_S, ACCEPT_S, DECLINE_S]);
    assert_eq!(m.cat.len(), 6);
    assert_eq!(m.cat[2], CatEvent::DragAt(cell(12, 5)));

    // The first move of each drag is answered, even with the same answer.
    let mut m = Mock::default();
    m.on(mv(12, Some(list)), 0);
    m.on(
        DndEvent::Move {
            cell: None,
            copy: true,
            mimes: None,
        },
        0,
    );
    m.on(mv(12, Some(list)), 0);
    assert_eq!(m.wire, [ACCEPT_S, ACCEPT_S]);
    assert_eq!(m.cat[1], CatEvent::DragAt(None));

    // A drag that only allows a move is never accepted: the source would
    // delete the original.
    let mut m = Mock::default();
    m.on(
        DndEvent::Move {
            cell: cell(12, 5),
            copy: false,
            mimes: Some(mimes(list)),
        },
        0,
    );
    assert_eq!(m.wire, [DECLINE_S]);
}

#[test]
fn a_drop_on_the_cat_requests_the_list_then_completes() {
    let mut m = Mock::default();
    m.on(mv(12, Some(&["text/plain", "text/uri-list"])), 0);
    m.on(drop_at(12), 1);
    assert_eq!(m.wire, [ACCEPT_S, REQUEST_S]);
    m.on(data("file:///tmp/a.pdf\r\n"), 1);
    assert_eq!(m.wire, [ACCEPT_S, REQUEST_S, DONE_S]);
    assert_eq!(m.files[0][0].path, Some(PathBuf::from("/tmp/a.pdf")));
    // Data that nobody asked for (now, or for another index) is ignored.
    m.on(data("file:///tmp/b.pdf"), 1);
    m.on(
        DndEvent::Data {
            idx: Some(9),
            data: Ok(Vec::new()),
        },
        1,
    );
    assert_eq!(m.wire.len(), 3);
    assert_eq!(m.files.len(), 1);
}

/// The requested list's first chunk, then a leave breaking into the chain.
fn abandoned_data() -> Vec<u8> {
    let mut stream = st("t=r:x=2:m=1", BASE64.encode(b"file:///a.pdf").as_bytes());
    stream.extend(st("t=m:x=-1:y=-1", b""));
    stream
}

#[test]
fn every_way_a_drop_ends_sends_its_completion() {
    struct Case {
        name: &'static str,
        events: Vec<DndEvent>,
        take: usize,
        wire: Vec<&'static str>,
        hint: Option<&'static str>,
    }
    let cases = vec![
        Case {
            name: "files taken",
            events: vec![drop_at(12), data("file:///tmp/a.pdf")],
            take: 1,
            wire: vec![REQUEST_S, DONE_S],
            hint: None,
        },
        Case {
            name: "no file taken",
            events: vec![drop_at(12), data("file:///tmp/a.txt")],
            take: 0,
            wire: vec![REQUEST_S, CANCEL_S],
            hint: None,
        },
        Case {
            name: "an empty list",
            events: vec![drop_at(12), data("# nothing\r\n")],
            take: 0,
            wire: vec![REQUEST_S, CANCEL_S],
            hint: Some(strings::DROP_NOT_FILES),
        },
        Case {
            name: "over the cap",
            events: vec![
                drop_at(12),
                DndEvent::Data {
                    idx: Some(2),
                    data: Err(DataError::TooBig),
                },
            ],
            take: 0,
            wire: vec![REQUEST_S, CANCEL_S],
            hint: Some(strings::DROP_TOO_MANY),
        },
        Case {
            name: "garbled",
            events: vec![
                drop_at(12),
                DndEvent::Data {
                    idx: Some(2),
                    data: Err(DataError::Garbled),
                },
            ],
            take: 0,
            wire: vec![REQUEST_S, CANCEL_S],
            hint: Some(strings::DROP_FAILED),
        },
        Case {
            name: "data with no index",
            events: vec![
                drop_at(12),
                DndEvent::Data {
                    idx: None,
                    data: Ok(b"file:///tmp/a.pdf".to_vec()),
                },
            ],
            take: 1,
            wire: vec![REQUEST_S, DONE_S],
            hint: None,
        },
        Case {
            name: "the data transfer abandoned mid-chain",
            events: [vec![drop_at(12)], dnd(&abandoned_data())].concat(),
            take: 0,
            wire: vec![REQUEST_S, CANCEL_S],
            hint: Some(strings::DROP_FAILED),
        },
        Case {
            name: "the terminal's error (t=R)",
            events: vec![
                drop_at(12),
                DndEvent::Error {
                    name: "EPERM".into(),
                    description: Some("same window".into()),
                },
            ],
            take: 0,
            wire: vec![REQUEST_S, CANCEL_S],
            hint: Some(strings::DROP_REFUSED),
        },
        Case {
            name: "off the cat",
            events: vec![drop_at(30)],
            take: 0,
            wire: vec![CANCEL_S],
            hint: None,
        },
        Case {
            name: "a move-only drop",
            events: vec![DndEvent::Drop {
                cell: cell(12, 5),
                copy: false,
                mimes: mimes(&["text/uri-list"]),
            }],
            take: 0,
            wire: vec![CANCEL_S],
            hint: None,
        },
        Case {
            name: "no files in the drop",
            events: vec![DndEvent::Drop {
                cell: cell(12, 5),
                copy: true,
                mimes: mimes(&["text/plain"]),
            }],
            take: 0,
            wire: vec![CANCEL_S],
            hint: Some(strings::DROP_NOT_FILES),
        },
    ];
    for c in cases {
        let mut m = Mock::default();
        for ev in c.events {
            m.on(ev, c.take);
        }
        assert_eq!(m.wire, c.wire, "{}", c.name);
        assert_eq!(m.hints.last().copied().flatten(), c.hint, "{}", c.name);
        if c.wire.last() == Some(&CANCEL_S) && m.files.is_empty() {
            assert_eq!(
                m.cat.last(),
                Some(&CatEvent::DragAt(None)),
                "{}: the cat stops watching",
                c.name
            );
        }
    }
}

#[test]
fn every_error_name_ends_the_drop_with_a_hint() {
    for (name, hint) in [
        ("EPERM", strings::DROP_REFUSED),
        ("ENOENT", strings::DROP_GONE),
        ("EIO", strings::DROP_FAILED),
        ("EINVAL", strings::DROP_FAILED),
        ("EMFILE", strings::DROP_TOO_MANY),
        ("ENOMEM", strings::DROP_TOO_MANY),
        ("EUNKNOWN", strings::DROP_FAILED),
        ("EWHATEVER", strings::DROP_FAILED),
    ] {
        let mut m = Mock::default();
        m.on(drop_at(12), 0);
        m.on(
            DndEvent::Error {
                name: name.into(),
                description: None,
            },
            0,
        );
        assert_eq!(m.wire, [REQUEST_S, CANCEL_S], "{name}");
        assert_eq!(m.hints, [Some(hint)], "{name}");
    }
    // An error with no drop in progress ends nothing and sends nothing.
    let mut m = Mock::default();
    m.on(
        DndEvent::Error {
            name: "EPERM".into(),
            description: None,
        },
        0,
    );
    assert!(m.wire.is_empty());
    assert_eq!(m.hints, [None]);
    assert!(m.session.finish(1).is_none(), "no drop is waiting");
}
