//! Ignored REPDF corpus harness, corpus-only (T-26, D-002).
//!
//! `PDFPUNDIT_CORPUS=<dir> cargo test --release --lib -- --ignored
//! bench::corpus::smoke` (or `bench::corpus::full`) runs every corrupted file
//! of the D-013 smoke subset (or of the whole corpus) through the engine
//! facade, `analyze → plan → repair` under [`UseBest`], and scores each page
//! of the repaired file's text layer against the same page of the original
//! with T-25's metrics. Without the variable the tests print why and pass.
//!
//! **Corpus-only (goal-r3-q8).** Before any file is analysed its git blob id,
//! `sha1("blob <len>\0" + bytes)`, must be the one `bench/repdf_manifest.txt`
//! records for its path (the e547d4d tree, FR-07, lead-r4-fr3); anything else
//! ends the run with [`NOT_CORPUS`]. The check hashes the bytes it read, so a
//! checkout that altered them is refused, never analysed. This is what keeps
//! an ignored test that analyses, repairs and writes outputs for a whole tree
//! from being a headless batch repairer for anyone's files.
//!
//! **The smoke list (D-013, D-077).** The two lowest base-document names in
//! byte order, the defective C6 document excluded; both producers; the
//! original then the ten classes. [`resolve_smoke`] re-derives it from the
//! manifest's paths on every run and the run stops if it differs from the
//! committed `bench/smoke_subset.txt`.
//!
//! **Outputs.** `target/corpus/corpus_results.csv` (one row per page of
//! every corrupted file; it names corpus files and per-page scores, so it stays
//! on the machine that ran it and is never a CI artifact), the aggregate
//! tables on stdout, each headed by the D-063 label, and
//! `target/corpus/golden_smoke.csv`, the candidate golden: copying it over
//! `bench/golden_smoke.csv` is a deliberate re-baseline. The smoke run fails
//! when a class's mean LCS-F1 falls more than 2% (20 thousandths) below the
//! committed golden.
//!
//! **The `ocr` mode (T-38a).** `bench::corpus::ocr` runs the smoke run, then
//! renders every page of each smoke original and of each repaired output
//! through hayro (default interpreter settings, embedded fonts, 200 dpi,
//! white background, 8-bit RGB PNG) into
//! `target/corpus/ocr_inputs/<file-stem>/p<N>.png`, `N` the 0-based page, and
//! writes `ocr_inputs/manifest.csv` for the OCR harness (T-38b): `file, class,
//! producer, base_doc, role, page, lang, png, open_ok, lcs_f1`. `png` is
//! relative to the manifest; `lang` is the original page's label (`und` past
//! the original's page count); `lcs_f1` is the page's text-layer score as
//! `num/den` (an original against itself). A repaired file hayro cannot open,
//! or with no output at all, gets one row with no page, no PNG and
//! `open_ok = 0`; a page that panics or is too large to rasterise gets its row
//! with no PNG and `open_ok = 0`. Only PNGs and the manifest go under
//! `ocr_inputs/`, which each run empties first. Pixels are not portable
//! (FR-g1); nothing here compares them.
//!
//! **Languages (D-069, eng-r4-fr1).** A page's label comes from the original's
//! extracted text, never from its index: the dominant non-Latin script when
//! non-Latin letters are at least 20% of the letters (and at least 3), else
//! Latin split into en, fr and es by stopword tables that share no word; one
//! unknown page per document may be filled from the six-language set. Pages of
//! a repaired file inherit the label of the original's page with their index.
//!
//! **Paper reference columns (D-070).** Blocked until the user answers:
//! `bench/repdf_paper_tables.toml` holds the cited constants and nothing here
//! reads it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use sha1::{Digest, Sha1};

use crate::bench::metrics::{Aggregate, Lang, TextScores, aggregate, score, tokens};
use crate::engine::{
    self, AnalyzeOptions, CandidateReport, FontDb, NullProgress, OutcomeStatus, RepairOptions,
    Toolpath, UseBest,
};
use crate::pdf::carver::{Body, CarveReport};
use crate::pdf::model::{ByteSpan, CorruptionClass, Ratio};
use crate::pdf::streams::inflate::{InflateResult, InflateStatus, inflate};
use crate::pdf::streams::{DEFAULT_CAP, Filter, filters_of};
use crate::pdf::text::{ExtractOptions, PageText, extract_text};

#[cfg(test)]
mod tests;

/// The D-063 label: the first line of every printed table and of the golden
/// CSV. The repository and its CI logs are public (goal-r3-q2); this line is
/// what makes the numbers a baseline rather than a claim.
const D063_HEADER: &str = "regression baseline; 44-file REPDF smoke subset; text-layer LCS-F1; \
                           not REPDF's metric; not comparable to the paper; \
                           not a published recovery rate";

/// The same label for a full run, which is not the smoke subset.
const FULL_HEADER: &str = "regression baseline; 1,000-file REPDF corpus; text-layer LCS-F1; \
                           not REPDF's metric; not comparable to the paper; \
                           not a published recovery rate";

/// The one line a file outside the pinned corpus ends the run with.
const NOT_CORPUS: &str =
    "not a REPDF corpus file: the harness runs only on the pinned corpus (e547d4d)";

/// `path blob-sha1 size` per corpus PDF at e547d4d, sorted by path in byte
/// order (lead-r4-fr3). A changed manifest is a re-pin of the corpus.
const MANIFEST: &str = include_str!("../../bench/repdf_manifest.txt");
const MANIFEST_SHA256: &str = "3fb3cb0478e9ea21bea69ca16d62fde1d9dc9dccd9d8d81b35141bf36df27cf1";
const SMOKE_SUBSET: &str = include_str!("../../bench/smoke_subset.txt");
const GOLDEN_SMOKE: &str = include_str!("../../bench/golden_smoke.csv");

/// The document whose Save-As C6 file is defective (OBS-0004): 33,754 B
/// against a 436,319 B original. D-013 excludes it from the smoke list.
const DEFECTIVE_DOC: &str = "AeroChef_Drone_Delivery_Cooking_Service";

/// REPDF's file-name suffix per class (FR-07), C1 first.
const SUFFIXES: [(&str, CorruptionClass); 10] = [
    ("header", CorruptionClass::C1Header),
    ("xref", CorruptionClass::C2XrefMissing),
    ("trailer", CorruptionClass::C3TrailerDamaged),
    ("page_tree", CorruptionClass::C4PageTreeBroken),
    ("object_header", CorruptionClass::C5ObjectTagStripped),
    ("font_mapping_loss", CorruptionClass::C6FontMapLost),
    ("remove_fonts", CorruptionClass::C7FontStreamDeleted),
    (
        "remove_unicode_fonts",
        CorruptionClass::C8FontResourcesDeleted,
    ),
    ("stream_zlib", CorruptionClass::C9ZlibTampered),
    ("partial_cut", CorruptionClass::C10Truncated),
];

const PRODUCERS: [&str; 2] = ["print", "saveas"];
const KINDS: [&str; 2] = ["text", "text+img"];

/// How far below the golden a class's mean may fall, in thousandths (TD §8:
/// `recovery ≥ golden − 2%`).
const GATE_SLACK_PERMILLE: u64 = 20;

