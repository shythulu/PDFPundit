//! The C9 corruptor (T-03b): the log is the diff, every site is what it
//! claims, and the counts follow REPDF's measured distribution. Sites are
//! re-derived here from lopdf's view of the file and a separate walk over the
//! object's bytes, not through the corruptor's own scanner.

use super::*;

use lopdf::xref::XrefEntry;

const C9: CorruptionClass = CorruptionClass::C9ZlibTampered;
const FORBIDDEN: [&[u8]; 5] = [b"Root", b"Pages", b"Kids", b"Count", b"Type"];

/// Every fixture C9 is run over here: the goldens, the ObjStm golden, one with
/// a bare-number object (an indirect `/Length`) and one with an ASCII85 stage.
fn inputs() -> Vec<(&'static str, Vec<u8>)> {
    let mut v: Vec<_> = goldens().into_iter().collect();
    v.push(("golden_pdf_objstm", golden_pdf_objstm()));
    v.push((
        "with_wrong_length(Indirect)",
        with_wrong_length(LengthKind::Indirect),
    ));
    v.push(("with_multi_filter", with_multi_filter()));
    v
}

struct Body {
    data: Range<usize>,
    flate: bool,
    container: bool,
}

fn first_filter_is_flate(dict: &Dictionary) -> bool {
    match dict.get(b"Filter") {
        Ok(Object::Name(n)) => n == b"FlateDecode",
        Ok(Object::Array(a)) => a.first() == Some(&name(b"FlateDecode")),
        _ => false,
    }
}

/// Every top-level stream's data, from lopdf's parse.
fn bodies(pdf: &[u8], doc: &Document) -> Vec<Body> {
    doc.objects
        .iter()
        .filter_map(|(&(id, _), object)| {
            let s = object.as_stream().ok()?;
            Some(Body {
                data: data_range(pdf, doc, id),
                flate: first_filter_is_flate(&s.dict),
                container: s.dict.has_type(b"XRef") || s.dict.has_type(b"ObjStm"),
            })
        })
        .collect()
}

/// The top-level object whose header is the last one at or before `at`.
fn enclosing_object(doc: &Document, at: usize) -> (u32, usize) {
    doc.reference_table
        .entries
        .iter()
        .filter_map(|(&id, e)| match e {
            XrefEntry::Normal { offset, .. } if (*offset as usize) <= at => {
                Some((id, *offset as usize))
            }
            _ => None,
        })
        .max_by_key(|&(_, offset)| offset)
        .expect("an object before the site")
}

#[derive(Debug, Clone, Default)]
struct Level {
    dict: bool,
    /// The dictionary key this container is the value of (inherited by nested
    /// arrays).
    of_key: Vec<u8>,
    /// Dictionaries: the last key read, and whether the last token was a key.
    key: Vec<u8>,
    after_key: bool,
}

