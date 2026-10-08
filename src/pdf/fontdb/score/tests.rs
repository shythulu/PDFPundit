use super::*;

use lopdf::{Document, Object};
use sha2::{Digest, Sha256};

use crate::engine::{FontResolutionKind, ToUnicodeState};
use crate::pdf::fixtures::{GOLDEN_TEXT, TEST_FONT, golden_pdf};
use crate::pdf::fontdb::build::build_from_ttf;
use crate::pdf::fontdb::dict::FrequencyList;
use crate::pdf::fontdb::gmap::{self, GmapRecord};
use crate::pdf::streams::content_ops;

/// The id of the test font's entry, and of its shuffled twin.
const RIGHT: &str = "NotoSans-Regular";
const SHUFFLED: &str = "Shuffled-Regular";

/// A hand-written 50-word English list in FR-05's format: the golden's first
/// page and common words.
const ENGLISH: &str = "the 50\nof 49\nand 48\nto 47\na 46\nin 45\nis 44\nit 43\nyou 42\n\
that 41\nhe 40\nwas 39\nfor 38\non 37\nare 36\nwith 35\nas 34\nhis 33\nthey 32\nbe 31\n\
at 30\none 29\nhave 28\nthis 27\nfrom 26\nor 25\nhad 24\nby 23\nword 22\nbut 21\n\
what 20\nsome 19\nwe 18\ncan 17\nout 16\nother 15\nwere 14\nall 13\nthere 12\n\
when 11\npage 10\ngolden 9\npdfpundit 8\nquick 7\nbrown 6\nfox 5\njumps 4\nover 3\n\
lazy 2\ndog 1\n";

fn english() -> FrequencyList {
    let list = FrequencyList::from_bytes(Lang::En, ENGLISH.as_bytes()).unwrap();
    assert_eq!(list.len(), 50);
    list
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The test font's `.gmap` with every record's code point moved seven
/// records on: the same glyphs, read as the wrong characters.
fn shuffled_gmap(right: &[u8]) -> Vec<u8> {
    let table = GmapTable::new(right).unwrap();
    let records = table.records();
    let n = records.len();
    let out = (0..n)
        .map(|i| {
            let r = records[i];
            let u = records[(i + 7) % n].unicode();
            GmapRecord::new(r.gid(), u, r.width(), r.source())
        })
        .collect();
    gmap::encode(out)
}

/// The two-font test database: the test font through `build_from_ttf`, and
/// the same program with a shuffled `.gmap`.
fn test_db() -> FontDb {
    test_db_with(|_| {})
}

fn test_db_with(edit: impl FnOnce(&mut Vec<IndexEntry>)) -> FontDb {
    let (right, right_gmap) = build_from_ttf(TEST_FONT).unwrap();
    assert_eq!(right.id, RIGHT);
    let wrong_gmap = shuffled_gmap(&right_gmap);
    let mut wrong = right.clone();
    wrong.id = SHUFFLED.into();
    wrong.family = "Shuffled".into();
    wrong.gmap_sha256 = hex(&wrong_gmap);
    let mut entries = vec![right, wrong];
    edit(&mut entries);
    let index = serde_json::to_vec(&entries).unwrap();
    let blob = |name: &str| -> Option<&[u8]> {
        match name {
            "NotoSans-Regular.ttf" | "Shuffled-Regular.ttf" | "Arabic-Only.ttf" => Some(TEST_FONT),
            "NotoSans-Regular.gmap" | "Arabic-Only.gmap" => Some(&right_gmap),
            "Shuffled-Regular.gmap" => Some(&wrong_gmap),
            _ => None,
        }
    };
    FontDb::from_bytes(&index, &blob).expect("test db loads")
}

fn run_infer(
    db: &FontDb,
    codes: &[CodeRun],
    dicts: &[&dyn Dictionary],
    tounicode: Option<&dyn Dictionary>,
) -> (Vec<Candidate>, InferenceTrace) {
    let gmaps = db.gmaps();
    let lookup = |id: &str| gmaps.iter().find(|(i, _)| *i == id).map(|(_, g)| g);
    infer(codes, &db.entries(), &lookup, dicts, tounicode)
}

/// The test font's glyph for `c`.
fn gid(c: char) -> u16 {
    let (_, bytes) = build_from_ttf(TEST_FONT).unwrap();
    let table = GmapTable::new(&bytes).unwrap();
    table
        .records()
        .iter()
        .find(|r| r.unicode() == c)
        .map(GmapRecord::gid)
        .unwrap_or_else(|| panic!("test font maps {c:?}"))
}

/// One run per line, a token break after every space glyph.
fn runs(codes: Vec<Vec<u16>>) -> Vec<CodeRun> {
    let space = gid(' ');
    codes
        .into_iter()
        .map(|codes| CodeRun {
            breaks: (1..codes.len())
                .filter(|&i| codes[i - 1] == space)
                .collect(),
            codes,
        })
        .collect()
}

/// `lines` encoded through the test font.
fn slot_of(lines: &[&str]) -> Vec<CodeRun> {
    runs(lines.iter().map(|l| l.chars().map(gid).collect()).collect())
}

/// The codes of page `page`'s `Tj`s in the golden, one run per line.
fn golden_slot(page: usize) -> Vec<CodeRun> {
    let doc = Document::load_mem(&golden_pdf()).unwrap();
    let page_id = doc.get_pages()[&(page as u32 + 1)];
    let content = doc.get_page_content(page_id);
    let lines = content_ops(&content)
        .filter(|op| op.op == b"Tj")
        .map(|op| match &op.operands[..] {
            [Object::String(bytes, _)] => bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&pair| u16::from_be_bytes(pair))
                .collect(),
            other => panic!("Tj operands {other:?}"),
        })
        .collect();
    runs(lines)
}