/// The per-page results file's columns (plan §6 T-26), in order.
const CSV_COLUMNS: [&str; 28] = [
    "file",
    "class",
    "producer",
    "base_doc",
    "page",
    "lang",
    "outcome",
    "chosen_toolpath",
    "v0",
    "v1",
    "v2",
    "v3",
    "v4",
    "lcs_f1",
    "recall",
    "precision",
    "bag_f1",
    "char_f1",
    "baseline_lcs_f1",
    "extract_failed",
    "unmapped_glyphs",
    "c9_streams_exact",
    "c9_streams_accepted",
    "c9_streams_ambiguous",
    "c9_streams_unsearched",
    "c9_work_total",
    "reals_narrowed",
    "inflate_backend_disagreements",
];

// ── the manifest gate ────────────────────────────────────────────────────

/// One corpus file as the pinned tree records it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    path: String,
    blob: [u8; 20],
    size: u64,
}

/// The parsed manifest: its entries in file order, indexed by blob id.
#[derive(Debug)]
struct Manifest {
    entries: Vec<Entry>,
    by_blob: BTreeMap<[u8; 20], usize>,
}

/// A file whose bytes are not a pinned corpus file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NotCorpus;

impl fmt::Display for NotCorpus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(NOT_CORPUS)
    }
}

/// The git blob id of `bytes`: `sha1("blob <len>\0" + bytes)`.
fn blob_id(bytes: &[u8]) -> [u8; 20] {
    let mut h = Sha1::new();
    h.update(format!("blob {}\0", bytes.len()).as_bytes());
    h.update(bytes);
    let mut id = [0; 20];
    id.copy_from_slice(&h.finalize());
    id
}

impl Manifest {
    /// The committed manifest.
    fn pinned() -> Manifest {
        Manifest::parse(MANIFEST).expect("bench/repdf_manifest.txt parses")
    }

    /// Lines of `path blob-sha1 size`, single-space separated, LF, sorted by
    /// path in byte order, no blob twice.
    fn parse(text: &str) -> Result<Manifest, String> {
        let mut entries = Vec::new();
        let mut by_blob = BTreeMap::new();
        for (n, line) in text.lines().enumerate() {
            let bad = || format!("manifest line {}: {line:?}", n + 1);
            let fields: Vec<&str> = line.split(' ').collect();
            let [path, blob, size] = fields[..] else {
                return Err(bad());
            };
            let blob = parse_hex20(blob).ok_or_else(bad)?;
            let size = size.parse::<u64>().map_err(|_| bad())?;
            if path.is_empty() || size.to_string().len() != fields[2].len() {
                return Err(bad());
            }
            if entries
                .last()
                .is_some_and(|e: &Entry| e.path.as_str() >= path)
            {
                return Err(format!("{}: not sorted by path", bad()));
            }
            if by_blob.insert(blob, entries.len()).is_some() {
                return Err(format!("{}: blob listed twice", bad()));
            }
            entries.push(Entry {
                path: path.to_owned(),
                blob,
                size,
            });
        }
        Ok(Manifest { entries, by_blob })
    }

    /// The entry `bytes` hash to, or [`NotCorpus`].
    fn check(&self, bytes: &[u8]) -> Result<&Entry, NotCorpus> {
        let entry = &self.entries[*self.by_blob.get(&blob_id(bytes)).ok_or(NotCorpus)?];
        if entry.size == bytes.len() as u64 {
            Ok(entry)
        } else {
            Err(NotCorpus)
        }
    }

    /// [`Self::check`], and the entry must be the file at `path`: a corpus
    /// file copied under another corpus name is refused too.
    fn check_at(&self, bytes: &[u8], path: &str) -> Result<&Entry, NotCorpus> {
        self.check(bytes).and_then(|e| {
            if e.path == path {
                Ok(e)
            } else {
                Err(NotCorpus)
            }
        })
    }

    fn by_path(&self, path: &str) -> Option<&Entry> {
        self.entries
            .binary_search_by(|e| e.path.as_str().cmp(path))
            .ok()
            .map(|i| &self.entries[i])
    }
}

fn parse_hex20(s: &str) -> Option<[u8; 20]> {
    if s.len() != 40 {
        return None;
    }
    let mut out = [0; 20];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

// ── corpus paths and the smoke list ──────────────────────────────────────

/// `{corrupted,original}/{print,saveas}/{text,text+img}/<base>(<producer>)[_<suffix>].pdf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CorpusPath<'a> {
    producer: &'a str,
    kind: &'a str,
    base: &'a str,
    /// `None` for an original.
    class: Option<CorruptionClass>,
}

