//! T-30 acceptance: the C7 and C8 passes, template assembly and the font
//! questions, with the two-font test database (the test font, and the same
//! program with a shuffled `.gmap`) and a 50-word English list, as in T-28.

use std::collections::{BTreeMap, BTreeSet};

use lopdf::{Document, LoadOptions};

use super::*;
use crate::bench::metrics::Lang;
use crate::engine::{
    AnalysisResult, AnalyzeStats, CarveSummary, FontResolutionKind, Interact, InteractionReply,
    InteractionRequest, InteractionSource, PageSize, RepairReport, StateHandle, SubstituteChoice,
    UnreproduciblePolicy,
};
use crate::pdf::emit::{EmitCtx, Substitution, emit_doc, emit_template_assemble};
use crate::pdf::fixtures::TEST_FONT;
use crate::pdf::fontdb::build::{IndexEntry, build_from_ttf};
use crate::pdf::fontdb::dict::FrequencyList;
use crate::pdf::fontdb::gmap::{self, GmapRecord, GmapTable};
use crate::pdf::model::{FileMeta, InteractionKind};
use crate::pdf::text::{ExtractOptions, extract_text};

use CorruptionClass::{C7FontStreamDeleted, C8FontResourcesDeleted};

// ── the database and the word list ───────────────────────────────────────

const RIGHT: &str = "NotoSans-Regular";
const SHUFFLED: &str = "Shuffled-Regular";
const SPARSE: &str = "Sparse-Regular";

/// T-28's 50-word English list.
const ENGLISH: &str = "the 50\nof 49\nand 48\nto 47\na 46\nin 45\nis 44\nit 43\nyou 42\n\
that 41\nhe 40\nwas 39\nfor 38\non 37\nare 36\nwith 35\nas 34\nhis 33\nthey 32\nbe 31\n\
at 30\none 29\nhave 28\nthis 27\nfrom 26\nor 25\nhad 24\nby 23\nword 22\nbut 21\n\
what 20\nsome 19\nwe 18\ncan 17\nout 16\nother 15\nwere 14\nall 13\nthere 12\n\
when 11\npage 10\ngolden 9\npdfpundit 8\nquick 7\nbrown 6\nfox 5\njumps 4\nover 3\n\
lazy 2\ndog 1\n";

fn english() -> FrequencyList {
    FrequencyList::from_bytes(Lang::En, ENGLISH.as_bytes()).unwrap()
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
    let records = GmapTable::new(right).unwrap().records().to_vec();
    let n = records.len();
    let out = (0..n)
        .map(|i| {
            let r = records[i];
            GmapRecord::new(
                r.gid(),
                records[(i + 7) % n].unicode(),
                r.width(),
                r.source(),
            )
        })
        .collect();
    gmap::encode(out)
}

/// The test font's `.gmap` with only its digits: a font that reproduces
/// almost none of the golden's text.
fn sparse_gmap(right: &[u8]) -> Vec<u8> {
    let records = GmapTable::new(right).unwrap().records().to_vec();
    gmap::encode(
        records
            .into_iter()
            .filter(|r| r.unicode().is_ascii_digit())
            .collect(),
    )
}

/// A database of the fonts `ids` names, built from the test font.
fn db_of(ids: &[&str]) -> FontDb {
    let (right, right_gmap) = build_from_ttf(TEST_FONT).unwrap();
    let blobs: Vec<(&str, IndexEntry, Vec<u8>)> = ids
        .iter()
        .map(|&id| {
            let gmap = match id {
                RIGHT => right_gmap.clone(),
                SHUFFLED => shuffled_gmap(&right_gmap),
                SPARSE => sparse_gmap(&right_gmap),
                other => panic!("no test font {other}"),
            };
            let mut entry = right.clone();
            entry.id = id.to_owned();
            entry.family = id.trim_end_matches("-Regular").to_owned();
            entry.gmap_sha256 = hex(&gmap);
            (id, entry, gmap)
        })
        .collect();
    let index = serde_json::to_vec(&blobs.iter().map(|b| &b.1).collect::<Vec<_>>()).unwrap();
    let blob = |name: &str| -> Option<&[u8]> {
        let (id, ext) = name.rsplit_once('.')?;
        let (_, _, gmap) = blobs.iter().find(|b| b.0 == id)?;
        match ext {
            "ttf" => Some(TEST_FONT),
            "gmap" => Some(gmap),
            _ => None,
        }
    };
    FontDb::from_bytes(&index, &blob).expect("test db loads")
}

/// The two-font test database.
fn test_db() -> FontDb {
    db_of(&[RIGHT, SHUFFLED])
}

// ── fixtures ─────────────────────────────────────────────────────────────

/// The golden's font name, and a `CIDFont+F1` of the same length.
const GOLDEN_NAME: &[u8] = b"AAAAAA+NotoSans-Regular";
const PRINT_NAME: &[u8] = b"CIDFont+F1             ";

/// `pdf` with every `GOLDEN_NAME` turned into a `CIDFont+F1` name, padded
/// with spaces so that no offset moves.
fn renamed(pdf: &[u8]) -> Vec<u8> {
    assert_eq!(GOLDEN_NAME.len(), PRINT_NAME.len());
    let mut out = pdf.to_vec();
    let mut at = 0;
    while let Some(i) = out[at..]
        .windows(GOLDEN_NAME.len())
        .position(|w| w == GOLDEN_NAME)
    {
        out[at + i..at + i + PRINT_NAME.len()].copy_from_slice(PRINT_NAME);
        at += i + PRINT_NAME.len();
    }
    out
}

/// The golden's objects, edited by `edit`, written again.
fn rewrite_golden(edit: impl FnOnce(&mut BTreeMap<u32, Object>)) -> Vec<u8> {
    let doc = Document::load_mem(&golden_pdf()).unwrap();
    let mut objects: BTreeMap<u32, Object> = doc
        .objects
        .iter()
        .map(|(&(n, _), o)| (n, o.clone()))
        .collect();
    edit(&mut objects);
    let mut w = Writer::with_version("1.7");
    for (n, o) in objects {
        w.add(n, o);
    }
    w.trailer((1, 0), [7; 32], None);
    w.finish().unwrap()
}

/// The golden with a second copy of its font (objects 15 to 19) on page 2:
/// two fonts of one family.
fn two_fonts() -> Vec<u8> {
    fn shift(v: &mut Object) {
        match v {
            Object::Reference((n, _)) if (5..=9).contains(n) => *n += 10,
            Object::Array(a) => a.iter_mut().for_each(shift),
            Object::Dictionary(d) => d.iter_mut().for_each(|(_, o)| shift(o)),
            Object::Stream(s) => s.dict.iter_mut().for_each(|(_, o)| shift(o)),
            _ => {}
        }
    }
    rewrite_golden(|objects| {
        for n in 5..=9 {
            let mut copy = objects[&n].clone();
            shift(&mut copy);
            objects.insert(n + 10, copy);
        }
        let page2 = objects.get_mut(&4).unwrap().as_dict_mut().unwrap();
        let resources = page2.get_mut(b"Resources").unwrap().as_dict_mut().unwrap();
        let fonts = resources.get_mut(b"Font").unwrap().as_dict_mut().unwrap();
        fonts.set("F1", Object::Reference((15, 0)));
    })
}

/// The golden with page 1's resources in their own object (20), which a
/// form XObject (21) shares: page 1 draws the form (its content is a copy of
/// page 1's), so the form shows text through the same `/F1`.
fn shared_resources() -> Vec<u8> {
    rewrite_golden(|objects| {
        let page1 = objects.get_mut(&3).unwrap().as_dict_mut().unwrap();
        let mut resources = page1.get(b"Resources").unwrap().as_dict().unwrap().clone();
        let xobjects = resources
            .get_mut(b"XObject")
            .unwrap()
            .as_dict_mut()
            .unwrap();
        xobjects.set("Fm1", Object::Reference((21, 0)));
        page1.set("Resources", Object::Reference((20, 0)));
        page1.set(
            "Contents",
            Object::Array(vec![Object::Reference((11, 0)), Object::Reference((22, 0))]),
        );
        objects.insert(20, Object::Dictionary(resources));
        let content = objects[&11]
            .as_stream()
            .unwrap()
            .decompressed_content()
            .unwrap();
        let mut form = Dictionary::new();
        form.set("Type", Object::Name(b"XObject".to_vec()));
        form.set("Subtype", Object::Name(b"Form".to_vec()));
        let bbox = [0, 0, 612, 792].map(Object::Integer).to_vec();
        form.set("BBox", Object::Array(bbox));
        form.set("Resources", Object::Reference((20, 0)));
        objects.insert(21, Object::Stream(Stream::new(form, content)));
        let draw = b"q 1 0 0 1 0 -300 cm /Fm1 Do Q\n".to_vec();
        objects.insert(22, Object::Stream(Stream::new(Dictionary::new(), draw)));
    })
}

