//! T-13b acceptance: the C9 swap (the measured C9 fixture repairs clean
//! with its `Exact` streams as the original's raw data; a small budget
//! reports `Accepted` streams apart from `Exact` ones with the fixed line;
//! the `Ambiguous` synthetic, an `Unrecoverable` and an `Unsearched` stream
//! are `Partial`) and the C6 re-link (re-linked and V2 as the golden's; no
//! orphan font, or none that fits, escalates a `FontPick`; matches by
//! `/Name`, widths and proximity and never by `/BaseFont`; a slot only a
//! form uses is not re-linked).

use crate::engine::RepairReport;
use crate::pdf::model::{InteractionKind, SalvageGrade};
use crate::pdf::streams::salvage::{Grade, Salvage};

use super::*;

// ── helpers ──────────────────────────────────────────────────────────────

/// [`analysed`] under `budget`.
fn analysed_with(bytes: Vec<u8>, budget: &SalvageBudget) -> Input {
    let carve = carve(&bytes, &|| false).expect("never cancelled");
    let salvage = salvage_all(
        &CarveSource::new(&carve, &bytes),
        budget,
        NonZeroUsize::MIN,
        1 << 30,
        &|| false,
    )
    .expect("never cancelled");
    with_index(bytes, salvage)
}

/// `bytes` analysed against `salvage` instead of its own.
fn with_index(bytes: Vec<u8>, salvage: SalvageIndex) -> Input {
    let carve = carve(&bytes, &|| false).expect("never cancelled");
    let graph = ObjectGraph::from_carve(&carve);
    let findings = diagnose(&bytes, &carve, &graph, &salvage);
    Input {
        bytes,
        carve,
        graph,
        salvage,
        findings,
    }
}

/// The raw (still encoded) data of stream `n` in `doc`.
fn raw_stream(doc: &Document, n: u32) -> Vec<u8> {
    match doc.get_object((n, 0)) {
        Ok(Object::Stream(s)) => s.content.clone(),
        other => panic!("{n} 0 obj is no stream: {other:?}"),
    }
}

/// The output number of the input's `id`.
fn out_number(input: &Input, id: ObjId) -> u32 {
    plan_ids(&input.carve, &input.graph)
        .number(id)
        .expect("written")
}

fn report_of(g: &Generated) -> RepairReport {
    let mut report = test_report();
    g.record(&mut report);
    report
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// `buf` with every byte of `at` turned to a space: offsets stay put.
fn blank(buf: &mut [u8], at: std::ops::Range<usize>) {
    buf[at].fill(b' ');
}

/// The span of `n 0 obj` through its `endobj` in `buf`.
fn object_span(buf: &[u8], n: u32) -> std::ops::Range<usize> {
    let start = find(buf, format!("\n{n} 0 obj").as_bytes()).expect("the object") + 1;
    let end = start + find(&buf[start..], b"endobj").expect("endobj") + b"endobj".len();
    start..end
}

/// `buf` with `/Font << … >>` blanked inside object `n`.
fn without_font_entry(buf: &mut [u8], n: u32) {
    let span = object_span(buf, n);
    let key = span.start + find(&buf[span.clone()], b"/Font").expect("/Font");
    let open = key + find(&buf[key..], b"<<").expect("<<");
    let close = open + find(&buf[open..], b">>").expect(">>") + 2;
    blank(buf, key..close);
}

/// The golden with both pages' `/Font` entries blanked: its Type0 font 5 is
/// then reachable from nothing, the re-link candidate of both pages' `/F1`.
fn golden_c6_with_orphan() -> Vec<u8> {
    let mut buf = golden_pdf();
    for page in [3, 4] {
        without_font_entry(&mut buf, page);
    }
    buf
}

/// `%PDF-1.7`, the objects, a classic xref table (none when `xref` is
/// false) and a trailer naming `1 0 R` as the root.
fn hand_made(objects: &[(u32, Vec<u8>)], xref: bool) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = BTreeMap::new();
    for (n, body) in objects {
        offsets.insert(*n, out.len());
        out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let size = offsets.keys().next_back().map_or(1, |m| m + 1);
    let at = out.len();
    if xref {
        out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
        for n in 1..size {
            let line = match offsets.get(&n) {
                Some(o) => format!("{o:010} 00000 n \n"),
                None => "0000000000 65535 f \n".to_owned(),
            };
            out.extend_from_slice(line.as_bytes());
        }
    }
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
    if xref {
        out.extend_from_slice(format!("startxref\n{at}\n").as_bytes());
    }
    out.extend_from_slice(b"%%EOF\n");
    out
}

fn body(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}

/// An unfiltered stream object's body.
fn stream_body(extra: &str, data: &str) -> Vec<u8> {
    body(&format!(
        "<< /Length {} {extra}>>\nstream\n{data}\nendstream",
        data.len()
    ))
}

const CATALOG: &str = "<< /Type /Catalog /Pages 2 0 R >>";
const PAGES: &str = "<< /Type /Pages /Kids [3 0 R] /Count 1 >>";

/// A page with `resources` drawing content stream 4.
fn page(resources: &str) -> Vec<u8> {
    body(&format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources {resources} \
         /Contents 4 0 R >>"
    ))
}