impl<'a> CorpusPath<'a> {
    fn parse(path: &'a str) -> Result<CorpusPath<'a>, String> {
        let bad = || format!("not a REPDF path: {path}");
        let parts: Vec<&str> = path.split('/').collect();
        let [top, producer, kind, name] = parts[..] else {
            return Err(bad());
        };
        if !PRODUCERS.contains(&producer) || !KINDS.contains(&kind) {
            return Err(bad());
        }
        let stem = name.strip_suffix(".pdf").ok_or_else(bad)?;
        let tag = format!("({producer})");
        let at = stem.rfind(&tag).ok_or_else(bad)?;
        let (base, rest) = (&stem[..at], &stem[at + tag.len()..]);
        if base.is_empty() {
            return Err(bad());
        }
        let class = match (top, rest) {
            ("original", "") => None,
            ("corrupted", rest) => {
                let suffix = rest.strip_prefix('_').ok_or_else(bad)?;
                let (_, class) = SUFFIXES
                    .iter()
                    .find(|(s, _)| *s == suffix)
                    .ok_or_else(bad)?;
                Some(*class)
            }
            _ => return Err(bad()),
        };
        Ok(CorpusPath {
            producer,
            kind,
            base,
            class,
        })
    }

    /// The original this file was corrupted from.
    fn original(&self) -> String {
        let (p, k, b) = (self.producer, self.kind, self.base);
        format!("original/{p}/{k}/{b}({p}).pdf")
    }
}

/// D-013 under D-077: the two lowest base-document names in byte order, the
/// defective one excluded; for each, print then saveas, the original then the
/// classes in C1..C10 order.
fn resolve_smoke(manifest: &Manifest) -> Result<Vec<String>, String> {
    let mut kinds = BTreeMap::<&str, BTreeSet<&str>>::new();
    for entry in &manifest.entries {
        let p = CorpusPath::parse(&entry.path)?;
        kinds.entry(p.base).or_default().insert(p.kind);
    }
    let mut out = Vec::new();
    for (base, kind) in kinds.iter().filter(|(b, _)| **b != DEFECTIVE_DOC).take(2) {
        let [kind] = kind.iter().copied().collect::<Vec<_>>()[..] else {
            return Err(format!("{base} is in more than one kind directory"));
        };
        for producer in PRODUCERS {
            let paths =
                std::iter::once(format!("original/{producer}/{kind}/{base}({producer}).pdf"))
                    .chain(SUFFIXES.iter().map(|(suffix, _)| {
                        format!("corrupted/{producer}/{kind}/{base}({producer})_{suffix}.pdf")
                    }));
            for path in paths {
                manifest
                    .by_path(&path)
                    .ok_or_else(|| format!("smoke path not in the manifest: {path}"))?;
                out.push(path);
            }
        }
    }
    Ok(out)
}

// ── page text and languages ──────────────────────────────────────────────

/// A page's text in content order: a newline where the baseline moves by more
/// than 0.4 em, a space where the gap to the previous glyph exceeds 0.15 em
/// in either direction (Arabic steps left), U+FFFD for an unmapped glyph.
fn page_string(page: &PageText) -> String {
    let mut out = String::new();
    let mut prev: Option<&crate::pdf::text::GlyphItem> = None;
    for g in &page.glyphs {
        if let Some(p) = prev {
            let size = p.size.max(g.size);
            let width = |x: &crate::pdf::text::GlyphItem| {
                x.advance.map_or(0.0, |a| f64::from(a) / 1000.0 * x.size)
            };
            let gap = if g.x >= p.x {
                g.x - (p.x + width(p))
            } else {
                p.x - (g.x + width(g))
            };
            if (g.y - p.y).abs() > 0.4 * size {
                out.push('\n');
            } else if gap > 0.15 * size {
                out.push(' ');
            }
        }
        out.push_str(g.text.as_deref().unwrap_or("\u{fffd}"));
        prev = Some(g);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Script {
    Latin,
    Arabic,
    Devanagari,
    Han,
}

impl Script {
    fn of(c: char) -> Option<Script> {
        let u = u32::from(c);
        if !c.is_alphabetic() {
            return None;
        }
        Some(match u {
            0x0600..=0x06ff
            | 0x0750..=0x077f
            | 0x08a0..=0x08ff
            | 0xfb50..=0xfdff
            | 0xfe70..=0xfeff => Script::Arabic,
            0x0900..=0x097f => Script::Devanagari,
            0x4e00..=0x9fff | 0x3400..=0x4dbf | 0xf900..=0xfaff => Script::Han,
            _ if u < 0x0250 || (0x1e00..=0x1eff).contains(&u) => Script::Latin,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Script::Latin => "latin",
            Script::Arabic => "arabic",
            Script::Devanagari => "devanagari",
            Script::Han => "han",
        }
    }
}

/// The page's script (eng-r4-fr1 §3): the dominant non-Latin script when
/// non-Latin letters are at least 20% of all letters and at least 3, else
/// Latin when there is a Latin letter, else `None`.
fn script_of(text: &str) -> Option<Script> {
    let mut counts = BTreeMap::<Script, u64>::new();
    for s in text.chars().filter_map(Script::of) {
        *counts.entry(s).or_default() += 1;
    }
    let latin = counts.get(&Script::Latin).copied().unwrap_or(0);
    let non_latin: u64 = counts
        .iter()
        .filter(|(s, _)| **s != Script::Latin)
        .map(|(_, n)| n)
        .sum();
    if non_latin >= 3 && non_latin * 5 >= (latin + non_latin) {
        // The most letters wins; a tie goes to the first in Script order.
        let mut best: Option<(Script, u64)> = None;
        for (&s, &n) in counts.iter().filter(|(s, _)| **s != Script::Latin) {
            if best.is_none_or(|(_, m)| n > m) {
                best = Some((s, n));
            }
        }
        best.map(|(s, _)| s)
    } else if latin > 0 {
        Some(Script::Latin)
    } else {
        None
    }
}

/// eng-r4-fr1's unique tables: no word is in two of them (facts, no licence).
const STOPWORDS_EN: [&str; 20] = [
    "the", "and", "of", "to", "in", "is", "that", "for", "it", "with", "as", "are", "on", "by",
    "this", "be", "from", "or", "at", "an",
];
const STOPWORDS_FR: [&str; 15] = [
    "le", "les", "et", "des", "du", "une", "est", "pour", "dans", "sur", "au", "aux", "avec",
    "par", "qui",
];
const STOPWORDS_ES: [&str; 14] = [
    "el", "los", "las", "y", "una", "es", "por", "con", "para", "se", "del", "al", "como", "su",
];

/// A Latin page's language: the table with the most hits when it has at
/// least 3 and at least three times the runner-up's, else `Unknown`.
fn latin_lang(text: &str) -> Lang {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let hits = |table: &[&str]| words.iter().filter(|w| table.contains(w)).count();
    let mut scored = [
        (hits(&STOPWORDS_EN), Lang::En),
        (hits(&STOPWORDS_FR), Lang::Fr),
        (hits(&STOPWORDS_ES), Lang::Es),
    ];
    scored.sort_by_key(|s| std::cmp::Reverse(s.0));
    let (top, second) = (scored[0].0, scored[1].0);
    if top >= 3 && top >= 3 * second {
        scored[0].1
    } else {
        Lang::Unknown
    }
}

/// One page's label from its own text.
fn page_lang(text: &str) -> Lang {
    match script_of(text) {
        Some(Script::Arabic) => Lang::Ar,
        Some(Script::Devanagari) => Lang::Hi,
        Some(Script::Han) => Lang::Zh,
        Some(Script::Latin) => latin_lang(text),
        None => Lang::Unknown,
    }
}

const SIX: [Lang; 6] = [Lang::En, Lang::Zh, Lang::Hi, Lang::Es, Lang::Fr, Lang::Ar];

/// An original's page labels and scripts.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Labels {
    langs: Vec<Lang>,
    scripts: Vec<Option<Script>>,
}

fn label_pages(pages: &[String]) -> Labels {
    Labels {
        langs: pages.iter().map(|p| page_lang(p)).collect(),
        scripts: pages.iter().map(|p| script_of(p)).collect(),
    }
}

impl Labels {
    /// When exactly one page is `Unknown` and the others are five distinct
    /// languages of the six, that page gets the sixth (eng-r4-fr1 §4).
    /// Returns the page filled.
    fn fill_one_unknown(&mut self) -> Option<usize> {
        let unknown: Vec<usize> = (0..self.langs.len())
            .filter(|&i| self.langs[i] == Lang::Unknown)
            .collect();
        let [at] = unknown[..] else { return None };
        let missing: Vec<Lang> = SIX
            .into_iter()
            .filter(|l| !self.langs.contains(l))
            .collect();
        if self.langs.len() != 6 || missing.len() != 1 {
            return None;
        }
        self.langs[at] = missing[0];
        Some(at)
    }

    /// The four scripts are all present (the D-069 test until eng-r4-fr1
    /// answered) and, after the fill, the six languages once each.
    fn check(&self, path: &str) -> Result<(), String> {
        for s in [
            Script::Latin,
            Script::Arabic,
            Script::Devanagari,
            Script::Han,
        ] {
            if !self.scripts.contains(&Some(s)) {
                return Err(format!("{path}: no {} page", s.name()));
            }
        }
        let mut langs = self.langs.clone();
        langs.sort_by_key(|l| l.label());
        let mut six = SIX.to_vec();
        six.sort_by_key(|l| l.label());
        if langs != six {
            let got: Vec<&str> = self.langs.iter().map(|l| l.label()).collect();
            return Err(format!("{path}: languages {got:?}, not the six once each"));
        }
        Ok(())
    }
}

// ── the inflate tripwire ─────────────────────────────────────────────────

/// Flate streams compared, and how many the two inflaters disagree on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct InflateTally {
    compared: u32,
    disagree: u32,
}

/// Every carved stream whose first filter is Flate, decoded by our inflater
/// (miniz_oxide 0.9.1's core API) and by lopdf's loader (flate2 on zlib-rs,
/// eng-r1-fr2). lopdf reports no status, only bytes, so the class is compared
/// as far as its bytes show it: where ours is clean (`Done`) lopdf must give
/// the same bytes, and where ours fails (an Adler-32 mismatch, invalid data,
/// truncation) lopdf must give a prefix of ours and nothing else. On a failure
/// flate2's reader keeps what it returned before the error and drops the chunk
/// it was decoding, so a shorter prefix is the same class (measured on the
/// smoke subset's C9 files: every Adler-32 stream). A rejected zlib header is
/// skipped: lopdf then retries raw deflate past the header, a different
/// decoder path, not a class judgement. eng-r1-fr2 found no disagreement on
/// the corpus; the column is a tripwire.
fn inflate_disagreements(input: &[u8], carve: &CarveReport) -> InflateTally {
    let mut tally = InflateTally::default();
    for obj in &carve.objects {
        let Body::Stream { dict, data, .. } = &obj.body else {
            continue;
        };
        if !matches!(filters_of(dict).first(), Some((Filter::Flate, _))) {
            continue;
        }
        let raw = span(input, *data);
        let ours = inflate(raw, DEFAULT_CAP);
        let mut d = lopdf::Dictionary::new();
        d.set("Filter", lopdf::Object::Name(b"FlateDecode".to_vec()));
        let theirs = lopdf::Stream::new(d, raw.to_vec())
            .decompressed_content_with_limit(DEFAULT_CAP)
            .unwrap_or_default();
        tally.compared += 1;
        if disagree(&ours, &theirs) {
            tally.disagree += 1;
        }
    }
    tally
}

fn disagree(ours: &InflateResult, theirs: &[u8]) -> bool {
    match ours.status {
        InflateStatus::Done => theirs != ours.out.as_slice(),
        InflateStatus::Failed { at } if at <= 2 && ours.out.is_empty() => false,
        InflateStatus::CapHit => false,
        InflateStatus::AdlerMismatch
        | InflateStatus::Failed { .. }
        | InflateStatus::NeedsMoreInput => !ours.out.starts_with(theirs),
    }
}

fn span(bytes: &[u8], s: ByteSpan) -> &[u8] {
    let start = usize::try_from(s.start)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    let end = usize::try_from(s.end)
        .unwrap_or(usize::MAX)
        .clamp(start, bytes.len());
    &bytes[start..end]
}

// ── results ──────────────────────────────────────────────────────────────

/// The chosen candidate's selection tuple (V0..V4, SE Q2) as CSV cells.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Tuple {
    v0: String,
    v1: String,
    v2: String,
    v3: String,
    v4: String,
}

/// One page of one corrupted file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    file: String,
    class: String,
    producer: String,
    base_doc: String,
    page: u32,
    lang: Lang,
    outcome: String,
    chosen_toolpath: String,
    v: Tuple,
    /// The repaired page against the original's.
    scores: TextScores,
    /// The unrepaired input's page against the original's.
    baseline: TextScores,
    extract_failed: bool,
    unmapped_glyphs: u32,
    c9_exact: u32,
    c9_accepted: u32,
    c9_ambiguous: u32,
    c9_unsearched: u32,
    c9_work_total: u64,
    reals_narrowed: u32,
    inflate_disagreements: u32,
}