/// The golden with the text of its first `/ToUnicode` entry followed by a
/// combining acute: one code whose text is two characters.
fn combining_tounicode() -> Vec<u8> {
    rewrite_golden(|objects| {
        let Object::Stream(s) = objects.get_mut(&9).unwrap() else {
            panic!("the golden's /ToUnicode")
        };
        let cmap = String::from_utf8(s.decompressed_content().unwrap()).unwrap();
        let at = cmap.find("beginbfchar\n").unwrap() + "beginbfchar\n".len();
        let end = at + cmap[at..].find('\n').unwrap();
        let line = &cmap[at..end];
        let edited = format!("{}0301>", &line[..line.len() - 1]);
        let cmap = format!("{}{edited}{}", &cmap[..at], &cmap[end..]);
        s.dict.remove(b"Filter");
        s.set_content(cmap.into_bytes());
    })
}

fn c7() -> Input {
    let input = analysed(corrupt(C7FontStreamDeleted, &golden_pdf(), 0));
    assert_eq!(input.classes(), [C7FontStreamDeleted]);
    input
}

fn c8() -> Input {
    let input = analysed(corrupt(C8FontResourcesDeleted, &golden_pdf(), 0));
    assert_eq!(input.classes(), [C8FontResourcesDeleted]);
    input
}

/// The golden's text, page by page, as T-36 extracts it.
fn golden_text() -> Vec<String> {
    GOLDEN_TEXT.iter().map(|lines| lines.concat()).collect()
}

/// `pdf`'s text, page by page; `?` for an unmapped glyph.
fn text_of(pdf: &[u8]) -> Vec<String> {
    extract_text(pdf, &ExtractOptions::default())
        .unwrap()
        .iter()
        .map(|p| {
            p.glyphs
                .iter()
                .map(|g| g.text.clone().unwrap_or_else(|| "?".to_owned()))
                .collect()
        })
        .collect()
}

fn load_strict(pdf: &[u8]) -> Document {
    let opts = LoadOptions {
        strict: true,
        max_decompressed_size: Some(64 << 20),
        ..LoadOptions::default()
    };
    Document::load_mem_with_options(pdf, opts).expect("strict reload")
}

// ── running one pass ─────────────────────────────────────────────────────

/// An `Interact` that answers from a script and keeps every question.
struct Scripted {
    replies: Vec<InteractionReply>,
    asked: Vec<InteractionRequest>,
}

impl Scripted {
    fn new(replies: Vec<InteractionReply>) -> Self {
        Scripted {
            replies,
            asked: Vec::new(),
        }
    }
}

impl Interact for Scripted {
    fn ask(&mut self, req: InteractionRequest) -> Result<InteractionReply, Cancelled> {
        self.asked.push(req);
        assert!(!self.replies.is_empty(), "asked more than scripted");
        Ok(self.replies.remove(0))
    }
}

/// What one font pass did in a `TemplateAssemble` candidate, and the file
/// written after it.
struct Ran {
    report: PassReport,
    notes: PassNotes,
    output: Vec<u8>,
}

fn run_pass(
    input: &Input,
    class: CorruptionClass,
    fonts: &FontDb,
    opts: &RepairOptions,
    ask: &mut dyn Interact,
    dicts: &[&dyn WordList],
) -> Ran {
    let remap = plan_ids(&input.carve, &input.graph);
    let tree = rebuild_page_tree(&input.carve, &input.graph, &remap, opts.default_page_size);
    let mut doc = RebuildDoc::new(remap.clone());
    let findings: Vec<Finding> = (input.findings.iter())
        .filter(|f| f.class == FindingKind::Corruption(class))
        .cloned()
        .collect();
    let mut sink = NullProgress;
    let mut ctx = RepairCtx {
        bytes: &input.bytes,
        carve: &input.carve,
        graph: &input.graph,
        remap: &remap,
        page_tree: &tree,
        doc: &mut doc,
        salvage: &input.salvage,
        toolpath: Toolpath::TemplateAssemble,
        opts,
        fonts,
        dicts,
        ask,
        sink: &mut sink,
        notes: PassNotes::default(),
    };
    let pass = super::super::pass_for(class).expect("a font pass");
    let report = pass.repair(&mut ctx, &findings);
    let notes = std::mem::take(&mut ctx.notes);
    let mut sink = NullProgress;
    let mut emit = EmitCtx {
        bytes: &input.bytes,
        input_sha256: Sha256::digest(&input.bytes).into(),
        salvage: &input.salvage,
        sink: &mut sink,
        notes: Default::default(),
    };
    let output = emit_doc(doc, &input.carve, &input.graph, &tree, &mut emit).expect("emits");
    Ran {
        report,
        notes,
        output,
    }
}

fn run_db(input: &Input, fonts: &FontDb, opts: &RepairOptions) -> (Generated, Recorder) {
    let plan = plan(&input.findings, &input.carve, &input.graph, opts);
    let mut sink = Recorder::default();
    let g = generate_and_validate(&input.view(), &plan, opts, fonts, &mut UseBest, &mut sink)
        .expect("never cancelled");
    (g, sink)
}

/// Options under which the scorer never auto-accepts: every slot asks.
fn always_ask() -> RepairOptions {
    RepairOptions {
        auto_accept_confidence: Ratio { num: 2, den: 1 },
        ..RepairOptions::default()
    }
}

fn provenance(notes: &PassNotes) -> Vec<String> {
    notes
        .resolutions
        .iter()
        .flat_map(|(_, _, r)| r.provenance.iter().cloned())
        .collect()
}

fn actions(report: &PassReport) -> Vec<&str> {
    report.actions.iter().map(|a| a.what.as_str()).collect()
}

// ── C7 ───────────────────────────────────────────────────────────────────

#[test]
fn c7_under_use_best_is_substituted_and_reads_as_the_golden() {
    let input = c7();
    let db = test_db();
    let opts = RepairOptions::default();
    let (g, _) = run_db(&input, &db, &opts);

    assert_eq!(g.chosen, Some(Toolpath::TemplateAssemble));
    let tooled: Vec<Toolpath> = g.candidates.iter().map(|c| c.toolpath).collect();
    assert_eq!(tooled, [Toolpath::TemplateAssemble, Toolpath::Resave]);
    // Not by V2: the surviving /ToUnicode gives Resave's output the same
    // text, so V0 to V3 tie and the font track's prior decides (plan.rs).
    let [assembled, resaved] = &g.candidates[..] else {
        panic!("{:?}", g.candidates)
    };
    assert_eq!(assembled.verification.v2, resaved.verification.v2);
    assert_eq!(assembled.verification.v1, resaved.verification.v1);
    let v = &chosen(&g).verification;
    assert!(v.v0.all_pass(), "{v:?}");
    assert_eq!(v.v2.unmapped_glyph.num, 0, "{v:?}");
    let output = g.output.clone().unwrap();
    assert_eq!(text_of(&output), golden_text());
    assert!(!classes_in(&output).contains(&C7FontStreamDeleted));

    // The report lists the substitution and the database it came from.
    let analysis = AnalysisResult {
        meta: FileMeta {
            version: Some("1.7".to_owned()),
            pages: 2,
            title: None,
            page_sizes: Vec::new(),
        },
        findings: input.findings.clone(),
        carve: CarveSummary::default(),
        font_slots: Vec::new(),
        stats: AnalyzeStats::default(),
        input_sha256: Sha256::digest(&input.bytes).into(),
        state: StateHandle::default(),
    };
    let mut report = RepairReport::default_for(&analysis, &opts, &db);
    g.record(&mut report);
    assert_eq!(report.font_db_sha256, db.sha256());
    let c7 = report
        .passes
        .iter()
        .find(|p| p.class == C7FontStreamDeleted)
        .unwrap();
    assert_eq!(c7.outcome, PassOutcome::Fixed);
    assert_eq!(
        actions(c7),
        [format!("font program substituted: {RIGHT}").as_str()]
    );
    assert_eq!(c7.actions[0].object, (7, 0), "the descriptor");
    // The /ToUnicode dictionary pinned the font: no question was asked.
    assert!(report.interactions.is_empty());
    assert!(g.resolutions.iter().all(|(_, _, r)| matches!(
        &r.kind,
        FontResolutionKind::Picked { font_id, .. } if font_id == RIGHT
    )));
}