/// A simple Type1 font: `/Name` when `name` is set, and `/Widths` from
/// `first` when `widths` is set.
fn type1(name: Option<&str>, base: &str, widths: Option<(u8, &[i64])>) -> Vec<u8> {
    let name = name.map_or(String::new(), |n| format!("/Name /{n} "));
    let widths = widths.map_or(String::new(), |(first, w)| {
        let each: Vec<String> = w.iter().map(i64::to_string).collect();
        format!(
            "/FirstChar {first} /LastChar {} /Widths [{}] ",
            usize::from(first) + w.len() - 1,
            each.join(" ")
        )
    });
    body(&format!(
        "<< /Type /Font /Subtype /Type1 {name}/BaseFont /{base} {widths}>>"
    ))
}

/// A one-page file whose page lost its `/Font` and shows `content`, with
/// `fonts` written after the page's objects as unreachable orphans.
fn lost_slots(content: &str, fonts: &[(u32, Vec<u8>)]) -> Input {
    let mut objects = vec![
        (1, body(CATALOG)),
        (2, body(PAGES)),
        (3, page("<< >>")),
        (4, stream_body("", content)),
    ];
    objects.extend(fonts.iter().cloned());
    analysed(hand_made(&objects, true))
}

fn c6_findings(input: &Input) -> Vec<&Finding> {
    let c6 = FindingKind::Corruption(C6FontMapLost);
    input.findings.iter().filter(|f| f.class == c6).collect()
}

/// The C6 pass's actions, as text.
fn relinked(g: &Generated) -> Vec<String> {
    pass(g, C6FontMapLost)
        .actions
        .iter()
        .map(|a| a.what.clone())
        .collect()
}

// ── C9 ───────────────────────────────────────────────────────────────────

#[test]
fn the_c9_fixture_repairs_clean_with_exact_streams_as_the_original() {
    let golden = golden_pdf();
    let original = Document::load_mem(&golden).expect("the golden loads");
    for seed in [0, 1, 2, 4] {
        let input = analysed(corrupt(C9ZlibTampered, &golden, seed));
        assert!(input.classes().contains(&C9ZlibTampered), "seed {seed}");
        let (g, _) = run(&input);
        let out = g.output.as_deref().expect("an output");
        assert!(chosen(&g).verification.v0.all_pass(), "seed {seed}");
        assert!(
            !classes_in(out).contains(&C9ZlibTampered),
            "seed {seed}: {:?}",
            classes_in(out)
        );
        let written = Document::load_mem(out).expect("the output loads");
        let c9 = pass(&g, C9ZlibTampered);
        let mut exact = 0;
        for (&id, e) in &input.salvage.by_obj {
            let Salvage::Repaired {
                grade: Grade::Exact,
                ..
            } = e.salvage
            else {
                continue;
            };
            exact += 1;
            assert_eq!(
                raw_stream(&written, out_number(&input, id)),
                raw_stream(&original, id.0),
                "seed {seed}: {id:?}"
            );
            assert!(
                c9.actions
                    .iter()
                    .any(|a| a.object == id && a.grade == Some(SalvageGrade::Exact)),
                "seed {seed}: {:?}",
                c9.actions
            );
        }
        assert!(exact > 0, "seed {seed}: no Exact stream");
        assert_eq!(g.c9_summary.exact, exact, "seed {seed}");
        assert_eq!(g.c9_summary.accepted, 0, "seed {seed}");
        let streams = (input.salvage.by_obj.values())
            .filter(|e| !matches!(e.salvage, Salvage::Clean { .. }))
            .count();
        assert_eq!(
            g.c9_summary.streams_damaged as usize, streams,
            "seed {seed}"
        );
        assert!(!g.c9_survivors.is_empty(), "seed {seed}");
    }
}