fn ratio(r: Ratio) -> String {
    format!("{}/{}", r.num, r.den)
}

/// A CSV field, quoted when it holds a comma, a quote or a line break.
fn field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

/// The per-page results file: the column line, then one line per row.
fn csv(rows: &[Row]) -> String {
    let mut out = CSV_COLUMNS.join(",");
    out.push('\n');
    for r in rows {
        let cells = [
            field(&r.file),
            field(&r.class),
            field(&r.producer),
            field(&r.base_doc),
            r.page.to_string(),
            r.lang.label().to_owned(),
            field(&r.outcome),
            field(&r.chosen_toolpath),
            field(&r.v.v0),
            field(&r.v.v1),
            field(&r.v.v2),
            field(&r.v.v3),
            field(&r.v.v4),
            ratio(r.scores.lcs_f1),
            ratio(r.scores.recall),
            ratio(r.scores.precision),
            ratio(r.scores.bag_f1),
            ratio(r.scores.char_f1),
            ratio(r.baseline.lcs_f1),
            u8::from(r.extract_failed).to_string(),
            r.unmapped_glyphs.to_string(),
            r.c9_exact.to_string(),
            r.c9_accepted.to_string(),
            r.c9_ambiguous.to_string(),
            r.c9_unsearched.to_string(),
            r.c9_work_total.to_string(),
            r.reals_narrowed.to_string(),
            r.inflate_disagreements.to_string(),
        ];
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

/// The committed golden: mean LCS-F1 per class, in thousandths.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Golden {
    per_class: BTreeMap<String, u64>,
}

const GOLDEN_COLUMNS: &str = "class,pages,lcs_f1";

impl Golden {
    /// The D-063 header, the column line, then `class,pages,d.ddd` rows.
    fn parse(text: &str) -> Result<Golden, String> {
        let mut lines = text.lines();
        if lines.next() != Some(D063_HEADER) {
            return Err("the golden's first line is not the D-063 header".to_owned());
        }
        if lines.next() != Some(GOLDEN_COLUMNS) {
            return Err(format!(
                "the golden's second line is not {GOLDEN_COLUMNS:?}"
            ));
        }
        let mut per_class = BTreeMap::new();
        for line in lines {
            let bad = || format!("golden row {line:?}");
            let [class, _pages, mean] = line.split(',').collect::<Vec<_>>()[..] else {
                return Err(bad());
            };
            let (whole, frac) = mean.split_once('.').ok_or_else(bad)?;
            if frac.len() != 3 {
                return Err(bad());
            }
            let permille = whole.parse::<u64>().map_err(|_| bad())? * 1000
                + frac.parse::<u64>().map_err(|_| bad())?;
            per_class.insert(class.to_owned(), permille);
        }
        Ok(Golden { per_class })
    }
}

/// Mean `lcs_f1` and mean baseline per key.
fn means(rows: &[Row], key: impl Fn(&Row) -> String) -> (Aggregate, Aggregate) {
    let keys: Vec<String> = rows.iter().map(&key).collect();
    let repaired = aggregate(
        keys.iter()
            .map(String::as_str)
            .zip(rows.iter().map(|r| &r.scores)),
    );
    let baseline = aggregate(
        keys.iter()
            .map(String::as_str)
            .zip(rows.iter().map(|r| &r.baseline)),
    );
    (repaired, baseline)
}

/// The candidate golden written from `rows`.
fn golden_csv(rows: &[Row]) -> String {
    let (repaired, _) = means(rows, |r| r.class.clone());
    let mut out = format!("{D063_HEADER}\n{GOLDEN_COLUMNS}\n");
    for class in CorruptionClass::ALL {
        if let Some(m) = repaired.groups.get(class.code()) {
            out.push_str(&format!(
                "{},{},{}\n",
                class.code(),
                m.n,
                m.ratio().display_permille()
            ));
        }
    }
    out
}

/// The printed tables and the classes that failed the gate.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Tables {
    text: String,
    failed: Vec<String>,
}