#[test]
fn the_resave_candidate_keeps_the_font_and_says_so() {
    let input = c7();
    let (g, _) =
        super::seam::with_broken_assembly(|| run_db(&input, &test_db(), &RepairOptions::default()));
    // Template assembly fails V0, so Resave is the fallback.
    assert_eq!(g.chosen, Some(Toolpath::Resave));
    let assembled = &g.candidates[0];
    assert_eq!(assembled.toolpath, Toolpath::TemplateAssemble);
    assert!(!assembled.verification.v0.all_pass());
    assert!(chosen(&g).verification.v0.all_pass());
    assert!(
        matches!(&pass(&g, C7FontStreamDeleted).outcome,
            PassOutcome::Partial(why) if why.contains("keeps the font as found")),
        "{:?}",
        g.passes
    );
    assert!(g.partial_reasons.iter().any(|r| r.starts_with("C7")));
}

#[test]
fn c8_beats_resave_on_v2() {
    let input = c8();
    let (g, _) = run_db(&input, &test_db(), &RepairOptions::default());
    assert_eq!(g.chosen, Some(Toolpath::TemplateAssemble));
    let [assembled, resaved] = &g.candidates[..] else {
        panic!("{:?}", g.candidates)
    };
    assert!(assembled.verification.v0.all_pass());
    assert!(resaved.verification.v0.all_pass());
    assert_eq!(assembled.verification.v2.unmapped_glyph.num, 0);
    assert!(resaved.verification.v2.unmapped_glyph.num > 0);
    assert_eq!(text_of(g.output.as_ref().unwrap()), golden_text());
}

// ── C8 ───────────────────────────────────────────────────────────────────

#[test]
fn c8_with_its_real_base_font_takes_the_name_path() {
    let input = c8();
    let english = english();
    let ran = run_pass(
        &input,
        C8FontResourcesDeleted,
        &test_db(),
        &RepairOptions::default(),
        &mut Scripted::new(Vec::new()),
        &[&english],
    );
    assert_eq!(ran.report.outcome, PassOutcome::Fixed);
    let what = actions(&ran.report);
    assert_eq!(what.len(), 1);
    assert!(what[0].starts_with(&format!(
        "font program substituted: {RIGHT}; /ToUnicode rebuilt"
    )));
    let lines = provenance(&ran.notes);
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("name:") && l.contains("glyphs agree with")),
        "{lines:?}"
    );
    assert!(!lines.iter().any(|l| l.contains("rejected")), "{lines:?}");
    assert!(
        !lines.iter().any(|l| l.starts_with("inference:")),
        "{lines:?}"
    );
    assert_eq!(text_of(&ran.output), golden_text());
    load_strict(&ran.output);
}

#[test]
fn hostile_w_widths_reject_the_name_match_without_overflow() {
    // The first two widths of each /W run: i64::MIN, and a Real that a cast
    // saturates to i64::MIN. A subtraction would overflow on either.
    let pdf = rewrite_golden(|objects| {
        for o in objects.values_mut() {
            let Object::Dictionary(d) = o else { continue };
            let Ok(Object::Array(w)) = d.get_mut(b"W") else {
                continue;
            };
            for item in w.iter_mut() {
                if let Object::Array(run) = item {
                    let hostile = [Object::Integer(i64::MIN), Object::Real(-1e30)];
                    for (slot, v) in run.iter_mut().zip(hostile) {
                        *slot = v;
                    }
                }
            }
        }
    });
    let input = analysed(corrupt(C8FontResourcesDeleted, &pdf, 0));
    assert_eq!(input.classes(), [C8FontResourcesDeleted]);
    let ran = run_pass(
        &input,
        C8FontResourcesDeleted,
        &test_db(),
        &RepairOptions::default(),
        &mut UseBest,
        &[],
    );
    let lines = provenance(&ran.notes);
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("name:") && l.contains("/W widths differ")),
        "{lines:?}"
    );
}

#[test]
fn c8_with_a_cidfont_name_goes_to_inference() {
    let input = analysed(renamed(&corrupt(C8FontResourcesDeleted, &golden_pdf(), 0)));
    assert_eq!(input.classes(), [C8FontResourcesDeleted]);
    let english = english();
    let ran = run_pass(
        &input,
        C8FontResourcesDeleted,
        &test_db(),
        &RepairOptions::default(),
        &mut UseBest,
        &[&english],
    );
    let lines = provenance(&ran.notes);
    assert!(!lines.iter().any(|l| l.starts_with("name:")), "{lines:?}");
    assert!(
        lines.iter().any(|l| l.starts_with("inference:")),
        "{lines:?}"
    );
    assert!(
        actions(&ran.report)[0].starts_with(&format!("font program substituted: {RIGHT}")),
        "{:?}",
        ran.report
    );
    assert_eq!(text_of(&ran.output), golden_text());
}

// ── UseBest on a weak C8 guess (G-02, D-122 (b)) ─────────────────────────

/// The C8 fixture with a `CIDFont+F1` name, so the slot goes to inference.
fn c8_unnamed() -> Input {
    let input = analysed(renamed(&corrupt(C8FontResourcesDeleted, &golden_pdf(), 0)));
    assert_eq!(input.classes(), [C8FontResourcesDeleted]);
    input
}

/// `PRINT_NAME` as a name token: the padding ends it.
const PRINT_BASE_FONT: &[u8] = b"CIDFont+F1";

/// The font dictionary page `page`'s `/F1` names in `doc`.
fn f1_font(doc: &Document, page: u32) -> Dictionary {
    let page_id = doc.get_pages()[&(page + 1)];
    let page = doc.get_dictionary(page_id).unwrap();
    let resources = page.get(b"Resources").unwrap().as_dict().unwrap();
    let fonts = resources.get(b"Font").unwrap().as_dict().unwrap();
    let id = fonts.get(b"F1").unwrap().as_reference().unwrap();
    doc.get_dictionary(id).unwrap().clone()
}

/// The one `FontPick` asked, and its top candidate's hit.
fn top_hit(ask: &Scripted) -> Ratio {
    let [InteractionRequest::FontPick(req)] = &ask.asked[..] else {
        panic!("{:?}", ask.asked)
    };
    assert_eq!(req.candidates[0].font_id, RIGHT, "the best first");
    req.candidates[0].score
}

#[test]
fn use_best_on_a_weak_c8_guess_recovers_text_without_substituting_a_font() {
    // No word list (as in production): the top candidate's hit is 0.
    let input = c8_unnamed();
    let mut ask = Scripted::new(vec![InteractionReply::UseBest]);
    let ran = run_pass(
        &input,
        C8FontResourcesDeleted,
        &db_of(&[RIGHT]),
        &RepairOptions::default(),
        &mut ask,
        &[],
    );
    assert!(top_hit(&ask) < Ratio { num: 1, den: 2 });

    assert_eq!(
        ran.report.outcome,
        PassOutcome::Partial("text recovered; font not confirmed".to_owned())
    );
    let what = actions(&ran.report);
    assert_eq!(what.len(), 1, "{what:?}");
    assert!(
        what[0].starts_with("text recovered, no font substituted: /ToUnicode rebuilt over")
            && what[0].contains(RIGHT),
        "{what:?}"
    );
    assert!(!what[0].contains("font program substituted"), "{what:?}");
    let rec = record(&ran.notes);
    assert_eq!(
        (rec.reply.clone(), rec.source),
        (InteractionReply::UseBest, InteractionSource::UseBest)
    );
    assert_eq!(ran.notes.resolutions.len(), 2);
    assert!(
        (ran.notes.resolutions.iter()).all(|(_, _, r)| r.kind == FontResolutionKind::TextOnly),
        "{:?}",
        ran.notes.resolutions
    );

    // No font program was substituted: each page's /F1 is the font as found,
    // still without a program, now with a /ToUnicode.
    let doc = load_strict(&ran.output);
    for page in 0..2 {
        let font = f1_font(&doc, page);
        assert_eq!(
            font.get(b"BaseFont").unwrap().as_name().unwrap(),
            PRINT_BASE_FONT,
            "page {page}"
        );
    }
    let classes = classes_in(&ran.output);
    assert!(classes.contains(&C7FontStreamDeleted), "{classes:?}");
    assert!(!classes.contains(&C8FontResourcesDeleted), "{classes:?}");
    // The rebuilt /ToUnicode extracts the text.
    assert_eq!(text_of(&ran.output), golden_text());
}