/// D-041 / goal-r3-q12: under a small budget the small content streams
/// accept before their windows are exhausted. The ticket names `work = 1e5`;
/// that buys about 400 candidates, and no seed's stream accepts within them
/// (measured: each is `ChecksumMismatch` or `Prefix`), so this runs at 1e7.
#[test]
fn a_small_budget_reports_accepted_streams_apart_with_the_fixed_line() {
    let budget = SalvageBudget {
        work: 10_000_000,
        deep_work: 0,
        deep_pool: 0,
        ..SalvageBudget::default()
    };
    let input = analysed_with(corrupt(C9ZlibTampered, &golden_pdf(), 9), &budget);
    let small: Vec<ObjId> = (input.salvage.by_obj.iter())
        .filter(|(id, e)| {
            !matches!(e.salvage, Salvage::Clean { .. }) && input.carve.objects.iter().any(|o| {
                o.declared_id == **id
                    && matches!(o.body, Body::Stream { data, .. } if data.end - data.start <= 512)
            })
        })
        .map(|(id, _)| *id)
        .collect();
    assert!(small.len() >= 2, "{small:?}");
    for id in &small {
        assert!(
            matches!(
                input.salvage.by_obj[id].salvage,
                Salvage::Repaired {
                    grade: Grade::Accepted { .. },
                    ..
                }
            ),
            "{id:?}: {:?}",
            input.salvage.by_obj[id].salvage
        );
    }
    let accepted = (input.salvage.by_obj.values())
        .filter(|e| {
            matches!(
                e.salvage,
                Salvage::Repaired {
                    grade: Grade::Accepted { .. },
                    ..
                }
            )
        })
        .count() as u32;

    let (g, _) = run(&input);
    assert!(g.output.is_some());
    assert_eq!(g.c9_summary.accepted, accepted);
    assert_eq!(g.c9_summary.exact, 0);
    let c9 = pass(&g, C9ZlibTampered);
    for id in &small {
        let action = c9
            .actions
            .iter()
            .find(|a| a.object == *id)
            .expect("an action");
        assert_eq!(action.grade, Some(SalvageGrade::Accepted), "{action:?}");
        assert!(
            action.what.contains("without a uniqueness check"),
            "{action:?}"
        );
    }
    let lines = report_of(&g).lines();
    let line = format!("{accepted} accepted without a uniqueness check");
    assert!(lines.iter().any(|l| l.contains(&line)), "{lines:?}");
}

#[test]
fn the_ambiguous_synthetic_is_partial_with_both_survivors_in_the_report() {
    const CONTENT: ObjId = (11, 0);
    let golden = golden_pdf();
    let bytes = corrupt(C9ZlibTampered, &golden, 2);
    let mut salvage = analysed(bytes.clone()).salvage;
    let other = vec![(3, 0x00, 0x01)];
    let edits = match &mut salvage.by_obj.get_mut(&CONTENT).expect("hit").salvage {
        Salvage::Repaired {
            edits,
            grade,
            survivors,
            ..
        } => {
            *grade = Grade::Ambiguous { outputs: 2 };
            *survivors = vec![edits.clone(), other.clone()];
            edits.clone()
        }
        s => panic!("{s:?}"),
    };
    let input = with_index(bytes, salvage);
    let (g, _) = run(&input);
    let c9 = pass(&g, C9ZlibTampered);
    match &c9.outcome {
        PassOutcome::Partial(why) => assert!(
            why.contains("11 0 obj: ambiguous: 2 candidate repairs, see report"),
            "{why}"
        ),
        other => panic!("{other:?}"),
    }
    let action = c9
        .actions
        .iter()
        .find(|a| a.object == CONTENT)
        .expect("an action");
    assert_eq!(action.grade, Some(SalvageGrade::Ambiguous));
    assert_eq!(g.c9_summary.ambiguous, 1);
    assert_eq!(g.c9_survivors, vec![(CONTENT, vec![edits, other])]);
    let report = report_of(&g);
    assert_eq!(report.c9_survivors, g.c9_survivors);
    // The first survivor, which restores the original, is the one written.
    let out = g.output.as_deref().expect("an output");
    assert!(chosen(&g).verification.v0.all_pass());
    let original = Document::load_mem(&golden).expect("the golden loads");
    let written = Document::load_mem(out).expect("the output loads");
    assert_eq!(
        raw_stream(&written, out_number(&input, CONTENT)),
        raw_stream(&original, CONTENT.0)
    );
    assert!(g.partial_reasons.iter().any(|r| r.starts_with("C9: ")));
}