fn slot(tounicode: ToUnicodeState) -> FontSlot {
    FontSlot {
        page: 1,
        slot: "F1".into(),
        base_font: Some("/ABCDEF+Garamond".into()),
        subtype: Some("/Type0".into()),
        embedded: false,
        tounicode,
        glyph_count: 0,
        resolution: None,
    }
}

fn code_points(lines: &[&str]) -> Vec<u32> {
    lines
        .iter()
        .flat_map(|l| l.chars())
        .map(u32::from)
        .collect()
}

fn r(num: u64, den: u64) -> Ratio {
    Ratio { num, den }
}

#[test]
fn golden_codes_rank_the_right_font_first() {
    let db = test_db();
    let en = english();
    let codes = golden_slot(0);
    let (cands, trace) = run_infer(&db, &codes, &[&en], None);
    assert_eq!(cands.len(), 2);
    assert_eq!(cands[0].font_id, RIGHT);
    assert_eq!(cands[0].lang, Lang::En);
    assert!(cands[0].hit >= r(9, 10), "hit {:?}", cands[0].hit);
    assert!(cands[0].score > cands[1].score);
    assert!(cands[1].hit < r(1, 2), "shuffled hit {:?}", cands[1].hit);
    assert_eq!(cands[0].preview, GOLDEN_TEXT[0].join(" "));
    // Deterministic: the same ranking, scores and trace again.
    assert_eq!(run_infer(&db, &codes, &[&en], None), (cands, trace));
}

#[test]
fn trace_fields_are_filled() {
    let db = test_db();
    let en = english();
    let codes = golden_slot(0);
    let (cands, trace) = run_infer(&db, &codes, &[&en], None);
    assert_eq!(trace.hit, cands[0].hit);
    assert_eq!(trace.margin, cands[0].margin);
    assert_eq!(trace.raw_best, Some(cands[0].score));
    assert_eq!(trace.raw_second, Some(cands[1].score));
    // "PDFPundit golden page one." + "The quick brown fox jumps over the lazy dog."
    assert_eq!(trace.n_chars, 26 - 3 + 44 - 8);
    assert_eq!(trace.n_runs, 2);
    assert_eq!(trace.lang, Lang::En);
    assert_eq!(trace.script, "Latn");
    assert!(!trace.tounicode_dict_used);
    let fr = FrequencyList::from_text(Lang::Fr, "x");
    assert!(
        run_infer(&db, &codes, &[&en], Some(&fr))
            .1
            .tounicode_dict_used
    );
}