#[test]
fn use_best_on_a_strong_c8_guess_still_substitutes() {
    // The 50-word list confirms the top candidate (hit ≥ 1/2); the options
    // keep it from auto-accepting, so the question is asked.
    let input = c8_unnamed();
    let english = english();
    let mut ask = Scripted::new(vec![InteractionReply::UseBest]);
    let ran = run_pass(
        &input,
        C8FontResourcesDeleted,
        &db_of(&[RIGHT]),
        &always_ask(),
        &mut ask,
        &[&english],
    );
    assert!(top_hit(&ask) >= Ratio { num: 1, den: 2 });
    assert!(
        matches!(&ran.report.outcome, PassOutcome::Partial(why)
            if why.starts_with("best candidate substituted unconfirmed")),
        "{:?}",
        ran.report
    );
    let what = actions(&ran.report);
    assert!(
        what[0].starts_with(&format!(
            "font program substituted: {RIGHT}; /ToUnicode rebuilt"
        )),
        "{what:?}"
    );
    assert!(ran.notes.resolutions.iter().all(|(_, _, r)| matches!(
        &r.kind,
        FontResolutionKind::Picked { font_id, .. } if font_id == RIGHT
    )));
    let doc = load_strict(&ran.output);
    assert_ne!(
        f1_font(&doc, 0)
            .get(b"BaseFont")
            .unwrap()
            .as_name()
            .unwrap(),
        PRINT_BASE_FONT
    );
    assert!(!classes_in(&ran.output).contains(&C7FontStreamDeleted));
    assert_eq!(text_of(&ran.output), golden_text());
}

#[test]
fn a_weak_c8_guess_under_use_best_reaches_the_engine_report() {
    let bytes = renamed(&corrupt(C8FontResourcesDeleted, &golden_pdf(), 0));
    let mut ask = Scripted::new(vec![InteractionReply::UseBest]);
    let out = through_engine(
        &bytes,
        &db_of(&[RIGHT]),
        &RepairOptions::default(),
        &mut ask,
    );
    assert_eq!(ask.asked.len(), 1);
    let output = out.output.expect("an output");
    assert_eq!(text_of(&output), golden_text());
    let doc = load_strict(&output);
    assert_eq!(
        f1_font(&doc, 0)
            .get(b"BaseFont")
            .unwrap()
            .as_name()
            .unwrap(),
        PRINT_BASE_FONT
    );
    let c8 = (out.report.passes.iter())
        .find(|p| p.class == C8FontResourcesDeleted)
        .expect("the C8 pass");
    assert_eq!(
        c8.outcome,
        PassOutcome::Partial("text recovered; font not confirmed".to_owned())
    );
    assert!(
        c8.actions[0]
            .what
            .starts_with("text recovered, no font substituted"),
        "{:?}",
        c8.actions
    );
}

/// `two_fonts` renamed, with both font programs blank and page 2's font's
/// `/ToUnicode` blank too: a C7 font on page 1 and a C8 font on page 2.
fn c7_and_c8() -> Input {
    let doc = Document::load_mem(&renamed(&two_fonts())).unwrap();
    let mut objects: BTreeMap<u32, Object> = (doc.objects.iter())
        .map(|(&(n, _), o)| (n, o.clone()))
        .collect();
    for n in [8, 18, 19] {
        let Object::Stream(s) = objects.get_mut(&n).unwrap() else {
            panic!("object {n} is a stream")
        };
        s.dict.remove(b"Filter");
        s.set_content(b"    ".to_vec());
    }
    let mut w = Writer::with_version("1.7");
    for (n, o) in objects {
        w.add(n, o);
    }
    w.trailer((1, 0), [7; 32], None);
    let input = analysed(w.finish().unwrap());
    assert_eq!(
        input.classes(),
        [C7FontStreamDeleted, C8FontResourcesDeleted]
    );
    input
}

#[test]
fn a_recovered_c8_font_beside_a_c7_font_still_verifies() {
    // The recovered font has a /ToUnicode and no program, so the output's
    // re-diagnosis calls it C7: the C8 pass's Partial must excuse that.
    let input = c7_and_c8();
    let opts = RepairOptions::default();
    let plan = plan(&input.findings, &input.carve, &input.graph, &opts);
    let mut ask = Scripted::new(vec![InteractionReply::UseBest, InteractionReply::UseBest]);
    let g = generate_and_validate(
        &input.view(),
        &plan,
        &opts,
        &db_of(&[RIGHT]),
        &mut ask,
        &mut NullProgress,
    )
    .unwrap();
    assert_eq!(
        g.chosen,
        Some(Toolpath::TemplateAssemble),
        "{:?}",
        g.candidates
    );
    assert!(chosen(&g).verification.v0.all_pass(), "{:?}", chosen(&g));
    assert_eq!(
        pass(&g, C8FontResourcesDeleted).outcome,
        PassOutcome::Partial("text recovered; font not confirmed".to_owned())
    );
    assert_eq!(text_of(g.output.as_ref().unwrap()), golden_text());
}

// ── the questions ────────────────────────────────────────────────────────

/// The C7 fixture's pass under `opts` and `replies`.
fn c7_with(
    fonts: &FontDb,
    opts: &RepairOptions,
    replies: Vec<InteractionReply>,
) -> (Ran, Scripted) {
    let mut ask = Scripted::new(replies);
    let ran = run_pass(&c7(), C7FontStreamDeleted, fonts, opts, &mut ask, &[]);
    (ran, ask)
}

fn record(notes: &PassNotes) -> &InteractionRecord {
    assert_eq!(notes.interactions.len(), 1, "{:?}", notes.interactions);
    &notes.interactions[0]
}

/// What one `FontPick` reply must come to.
struct PickCase {
    reply: InteractionReply,
    /// The action's text starts with this.
    action: String,
    outcome: fn(&PassOutcome) -> bool,
    source: InteractionSource,
    kind: fn(&FontResolutionKind) -> bool,
    /// The damaged font is still in the output.
    kept: bool,
}