/// One page whose content stream 4 is Flate data nothing decodes.
fn with_unrecoverable_stream() -> (Vec<u8>, Vec<u8>) {
    let dead: Vec<u8> = (0u8..64).map(|i| 0xff - (i % 3)).collect();
    let mut stream = format!(
        "<< /Filter /FlateDecode /Length {} >>\nstream\n",
        dead.len()
    )
    .into_bytes();
    stream.extend_from_slice(&dead);
    stream.extend_from_slice(b"\nendstream");
    let objects = [
        (1, body(CATALOG)),
        (2, body(PAGES)),
        (
            3,
            body("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>"),
        ),
        (4, stream),
    ];
    (hand_made(&objects, true), dead)
}

#[test]
fn one_unrecoverable_stream_is_partial_and_the_output_passes_v0() {
    let (bytes, dead) = with_unrecoverable_stream();
    let input = analysed(bytes);
    assert_eq!(
        input.salvage.by_obj[&(4, 0)].salvage,
        Salvage::Unrecoverable
    );
    let (g, _) = run(&input);
    let c9 = pass(&g, C9ZlibTampered);
    assert_eq!(
        c9.outcome,
        PassOutcome::Partial("4 0 obj: unrecoverable stream".to_owned())
    );
    assert_eq!(c9.actions.len(), 1);
    assert_eq!(c9.actions[0].grade, None);
    assert!(c9.actions[0].what.contains("written as carved"));
    assert_eq!(g.c9_summary.unrecoverable, 1);
    assert_eq!(g.c9_summary.repaired, 0);
    let out = g.output.as_deref().expect("an output");
    assert!(chosen(&g).verification.v0.all_pass());
    // The dead stream is evidence: written exactly as carved, still C9.
    let written = Document::load_mem(out).expect("the output loads");
    assert_eq!(raw_stream(&written, out_number(&input, (4, 0))), dead);
    assert!(classes_in(out).contains(&C9ZlibTampered));
}

#[test]
fn an_unsearched_stream_is_partial_and_written_as_carved() {
    let budget = SalvageBudget {
        max_search_stream: 64,
        ..SalvageBudget::default()
    };
    let bytes = corrupt(C9ZlibTampered, &golden_pdf(), 1);
    let input = analysed_with(bytes.clone(), &budget);
    assert!(matches!(
        input.salvage.by_obj[&(12, 0)].salvage,
        Salvage::Unsearched { .. }
    ));
    let (g, _) = run(&input);
    let c9 = pass(&g, C9ZlibTampered);
    match &c9.outcome {
        PassOutcome::Partial(why) => assert!(
            why.contains("12 0 obj: stream over max_search_stream, not searched"),
            "{why}"
        ),
        other => panic!("{other:?}"),
    }
    let action = c9
        .actions
        .iter()
        .find(|a| a.object == (12, 0))
        .expect("an action");
    assert_eq!(action.grade, Some(SalvageGrade::Unsearched));
    assert_eq!(g.c9_summary.unsearched, 1);
    let out = g.output.as_deref().expect("an output");
    let carved = Document::load_mem(&bytes).expect("the input loads");
    let written = Document::load_mem(out).expect("the output loads");
    assert_eq!(
        raw_stream(&written, out_number(&input, (12, 0))),
        raw_stream(&carved, 12)
    );
}

#[test]
fn passes_without_c9_copy_every_stream_as_carved() {
    let bytes = corrupt(C9ZlibTampered, &golden_pdf(), 1);
    let input = analysed(bytes.clone());
    let opts = RepairOptions {
        passes: Some(vec![C1Header]),
        ..RepairOptions::default()
    };
    // C9 alone plans a Resave; with C9 left out nothing is selected.
    let mut findings = input.findings.clone();
    findings.push(Finding {
        id: "C1-001".to_owned(),
        class: FindingKind::Corruption(C1Header),
        severity: crate::pdf::model::Severity::Warning,
        location: Location::File,
        summary: String::new(),
        evidence: Vec::new(),
        repair: Repairability::Auto,
    });
    let input = Input { findings, ..input };
    let (g, _) = run_with(&input, &opts);
    assert!(matches!(
        pass(&g, C9ZlibTampered).outcome,
        PassOutcome::Skipped(_)
    ));
    assert_eq!(g.c9_summary, C9Summary::default());
    let out = g.output.as_deref().expect("an output");
    let carved = Document::load_mem(&bytes).expect("the input loads");
    let written = Document::load_mem(out).expect("the output loads");
    assert_eq!(
        raw_stream(&written, out_number(&input, (12, 0))),
        raw_stream(&carved, 12)
    );
}