#[test]
fn empty_dictionary_gives_no_hits_and_always_asks() {
    let db = test_db();
    let codes = golden_slot(0);
    for dicts in [&[][..], &[&EmptyDictionary as &dyn Dictionary][..]] {
        let (cands, trace) = run_infer(&db, &codes, dicts, None);
        assert_eq!(cands.len(), 2);
        for c in &cands {
            assert_eq!(c.hit, ZERO, "{}", c.font_id);
            assert_eq!(c.margin, ZERO);
            assert_eq!(c.confidence, ZERO);
            assert_eq!(c.lang, Lang::Unknown);
        }
        // Neither font scores anything: the tie is pinned by font id.
        assert_eq!((cands[0].score, cands[1].score), (0, 0));
        assert_eq!(cands[0].font_id, RIGHT);
        let needed = code_points(GOLDEN_TEXT[0]);
        for tounicode in [ToUnicodeState::Missing, ToUnicodeState::Present] {
            let (decision, provenance) = resolve(
                &cands,
                &trace,
                &RepairOptions::default(),
                &slot(tounicode),
                &needed,
                &codes,
                &db,
            );
            assert!(
                matches!(decision, FontDecision::Ask(_)),
                "{tounicode:?}: {decision:?}"
            );
            assert!(provenance.iter().any(|p| p.starts_with("asked: ")));
        }
    }
}

#[test]
fn negative_scores_never_panic_and_margin_stays_in_range() {
    // A font whose index says Arabic only: every Latin letter is outside its
    // scripts; codes past the font's glyphs read as U+FFFD.
    let db = test_db_with(|entries| {
        let mut arabic = entries[0].clone();
        arabic.id = "Arabic-Only".into();
        arabic.scripts = vec!["Arab".into()];
        entries.push(arabic);
    });
    let mut codes = golden_slot(0);
    codes.push(CodeRun {
        codes: vec![60_000, 60_001, 60_002],
        breaks: vec![1, 2, 99],
    });
    codes.push(CodeRun {
        codes: Vec::new(),
        breaks: vec![0],
    });
    let (cands, trace) = run_infer(&db, &codes, &[&EmptyDictionary], None);
    assert_eq!(cands.len(), 3);
    assert!(cands.iter().all(|c| c.score < 0), "{cands:?}");
    let arabic = cands.iter().find(|c| c.font_id == "Arabic-Only").unwrap();
    assert_eq!(
        cands.last().unwrap(),
        arabic,
        "the script penalty ranks it last"
    );
    for c in &cands {
        assert!(c.margin >= ZERO && c.margin <= ONE);
        assert!(c.confidence >= ZERO && c.confidence <= ONE);
        assert!(c.preview.contains('\u{fffd}'));
    }
    assert!(trace.raw_best.unwrap() < 0);
    assert_eq!(trace.n_runs, 4);
    // The same with a dictionary: the best has a hit, the margin is still
    // clamped where a lower-scored font hits more.
    let en = english();
    let (cands, _) = run_infer(&db, &codes, &[&en], None);
    for c in &cands {
        assert!(c.margin <= ONE);
    }
    // No font and no codes at all.
    let (none, trace) = infer(&[], &[], &|_| None, &[], None);
    assert!(none.is_empty());
    assert_eq!((trace.raw_best, trace.n_chars, trace.hit), (None, 0, ZERO));
    assert_eq!(trace.script, "Zyyy");
}

#[test]
fn margin_is_the_hit_over_the_runner_up_clamped() {
    assert_eq!(minus_clamped(r(9, 10), r(1, 10)), r(4, 5));
    assert_eq!(minus_clamped(r(1, 10), r(9, 10)), ZERO);
    assert_eq!(minus_clamped(ONE, ZERO), ONE);
    assert_eq!(mean(ONE, r(1, 2)), r(3, 4));
    assert_eq!(ratio(6, 8), r(3, 4));
}

#[test]
fn a_short_slot_never_auto_accepts() {
    let db = test_db();
    let en = english();
    let text = ["lazy dog jumps"];
    let codes = slot_of(&text);
    let (cands, trace) = run_infer(&db, &codes, &[&en], None);
    assert_eq!(cands[0].font_id, RIGHT);
    assert_eq!(cands[0].hit, ONE);
    assert_eq!(trace.n_chars, 12);
    assert!(cands[0].confidence >= RepairOptions::default().auto_accept_confidence);
    let (decision, provenance) = resolve(
        &cands,
        &trace,
        &RepairOptions::default(),
        &slot(ToUnicodeState::Missing),
        &code_points(&text),
        &codes,
        &db,
    );
    assert!(matches!(decision, FontDecision::Ask(_)), "{decision:?}");
    assert!(
        provenance
            .last()
            .unwrap()
            .contains("below the sample floor"),
        "{provenance:?}"
    );
}