#[test]
fn font_pick_replies_give_the_documented_outcomes() {
    let db = test_db();
    let opts = always_ask();
    let substituted = |id: &str| format!("font program substituted: {id}");
    let shuffled = SubstituteChoice {
        font_id: SHUFFLED.to_owned(),
        label: "Shuffled".to_owned(),
    };
    let cases = [
        PickCase {
            reply: InteractionReply::Pick(SHUFFLED.to_owned()),
            action: substituted(SHUFFLED),
            outcome: |o| *o == PassOutcome::Fixed,
            source: InteractionSource::User,
            kind: |k| matches!(k, FontResolutionKind::Picked { font_id, .. } if font_id == SHUFFLED),
            kept: false,
        },
        PickCase {
            reply: InteractionReply::UseBest,
            action: substituted(RIGHT),
            outcome: |o| {
                matches!(o, PassOutcome::Partial(why)
                    if why.starts_with("best candidate substituted unconfirmed"))
            },
            source: InteractionSource::UseBest,
            kind: |k| matches!(k, FontResolutionKind::Picked { font_id, .. } if font_id == RIGHT),
            kept: false,
        },
        PickCase {
            reply: InteractionReply::Skip,
            action: substituted(RIGHT),
            outcome: |o| {
                *o == PassOutcome::Partial("font substituted without confirmation".to_owned())
            },
            source: InteractionSource::User,
            kind: |k| matches!(k, FontResolutionKind::Picked { font_id, .. } if font_id == RIGHT),
            kept: false,
        },
        PickCase {
            reply: InteractionReply::Substitute(shuffled.clone()),
            action: substituted(SHUFFLED),
            outcome: |o| *o == PassOutcome::Fixed,
            source: InteractionSource::User,
            kind: |k| matches!(k, FontResolutionKind::Substituted(c) if c.font_id == SHUFFLED),
            kept: false,
        },
        PickCase {
            reply: InteractionReply::TextOnly,
            action: "text only: no reproducible font".to_owned(),
            outcome: |o| *o == PassOutcome::Partial("text only: no reproducible font".to_owned()),
            source: InteractionSource::User,
            kind: |k| *k == FontResolutionKind::TextOnly,
            kept: true,
        },
    ];
    for case in cases {
        let reply = case.reply.clone();
        let (ran, ask) = c7_with(&db, &opts, vec![reply.clone()]);
        let [InteractionRequest::FontPick(req)] = &ask.asked[..] else {
            panic!("{:?}", ask.asked)
        };
        assert_eq!((req.page, req.slot.as_str()), (0, "F1"));
        assert_eq!(req.candidates[0].font_id, RIGHT, "the best first");
        assert!(
            (case.outcome)(&ran.report.outcome),
            "{reply:?}: {:?}",
            ran.report
        );
        let what = actions(&ran.report);
        assert_eq!(what.len(), 1, "{reply:?}");
        assert!(what[0].starts_with(&case.action), "{reply:?}: {what:?}");
        let rec = record(&ran.notes);
        assert_eq!(rec.request.kind, InteractionKind::FontPick);
        assert_eq!(rec.request.candidates, [RIGHT, SHUFFLED]);
        assert_eq!(
            (rec.reply.clone(), rec.source),
            (reply.clone(), case.source)
        );
        assert_eq!(ran.notes.resolutions.len(), 2, "{reply:?}");
        assert!(
            ran.notes
                .resolutions
                .iter()
                .all(|(_, _, r)| (case.kind)(&r.kind)),
            "{reply:?}: {:?}",
            ran.notes.resolutions
        );
        assert_eq!(
            classes_in(&ran.output).contains(&C7FontStreamDeleted),
            case.kept,
            "{reply:?}"
        );
        // The /ToUnicode text survives whichever font draws it.
        assert_eq!(text_of(&ran.output), golden_text(), "{reply:?}");
    }
}

/// The sparse database: no font maps the golden's text, so every slot is
/// unreproducible.
fn sparse_db() -> FontDb {
    db_of(&[SPARSE])
}

fn sparse_choice() -> SubstituteChoice {
    SubstituteChoice {
        font_id: SPARSE.to_owned(),
        label: "Sparse".to_owned(),
    }
}

#[test]
fn font_unreproducible_replies_give_the_documented_outcomes() {
    let db = sparse_db();
    let opts = RepairOptions::default();

    let (ran, ask) = c7_with(
        &db,
        &opts,
        vec![InteractionReply::Substitute(sparse_choice())],
    );
    let [InteractionRequest::FontUnreproducible(req)] = &ask.asked[..] else {
        panic!("{:?}", ask.asked)
    };
    assert_eq!(req.options, [sparse_choice()]);
    assert!(
        matches!(&ran.report.outcome, PassOutcome::Partial(why) if why.starts_with("generic font substituted"))
    );
    assert_eq!(
        actions(&ran.report),
        [format!("font program substituted: {SPARSE}").as_str()]
    );
    let rec = record(&ran.notes);
    assert_eq!(rec.request.kind, InteractionKind::FontUnreproducible);
    assert_eq!(rec.reply, InteractionReply::Substitute(sparse_choice()));
    assert_eq!(rec.source, InteractionSource::User);
    assert_eq!(text_of(&ran.output), golden_text());

    let (ran, _) = c7_with(&db, &opts, vec![InteractionReply::TextOnly]);
    assert_eq!(
        ran.report.outcome,
        PassOutcome::Partial("text only: no reproducible font".to_owned())
    );
    assert!(actions(&ran.report)[0].starts_with("text only: no reproducible font"));
    let rec = record(&ran.notes);
    assert_eq!(
        (rec.reply.clone(), rec.source),
        (InteractionReply::TextOnly, InteractionSource::User)
    );
    assert!(
        ran.notes
            .resolutions
            .iter()
            .all(|(_, _, r)| r.kind == FontResolutionKind::TextOnly)
    );
    // The font is kept as found, and its /ToUnicode text with it.
    assert!(classes_in(&ran.output).contains(&C7FontStreamDeleted));
    assert_eq!(text_of(&ran.output), golden_text());
}

#[test]
fn one_family_is_asked_once_and_every_slot_takes_the_answer() {
    let input = analysed(corrupt(C7FontStreamDeleted, &two_fonts(), 0));
    let c7: Vec<&Finding> = (input.findings.iter())
        .filter(|f| f.class == FindingKind::Corruption(C7FontStreamDeleted))
        .collect();
    assert_eq!(c7.len(), 2, "two damaged fonts");
    let mut ask = Scripted::new(vec![InteractionReply::Substitute(sparse_choice())]);
    let ran = run_pass(
        &input,
        C7FontStreamDeleted,
        &sparse_db(),
        &RepairOptions::default(),
        &mut ask,
        &[],
    );
    let [InteractionRequest::FontUnreproducible(req)] = &ask.asked[..] else {
        panic!("{:?}", ask.asked)
    };
    assert_eq!(req.family, "NotoSans-Regular");
    assert_eq!(req.slots, [(0, "F1".to_owned()), (1, "F1".to_owned())]);
    assert_eq!(ran.notes.interactions.len(), 1);
    let slots: Vec<(u32, &str)> = (ran.notes.resolutions.iter())
        .map(|(p, s, _)| (*p, s.as_str()))
        .collect();
    assert_eq!(slots, [(0, "F1"), (1, "F1")]);
    assert!(
        ran.notes
            .resolutions
            .iter()
            .all(|(_, _, r)| r.kind == FontResolutionKind::Substituted(sparse_choice()))
    );
    assert_eq!(ran.report.actions.len(), 2);
    assert_eq!(text_of(&ran.output), golden_text());
}

#[test]
fn two_nameless_fonts_in_one_slot_name_are_asked_apart() {
    // `two_fonts` with no /BaseFont and no /FontName anywhere: both fonts
    // are only "F1", on different pages.
    let nameless = {
        let doc = Document::load_mem(&two_fonts()).unwrap();
        let mut objects: BTreeMap<u32, Object> = (doc.objects.iter())
            .map(|(&(n, _), o)| (n, o.clone()))
            .collect();
        for o in objects.values_mut() {
            if let Object::Dictionary(d) = o {
                d.remove(b"BaseFont");
                d.remove(b"FontName");
            }
        }
        let mut w = Writer::with_version("1.7");
        for (n, o) in objects {
            w.add(n, o);
        }
        w.trailer((1, 0), [7; 32], None);
        w.finish().unwrap()
    };
    let input = analysed(corrupt(C7FontStreamDeleted, &nameless, 0));
    assert_eq!(input.classes(), [C7FontStreamDeleted]);
    let mut ask = Scripted::new(vec![
        InteractionReply::Substitute(sparse_choice()),
        InteractionReply::TextOnly,
    ]);
    let ran = run_pass(
        &input,
        C7FontStreamDeleted,
        &sparse_db(),
        &RepairOptions::default(),
        &mut ask,
        &[],
    );
    let slots: Vec<Vec<(u32, String)>> = (ask.asked.iter())
        .map(|r| match r {
            InteractionRequest::FontUnreproducible(u) => u.slots.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        slots,
        [vec![(0, "F1".to_owned())], vec![(1, "F1".to_owned())]]
    );
    let kinds: Vec<&FontResolutionKind> = (ran.notes.resolutions.iter())
        .map(|(_, _, r)| &r.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            &FontResolutionKind::Substituted(sparse_choice()),
            &FontResolutionKind::TextOnly
        ]
    );
}