/// The containers open around `at` in the object at `start`, outermost first,
/// and whether the token at `at` is a dictionary key. A name is a key exactly
/// when the token before it at that level was not a key.
fn where_is(pdf: &[u8], start: usize, at: usize) -> (Vec<Level>, bool) {
    let mut i = start + find(&pdf[start..], b" obj").unwrap() + 4;
    let mut stack: Vec<Level> = Vec::new();
    loop {
        let b = pdf[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let begin = i;
        let (end, kind) = match b {
            b'<' if pdf[i + 1] == b'<' => (i + 2, b'D'),
            b'>' if pdf[i + 1] == b'>' => (i + 2, b'd'),
            b'[' => (i + 1, b'A'),
            b']' => (i + 1, b'a'),
            b'(' => {
                let (mut depth, mut j) = (0, i);
                loop {
                    match pdf[j] {
                        b'\\' => j += 1,
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                (j + 1, b'v')
            }
            b'<' => (i + find(&pdf[i..], b">").unwrap() + 1, b'v'),
            b'/' => {
                let n = pdf[i + 1..]
                    .iter()
                    .position(|&c| is_delimiter_or_space(c))
                    .unwrap();
                (i + 1 + n, b'n')
            }
            _ => {
                let n = pdf[i..]
                    .iter()
                    .position(|&c| is_delimiter_or_space(c))
                    .unwrap();
                (i + n.max(1), b'v')
            }
        };
        let word = &pdf[begin..end];
        assert!(
            !(stack.is_empty() && (word == b"stream" || word == b"endobj")),
            "site {at} is past the object body at {start}"
        );
        let is_key = kind == b'n' && stack.last().is_some_and(|l| l.dict && !l.after_key);
        if (begin..end).contains(&at) {
            return (stack, is_key);
        }
        match kind {
            b'D' | b'A' => {
                let of_key = match stack.last_mut() {
                    Some(l) if l.dict => {
                        l.after_key = false;
                        l.key.clone()
                    }
                    Some(l) => l.of_key.clone(),
                    None => Vec::new(),
                };
                stack.push(Level {
                    dict: kind == b'D',
                    of_key,
                    ..Level::default()
                });
            }
            b'd' | b'a' => {
                stack.pop();
            }
            _ if is_key => {
                let l = stack.last_mut().unwrap();
                l.key = pdf[begin + 1..end].to_vec();
                l.after_key = true;
            }
            _ => {
                if let Some(l) = stack.last_mut() {
                    l.after_key = false;
                }
            }
        }
        i = end;
    }
}

fn assert_site(label: &str, g: &[u8], doc: &Document, bodies: &[Body], rep: &Replacement) {
    let at = rep.at;
    let body = bodies.iter().find(|b| b.data.contains(&at));
    match rep.site {
        ReplacementSite::FlateBody | ReplacementSite::OtherBody => {
            let body = body.unwrap_or_else(|| panic!("{label}: {at} is in no stream body"));
            assert!(!body.container, "{label}: {at} is in an ObjStm or XRef");
            let flate = rep.site == ReplacementSite::FlateBody;
            assert_eq!(body.flate, flate, "{label}: {at} is {:?}", rep.site);
        }
        site => {
            assert!(body.is_none(), "{label}: {at} ({site:?}) is in stream data");
            let (id, start) = enclosing_object(doc, at);
            let object = doc.get_object((id, 0)).unwrap();
            if let Ok(s) = object.as_stream() {
                assert!(
                    !s.dict.has_type(b"XRef") && !s.dict.has_type(b"ObjStm"),
                    "{label}: {at} is in container {id}"
                );
            }
            let (levels, is_key) = where_is(g, start, at);
            assert!(!is_key, "{label}: {at} is a dictionary key");
            for l in &levels {
                assert!(!FORBIDDEN.contains(&l.of_key.as_slice()), "{label}: {at}");
            }
            match site {
                ReplacementSite::Array => {
                    let inner = levels.last().expect("an open array");
                    assert!(!inner.dict, "{label}: {at} is not in an array");
                    // Width arrays first: every fixture here has a /W.
                    assert_eq!(inner.of_key, b"W", "{label}: {at} is not in /W");
                }
                ReplacementSite::DictValue => {
                    let inner = levels.last().expect("an open dictionary");
                    assert!(inner.dict, "{label}: {at} is not in a dictionary");
                    assert!(
                        !FORBIDDEN.contains(&inner.key.as_slice()),
                        "{label}: {at} is the value of /{}",
                        String::from_utf8_lossy(&inner.key)
                    );
                }
                ReplacementSite::Number => {
                    assert!(levels.is_empty(), "{label}: {at} is in a container");
                    assert!(
                        matches!(object, Object::Integer(_) | Object::Real(_)),
                        "{label}: object {id} is not a bare number"
                    );
                }
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn c9_log_is_the_diff_and_every_site_is_right() {
    for (label, g) in inputs() {
        let doc = load(&g);
        let bodies = bodies(&g, &doc);
        let mut sites = std::collections::BTreeSet::new();
        for seed in 0..48 {
            let (out, log) = corrupt_with_log(C9, &g, seed);
            assert_eq!(out, corrupt(C9, &g, seed), "{label} seed {seed}");
            assert_eq!(out.len(), g.len(), "{label}: C9 replaces, never deletes");
            let diff: Vec<usize> = (0..g.len()).filter(|&i| g[i] != out[i]).collect();
            let logged: Vec<usize> = log.iter().map(|r| r.at).collect();
            assert_eq!(diff, logged, "{label} seed {seed}");
            for rep in &log {
                assert_eq!(rep.from, g[rep.at], "{label} seed {seed}");
                assert_eq!(rep.to, out[rep.at], "{label} seed {seed}");
                assert_ne!(rep.from, rep.to, "{label} seed {seed}");
                assert_site(label, &g, &doc, &bodies, rep);
                sites.insert(rep.site);
            }
            let flate_hits = log
                .iter()
                .filter(|r| r.site == ReplacementSite::FlateBody)
                .count();
            assert!(flate_hits >= 1, "{label} seed {seed}: no Flate stream hit");
            for b in bodies.iter().filter(|b| b.flate) {
                let n = logged.iter().filter(|&&at| b.data.contains(&at)).count();
                assert!(n <= 1, "{label} seed {seed}: {n} hits in one Flate stream");
            }
        }
        assert!(
            sites.contains(&ReplacementSite::OtherBody)
                && sites.contains(&ReplacementSite::DictValue),
            "{label}: 48 seeds only hit {sites:?}"
        );
        // The ObjStm golden packs its only arrays (the /W among them).
        let packs_arrays = label == "golden_pdf_objstm";
        assert_eq!(
            sites.contains(&ReplacementSite::Array),
            !packs_arrays,
            "{label}: {sites:?}"
        );
        if label.starts_with("with_wrong_length") {
            assert!(
                sites.contains(&ReplacementSite::Number),
                "{label}: {sites:?}"
            );
        }
    }
}

#[test]
fn c9_reaches_every_site_kind() {
    let mut sites = std::collections::BTreeSet::new();
    let g = with_wrong_length(LengthKind::Indirect);
    for seed in 0..256 {
        sites.extend(corrupt_with_log(C9, &g, seed).1.iter().map(|r| r.site));
    }
    assert_eq!(sites.len(), 5, "{sites:?}");
}

#[test]
fn c9_follows_the_measured_distribution() {
    // OBS-0005 / OBS-0302: 1,445 of 3,429 Flate streams hit once each (42%),
    // 304 non-Flate body bytes per 1,445 Flate hits, about one outside-stream
    // replacement per three stream hits.
    let g = golden_pdf();
    let doc = load(&g);
    let n_flate = bodies(&g, &doc).iter().filter(|b| b.flate).count();
    assert_eq!(n_flate, 4, "two contents, FontFile2, ToUnicode");
    let seeds = 2_000u64;
    let (mut flate, mut other, mut outside) = (0usize, 0usize, 0usize);
    let mut deltas = std::collections::BTreeSet::new();
    for seed in 0..seeds {
        for rep in corrupt_with_log(C9, &g, seed).1 {
            match rep.site {
                ReplacementSite::FlateBody => flate += 1,
                ReplacementSite::OtherBody => other += 1,
                _ => outside += 1,
            }
            deltas.insert(rep.to.wrapping_sub(rep.from));
        }
    }
    let share = flate as f64 / (seeds as f64 * n_flate as f64);
    assert!((0.40..=0.44).contains(&share), "Flate share {share}");
    let per_flate = other as f64 / flate as f64;
    assert!(
        (0.17..=0.25).contains(&per_flate),
        "other per Flate {per_flate}"
    );
    let per_hit = outside as f64 / (flate + other) as f64;
    assert!(
        (0.30..=0.37).contains(&per_hit),
        "outside per stream hit {per_hit}"
    );
    // Each replacement is one of the 255 other values.
    assert!(!deltas.contains(&0));
    assert_eq!(deltas.len(), 255, "{} deltas seen", deltas.len());
}

#[test]
fn c9_is_deterministic_per_seed_and_moves_with_it() {
    for (label, g) in inputs() {
        let mut outs = std::collections::BTreeSet::new();
        for seed in SEEDS {
            let a = corrupt_with_log(C9, &g, seed);
            assert_eq!(a, corrupt_with_log(C9, &g, seed), "{label} seed {seed}");
            outs.insert(a.0);
        }
        assert!(
            outs.len() > SEEDS.count() / 2,
            "{label}: the seed barely matters"
        );
    }
}

#[test]
fn other_classes_log_nothing() {
    let g = golden_pdf();
    for class in CorruptionClass::ALL {
        if class == C9 {
            continue;
        }
        let (out, log) = corrupt_with_log(class, &g, 3);
        assert_eq!(out, corrupt(class, &g, 3), "{}", class.code());
        assert!(log.is_empty(), "{}", class.code());
    }
}