/// Three tables (per class with the gate, per class and producer, per
/// language), each headed by `header`, separated by blank lines.
fn tables(header: &str, rows: &[Row], golden: Option<&Golden>) -> Tables {
    let mut failed = Vec::new();
    let mut text = String::new();

    let (repaired, baseline) = means(rows, |r| r.class.clone());
    text.push_str(&format!(
        "{header}\nmean text-layer LCS-F1 per page, repaired and unrepaired against the original, per class\n"
    ));
    text.push_str(&format!(
        "{:<6} {:>6} {:>9} {:>11} {:>8} {:>5}\n",
        "class", "pages", "repaired", "unrepaired", "golden", "gate"
    ));
    for class in CorruptionClass::ALL.map(CorruptionClass::code) {
        let row = repaired.groups.get(class);
        let want = golden.and_then(|g| g.per_class.get(class).copied());
        if row.is_none() && want.is_none() {
            continue;
        }
        let got = row.map(|m| m.ratio());
        let gate = match (want, got) {
            // A class with results and no golden fails a gated run.
            (None, Some(_)) if golden.is_some() => {
                failed.push(class.to_owned());
                "FAIL"
            }
            (None, _) => "-",
            (Some(w), Some(g)) if permille_at_least(g, w.saturating_sub(GATE_SLACK_PERMILLE)) => {
                "pass"
            }
            (Some(_), _) => {
                failed.push(class.to_owned());
                "FAIL"
            }
        };
        let base = baseline
            .groups
            .get(class)
            .map(|m| m.ratio().display_permille());
        text.push_str(&format!(
            "{:<6} {:>6} {:>9} {:>11} {:>8} {:>5}\n",
            class,
            row.map_or(0, |m| m.n),
            got.map_or("-".to_owned(), |g| g.display_permille()),
            base.unwrap_or_else(|| "-".to_owned()),
            want.map_or("-".to_owned(), |w| format!("{}.{:03}", w / 1000, w % 1000)),
            gate
        ));
    }

    let order = |code: &str| CorruptionClass::ALL.iter().position(|c| c.code() == code);
    let (repaired, baseline) = means(rows, |r| format!("{} {}", r.class, r.producer));
    let mut keys: Vec<&String> = repaired.groups.keys().collect();
    keys.sort_by_key(|k| {
        let (class, producer) = k.split_once(' ').unwrap_or((k, ""));
        (order(class), producer.to_owned())
    });
    text.push_str(&format!("\n{header}\nper class and producer\n"));
    text.push_str(&format!(
        "{:<6} {:<8} {:>6} {:>9} {:>11}\n",
        "class", "producer", "pages", "repaired", "unrepaired"
    ));
    for k in keys {
        let (class, producer) = k.split_once(' ').unwrap_or((k, ""));
        text.push_str(&format!(
            "{:<6} {:<8} {:>6} {:>9} {:>11}\n",
            class,
            producer,
            repaired.groups[k].n,
            repaired.groups[k].ratio().display_permille(),
            baseline.groups[k].ratio().display_permille()
        ));
    }

    let (repaired, baseline) = means(rows, |r| r.lang.label().to_owned());
    text.push_str(&format!(
        "\n{header}\nper language of the original's page (D-069)\n"
    ));
    text.push_str(&format!(
        "{:<6} {:>6} {:>9} {:>11}\n",
        "lang", "pages", "repaired", "unrepaired"
    ));
    for (k, m) in &repaired.groups {
        text.push_str(&format!(
            "{:<6} {:>6} {:>9} {:>11}\n",
            k,
            m.n,
            m.ratio().display_permille(),
            baseline.groups[k].ratio().display_permille()
        ));
    }

    Tables { text, failed }
}

/// `r >= floor / 1000`, exactly.
fn permille_at_least(r: Ratio, floor: u64) -> bool {
    u128::from(r.num) * 1000 >= u128::from(floor) * u128::from(r.den)
}

// ── the ocr mode (T-38a) ─────────────────────────────────────────────────

/// 200 dpi: a PDF unit is 1/72 inch. A plain division, no libm; hayro
/// truncates the scaled page size, so A4 is 1653 × 2338 px.
const OCR_SCALE: f32 = 200.0 / 72.0;

/// A page whose longer raster side would pass this many pixels is not
/// rendered: hayro allocates width × height pixels for whatever a carved
/// `/MediaBox` says, and an allocation failure aborts the process, which
/// `catch_unwind` cannot catch. 10,000 px is 50 inches at 200 dpi; every
/// REPDF page is A4 or Letter.
const OCR_MAX_SIDE: f32 = 10_000.0;

/// The OCR manifest's columns (plan §6 T-38a), in order.
const OCR_COLUMNS: [&str; 10] = [
    "file", "class", "producer", "base_doc", "role", "page", "lang", "png", "open_ok", "lcs_f1",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OcrRole {
    Original,
    Repaired,
}

impl OcrRole {
    fn name(self) -> &'static str {
        match self {
            OcrRole::Original => "original",
            OcrRole::Repaired => "repaired",
        }
    }
}

/// One document to rasterise: who it is, and per page of its original that
/// page's label and this document's text-layer LCS-F1 on it.
#[derive(Debug, Clone)]
struct OcrDoc<'a> {
    /// The corpus path (the corrupted file's, for a repaired output).
    file: &'a str,
    /// The class code; empty for an original.
    class: &'a str,
    producer: &'a str,
    base_doc: &'a str,
    role: OcrRole,
    langs: &'a [Lang],
    lcs_f1: Vec<Ratio>,
}

/// One manifest row: one rendered page, or a page or file hayro could not
/// render (no PNG, `open_ok = 0`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct OcrRow {
    file: String,
    class: String,
    producer: String,
    base_doc: String,
    role: OcrRole,
    /// `None` on the one row of a file hayro cannot open (or that has no page).
    page: Option<u32>,
    lang: Lang,
    /// Relative to the manifest's directory, `/`-separated.
    png: Option<String>,
    lcs_f1: Option<Ratio>,
}

impl OcrRow {
    fn open_ok(&self) -> bool {
        self.png.is_some()
    }
}

/// Every page of `bytes` as an 8-bit RGB PNG: hayro with
/// `InterpreterSettings::default()` (embedded fonts), 200 dpi, white
/// background. `None` when hayro cannot open the file; a page that panics or
/// passes [`OCR_MAX_SIDE`] is `None` in the list.
fn rasterise(bytes: &[u8]) -> Option<Vec<Option<Vec<u8>>>> {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::hayro_syntax::Pdf;
    use hayro::vello_cpu::color::palette::css::WHITE;
    use hayro::{PixmapSettings, RenderCache, RenderSettings, render};

    let pdf = catch_unwind(AssertUnwindSafe(|| Pdf::new(bytes.to_vec()).ok()))
        .ok()
        .flatten()?;
    let pages = catch_unwind(AssertUnwindSafe(|| pdf.pages())).ok()?;
    let cache = RenderCache::new();
    let interpreter = InterpreterSettings::default();
    let settings = RenderSettings::default();
    let pixmap = PixmapSettings {
        x_scale: OCR_SCALE,
        y_scale: OCR_SCALE,
        bg_color: WHITE,
    };
    let out = pages
        .iter()
        .map(|page| {
            catch_unwind(AssertUnwindSafe(|| {
                let (w, h) = page.render_dimensions();
                let side = w.max(h) * OCR_SCALE;
                if !side.is_finite() || side > OCR_MAX_SIDE {
                    return None;
                }
                render(page, &cache, &interpreter, &settings, &pixmap)
                    .into_png()
                    .ok()
            }))
            .ok()
            .flatten()
        })
        .collect();
    Some(out)
}

/// The file name without `.pdf`: the PNG directory of a document.
fn file_stem(file: &str) -> &str {
    let name = file.rsplit('/').next().unwrap_or(file);
    name.strip_suffix(".pdf").unwrap_or(name)
}