#[test]
fn a_policy_answers_without_asking() {
    let input = analysed(corrupt(C7FontStreamDeleted, &two_fonts(), 0));
    for (policy, reply) in [
        (
            UnreproduciblePolicy::SubstituteGeneric,
            InteractionReply::Substitute(sparse_choice()),
        ),
        (UnreproduciblePolicy::TextOnly, InteractionReply::TextOnly),
    ] {
        let opts = RepairOptions {
            unreproducible: policy,
            ..RepairOptions::default()
        };
        let mut ask = Scripted::new(Vec::new());
        let ran = run_pass(
            &input,
            C7FontStreamDeleted,
            &sparse_db(),
            &opts,
            &mut ask,
            &[],
        );
        assert!(ask.asked.is_empty(), "{policy:?}");
        let rec = record(&ran.notes);
        assert_eq!(rec.source, InteractionSource::Policy, "{policy:?}");
        assert_eq!(rec.reply, reply, "{policy:?}");
        assert_eq!(rec.request.kind, InteractionKind::FontUnreproducible);
        assert_eq!(ran.notes.resolutions.len(), 2);
    }
}

#[test]
fn every_question_reaches_the_report() {
    let input = c7();
    let opts = always_ask();
    let plan = plan(&input.findings, &input.carve, &input.graph, &opts);
    let mut ask = Scripted::new(vec![InteractionReply::UseBest]);
    let g = generate_and_validate(
        &input.view(),
        &plan,
        &opts,
        &test_db(),
        &mut ask,
        &mut NullProgress,
    )
    .unwrap();
    // Asked once: in the TemplateAssemble candidate only.
    assert_eq!(ask.asked.len(), 1);
    assert_eq!(g.interactions.len(), 1);
    let mut report = RepairReport::default_for(
        &AnalysisResult {
            meta: FileMeta {
                version: None,
                pages: 2,
                title: None,
                page_sizes: Vec::new(),
            },
            findings: Vec::new(),
            carve: CarveSummary::default(),
            font_slots: Vec::new(),
            stats: AnalyzeStats::default(),
            input_sha256: [0; 32],
            state: StateHandle::default(),
        },
        &opts,
        &test_db(),
    );
    g.record(&mut report);
    assert_eq!(report.interactions, g.interactions);
}

/// `bytes` analysed, planned and repaired through the engine facade.
fn through_engine(
    bytes: &[u8],
    fonts: &FontDb,
    opts: &RepairOptions,
    ask: &mut dyn Interact,
) -> crate::engine::RepairOutcome {
    let analysis =
        crate::engine::analyze(bytes, &opts.analyze, &mut NullProgress).expect("never cancelled");
    let plan = crate::engine::plan(&analysis, opts);
    crate::engine::repair(bytes, &analysis, &plan, opts, fonts, ask, &mut NullProgress)
        .expect("never cancelled")
}

#[test]
fn a_policy_answer_reaches_the_engine_report() {
    let bytes = corrupt(C7FontStreamDeleted, &two_fonts(), 0);
    let opts = RepairOptions {
        unreproducible: UnreproduciblePolicy::SubstituteGeneric,
        ..RepairOptions::default()
    };
    let mut ask = Scripted::new(Vec::new());
    let out = through_engine(&bytes, &sparse_db(), &opts, &mut ask);
    assert!(ask.asked.is_empty());
    let [rec] = &out.report.interactions[..] else {
        panic!("{:?}", out.report.interactions)
    };
    assert_eq!(rec.source, InteractionSource::Policy);
    assert_eq!(rec.reply, InteractionReply::Substitute(sparse_choice()));
    assert_eq!(rec.request.kind, InteractionKind::FontUnreproducible);
}

#[test]
fn an_asked_question_reaches_the_engine_report_once_and_replays() {
    let bytes = corrupt(C7FontStreamDeleted, &golden_pdf(), 0);
    let opts = always_ask();
    let mut ask = Scripted::new(vec![InteractionReply::UseBest]);
    let first = through_engine(&bytes, &test_db(), &opts, &mut ask);
    assert_eq!(ask.asked.len(), 1);
    let InteractionRequest::FontPick(req) = &ask.asked[0] else {
        panic!("{:?}", ask.asked)
    };
    assert_eq!(req.id, crate::engine::InteractionRequestId(1));
    let [rec] = &first.report.interactions[..] else {
        panic!("{:?}", first.report.interactions)
    };
    assert_eq!(rec.source, InteractionSource::UseBest);
    assert_eq!(rec.request.kind, InteractionKind::FontPick);

    // Replaying the asked replies gives the same file and report.
    let replies = (first.report.interactions.iter())
        .filter(|r| r.source != InteractionSource::Policy)
        .map(|r| r.reply.clone())
        .collect();
    let mut again = Scripted::new(replies);
    let second = through_engine(&bytes, &test_db(), &opts, &mut again);
    assert!(again.replies.is_empty());
    assert_eq!(first.output, second.output);
    assert_eq!(first.report, second.report);
}

// ── template assembly ────────────────────────────────────────────────────

/// The `/ToUnicode` of the font page `page`'s `/F1` names in `doc`, decoded.
fn tounicode_of(doc: &Document, page: u32) -> Vec<u8> {
    let page_id = doc.get_pages()[&(page + 1)];
    let page = doc.get_dictionary(page_id).unwrap();
    let resources = page.get(b"Resources").unwrap().as_dict().unwrap();
    let fonts = resources.get(b"Font").unwrap().as_dict().unwrap();
    let font = doc
        .get_dictionary(fonts.get(b"F1").unwrap().as_reference().unwrap())
        .unwrap();
    let cmap = doc
        .get_object(font.get(b"ToUnicode").unwrap().as_reference().unwrap())
        .unwrap()
        .as_stream()
        .unwrap();
    cmap.decompressed_content().unwrap()
}

/// Every block of a `/ToUnicode` CMap: its entry count and the codes it
/// maps.
fn blocks(cmap: &[u8]) -> Vec<(usize, Vec<u16>)> {
    let text = String::from_utf8_lossy(cmap);
    let mut out = Vec::new();
    let mut lines = text.lines();
    let code = |s: &str| u16::from_str_radix(s.trim_matches(['<', '>']), 16).unwrap();
    while let Some(line) = lines.next() {
        let Some((n, kind)) = line.split_once(' ') else {
            continue;
        };
        if kind != "beginbfchar" && kind != "beginbfrange" {
            continue;
        }
        let n: usize = n.parse().unwrap();
        let mut codes = Vec::new();
        for _ in 0..n {
            let parts: Vec<&str> = lines.next().unwrap().split(' ').collect();
            if kind == "beginbfchar" {
                codes.push(code(parts[0]));
            } else {
                codes.extend(code(parts[0])..=code(parts[1]));
            }
        }
        out.push((n, codes));
    }
    out
}

#[test]
fn assembled_tounicode_covers_exactly_the_used_codes_in_small_blocks() {
    let input = c7();
    let db = test_db();
    let remap = plan_ids(&input.carve, &input.graph);
    let tree = rebuild_page_tree(&input.carve, &input.graph, &remap, PageSize::A4);
    // 250 codes: singletons and runs, more than two blocks of each.
    let used: BTreeMap<u16, char> = (0u16..250)
        .map(|c| {
            let ch = if c % 3 == 0 { 'a' } else { 'A' };
            (
                c * 2,
                char::from_u32(u32::from(ch) + u32::from(c % 20)).unwrap(),
            )
        })
        .chain((600u16..900).map(|c| (c, char::from_u32(0x400 + u32::from(c - 600)).unwrap())))
        .collect();
    let subs = [
        Substitution {
            page: 0,
            slot: "F1".to_owned(),
            font_id: RIGHT.to_owned(),
            used_codes: used.clone(),
        },
        Substitution {
            page: 1,
            slot: "F1".to_owned(),
            font_id: RIGHT.to_owned(),
            used_codes: used.clone(),
        },
    ];
    let emit = |subs: &[Substitution]| {
        let mut sink = NullProgress;
        let mut ctx = EmitCtx {
            bytes: &input.bytes,
            input_sha256: Sha256::digest(&input.bytes).into(),
            salvage: &input.salvage,
            sink: &mut sink,
            notes: Default::default(),
        };
        emit_template_assemble(
            &input.carve,
            &input.graph,
            &remap,
            &tree,
            subs,
            &db,
            &mut ctx,
        )
        .unwrap()
    };
    let out = emit(&subs);
    assert_eq!(out, emit(&subs), "byte-identical on two runs");

    let doc = load_strict(&out);
    for page in 0..2 {
        let cmap = tounicode_of(&doc, page);
        let blocks = blocks(&cmap);
        assert!(blocks.len() >= 4, "{}", blocks.len());
        assert!(blocks.iter().all(|(n, _)| *n <= 100));
        let mut codes: Vec<u16> = blocks.into_iter().flat_map(|(_, c)| c).collect();
        codes.sort_unstable();
        assert_eq!(codes, used.keys().copied().collect::<Vec<_>>());
    }
    // One subtree serves both slots; the damaged font is gone.
    let fonts: Vec<_> = doc
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .filter(|d| d.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) == Some(b"Type0"))
        .collect();
    assert_eq!(fonts.len(), 1);
    assert!(!classes_in(&out).contains(&C7FontStreamDeleted));
    // It renders in hayro, with no fallback font: page 2 has no image, so
    // its ink is the substituted font's glyphs.
    let pdf = hayro::hayro_syntax::Pdf::new(out.clone()).expect("hayro loads it");
    assert_eq!(pdf.pages().len(), 2);
    let settings = hayro::hayro_interpret::InterpreterSettings {
        font_resolver: std::sync::Arc::new(|_| None),
        ..Default::default()
    };
    let scale = hayro::PixmapSettings {
        x_scale: 0.5,
        y_scale: 0.5,
        ..Default::default()
    };
    let pixmap = hayro::render(
        &pdf.pages()[1],
        &hayro::RenderCache::new(),
        &settings,
        &hayro::RenderSettings::default(),
        &scale,
    );
    let rgba = pixmap.data_as_u8_slice();
    assert!(
        rgba.chunks(4)
            .any(|p| p[3] != 0 && p[..3] != [255, 255, 255]),
        "page 2 has ink"
    );
    assert!(extract_text(&out, &ExtractOptions::default()).is_ok());
}