// ── C6 ───────────────────────────────────────────────────────────────────

#[test]
fn the_c6_fixture_is_re_linked_and_its_v2_is_the_golden_s() {
    let input = analysed(golden_c6_with_orphan());
    assert_eq!(input.classes(), [C6FontMapLost]);
    let c6 = c6_findings(&input);
    assert_eq!(c6.len(), 2, "{:#?}", input.findings);
    assert!(c6.iter().all(|f| f.repair == Repairability::Auto));

    let (g, _) = run(&input);
    let report = pass(&g, C6FontMapLost);
    assert_eq!(report.outcome, PassOutcome::Fixed);
    let what = relinked(&g);
    assert_eq!(what.len(), 2, "{what:?}");
    for (k, w) in what.iter().enumerate() {
        assert!(
            w.starts_with(&format!("page {}: /F1 re-linked to 5 0 obj", k + 1)),
            "{w}"
        );
    }
    assert!(g.escalations.is_empty(), "{:?}", g.escalations);
    let out = g.output.as_deref().expect("an output");
    assert!(chosen(&g).verification.v0.all_pass());
    assert!(!classes_in(out).contains(&C6FontMapLost));

    let golden = golden_pdf();
    let golden_carve = carve(&golden, &|| false).expect("never cancelled");
    let own = verify(
        &golden,
        &golden_carve,
        &baseline(&golden, &golden_carve),
        &[],
        &[],
    );
    let v2 = &chosen(&g).verification.v2;
    assert_eq!(v2.unmapped_glyph, own.v2.unmapped_glyph);
    assert_eq!(v2.fffd, own.v2.fffd);
    assert_eq!(v2.script_consistency, own.v2.script_consistency);
    assert_eq!(v2.unmapped_glyph.num, 0);
}

#[test]
fn with_the_orphan_font_deleted_c6_escalates_a_font_pick() {
    let mut buf = golden_c6_with_orphan();
    // The Type0 font and its CIDFont gone: no font is left to re-link.
    for n in [5, 6] {
        let span = object_span(&buf, n);
        blank(&mut buf, span);
    }
    let input = analysed(buf);
    let c6 = c6_findings(&input);
    assert_eq!(c6.len(), 2, "{:#?}", input.findings);
    let pick = Repairability::Interactive(InteractionKind::FontPick);
    assert!(c6.iter().all(|f| f.repair == pick), "{c6:#?}");

    let opts = RepairOptions::default();
    let planned = plan(&input.findings, &input.carve, &input.graph, &opts);
    let picks: Vec<(Option<u32>, Option<String>)> = (planned.escalations.iter())
        .filter(|e| e.kind == EscalationKind::Interaction(InteractionKind::FontPick))
        .map(|e| (e.page, e.slot.clone()))
        .collect();
    assert_eq!(
        picks,
        [
            (Some(0), Some("F1".to_owned())),
            (Some(1), Some("F1".to_owned()))
        ]
    );
    let (g, _) = run(&input);
    let report = pass(&g, C6FontMapLost);
    assert!(report.actions.is_empty(), "{:?}", report.actions);
    assert!(matches!(report.outcome, PassOutcome::Partial(_)));
    // The plan escalated them; the pass does not ask twice.
    assert!(g.escalations.is_empty(), "{:?}", g.escalations);
}

#[test]
fn a_candidate_whose_widths_do_not_fit_is_no_match_and_the_slot_escalates() {
    // "Hi" is codes 72 and 105; the orphan has no width for 105.
    let input = lost_slots(
        "BT /F1 12 Tf (Hi) Tj ET",
        &[(5, type1(None, "Helvetica", Some((72, &[600, 0, 0]))))],
    );
    let c6 = c6_findings(&input);
    assert_eq!(c6.len(), 1, "{:#?}", input.findings);
    assert_eq!(c6[0].repair, Repairability::Auto);
    let (g, _) = run(&input);
    assert!(relinked(&g).is_empty());
    assert!(matches!(
        pass(&g, C6FontMapLost).outcome,
        PassOutcome::Partial(_)
    ));
    assert_eq!(g.escalations.len(), 1, "{:?}", g.escalations);
    let e = &g.escalations[0];
    assert_eq!(
        e.kind,
        EscalationKind::Interaction(InteractionKind::FontPick)
    );
    assert_eq!((e.page, e.slot.as_deref()), (Some(0), Some("F1")));
}