/// Render `bytes` (`None`: there is no output to render) into
/// `dir/<file-stem>/p<N>.png` and return the document's rows. Pages past the
/// original's count are labelled `und` with no score.
fn ocr_document(dir: &Path, doc: &OcrDoc, bytes: Option<&[u8]>) -> Result<Vec<OcrRow>, String> {
    let row = |page: Option<u32>, png: Option<String>| {
        let i = page.map(|p| p as usize);
        OcrRow {
            file: doc.file.to_owned(),
            class: doc.class.to_owned(),
            producer: doc.producer.to_owned(),
            base_doc: doc.base_doc.to_owned(),
            role: doc.role,
            page,
            lang: i
                .and_then(|i| doc.langs.get(i).copied())
                .unwrap_or(Lang::Unknown),
            png,
            lcs_f1: i.and_then(|i| doc.lcs_f1.get(i).copied()),
        }
    };
    let pages = bytes.and_then(rasterise).unwrap_or_default();
    if pages.is_empty() {
        return Ok(vec![row(None, None)]);
    }
    let stem = file_stem(doc.file);
    let sub = dir.join(stem);
    std::fs::create_dir_all(&sub).map_err(|e| format!("{}: {e}", sub.display()))?;
    let mut rows = Vec::new();
    for (i, png) in pages.into_iter().enumerate() {
        let page = u32::try_from(i).map_err(|_| format!("{}: too many pages", doc.file))?;
        let rel = match png {
            Some(png) => {
                let name = format!("p{i}.png");
                std::fs::write(sub.join(&name), png).map_err(|e| format!("{stem}/{name}: {e}"))?;
                Some(format!("{stem}/{name}"))
            }
            None => None,
        };
        rows.push(row(Some(page), rel));
    }
    Ok(rows)
}

/// The OCR manifest: the column line, then one line per row.
fn ocr_csv(rows: &[OcrRow]) -> String {
    let mut out = OCR_COLUMNS.join(",");
    out.push('\n');
    for r in rows {
        let cells = [
            field(&r.file),
            field(&r.class),
            field(&r.producer),
            field(&r.base_doc),
            r.role.name().to_owned(),
            r.page.map_or(String::new(), |p| p.to_string()),
            r.lang.label().to_owned(),
            r.png.as_deref().map_or(String::new(), field),
            u8::from(r.open_ok()).to_string(),
            r.lcs_f1.map_or(String::new(), ratio),
        ];
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

/// The ocr phase of a smoke run: `dir` is emptied, then every page of each
/// smoke path, in list order, is rendered (an original re-read through the
/// manifest gate, a corrupted file as its repair), and `dir/manifest.csv` is
/// written. Nothing but the PNGs and the manifest goes under `dir`: a PDF
/// there would let PaddleOCR rasterise it with PDFium instead of hayro.
fn write_ocr(
    dir: &Path,
    manifest: &Manifest,
    root: &Path,
    paths: &[String],
    originals: &BTreeMap<String, Original>,
    repaired: &BTreeMap<String, (Option<Vec<u8>>, Vec<Ratio>)>,
) -> Result<Vec<OcrRow>, String> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut rows = Vec::new();
    for (n, path) in paths.iter().enumerate() {
        eprintln!("corpus: ocr rasters, file {} of {}", n + 1, paths.len());
        let p = CorpusPath::parse(path)?;
        let original = originals
            .get(&p.original())
            .ok_or_else(|| format!("{path}: its original was not read"))?;
        let langs = &original.labels.langs;
        let doc = |role, lcs_f1| OcrDoc {
            file: path,
            class: p.class.map_or("", CorruptionClass::code),
            producer: p.producer,
            base_doc: p.base,
            role,
            langs,
            lcs_f1,
        };
        rows.extend(match p.class {
            None => {
                let bytes = read_checked(manifest, root, path)?;
                let own = original
                    .pages
                    .iter()
                    .zip(langs)
                    .map(|(page, &lang)| score(page, page, lang).lcs_f1)
                    .collect();
                ocr_document(dir, &doc(OcrRole::Original, own), Some(&bytes))?
            }
            Some(_) => {
                let (bytes, scores) = repaired
                    .get(path)
                    .ok_or_else(|| format!("{path}: no repair was recorded"))?;
                ocr_document(
                    dir,
                    &doc(OcrRole::Repaired, scores.clone()),
                    bytes.as_deref(),
                )?
            }
        });
    }
    std::fs::write(dir.join("manifest.csv"), ocr_csv(&rows))
        .map_err(|e| format!("manifest.csv: {e}"))?;
    Ok(rows)
}

// ── the run ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Smoke,
    Full,
    /// Smoke, then the T-38a page rasters and manifest.
    Ocr,
}

/// What a run printed and whether its gate held.
struct Summary {
    tables: Tables,
    inflate: InflateTally,
    panicked: u32,
}

/// One original, read once: its pages' text and labels.
struct Original {
    pages: Vec<String>,
    labels: Labels,
}

fn texts(bytes: &[u8]) -> Option<(Vec<String>, Vec<u32>)> {
    let pages = extract_text(bytes, &ExtractOptions::default()).ok()?;
    Some((
        pages.iter().map(page_string).collect(),
        pages.iter().map(|p| p.unmapped).collect(),
    ))
}

fn read_checked(manifest: &Manifest, root: &Path, path: &str) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(root.join(path)).map_err(|e| format!("{path}: {e}"))?;
    manifest.check_at(&bytes, path).map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// The clone looks like REPDF: both top directories, and for a full run every
/// producer and kind directory (a sparse smoke checkout has only `text`).
fn check_layout(root: &Path, mode: Mode) -> Result<(), String> {
    for top in ["corrupted", "original"] {
        for producer in PRODUCERS {
            let kinds: &[&str] = if mode == Mode::Full {
                &KINDS
            } else {
                &KINDS[..1]
            };
            for kind in kinds {
                let dir = root.join(top).join(producer).join(kind);
                if !dir.is_dir() {
                    return Err(format!(
                        "{} is not a REPDF clone: {top}/{producer}/{kind} is missing",
                        root.display()
                    ));
                }
            }
        }
    }
    Ok(())
}

fn out_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("target"),
            PathBuf::from,
        )
        .join("corpus")
}

/// Selection tuple cells of the chosen candidate.
fn tuple(chosen: Option<&CandidateReport>, output: Option<&[u8]>, prior: Toolpath) -> Tuple {
    let Some(c) = chosen else {
        return Tuple::default();
    };
    let g = &c.verification.v0;
    let bit = |b: bool| if b { '1' } else { '0' };
    let v1 = &c.verification.v1;
    let v2 = &c.verification.v2;
    let v3 = output.map(crate::pdf::repair::preservation);
    Tuple {
        v0: [
            g.lopdf_reload,
            g.hayro_render_all_pages,
            g.rediagnose_clean,
            g.page_count_ok,
            g.root_and_pages_present,
        ]
        .map(bit)
        .iter()
        .collect(),
        v1: format!(
            "blank={};glyphs={};text_ops={};images={};path_fills={};content_bytes={}",
            v1.blank_pages,
            ratio(v1.glyph_count),
            ratio(v1.text_ops),
            ratio(v1.images),
            ratio(v1.path_fills),
            ratio(v1.content_bytes)
        ),
        v2: format!(
            "fffd={};unmapped={};script={};dict={}",
            ratio(v2.fffd),
            ratio(v2.unmapped_glyph),
            ratio(v2.script_consistency),
            v2.dictionary_hit.map_or("-".to_owned(), ratio)
        ),
        v3: v3.map_or(String::new(), |p| {
            format!("features={};annotations={}", p.features, p.annotations)
        }),
        v4: format!(
            "prior={};size={}",
            u8::from(c.toolpath == prior),
            output.map_or(0, <[u8]>::len)
        ),
    }
}

fn toolpath_name(t: Toolpath) -> &'static str {
    match t {
        Toolpath::Resave => "resave",
        Toolpath::TemplateAssemble => "template_assemble",
    }
}

/// One corrupted file's run: its rows, its inflate tally, whether the
/// repair panicked, and the bytes that were scored as its repair (the output,
/// or the input itself when there was nothing to repair).
struct FileRun {
    rows: Vec<Row>,
    tally: InflateTally,
    panicked: bool,
    repaired: Option<Vec<u8>>,
}

