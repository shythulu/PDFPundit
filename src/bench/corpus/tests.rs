//! The harness's own checks, none of which needs the corpus: the manifest
//! gate, the committed manifest and smoke list, the D-077 resolution, the
//! language labels, the CSV and table shapes and the golden gate.

use sha2::{Digest, Sha256};

use super::*;
use crate::pdf::fixtures::golden_pdf;
use crate::pdf::text::{ExtractOptions, extract_text};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ── the manifest gate ────────────────────────────────────────────────────

#[test]
fn blob_ids_are_git_blob_ids() {
    // `git hash-object` of the empty file and of "hello\n".
    assert_eq!(
        hex(&blob_id(b"")),
        "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
    );
    assert_eq!(
        hex(&blob_id(b"hello\n")),
        "ce013625030ba8dba906f756967f9e9ca394464a"
    );
}

#[test]
fn the_gate_accepts_bytes_that_hash_to_an_entry() {
    let bytes = golden_pdf();
    let line = format!(
        "corrupted/print/text/Doc(print)_header.pdf {} {}\n",
        hex(&blob_id(&bytes)),
        bytes.len()
    );
    let manifest = Manifest::parse(&line).expect("one-line manifest parses");
    let entry = manifest
        .check(&bytes)
        .expect("the fixture is in this manifest");
    assert_eq!(entry.path, "corrupted/print/text/Doc(print)_header.pdf");
    assert_eq!(entry.size, bytes.len() as u64);
}

#[test]
fn the_gate_refuses_the_golden_fixture_with_the_one_line_message() {
    let err = Manifest::pinned()
        .check(&golden_pdf())
        .expect_err("the fixture is not a corpus file");
    let message = err.to_string();
    assert_eq!(
        message,
        "not a REPDF corpus file: the harness runs only on the pinned corpus (e547d4d)"
    );
    assert!(!message.contains('\n'));
}

#[test]
fn a_file_at_another_path_or_changed_by_one_byte_is_refused() {
    let bytes = golden_pdf();
    let line = format!(
        "original/print/text/Doc(print).pdf {} {}\n",
        hex(&blob_id(&bytes)),
        bytes.len()
    );
    let manifest = Manifest::parse(&line).expect("parses");
    assert!(
        manifest
            .check_at(&bytes, "original/print/text/Doc(print).pdf")
            .is_ok()
    );
    assert!(
        manifest
            .check_at(&bytes, "original/saveas/text/Doc(saveas).pdf")
            .is_err()
    );
    let mut changed = bytes.clone();
    changed[10] ^= 1;
    assert!(manifest.check(&changed).is_err());
}

#[test]
fn malformed_manifest_lines_are_rejected() {
    for bad in [
        "a.pdf e69de29bb2d1d6434b8b29ae775ad8c2e48c5391\n",
        "a.pdf e69de29bb2d1d6434b8b29ae775ad8c2e48c539 0\n",
        "a.pdf e69de29bb2d1d6434b8b29ae775ad8c2e48c5391 x\n",
        "a.pdf  e69de29bb2d1d6434b8b29ae775ad8c2e48c5391 0\n",
        "b.pdf e69de29bb2d1d6434b8b29ae775ad8c2e48c5391 0\na.pdf ce013625030ba8dba906f756967f9e9ca394464a 6\n",
    ] {
        assert!(Manifest::parse(bad).is_err(), "{bad:?}");
    }
}

// ── the committed files ──────────────────────────────────────────────────

#[test]
fn the_manifest_is_the_pinned_one() {
    assert_eq!(MANIFEST.lines().count(), 1_100);
    assert_eq!(hex(&Sha256::digest(MANIFEST.as_bytes())), MANIFEST_SHA256);
    let manifest = Manifest::pinned();
    assert_eq!(manifest.entries.len(), 1_100);
    let corrupted = manifest
        .entries
        .iter()
        .filter(|e| e.path.starts_with("corrupted/"))
        .count();
    assert_eq!((corrupted, 1_100 - corrupted), (1_000, 100));
    let bytes: u64 = manifest.entries.iter().map(|e| e.size).sum();
    assert_eq!(bytes, 907_834_677);
}

#[test]
fn every_manifest_path_parses_and_every_document_has_ten_classes_and_an_original() {
    let manifest = Manifest::pinned();
    let mut per_doc = BTreeMap::<(String, String), Vec<Option<CorruptionClass>>>::new();
    for entry in &manifest.entries {
        let p = CorpusPath::parse(&entry.path).expect("manifest path parses");
        per_doc
            .entry((p.base.to_owned(), p.producer.to_owned()))
            .or_default()
            .push(p.class);
    }
    assert_eq!(per_doc.len(), 100);
    for ((base, producer), classes) in per_doc {
        assert_eq!(classes.len(), 11, "{base} {producer}");
        assert_eq!(classes.iter().filter(|c| c.is_none()).count(), 1);
        for class in CorruptionClass::ALL {
            assert!(
                classes.contains(&Some(class)),
                "{base} {producer} {class:?}"
            );
        }
    }
}