/// The golden with page 1's content split in two after its `Tf`, and the
/// second piece opening with `q /F9 16 Tf Q` (`/F9` a standard Helvetica):
/// the text is shown in another stream than the one that set its font,
/// after a `Q` restored that font.
fn split_contents() -> Vec<u8> {
    rewrite_golden(|objects| {
        let content = objects[&11]
            .as_stream()
            .unwrap()
            .decompressed_content()
            .unwrap();
        let text = String::from_utf8(content).unwrap();
        let at = text.find(" Tf\n").unwrap() + " Tf\n".len();
        let first = text.as_bytes()[..at].to_vec();
        let second = format!("q\n/F9 16 Tf\nQ\n{}", &text[at..]).into_bytes();
        objects.insert(11, Object::Stream(Stream::new(Dictionary::new(), first)));
        objects.insert(23, Object::Stream(Stream::new(Dictionary::new(), second)));
        let mut helvetica = Dictionary::new();
        helvetica.set("Type", Object::Name(b"Font".to_vec()));
        helvetica.set("Subtype", Object::Name(b"Type1".to_vec()));
        helvetica.set("BaseFont", Object::Name(b"Helvetica".to_vec()));
        objects.insert(24, Object::Dictionary(helvetica));
        let page1 = objects.get_mut(&3).unwrap().as_dict_mut().unwrap();
        page1.set(
            "Contents",
            Object::Array(vec![Object::Reference((11, 0)), Object::Reference((23, 0))]),
        );
        let resources = page1.get_mut(b"Resources").unwrap().as_dict_mut().unwrap();
        let fonts = resources.get_mut(b"Font").unwrap().as_dict_mut().unwrap();
        fonts.set("F9", Object::Reference((24, 0)));
    })
}

/// The two-byte codes of every hex string in page `page`'s golden content.
fn golden_codes(page: usize) -> BTreeSet<u16> {
    let doc = Document::load_mem(&golden_pdf()).unwrap();
    let id = doc.get_pages()[&(page as u32 + 1)];
    let content = doc.get_page_content(id);
    let text = String::from_utf8(content).unwrap();
    let mut out = BTreeSet::new();
    for piece in text.split('<').skip(1) {
        let hex = &piece[..piece.find('>').unwrap()];
        for c in hex.as_bytes().chunks(4) {
            out.insert(u16::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap());
        }
    }
    out
}

#[test]
fn a_font_set_in_one_content_stream_governs_the_next() {
    let input = analysed(corrupt(C7FontStreamDeleted, &split_contents(), 0));
    assert_eq!(input.classes(), [C7FontStreamDeleted]);
    let ran = run_pass(
        &input,
        C7FontStreamDeleted,
        &test_db(),
        &RepairOptions::default(),
        &mut UseBest,
        &[],
    );
    assert_eq!(ran.report.outcome, PassOutcome::Fixed, "{:?}", ran.report);
    let doc = load_strict(&ran.output);
    let mapped: BTreeSet<u16> = blocks(&tounicode_of(&doc, 0))
        .into_iter()
        .flat_map(|(_, c)| c)
        .collect();
    let drawn = golden_codes(0);
    assert!(!drawn.is_empty());
    assert!(
        drawn.is_subset(&mapped),
        "unmapped: {:?}",
        drawn.difference(&mapped).collect::<Vec<_>>()
    );
    assert_eq!(text_of(&ran.output), golden_text());
}

/// The map a `/ToUnicode` CMap `build_tounicode` wrote gives.
fn tounicode_map(cmap: &[u8]) -> BTreeMap<u16, char> {
    let text = String::from_utf8_lossy(cmap);
    let hex = |s: &str| -> Vec<u16> {
        let s = s.trim_matches(['<', '>']);
        (0..s.len() / 4)
            .map(|i| u16::from_str_radix(&s[i * 4..i * 4 + 4], 16).unwrap())
            .collect()
    };
    let one = |s: &str| -> char {
        let units = hex(s);
        char::decode_utf16(units).next().unwrap().unwrap()
    };
    let mut out = BTreeMap::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let Some((n, kind)) = line.split_once(' ') else {
            continue;
        };
        if kind != "beginbfchar" && kind != "beginbfrange" {
            continue;
        }
        for _ in 0..n.parse::<usize>().unwrap() {
            let parts: Vec<&str> = lines.next().unwrap().split(' ').collect();
            if kind == "beginbfchar" {
                out.insert(hex(parts[0])[0], one(parts[1]));
            } else {
                let (lo, hi, first) = (hex(parts[0])[0], hex(parts[1])[0], one(parts[2]));
                for c in lo..=hi {
                    let ch = char::from_u32(u32::from(first) + u32::from(c - lo)).unwrap();
                    out.insert(c, ch);
                }
            }
        }
    }
    out
}

#[test]
fn the_pass_and_emit_template_assemble_write_the_same_file() {
    // C7 alone: no stream's salvage changed, so the two paths agree.
    let input = c7();
    let db = test_db();
    let ran = run_pass(
        &input,
        C7FontStreamDeleted,
        &db,
        &RepairOptions::default(),
        &mut UseBest,
        &[],
    );
    assert_eq!(ran.report.outcome, PassOutcome::Fixed);
    let doc = load_strict(&ran.output);
    let subs: Vec<Substitution> = (0..2)
        .map(|page| Substitution {
            page,
            slot: "F1".to_owned(),
            font_id: RIGHT.to_owned(),
            used_codes: tounicode_map(&tounicode_of(&doc, page)),
        })
        .collect();
    assert!(!subs[0].used_codes.is_empty());
    let remap = plan_ids(&input.carve, &input.graph);
    let tree = rebuild_page_tree(&input.carve, &input.graph, &remap, PageSize::A4);
    let mut sink = NullProgress;
    let mut ctx = EmitCtx {
        bytes: &input.bytes,
        input_sha256: Sha256::digest(&input.bytes).into(),
        salvage: &input.salvage,
        sink: &mut sink,
        notes: Default::default(),
    };
    let out = emit_template_assemble(
        &input.carve,
        &input.graph,
        &remap,
        &tree,
        &subs,
        &db,
        &mut ctx,
    )
    .unwrap();
    assert!(out == ran.output, "the two paths differ");
}