/// Analyse, plan and repair one corrupted file and score its pages.
fn run_file(path: &str, bytes: &[u8], original: &Original, fonts: &FontDb) -> FileRun {
    let p = CorpusPath::parse(path).expect("a corrupted corpus path");
    let class = p.class.expect("a corrupted file").code();
    let aopts = AnalyzeOptions {
        threads: crate::jobs::salvage_threads(),
        ..AnalyzeOptions::default()
    };
    let ropts = RepairOptions {
        analyze: aopts.clone(),
        ..RepairOptions::default()
    };
    let work = catch_unwind(AssertUnwindSafe(|| {
        let analysis = engine::analyze(bytes, &aopts, &mut NullProgress).expect("never cancelled");
        let plan = engine::plan(&analysis, &ropts);
        let outcome = engine::repair(
            bytes,
            &analysis,
            &plan,
            &ropts,
            fonts,
            &mut UseBest,
            &mut NullProgress,
        )
        .expect("never cancelled");
        let tally = analysis
            .state
            .0
            .as_ref()
            .map(|s| inflate_disagreements(bytes, &s.carve))
            .unwrap_or_default();
        (plan, outcome, tally)
    }));

    let input = texts(bytes);
    let baseline = input.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
    let mut rows = Vec::new();
    // The planner found nothing to repair: the examiner keeps the input as
    // it is, so the input is what is scored.
    let untouched = work.as_ref().is_ok_and(|(plan, outcome, _)| {
        outcome.output.is_none()
            && outcome.status == OutcomeStatus::Ok
            && plan.candidates.is_empty()
    });
    let (outcome_name, chosen, v, repaired, report, tally) = match &work {
        Ok((plan, outcome, tally)) => {
            let name = match outcome.status {
                _ if untouched => "nothing_to_repair",
                OutcomeStatus::Ok => "ok",
                OutcomeStatus::Partial(_) => "partial",
                OutcomeStatus::Failed(_) => "failed",
            };
            let chosen = outcome.report.candidates.iter().find(|c| c.chosen);
            let v = tuple(chosen, outcome.output.as_deref(), plan.prior);
            let repaired = match outcome.output.as_deref() {
                Some(out) => texts(out),
                None if untouched => input.clone(),
                None => None,
            };
            (
                name,
                outcome.report.chosen.map_or("", toolpath_name),
                v,
                repaired,
                Some(&outcome.report),
                *tally,
            )
        }
        Err(_) => (
            "panicked",
            "",
            Tuple::default(),
            None,
            None,
            InflateTally::default(),
        ),
    };

    for (i, orig) in original.pages.iter().enumerate() {
        let lang = original.labels.langs[i];
        let (rep_text, unmapped) = repaired
            .as_ref()
            .map(|(t, u)| {
                (
                    t.get(i).map_or("", String::as_str),
                    u.get(i).copied().unwrap_or(0),
                )
            })
            .unwrap_or(("", 0));
        let scores = score(orig, rep_text, lang);
        let base = score(orig, baseline.get(i).map_or("", String::as_str), lang);
        let orig_tokens = tokens(&crate::bench::metrics::normalize(orig), lang).len();
        let c9 = report.map(|r| r.c9_summary).unwrap_or_default();
        rows.push(Row {
            file: path.to_owned(),
            class: class.to_owned(),
            producer: p.producer.to_owned(),
            base_doc: p.base.to_owned(),
            page: i as u32,
            lang,
            outcome: outcome_name.to_owned(),
            chosen_toolpath: chosen.to_owned(),
            v: v.clone(),
            scores,
            baseline: base,
            extract_failed: repaired.is_none() || orig_tokens == 0,
            unmapped_glyphs: unmapped,
            c9_exact: c9.exact,
            c9_accepted: c9.accepted,
            c9_ambiguous: c9.ambiguous,
            c9_unsearched: c9.unsearched,
            c9_work_total: report.map_or(0, |r| r.stats.salvage_work_total),
            reals_narrowed: report.map_or(0, |r| r.reals_narrowed),
            inflate_disagreements: tally.disagree,
        });
    }
    let panicked = work.is_err();
    let repaired = match work {
        Ok((_, outcome, _)) if !untouched => outcome.output,
        Ok(_) => Some(bytes.to_vec()),
        Err(_) => None,
    };
    FileRun {
        rows,
        tally,
        panicked,
        repaired,
    }
}

/// The whole run over `root`. Errors are one line each.
fn run(mode: Mode, root: &Path) -> Result<Summary, String> {
    check_layout(root, mode)?;
    let manifest = Manifest::pinned();
    let resolved = resolve_smoke(&manifest)?;
    let committed: Vec<&str> = SMOKE_SUBSET.lines().collect();
    if resolved != committed {
        return Err("the D-077 smoke resolution differs from bench/smoke_subset.txt".to_owned());
    }
    let paths: Vec<String> = match mode {
        Mode::Smoke | Mode::Ocr => resolved,
        Mode::Full => manifest.entries.iter().map(|e| e.path.clone()).collect(),
    };

    // Every file is checked before any is analysed; each is checked again
    // when it is read for analysis.
    for path in &paths {
        read_checked(&manifest, root, path)?;
    }

    let mut originals = BTreeMap::<String, Original>::new();
    for path in paths.iter().filter(|p| p.starts_with("original/")) {
        let bytes = read_checked(&manifest, root, path)?;
        let (pages, _) =
            texts(&bytes).ok_or_else(|| format!("{path}: the original did not load"))?;
        let mut labels = label_pages(&pages);
        if let Some(i) = labels.fill_one_unknown() {
            println!("corpus: {path} page {i}: language inferred from the other five");
        }
        labels.check(path)?;
        originals.insert(path.clone(), Original { pages, labels });
    }

    let fonts = FontDb::bundled();
    let corrupted: Vec<&String> = paths
        .iter()
        .filter(|p| p.starts_with("corrupted/"))
        .collect();
    let mut rows = Vec::new();
    let mut inflate = InflateTally::default();
    let mut panicked = 0;
    // The ocr mode's inputs: per corrupted file, its repair and page scores.
    let mut repaired = BTreeMap::<String, (Option<Vec<u8>>, Vec<Ratio>)>::new();
    for (n, path) in corrupted.iter().enumerate() {
        eprintln!("corpus: file {} of {}", n + 1, corrupted.len());
        let bytes = read_checked(&manifest, root, path)?;
        let original = CorpusPath::parse(path)?.original();
        let original = originals
            .get(&original)
            .ok_or_else(|| format!("{path}: its original was not read"))?;
        let file = run_file(path, &bytes, original, &fonts);
        if mode == Mode::Ocr {
            let scores = file.rows.iter().map(|r| r.scores.lcs_f1).collect();
            repaired.insert((*path).clone(), (file.repaired, scores));
        }
        rows.extend(file.rows);
        inflate.compared += file.tally.compared;
        inflate.disagree += file.tally.disagree;
        panicked += u32::from(file.panicked);
    }

    let out = out_dir();
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let write = |name: &str, text: String| {
        std::fs::write(out.join(name), text).map_err(|e| format!("{name}: {e}"))
    };
    write("corpus_results.csv", csv(&rows))?;
    let (header, golden) = match mode {
        Mode::Smoke | Mode::Ocr => {
            write("golden_smoke.csv", golden_csv(&rows))?;
            (D063_HEADER, Some(Golden::parse(GOLDEN_SMOKE)?))
        }
        Mode::Full => (FULL_HEADER, None),
    };
    if mode == Mode::Ocr {
        let dir = out.join("ocr_inputs");
        let rows = write_ocr(&dir, &manifest, root, &paths, &originals, &repaired)?;
        let unopened = rows.iter().filter(|r| !r.open_ok()).count();
        println!(
            "ocr: {} rows, {} PNGs, {unopened} without a raster (open_ok = 0); {}",
            rows.len(),
            rows.len() - unopened,
            dir.join("manifest.csv").display()
        );
    }
    Ok(Summary {
        tables: tables(header, &rows, golden.as_ref()),
        inflate,
        panicked,
    })
}