#[test]
fn the_suffix_map_is_repdfs() {
    let p = CorpusPath::parse("corrupted/saveas/text+img/X_y(saveas)_remove_unicode_fonts.pdf")
        .expect("parses");
    assert_eq!(p.class, Some(CorruptionClass::C8FontResourcesDeleted));
    assert_eq!((p.producer, p.kind, p.base), ("saveas", "text+img", "X_y"));
    let codes: Vec<&str> = SUFFIXES.iter().map(|(_, c)| c.code()).collect();
    assert_eq!(
        codes,
        ["C1", "C2", "C3", "C4", "C5", "C6", "C7", "C8", "C9", "C10"]
    );
    assert!(CorpusPath::parse("corrupted/print/text/X(print)_unknown.pdf").is_err());
    assert!(CorpusPath::parse("corrupted/print/text/X(saveas)_header.pdf").is_err());
    let o = CorpusPath::parse("original/print/text/X(print).pdf").expect("parses");
    assert_eq!(o.class, None);
    assert_eq!(o.original(), "original/print/text/X(print).pdf");
}

#[test]
fn every_smoke_path_is_in_the_manifest() {
    let manifest = Manifest::pinned();
    let smoke: Vec<&str> = SMOKE_SUBSET.lines().collect();
    assert_eq!(smoke.len(), 44);
    for path in &smoke {
        assert!(manifest.by_path(path).is_some(), "{path}");
    }
    assert_eq!(
        smoke.iter().filter(|p| p.starts_with("original/")).count(),
        4
    );
}

#[test]
fn the_d077_resolution_equals_the_committed_smoke_list_line_for_line() {
    let resolved = resolve_smoke(&Manifest::pinned()).expect("resolves");
    let committed: Vec<&str> = SMOKE_SUBSET.lines().collect();
    assert_eq!(resolved, committed);
    // Byte order, not case-insensitive order (D-077).
    assert!(resolved[0].contains("AI_Development_MultiLanguage"));
    assert!(resolved[22].contains("Aurora_VR_AR_Game_Lineup_Multilingual"));
    assert!(!resolved.iter().any(|p| p.contains(DEFECTIVE_DOC)));
}

#[test]
fn the_golden_csv_starts_with_the_d063_header() {
    assert_eq!(
        GOLDEN_SMOKE.lines().next(),
        Some(
            "regression baseline; 44-file REPDF smoke subset; text-layer LCS-F1; \
             not REPDF's metric; not comparable to the paper; not a published recovery rate"
        )
    );
    assert_eq!(GOLDEN_SMOKE.lines().next(), Some(D063_HEADER));
    let golden = Golden::parse(GOLDEN_SMOKE).expect("the committed golden parses");
    let classes: Vec<&str> = golden.per_class.keys().map(String::as_str).collect();
    let mut expected: Vec<&str> = CorruptionClass::ALL.iter().map(|c| c.code()).collect();
    expected.sort_unstable();
    assert_eq!(classes, expected);
}

#[test]
fn the_paper_reference_file_exists_and_its_columns_are_absent() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("bench/repdf_paper_tables.toml");
    let text = std::fs::read_to_string(path).expect("the constants file is committed");
    assert!(
        text.contains("Forensic Science International: Digital Investigation 56 (2026) 302061")
    );
    assert!(text.contains("Table 2 (p. 7)") && text.contains("Table 5 (p. 8)"));
    assert!(text.contains("NOT READ"));
    for column in ["repdf_t2_mean6", "repdf_t5"] {
        assert!(text.contains(column));
        assert!(!CSV_COLUMNS.iter().any(|c| c.contains(column)), "{column}");
        let rows = [sample_row("C1", "print", Lang::En, 1, 1)];
        assert!(!csv(&rows).contains(column));
        assert!(!tables(D063_HEADER, &rows, None).text.contains(column));
    }
}

// ── languages (D-069, eng-r4-fr1) ────────────────────────────────────────