#[test]
fn a_long_clear_slot_auto_accepts() {
    let db = test_db();
    let en = english();
    let text = ["pdfpundit golden page one", "quick brown fox jumps"];
    let codes = slot_of(&text);
    let (cands, trace) = run_infer(&db, &codes, &[&en], None);
    assert_eq!(trace.n_chars, 40);
    assert_eq!(cands[0].hit, ONE);
    assert!(cands[0].margin >= r(1, 2), "margin {:?}", cands[0].margin);
    let (decision, provenance) = resolve(
        &cands,
        &trace,
        &RepairOptions::default(),
        &slot(ToUnicodeState::Missing),
        &code_points(&text),
        &codes,
        &db,
    );
    assert_eq!(decision, FontDecision::AutoAccept(cands[0].clone()));
    assert!(provenance[1].starts_with("auto-accepted NotoSans-Regular"));
    // The resolution T-30 records from it.
    let kind = FontResolutionKind::Picked {
        font_id: cands[0].font_id.clone(),
        confidence: cands[0].confidence,
    };
    assert!(matches!(kind, FontResolutionKind::Picked { .. }));
}

#[test]
fn a_tounicode_dictionary_pins_an_inflected_french_slot() {
    let db = test_db();
    let en = english();
    let text = GOLDEN_TEXT[1][0];
    assert!(text.contains('é'));
    let codes = golden_slot(1)[..1].to_vec();
    // The general dictionaries alone know none of it.
    let (cands, _) = run_infer(&db, &codes, &[&en], None);
    assert!(cands[0].hit < r(1, 2));
    let true_words = FrequencyList::from_text(Lang::Fr, text);
    let (cands, trace) = run_infer(&db, &codes, &[&en], Some(&true_words));
    assert_eq!(cands[0].font_id, RIGHT);
    assert_eq!(cands[0].lang, Lang::Fr);
    assert!(cands[0].hit >= r(9, 10), "hit {:?}", cands[0].hit);
    assert!(cands[1].hit < r(1, 2));
    assert!(trace.tounicode_dict_used);
    assert_eq!(cands[0].preview, text);
}

#[test]
fn pick_requests_respect_their_limits() {
    let db = test_db();
    // Five copies of the golden's first page: past 200 characters.
    let codes: Vec<CodeRun> = (0..5).flat_map(|_| golden_slot(0)).collect();
    let (cands, trace) = run_infer(&db, &codes, &[], None);
    for max in [0, 1, 5] {
        let opts = RepairOptions {
            max_font_candidates: max,
            ..RepairOptions::default()
        };
        let (decision, _) = resolve(
            &cands,
            &trace,
            &opts,
            &slot(ToUnicodeState::Missing),
            &[],
            &codes,
            &db,
        );
        let FontDecision::Ask(req) = decision else {
            panic!("{decision:?}");
        };
        assert_eq!(req.candidates.len(), (max as usize).min(2));
        assert_eq!(req.preview.chars().count(), PREVIEW_CHARS);
        assert!(
            req.candidates
                .iter()
                .all(|c| c.preview.chars().count() <= PREVIEW_CHARS)
        );
        assert!(req.sample_codes.len() <= SAMPLE_CODES);
        assert!(!req.sample_codes.is_empty());
        let distinct: BTreeSet<u32> = req.sample_codes.iter().copied().collect();
        assert_eq!(distinct.len(), req.sample_codes.len());
        assert_eq!((req.page, req.slot.as_str()), (1, "F1"));
        if let Some(first) = req.candidates.first() {
            assert_eq!(first.family, "Noto Sans");
            assert_eq!(first.language, "und");
        }
    }
}