#[test]
fn assembled_output_is_deterministic_and_carries_no_ui_string() {
    let input = c8();
    let db = test_db();
    let first = run_db(&input, &db, &RepairOptions::default()).0;
    let second = run_db(&input, &db, &RepairOptions::default()).0;
    assert_eq!(first.output, second.output);
    assert_eq!(first.passes, second.passes);

    // The artefact deny-list (T-14's scan): no UI string, as a whole word.
    let output = first.output.unwrap();
    let mut texts = vec![String::from_utf8_lossy(&output).into_owned()];
    for p in &first.passes {
        texts.extend(p.actions.iter().map(|a| a.what.clone()));
        if let PassOutcome::Partial(why) | PassOutcome::Skipped(why) = &p.outcome {
            texts.push(why.clone());
        }
    }
    texts.extend(first.partial_reasons.iter().cloned());
    let c7 = run_db(&c7(), &db, &RepairOptions::default()).0;
    texts.push(String::from_utf8_lossy(&c7.output.unwrap()).into_owned());
    for text in &texts {
        for s in crate::ui::strings::ALL {
            assert!(!has_word(text, s), "{s:?} in an artefact");
        }
    }
}

/// `needle` occurs in `hay` with no letter or digit on either side.
fn has_word(hay: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(i) = hay[from..].find(needle) {
        let at = from + i;
        let before = hay[..at].chars().next_back();
        let after = hay[at + needle.len()..].chars().next();
        if !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric) {
            return true;
        }
        from = at + needle.len().max(1);
    }
    false
}

#[test]
fn a_bundled_font_draws_the_text_its_tounicode_says() {
    // The bundled Noto Sans numbers its glyphs unlike the test subset, so
    // the slot's codes are mapped to its glyphs (a /CIDToGIDMap stream).
    let input = c7();
    let db = FontDb::bundled();
    let (g, _) = run_db(&input, &db, &RepairOptions::default());
    assert_eq!(g.chosen, Some(Toolpath::TemplateAssemble));
    let output = g.output.unwrap();
    assert_eq!(text_of(&output), golden_text());
    let doc = load_strict(&output);
    let mapped = doc.objects.values().any(|o| {
        o.as_dict()
            .ok()
            .and_then(|d| d.get(b"CIDToGIDMap").ok())
            .is_some_and(|m| m.as_reference().is_ok())
    });
    assert!(mapped);
}

#[test]
fn a_font_this_version_cannot_substitute_is_kept_and_partial() {
    // Identity-V: two-byte codes, but vertical; never substituted.
    let pdf = corrupt(C7FontStreamDeleted, &golden_pdf(), 0);
    let at = pdf
        .windows(10)
        .position(|w| w == b"Identity-H")
        .expect("the golden's encoding");
    let mut pdf = pdf;
    pdf[at + 9] = b'V';
    let input = analysed(pdf);
    assert_eq!(input.classes(), [C7FontStreamDeleted]);
    let mut ask = Scripted::new(Vec::new());
    let ran = run_pass(
        &input,
        C7FontStreamDeleted,
        &test_db(),
        &RepairOptions::default(),
        &mut ask,
        &[],
    );
    assert!(ask.asked.is_empty());
    let PassOutcome::Partial(why) = &ran.report.outcome else {
        panic!("{:?}", ran.report)
    };
    assert!(why.contains("/Type0 Identity-H fonts only"), "{why}");
    assert!(ran.notes.interactions.is_empty());
    assert!(classes_in(&ran.output).contains(&C7FontStreamDeleted));
}

#[test]
fn a_name_match_whose_glyphs_differ_is_rejected_and_never_reported_fixed() {
    // The bundled Noto Sans numbers its glyphs unlike the test subset: the
    // /BaseFont names it, but its glyphs read the codes as other letters.
    let input = c8();
    let db = FontDb::bundled();
    let (g, _) = run_db(&input, &db, &RepairOptions::default());
    let c8 = pass(&g, C8FontResourcesDeleted);
    let text = text_of(g.output.as_ref().unwrap());
    assert!(
        c8.outcome != PassOutcome::Fixed || text == golden_text(),
        "{c8:?} with {text:?}"
    );
    assert!(!g.resolutions.is_empty());
    for (_, _, r) in &g.resolutions {
        let lines = &r.provenance;
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("name:") && l.contains("rejected: ")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.starts_with("inference:")),
            "{lines:?}"
        );
    }

    // With a word list as well: whatever the pass decides, Fixed means the
    // golden's text.
    let english = english();
    let ran = run_pass(
        &input,
        C8FontResourcesDeleted,
        &db,
        &RepairOptions::default(),
        &mut UseBest,
        &[&english],
    );
    let text = text_of(&ran.output);
    assert!(
        ran.report.outcome != PassOutcome::Fixed || text == golden_text(),
        "{:?} with {text:?}",
        ran.report
    );
}

#[test]
fn a_code_with_several_characters_makes_c7_partial() {
    let input = analysed(corrupt(C7FontStreamDeleted, &combining_tounicode(), 0));
    assert_eq!(input.classes(), [C7FontStreamDeleted]);
    let ran = run_pass(
        &input,
        C7FontStreamDeleted,
        &test_db(),
        &RepairOptions::default(),
        &mut UseBest,
        &[],
    );
    let PassOutcome::Partial(why) = &ran.report.outcome else {
        panic!("{:?}", ran.report)
    };
    let lost = "keeps only the first character of 1 codes";
    assert!(why.contains(lost), "{why}");
    let what = actions(&ran.report);
    assert!(what[0].starts_with(&format!("font program substituted: {RIGHT}")));
    assert!(what[0].contains(lost), "{what:?}");
}

#[test]
fn a_form_sharing_the_pages_resources_keeps_the_old_font() {
    let input = analysed(corrupt(C7FontStreamDeleted, &shared_resources(), 0));
    assert_eq!(input.classes(), [C7FontStreamDeleted]);
    let ran = run_pass(
        &input,
        C7FontStreamDeleted,
        &test_db(),
        &RepairOptions::default(),
        &mut UseBest,
        &[],
    );
    assert!(
        actions(&ran.report)[0].starts_with("font program substituted"),
        "{:?}",
        ran.report
    );
    let doc = load_strict(&ran.output);
    let type0 = |id: (u32, u16)| {
        doc.get_dictionary(id)
            .ok()
            .and_then(|d| d.get(b"Subtype").ok()?.as_name().ok())
            == Some(&b"Type0"[..])
    };
    let f1 = |resources: &Dictionary| {
        let fonts = match resources.get(b"Font").unwrap() {
            Object::Reference(id) => doc.get_dictionary(*id).unwrap(),
            other => other.as_dict().unwrap(),
        };
        fonts
            .get(b"F1")
            .unwrap()
            .as_reference()
            .expect("a font, not null")
    };
    // Page 1 draws through the new font; the form, through the shared
    // resources, still through the old one, which is kept.
    let page_id = doc.get_pages()[&1];
    let page = doc.get_dictionary(page_id).unwrap();
    let resources = page.get(b"Resources").unwrap().as_dict().unwrap();
    let new_font = f1(resources);
    let xobjects = resources.get(b"XObject").unwrap().as_dict().unwrap();
    let form_id = xobjects.get(b"Fm1").unwrap().as_reference().unwrap();
    let form = doc.get_object(form_id).unwrap().as_stream().unwrap();
    let shared_id = form.dict.get(b"Resources").unwrap().as_reference().unwrap();
    let old_font = f1(doc.get_dictionary(shared_id).unwrap());
    assert_ne!(new_font, old_font);
    assert!(type0(new_font) && type0(old_font));
}

#[test]
fn a_font_reached_only_through_a_re_link_is_substituted() {
    // Both pages lose their /Font entry: the damaged font is reachable
    // through the C6 re-links alone.
    let c6 = |pdf: &[u8], seed| corrupt(CorruptionClass::C6FontMapLost, pdf, seed);
    let pdf = c6(&c6(&corrupt(C7FontStreamDeleted, &golden_pdf(), 0), 0), 2);
    let input = analysed(pdf);
    assert_eq!(
        input.classes(),
        [CorruptionClass::C6FontMapLost, C7FontStreamDeleted]
    );
    let (g, _) = run_db(&input, &test_db(), &RepairOptions::default());
    assert_eq!(g.chosen, Some(Toolpath::TemplateAssemble));
    let c7 = pass(&g, C7FontStreamDeleted);
    assert_eq!(c7.outcome, PassOutcome::Fixed, "{c7:?}");
    let slots: Vec<(u32, &str)> = (g.resolutions.iter())
        .map(|(p, s, _)| (*p, s.as_str()))
        .collect();
    assert_eq!(slots, [(0, "F1"), (1, "F1")], "the re-linked page too");
    assert_eq!(text_of(g.output.as_ref().unwrap()), golden_text());
}