#[test]
fn scripts_follow_the_non_latin_share_rule() {
    assert_eq!(script_of("The cat sat on the mat"), Some(Script::Latin));
    assert_eq!(script_of("مرحبا بالعالم"), Some(Script::Arabic));
    assert_eq!(script_of("नमस्ते दुनिया"), Some(Script::Devanagari));
    assert_eq!(script_of("你好世界"), Some(Script::Han));
    // A Chinese page whose body is outlined keeps a Latin brand name: 7
    // Latin letters and 6 Han ones still make it Han (>= 20% and >= 3).
    assert_eq!(script_of("NeoHome 智能家居产品"), Some(Script::Han));
    // Two stray Han letters in a Latin page do not.
    assert_eq!(
        script_of("Welcome to the catalogue 中文"),
        Some(Script::Latin)
    );
    assert_eq!(script_of("1234 \u{fffd}\u{fffd} --"), None);
}

#[test]
fn latin_pages_split_by_the_unique_stopword_tables() {
    let en = "The report covers the state of the market and its effect on prices for this year.";
    let fr = "Le rapport présente les tendances du marché et les effets sur les prix pour une année dans le secteur.";
    let es = "El informe presenta las tendencias del mercado y los efectos para el sector con una mirada como su objetivo.";
    assert_eq!(page_lang(en), Lang::En);
    assert_eq!(page_lang(fr), Lang::Fr);
    assert_eq!(page_lang(es), Lang::Es);
    // Too few hits, or no clear winner: unknown.
    assert_eq!(page_lang("SkyGarden Indoor Solutions"), Lang::Unknown);
    assert_eq!(page_lang("the and of le les et"), Lang::Unknown);
    assert_eq!(page_lang("مرحبا"), Lang::Ar);
    assert_eq!(page_lang("नमस्ते"), Lang::Hi);
    assert_eq!(page_lang("你好世界"), Lang::Zh);
}

#[test]
fn one_unknown_page_is_filled_from_the_six_language_set() {
    let pages = [
        "the of and is to in",
        "你好世界",
        "नमस्ते दुनिया",
        "x",
        "le les et des du une",
        "مرحبا بالعالم",
    ];
    let pages: Vec<String> = pages.iter().map(|s| s.to_string()).collect();
    let mut labels = label_pages(&pages);
    assert_eq!(labels.langs[3], Lang::Unknown);
    assert_eq!(labels.fill_one_unknown(), Some(3));
    assert_eq!(
        labels.langs,
        [Lang::En, Lang::Zh, Lang::Hi, Lang::Es, Lang::Fr, Lang::Ar]
    );
    // Two unknowns, or a duplicate, are left alone.
    let mut two = label_pages(&["x".to_string(), "y".to_string()]);
    assert_eq!(two.fill_one_unknown(), None);
}

#[test]
fn the_golden_fixture_pages_get_text_and_labels() {
    let pages = extract_text(&golden_pdf(), &ExtractOptions::default()).expect("loads");
    let texts: Vec<String> = pages.iter().map(page_string).collect();
    assert_eq!(texts.len(), 2);
    let one = crate::bench::metrics::normalize(&texts[0]);
    assert!(
        one.contains("the quick brown fox jumps over the lazy dog."),
        "{one:?}"
    );
    assert_eq!(script_of(&texts[1]), Some(Script::Latin));
    // Two runs, one string.
    let again = extract_text(&golden_pdf(), &ExtractOptions::default()).expect("loads");
    assert_eq!(texts, again.iter().map(page_string).collect::<Vec<_>>());
}

// ── results ──────────────────────────────────────────────────────────────

fn sample_row(class: &str, producer: &str, lang: Lang, num: u64, den: u64) -> Row {
    let scores = crate::bench::metrics::TextScores {
        lcs_f1: Ratio { num, den },
        recall: Ratio { num, den },
        precision: Ratio { num, den },
        bag_f1: Ratio { num, den },
        char_f1: Ratio { num, den },
        len_orig: den,
        len_rep: den,
        lcs: num,
    };
    Row {
        file: format!("corrupted/{producer}/text/D({producer})_x.pdf"),
        class: class.to_owned(),
        producer: producer.to_owned(),
        base_doc: "D".to_owned(),
        page: 0,
        lang,
        outcome: "ok".to_owned(),
        chosen_toolpath: "resave".to_owned(),
        v: Default::default(),
        scores,
        baseline: scores,
        extract_failed: false,
        unmapped_glyphs: 0,
        c9_exact: 0,
        c9_accepted: 0,
        c9_ambiguous: 0,
        c9_unsearched: 0,
        c9_work_total: 0,
        reals_narrowed: 0,
        inflate_disagreements: 0,
    }
}