#[test]
fn uncovered_code_points_are_unreproducible() {
    let db = test_db();
    let codes = golden_slot(0);
    let (cands, trace) = run_infer(&db, &codes, &[], None);
    // The slot's /ToUnicode says Arabic; no test font maps it.
    let needed: Vec<u32> = "مرحبا بالعالم".chars().map(u32::from).collect();
    let decide = |policy| {
        let opts = RepairOptions {
            unreproducible: policy,
            ..RepairOptions::default()
        };
        resolve(
            &cands,
            &trace,
            &opts,
            &slot(ToUnicodeState::Present),
            &needed,
            &codes,
            &db,
        )
    };
    let (decision, provenance) = decide(UnreproduciblePolicy::Ask);
    let FontDecision::Unreproducible(req) = decision else {
        panic!("{decision:?}");
    };
    assert_eq!(req.family, "Garamond");
    assert_eq!(req.slots, vec![(1, "F1".to_owned())]);
    assert_eq!(req.reason, UNREPRODUCIBLE_REASON);
    let ids: Vec<&str> = req.options.iter().map(|o| o.font_id.as_str()).collect();
    assert_eq!(ids, [RIGHT, SHUFFLED]);
    assert!(provenance.iter().any(|p| p == "policy: ask"));

    assert_eq!(
        decide(UnreproduciblePolicy::TextOnly).0,
        FontDecision::TextOnly
    );
    assert!(matches!(
        decide(UnreproduciblePolicy::SubstituteGeneric).0,
        FontDecision::Unreproducible(_)
    ));

    // The same slot whose code points the fonts do map asks instead.
    let (decision, _) = resolve(
        &cands,
        &trace,
        &RepairOptions::default(),
        &slot(ToUnicodeState::Present),
        &code_points(GOLDEN_TEXT[0]),
        &codes,
        &db,
    );
    assert!(matches!(decision, FontDecision::Ask(_)));

    // An empty database has no candidate: unreproducible as well.
    let empty = FontDb::empty();
    let (none, trace) = run_infer(&empty, &codes, &[], None);
    let (decision, _) = resolve(
        &none,
        &trace,
        &RepairOptions::default(),
        &slot(ToUnicodeState::Missing),
        &[],
        &codes,
        &empty,
    );
    let FontDecision::Unreproducible(req) = decision else {
        panic!("{decision:?}");
    };
    assert!(req.options.is_empty());
}

#[test]
fn scripts_and_families() {
    assert_eq!(script_of('a'), Some("Latn"));
    assert_eq!(script_of('é'), Some("Latn"));
    assert_eq!(script_of('œ'), Some("Latn"));
    assert_eq!(script_of('ﬁ'), Some("Latn"));
    assert_eq!(script_of('م'), Some("Arab"));
    assert_eq!(script_of('क'), Some("Deva"));
    assert_eq!(script_of('的'), Some("Hani"));
    assert_eq!(script_of('ж'), Some("Cyrl"));
    assert_eq!(script_of('ᚠ'), Some("Zzzz"));
    for c in ['1', '.', '«', ' ', '\u{301}', '\u{fffd}', '×'] {
        assert_eq!(script_of(c), None, "{c:?}");
    }
    let mut s = slot(ToUnicodeState::Missing);
    assert_eq!(family(&s), "Garamond");
    s.base_font = Some("/CIDFont+F1".into());
    assert_eq!(family(&s), "CIDFont+F1");
    s.base_font = None;
    assert_eq!(family(&s), "F1");
}

#[test]
fn mixed_script_tokens_are_penalised() {
    let mut entry = build_from_ttf(TEST_FONT).unwrap().0;
    entry.scripts = vec!["Latn".into(), "Cyrl".into()];
    let mut records = vec![
        GmapRecord::new(1, 'a', 500, gmap::Source::Cmap),
        GmapRecord::new(2, 'ж', 500, gmap::Source::Cmap),
    ];
    records.sort_by_key(GmapRecord::sort_key);
    let bytes = gmap::encode(records);
    let table = GmapTable::new(&bytes).unwrap();
    let mixed = [CodeRun {
        codes: vec![1, 2],
        breaks: vec![],
    }];
    let split = [CodeRun {
        codes: vec![1, 2],
        breaks: vec![1],
    }];
    let score = |codes: &[CodeRun]| {
        let (cands, _) = infer(codes, &[&entry], &|_| Some(&table), &[], None);
        cands[0].score
    };
    assert_eq!(score(&mixed), MIXED_SCRIPTS);
    assert_eq!(score(&split), 0);
}

/// The libm ban is crate-wide (clippy.toml); this tripwire keeps floats out of
/// the scorer altogether.
#[test]
fn no_floats_in_the_scorer() {
    let source = include_str!("../score.rs");
    for float in ["f32", "f64"] {
        assert!(!source.contains(float), "score.rs mentions {float}");
    }
}