fn harness(mode: Mode) {
    let Some(root) = std::env::var_os("PDFPUNDIT_CORPUS") else {
        println!(
            "corpus: PDFPUNDIT_CORPUS is not set; skipped (set it to a local clone of \
             github.com/dfrc-korea/REPDF at e547d4d)"
        );
        return;
    };
    let summary = run(mode, Path::new(&root)).unwrap_or_else(|e| panic!("{e}"));
    println!("{}", summary.tables.text);
    println!(
        "inflate backend disagreements: {} of {} Flate streams (expected 0)",
        summary.inflate.disagree, summary.inflate.compared
    );
    println!("files whose repair panicked: {}", summary.panicked);
    assert_eq!(summary.panicked, 0, "a repair panicked");
    assert_eq!(summary.inflate.disagree, 0, "the inflate backends disagree");
    assert!(
        summary.tables.failed.is_empty(),
        "below the golden by more than 2%: {}",
        summary.tables.failed.join(", ")
    );
}

/// The D-013 smoke subset, gated against `bench/golden_smoke.csv`.
#[test]
#[ignore = "needs PDFPUNDIT_CORPUS: a local REPDF clone at e547d4d"]
fn smoke() {
    harness(Mode::Smoke);
}

/// The smoke run, then every page of each original and each repaired output
/// rasterised for the OCR harness (T-38a, T-38b).
#[test]
#[ignore = "needs PDFPUNDIT_CORPUS: a local REPDF clone at e547d4d"]
fn ocr() {
    harness(Mode::Ocr);
}

/// Every corrupted file of the corpus; no gate.
#[test]
#[ignore = "needs PDFPUNDIT_CORPUS: a local REPDF clone at e547d4d"]
fn full() {
    harness(Mode::Full);
}

/// One `CIDFontType2` of a corpus C8 file, as C8's name path takes it with
/// one database: `Some(Ok(id))` confirmed, `Some(Err(id, why))` matched but
/// rejected, `None` no name match.
type NameHit = Option<Result<String, (String, String)>>;

/// The bundled database with only its fonts that carry a program: the two
/// Noto fonts, as before G-10.
fn programs_only() -> FontDb {
    use crate::pdf::fontdb::build::IndexEntry;
    use crate::pdf::fontdb::template::{BUNDLED, BUNDLED_INDEX};
    let entries: Vec<IndexEntry> = serde_json::from_slice(BUNDLED_INDEX).expect("index parses");
    let kept: Vec<&IndexEntry> = entries.iter().filter(|e| e.drawn_with.is_none()).collect();
    let index = serde_json::to_vec(&kept).expect("index serialises");
    let blob = |name: &str| -> Option<&'static [u8]> {
        let (id, ext) = name.rsplit_once('.')?;
        let font = BUNDLED.iter().find(|f| f.id == id)?;
        match ext {
            "ttf" => font.ttf,
            "gmap" => Some(font.gmap),
            _ => None,
        }
    };
    FontDb::from_bytes(&index, &blob).expect("the Noto fonts load")
}

/// `doc`'s object a reference names, else the object itself.
fn follow<'d>(doc: &'d lopdf::Document) -> impl Fn(&'d lopdf::Object) -> Option<&'d lopdf::Object> {
    move |v| match v {
        lopdf::Object::Reference(id) => doc.objects.get(id),
        other => Some(other),
    }
}

/// The C8 name path over every `remove_unicode_fonts` file (G-10): each
/// `CIDFontType2`'s `/BaseFont` matched by name, and the match checked
/// against every code its `/W` gives a width, with the bundled database and
/// with its two Noto fonts alone. Prints one row per `/BaseFont` name and
/// fails unless the bundled database confirms more fonts. Nothing is
/// written.
#[test]
#[ignore = "needs PDFPUNDIT_CORPUS: a local REPDF clone at e547d4d"]
fn c8_names() {
    use crate::pdf::repair::name_match;
    use lopdf::Object;
    let Some(root) = std::env::var_os("PDFPUNDIT_CORPUS") else {
        println!("corpus: PDFPUNDIT_CORPUS is not set; skipped");
        return;
    };
    let root = Path::new(&root);
    let manifest = Manifest::pinned();
    let dbs = [programs_only(), (*FontDb::bundled()).clone()];
    // name → (fonts, per database: confirmed, a matched id, a rejection).
    let mut rows = BTreeMap::<String, (u32, [(u32, Option<String>, Option<String>); 2])>::new();
    let paths = manifest
        .entries
        .iter()
        .map(|e| &e.path)
        .filter(|p| p.ends_with("_remove_unicode_fonts.pdf"));
    for path in paths {
        let bytes = read_checked(&manifest, root, path).unwrap_or_else(|e| panic!("{e}"));
        let Ok(doc) = lopdf::Document::load_mem(&bytes) else {
            println!("c8_names: {path} does not load; skipped");
            continue;
        };
        let resolve = follow(&doc);
        for object in doc.objects.values() {
            let Object::Dictionary(d) = object else {
                continue;
            };
            let is_cid = d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"CIDFontType2");
            let Some(name) = (d.get(b"BaseFont").and_then(Object::as_name).ok())
                .filter(|_| is_cid)
                .map(|n| String::from_utf8_lossy(n).into_owned())
            else {
                continue;
            };
            let key = match name.split_once('+') {
                Some((tag, rest)) if tag.len() == 6 => rest.to_owned(),
                _ => name.clone(),
            };
            let row = rows.entry(key).or_default();
            row.0 += 1;
            for (db, cell) in dbs.iter().zip(row.1.iter_mut()) {
                let hit: NameHit = name_match(db, &name, d, &resolve)
                    .map(|(id, checked)| checked.map(|_| id.clone()).map_err(|why| (id, why)));
                match hit {
                    Some(Ok(id)) => {
                        cell.0 += 1;
                        cell.1 = Some(id);
                    }
                    Some(Err((id, why))) => {
                        cell.1.get_or_insert(id);
                        cell.2.get_or_insert(why);
                    }
                    None => {}
                }
            }
        }
    }
    println!("C8 name path over the corpus's remove_unicode_fonts files (G-10)");
    println!(
        "name | CIDFonts | Noto only: confirmed | bundled: confirmed, matched | first rejection"
    );
    let mut totals = [0u32; 2];
    for (name, (n, cells)) in &rows {
        totals[0] += cells[0].0;
        totals[1] += cells[1].0;
        println!(
            "{name} | {n} | {} | {}, {} | {}",
            cells[0].0,
            cells[1].0,
            cells[1].1.as_deref().unwrap_or("-"),
            cells[1].2.as_deref().unwrap_or("-")
        );
    }
    let fonts: u32 = rows.values().map(|r| r.0).sum();
    println!(
        "confirmed by name: {} of {fonts} CIDFonts with the Noto fonts alone, {} with the \
         bundled database",
        totals[0], totals[1]
    );
    assert!(
        totals[1] > totals[0],
        "the bundled database confirms no more"
    );
}