#[test]
fn the_csv_has_the_documented_columns() {
    assert_eq!(
        CSV_COLUMNS.join(","),
        "file,class,producer,base_doc,page,lang,outcome,chosen_toolpath,v0,v1,v2,v3,v4,\
         lcs_f1,recall,precision,bag_f1,char_f1,baseline_lcs_f1,extract_failed,unmapped_glyphs,\
         c9_streams_exact,c9_streams_accepted,c9_streams_ambiguous,c9_streams_unsearched,\
         c9_work_total,reals_narrowed,inflate_backend_disagreements"
    );
    let text = csv(&[sample_row("C9", "saveas", Lang::Zh, 3, 4)]);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], CSV_COLUMNS.join(","));
    assert_eq!(lines[1].split(',').count(), CSV_COLUMNS.len());
    assert!(
        lines[1].starts_with("corrupted/saveas/text/D(saveas)_x.pdf,C9,saveas,D,0,zh,ok,resave,")
    );
}

#[test]
fn every_printed_table_starts_with_the_d063_header() {
    let rows = [
        sample_row("C1", "print", Lang::En, 1, 1),
        sample_row("C10", "saveas", Lang::Fr, 1, 2),
    ];
    let out = tables(D063_HEADER, &rows, None);
    assert_eq!(out.text.lines().next(), Some(D063_HEADER));
    let blocks: Vec<&str> = out
        .text
        .split("\n\n")
        .filter(|b| !b.trim().is_empty())
        .collect();
    assert!(blocks.len() >= 3);
    for block in blocks {
        assert_eq!(block.lines().next(), Some(D063_HEADER), "{block}");
    }
}

#[test]
fn the_gate_allows_two_percent_below_the_golden_and_no_more() {
    let golden = Golden::parse(&format!(
        "{D063_HEADER}\nclass,pages,lcs_f1\nC1,2,0.900\nC2,2,0.500\n"
    ))
    .expect("parses");
    // C1 at 0.880 passes (0.900 - 0.020); C2 at 0.479 fails.
    let rows = [
        sample_row("C1", "print", Lang::En, 880, 1000),
        sample_row("C2", "print", Lang::En, 479, 1000),
    ];
    let out = tables(D063_HEADER, &rows, Some(&golden));
    assert_eq!(out.failed, ["C2"]);
    assert!(out.text.contains("FAIL"));
    // A class in the golden with no rows fails too, and so does a class
    // with rows and no golden.
    let out = tables(D063_HEADER, &rows[..1], Some(&golden));
    assert_eq!(out.failed, ["C2"]);
    let rows = [sample_row("C3", "print", Lang::En, 1, 1)];
    let out = tables(D063_HEADER, &rows, Some(&golden));
    assert_eq!(out.failed, ["C1", "C2", "C3"]);
    assert!(tables(D063_HEADER, &rows, None).failed.is_empty());
}

#[test]
fn a_golden_written_from_rows_reads_back() {
    let rows = [
        sample_row("C1", "print", Lang::En, 1, 1),
        sample_row("C1", "saveas", Lang::En, 1, 2),
    ];
    let text = golden_csv(&rows);
    assert_eq!(text.lines().next(), Some(D063_HEADER));
    let golden = Golden::parse(&text).expect("reads back");
    assert_eq!(golden.per_class["C1"], 750);
    let out = tables(D063_HEADER, &rows, Some(&golden));
    assert!(out.failed.is_empty());
}

#[test]
fn a_golden_with_another_first_line_is_refused() {
    assert!(Golden::parse("class,pages,lcs_f1\nC1,1,1.000\n").is_err());
}

#[test]
fn inflate_backends_agree_on_the_golden_fixtures_streams() {
    let bytes = golden_pdf();
    let carve = crate::pdf::carver::carve(&bytes, &|| false).expect("never cancelled");
    let tally = inflate_disagreements(&bytes, &carve);
    assert!(tally.compared > 0);
    assert_eq!(tally.disagree, 0);
}

#[test]
fn a_shorter_prefix_is_a_disagreement_only_on_a_clean_stream() {
    let result = |status| InflateResult {
        status,
        out: b"abcdef".to_vec(),
        consumed: 6,
    };
    let done = result(InflateStatus::Done);
    assert!(!disagree(&done, b"abcdef"));
    assert!(disagree(&done, b"abc"));
    for status in [
        InflateStatus::AdlerMismatch,
        InflateStatus::NeedsMoreInput,
        InflateStatus::Failed { at: 40 },
    ] {
        let failed = result(status);
        assert!(!disagree(&failed, b"abc"), "{status:?}");
        assert!(!disagree(&failed, b"abcdef"), "{status:?}");
        assert!(disagree(&failed, b"abcdefg"), "{status:?}");
        assert!(disagree(&failed, b"abX"), "{status:?}");
    }
}