#[test]
fn the_re_link_never_reads_basefont_and_prefers_the_nearest_font() {
    // Font 6 is named like the slot by its `/BaseFont` only (a Print file's
    // `CIDFont+F1`), and is written last; font 5 is nearer the page.
    let input = lost_slots(
        "BT /F1 12 Tf (Hi) Tj ET",
        &[
            (5, type1(None, "Helvetica", None)),
            (6, type1(None, "CIDFont+F1", None)),
        ],
    );
    let (g, _) = run(&input);
    let what = relinked(&g);
    assert_eq!(what.len(), 1, "{what:?}");
    assert!(
        what[0].starts_with("page 1: /F1 re-linked to 5 0 obj, matched by its place in the file"),
        "{what:?}"
    );
}

#[test]
fn the_re_link_prefers_the_slot_name_then_widths_that_fit() {
    let hi: &[i64] = &[600; 34]; // codes 72..=105
    let input = lost_slots(
        "BT /F1 12 Tf (Hi) Tj /F2 12 Tf (Hi) Tj ET",
        &[
            // Nearest, no name, widths unknown.
            (5, type1(None, "Helvetica", None)),
            // Widths that fit, no name.
            (6, type1(None, "Helvetica", Some((72, hi)))),
            // Named for F2, farthest.
            (7, type1(Some("F2"), "Helvetica", None)),
        ],
    );
    let (g, _) = run(&input);
    let what = relinked(&g);
    assert_eq!(what.len(), 2, "{what:?}");
    assert!(
        what[0].starts_with("page 1: /F1 re-linked to 6 0 obj")
            && what[0].contains("its widths fit"),
        "{what:?}"
    );
    assert!(
        what[1].starts_with("page 1: /F2 re-linked to 7 0 obj, matched by its /Name"),
        "{what:?}"
    );
    assert_eq!(pass(&g, C6FontMapLost).outcome, PassOutcome::Fixed);
    // Both slots now resolve in the output.
    let out = g.output.as_deref().expect("an output");
    assert!(!classes_in(out).contains(&C6FontMapLost));
    let doc = Document::load_mem(out).expect("the output loads");
    let (_, &page) = doc.get_pages().iter().next().expect("a page");
    let fonts = doc.get_page_fonts(page).expect("page fonts");
    let names: Vec<&[u8]> = fonts.keys().map(Vec::as_slice).collect();
    assert_eq!(names, [&b"F1"[..], b"F2"]);
}

#[test]
fn a_slot_only_a_form_uses_is_not_c6_and_not_re_linked() {
    // The page draws form 5, whose own resources map /F1 to font 6; font 7
    // is an orphan that `/Name`s /F1. The file has no xref table (C2), so a
    // repair runs.
    let objects = [
        (1, body(CATALOG)),
        (2, body(PAGES)),
        (3, page("<< /XObject << /X1 5 0 R >> >>")),
        (4, stream_body("", "q /X1 Do Q")),
        (
            5,
            stream_body(
                "/Type /XObject /Subtype /Form /BBox [0 0 10 10] \
                 /Resources << /Font << /F1 6 0 R >> >>",
                "BT /F1 12 Tf (Hi) Tj ET",
            ),
        ),
        (6, type1(None, "Helvetica", None)),
        (7, type1(Some("F1"), "Helvetica", None)),
    ];
    let input = analysed(hand_made(&objects, false));
    assert!(c6_findings(&input).is_empty(), "{:#?}", input.findings);
    assert!(
        input.classes().contains(&C2XrefMissing),
        "{:?}",
        input.classes()
    );
    let (g, _) = run(&input);
    assert!(g.passes.iter().all(|p| p.class != C6FontMapLost));
    let out = g.output.as_deref().expect("an output");
    let doc = Document::load_mem(out).expect("the output loads");
    let (_, &page) = doc.get_pages().iter().next().expect("a page");
    let page = doc.get_dictionary(page).expect("the page");
    let resources = page.get(b"Resources").expect("resources");
    let resources = match resources {
        Object::Reference(id) => doc.get_dictionary(*id).expect("resources"),
        Object::Dictionary(d) => d,
        other => panic!("{other:?}"),
    };
    assert!(resources.get(b"Font").is_err(), "{resources:?}");
}
