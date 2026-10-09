//! Font inference scorer and resolution policy (T-28, TD §5.2 and §18.2, SE Q3,
//! D-009).
//!
//! [`infer`] decodes a font slot's codes through every glyph map `gmap_of`
//! gives (the database fonts with a program,
//! [`FontDb::inference_gmaps`]) and scores the text against word lists; [`resolve`] turns the ranked
//! candidates into a [`FontDecision`]. Everything is integer: scores are
//! fixed point ×1000 and every fraction is an exact [`Ratio`], so a ranking is
//! the same on every platform. No hash container is iterated.
//!
//! Scoring, per font and per dictionary (TD §18.2, weights ×1000):
//! - each code is a glyph id (the slot is `Identity-H`, or the caller has
//!   mapped its codes through the slot's surviving encoding, T-27c), read as
//!   text through the font's `.gmap`, `U+FFFD` where the `.gmap` has no
//!   record;
//! - each segment between two `breaks` is [`normalize`]d and split on spaces
//!   into tokens; a token is looked up without its edge punctuation
//!   ([`word_core`]);
//! - a dictionary word scores [`WORD`] per `char`; otherwise the longest
//!   dictionary word that is a prefix of it scores [`PREFIX`] per `char`;
//! - [`REPLACEMENT`] per `U+FFFD`, [`OUTSIDE_SCRIPT`] per letter of a script
//!   the font's index entry does not list, [`MIXED_SCRIPTS`] per token whose
//!   letters come from more than one script;
//! - `zh`: the mean bigram log-probability when the dictionary knows any
//!   bigram of the sample (an unknown pair counts as [`BIGRAM_FLOOR`]), else
//!   every character is a one-character word.
//!
//! A font's candidate is its best dictionary (ties: the earlier dictionary,
//! the `/ToUnicode` one last). `hit` is the characters inside dictionary words
//! (inside known bigrams for `zh`) over all non-space characters; `margin` is
//! `hit` minus the hit of the best-scored other candidate, clamped to [0, 1]
//! (SE Q3: the TD's `(best − second)/max(best, ε)` divides by ε once penalties
//! push the best score to zero or below); `confidence = (hit + margin) / 2`.

// T-30 (the C7/C8 passes) is the first caller outside the tests.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::BTreeSet;

use super::FontDb;
use super::build::IndexEntry;
use super::dict::{Dictionary, EmptyDictionary, word_core};
use super::gmap::GmapTable;
use crate::bench::metrics::{Lang, normalize};
use crate::engine::{
    FontCandidate, FontPickRequest, FontSlot, InteractionRequestId, RepairOptions,
    SubstituteChoice, ToUnicodeState, UnreproduciblePolicy, UnreproducibleRequest,
};
use crate::pdf::model::Ratio;

/// Score per `char` of a dictionary word.
pub(crate) const WORD: i64 = 2000;
/// Score per `char` of the longest dictionary prefix of a non-word.
pub(crate) const PREFIX: i64 = 500;
/// Score per `U+FFFD` (a code the font's `.gmap` does not map).
pub(crate) const REPLACEMENT: i64 = -3000;
/// Score per letter of a script the font does not cover.
pub(crate) const OUTSIDE_SCRIPT: i64 = -1500;
/// Score per token mixing letters of two or more scripts.
pub(crate) const MIXED_SCRIPTS: i64 = -2000;
/// The log-probability, in millinats, of a bigram the dictionary lacks.
pub(crate) const BIGRAM_FLOOR: i64 = -20_000;

/// Below this many characters a slot never auto-accepts (SE Q3: the TD's
/// formula ignores the sample size).
pub(crate) const SAMPLE_FLOOR: u64 = 20;
/// The hit a slot needs to auto-accept (D-009's interim clamp).
pub(crate) const MIN_HIT: Ratio = Ratio { num: 1, den: 2 };
/// The share of a slot's code points a font must map to reproduce it.
pub(crate) const MIN_COVERAGE: Ratio = Ratio { num: 95, den: 100 };
/// The longest preview, in `char`s.
pub(crate) const PREVIEW_CHARS: usize = 200;
/// The most codes a [`FontPickRequest`] samples.
pub(crate) const SAMPLE_CODES: usize = 64;

/// What an [`UnreproducibleRequest`] gives as its reason.
pub(crate) const UNREPRODUCIBLE_REASON: &str = "no bundled font covers the glyphs";

/// One text-showing run of a slot: its codes and where its tokens start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodeRun {
    pub(crate) codes: Vec<u16>,
    /// Indices into `codes` where a new token starts: after space glyphs,
    /// at `TJ` offsets beyond −250/1000 em and at `Td`/`T*` line breaks.
    /// Indices outside `1..codes.len()` are ignored.
    pub(crate) breaks: Vec<usize>,
}

/// One database font's reading of a slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub(crate) font_id: String,
    pub(crate) lang: Lang,
    /// Fixed point ×1000; may be negative.
    pub(crate) score: i64,
    pub(crate) hit: Ratio,
    pub(crate) margin: Ratio,
    pub(crate) confidence: Ratio,
    /// The slot's text through this font, at most [`PREVIEW_CHARS`] chars,
    /// control characters shown as `U+FFFD`.
    pub(crate) preview: String,
}

/// What [`infer`] saw, for the corpus CSV (SE Q3 item 1; T-26).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InferenceTrace {
    /// The top candidate's; 0 when there is none.
    pub(crate) hit: Ratio,
    pub(crate) margin: Ratio,
    /// The top two scores.
    pub(crate) raw_best: Option<i64>,
    pub(crate) raw_second: Option<i64>,
    /// Non-space characters of the top candidate's text: `hit`'s denominator.
    pub(crate) n_chars: u64,
    pub(crate) n_runs: u64,
    /// The top candidate's language.
    pub(crate) lang: Lang,
    /// The ISO 15924 code of the commonest script among the top candidate's
    /// letters (`Zyyy` when it has none, `Zzzz` for letters of a script the
    /// scorer does not name).
    pub(crate) script: String,
    /// Whether a `/ToUnicode` dictionary was scored.
    pub(crate) tounicode_dict_used: bool,
}

/// How a damaged slot is to be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FontDecision {
    /// Substitute this candidate's font without asking.
    AutoAccept(Candidate),
    /// Ask which font (T-30 sends `FontPick`).
    Ask(FontPickRequest),
    /// No database font can reproduce the slot. Under
    /// `UnreproduciblePolicy::Ask` T-30 sends `FontUnreproducible`; under
    /// `SubstituteGeneric` it answers with the first option without a prompt.
    Unreproducible(UnreproducibleRequest),
    /// Keep the slot's text only (`UnreproduciblePolicy::TextOnly`).
    TextOnly,
}

/// Every font of `fonts` with a `.gmap` in `gmap_of`, scored against `dicts`
/// (none: [`EmptyDictionary`]) and the per-document `tounicode_dict`, sorted
/// by `(score desc, font_id asc)`. One candidate per font.
pub(crate) fn infer<'g>(
    codes: &[CodeRun],
    fonts: &[&IndexEntry],
    gmap_of: &dyn Fn(&str) -> Option<&'g GmapTable<'g>>,
    dicts: &[&dyn Dictionary],
    tounicode_dict: Option<&dyn Dictionary>,
) -> (Vec<Candidate>, InferenceTrace) {
    let mut all: Vec<&dyn Dictionary> = if dicts.is_empty() {
        vec![&EmptyDictionary]
    } else {
        dicts.to_vec()
    };
    all.extend(tounicode_dict);

    let readings: Vec<(&IndexEntry, Reading)> = fonts
        .iter()
        .filter_map(|&entry| Some((entry, read(codes, entry, gmap_of(&entry.id)?))))
        .collect();
    // Whether each dictionary scores by bigrams: decided over every font's
    // reading, so that every font is scored the same way.
    let bigrams: Vec<bool> = all
        .iter()
        .map(|d| {
            d.lang() == Lang::Zh
                && readings.iter().any(|(_, r)| {
                    r.tokens.iter().any(|t| {
                        pairs(word_core(t)).any(|(a, b)| d.bigram_millinats(a, b).is_some())
                    })
                })
        })
        .collect();

    let mut cands: Vec<(Candidate, &Reading)> = readings
        .iter()
        .map(|(entry, r)| {
            let (lang, credit, matched) = all
                .iter()
                .zip(&bigrams)
                .map(|(d, &bi)| {
                    let (credit, matched) = credit(&r.tokens, *d, bi);
                    (d.lang(), credit, matched)
                })
                // max_by_key keeps the last maximum; reversing keeps the first.
                .rev()
                .max_by_key(|&(_, credit, _)| credit)
                .unwrap_or((Lang::Unknown, 0, 0));
            let hit = if r.chars == 0 {
                ZERO
            } else {
                ratio(u128::from(matched), u128::from(r.chars))
            };
            let cand = Candidate {
                font_id: entry.id.clone(),
                lang,
                score: credit + r.penalty,
                hit,
                margin: ZERO,
                confidence: ZERO,
                preview: r.preview.clone(),
            };
            (cand, r)
        })
        .collect();
    cands.sort_by(|(a, _), (b, _)| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.font_id.cmp(&b.font_id))
    });

    let hits: Vec<Ratio> = cands.iter().map(|(c, _)| c.hit).collect();
    for (i, (c, _)) in cands.iter_mut().enumerate() {
        let other = if i == 0 { hits.get(1) } else { hits.first() };
        c.margin = minus_clamped(c.hit, other.copied().unwrap_or(ZERO));
        c.confidence = mean(c.hit, c.margin);
    }

    let top = cands.first();
    let trace = InferenceTrace {
        hit: top.map_or(ZERO, |(c, _)| c.hit),
        margin: top.map_or(ZERO, |(c, _)| c.margin),
        raw_best: top.map(|(c, _)| c.score),
        raw_second: cands.get(1).map(|(c, _)| c.score),
        n_chars: top.map_or(0, |(_, r)| r.chars),
        n_runs: codes.len() as u64,
        lang: top.map_or(Lang::Unknown, |(c, _)| c.lang),
        script: top.map_or(COMMON, |(_, r)| r.script).to_owned(),
        tounicode_dict_used: tounicode_dict.is_some(),
    };
    (cands.into_iter().map(|(c, _)| c).collect(), trace)
}

/// The decision for `slot` given `infer`'s ranking (D-009's interim clamp,
/// SE Q3), and the provenance lines its `FontResolution` records.
///
/// - The top candidate auto-accepts iff `confidence ≥
///   opts.auto_accept_confidence`, `hit ≥ 1/2` and the trace counts at least
///   [`SAMPLE_FLOOR`] characters.
/// - A slot that decodes (its `/ToUnicode` survives, or the top candidate
///   auto-accepts) and whose `needed_codes` (code points; control characters
///   are not counted) no font of `fonts` maps for at least 95% is
///   unreproducible, as is every slot when `fonts` has no candidate:
///   [`FontDecision::TextOnly`] under `UnreproduciblePolicy::TextOnly`, else
///   [`FontDecision::Unreproducible`] offering every font, best coverage
///   first.
/// - Otherwise the slot auto-accepts or asks, with the top
///   `opts.max_font_candidates` candidates and up to [`SAMPLE_CODES`]
///   distinct codes of `codes`.
///
/// Request ids are 0: the facade numbers the questions it asks.
pub(crate) fn resolve(
    cands: &[Candidate],
    trace: &InferenceTrace,
    opts: &RepairOptions,
    slot: &FontSlot,
    needed_codes: &[u32],
    codes: &[CodeRun],
    fonts: &FontDb,
) -> (FontDecision, Vec<String>) {
    let mut provenance = vec![format!(
        "inference: {} runs, {} characters, lang {}, script {}, hit {}, margin {}{}",
        trace.n_runs,
        trace.n_chars,
        trace.lang.label(),
        trace.script,
        show(trace.hit),
        show(trace.margin),
        if trace.tounicode_dict_used {
            ", /ToUnicode dictionary"
        } else {
            ""
        },
    )];
    let top = cands.first();
    let refusal = match top {
        None => Some("no candidate font".to_owned()),
        Some(c) if c.confidence < opts.auto_accept_confidence => Some(format!(
            "confidence {} below {}",
            show(c.confidence),
            show(opts.auto_accept_confidence)
        )),
        Some(c) if c.hit < MIN_HIT => Some(format!("hit {} below 1/2", show(c.hit))),
        Some(_) if trace.n_chars < SAMPLE_FLOOR => Some(format!(
            "{} characters, below the sample floor of {SAMPLE_FLOOR}",
            trace.n_chars
        )),
        Some(_) => None,
    };
    let decodes = slot.tounicode == ToUnicodeState::Present || refusal.is_none();

    let coverage = coverage(fonts, needed_codes);
    let best_coverage = coverage.first().map_or(ZERO, |(r, _)| *r);
    // `coverage` counts a slot that needs nothing as fully covered.
    let reproducible = !cands.is_empty() && (!decodes || best_coverage >= MIN_COVERAGE);

    if !reproducible {
        provenance.push(if cands.is_empty() {
            "unreproducible: no candidate font".to_owned()
        } else {
            format!(
                "unreproducible: the best font maps {} of the slot's code points, below 95/100",
                show(best_coverage)
            )
        });
        return match opts.unreproducible {
            UnreproduciblePolicy::TextOnly => {
                provenance.push("policy: text only".to_owned());
                (FontDecision::TextOnly, provenance)
            }
            policy => {
                provenance.push(
                    if policy == UnreproduciblePolicy::SubstituteGeneric {
                        "policy: substitute generic, no prompt"
                    } else {
                        "policy: ask"
                    }
                    .to_owned(),
                );
                let req = UnreproducibleRequest {
                    id: InteractionRequestId(0),
                    family: family(slot),
                    slots: vec![(slot.page, slot.slot.clone())],
                    reason: UNREPRODUCIBLE_REASON.to_owned(),
                    options: coverage
                        .iter()
                        .map(|(_, e)| SubstituteChoice {
                            font_id: e.id.clone(),
                            label: e.family.clone(),
                        })
                        .collect(),
                };
                (FontDecision::Unreproducible(req), provenance)
            }
        };
    }

    match (top, refusal) {
        (Some(c), None) => {
            provenance.push(format!(
                "auto-accepted {}: confidence {} ≥ {}, hit ≥ 1/2, {} characters ≥ {SAMPLE_FLOOR}",
                c.font_id,
                show(c.confidence),
                show(opts.auto_accept_confidence),
                trace.n_chars
            ));
            (FontDecision::AutoAccept(c.clone()), provenance)
        }
        (_, refusal) => {
            provenance.push(format!(
                "asked: {}",
                refusal.unwrap_or_else(|| "no candidate font".to_owned())
            ));
            let max = usize::try_from(opts.max_font_candidates).unwrap_or(usize::MAX);
            let mut seen = BTreeSet::new();
            let sample_codes = codes
                .iter()
                .flat_map(|r| &r.codes)
                .filter(|&&c| seen.insert(c))
                .take(SAMPLE_CODES)
                .map(|&c| u32::from(c))
                .collect();
            let req = FontPickRequest {
                id: InteractionRequestId(0),
                page: slot.page,
                slot: slot.slot.clone(),
                sample_codes,
                candidates: cands
                    .iter()
                    .take(max)
                    .map(|c| FontCandidate {
                        font_id: c.font_id.clone(),
                        family: fonts
                            .entry(&c.font_id)
                            .map_or_else(|| c.font_id.clone(), |e| e.family.clone()),
                        language: c.lang.label().to_owned(),
                        score: c.hit,
                        confidence: c.confidence,
                        preview: c.preview.clone(),
                    })
                    .collect(),
                preview: top.map(|c| c.preview.clone()).unwrap_or_default(),
            };
            (FontDecision::Ask(req), provenance)
        }
    }
}

// ── reading a slot through one font ─────────────────────────────────────

/// A slot's text through one font: what every dictionary is scored on.
struct Reading {
    /// Normalised, non-empty, space-free tokens.
    tokens: Vec<String>,
    /// Non-space characters over all tokens.
    chars: u64,
    /// The font-dependent penalties: `U+FFFD`, scripts.
    penalty: i64,
    preview: String,
    script: &'static str,
}

fn read(codes: &[CodeRun], entry: &IndexEntry, gmap: &GmapTable) -> Reading {
    let mut tokens = Vec::new();
    let mut preview = String::new();
    for run in codes {
        let mut starts: Vec<usize> = run
            .breaks
            .iter()
            .copied()
            .filter(|&b| b > 0 && b < run.codes.len())
            .collect();
        starts.sort_unstable();
        starts.dedup();
        let mut from = 0;
        for to in starts.into_iter().chain([run.codes.len()]) {
            let raw: String = run.codes[from..to]
                .iter()
                .map(|&code| gmap.unicode(code).unwrap_or(char::REPLACEMENT_CHARACTER))
                .collect();
            from = to;
            if !preview.is_empty() && !preview.ends_with(char::is_whitespace) {
                preview.push(' ');
            }
            preview.push_str(&raw);
            tokens.extend(
                normalize(&raw)
                    .split(' ')
                    .filter(|t| !t.is_empty())
                    .map(str::to_owned),
            );
        }
    }

    let mut penalty = 0;
    let mut chars = 0;
    // (script, letters) in first-seen order; a handful of entries.
    let mut counts: Vec<(&'static str, u64)> = Vec::new();
    for token in &tokens {
        let mut scripts: Vec<&'static str> = Vec::new();
        for c in token.chars() {
            chars += 1;
            if c == char::REPLACEMENT_CHARACTER {
                penalty += REPLACEMENT;
                continue;
            }
            let Some(script) = script_of(c) else { continue };
            if !entry.scripts.iter().any(|s| s == script) {
                penalty += OUTSIDE_SCRIPT;
            }
            if !scripts.contains(&script) {
                scripts.push(script);
            }
            match counts.iter_mut().find(|(s, _)| *s == script) {
                Some((_, n)) => *n += 1,
                None => counts.push((script, 1)),
            }
        }
        if scripts.len() > 1 {
            penalty += MIXED_SCRIPTS;
        }
    }
    let script = counts
        .iter()
        .max_by(|(a, an), (b, bn)| an.cmp(bn).then_with(|| b.cmp(a)))
        .map_or(COMMON, |(s, _)| *s);

    let preview = preview
        .chars()
        .map(|c| {
            if c.is_control() {
                char::REPLACEMENT_CHARACTER
            } else {
                c
            }
        })
        .take(PREVIEW_CHARS)
        .collect();
    Reading {
        tokens,
        chars,
        penalty,
        preview,
        script,
    }
}

/// One dictionary's credit for `tokens` and the characters it matched.
fn credit(tokens: &[String], dict: &dyn Dictionary, bigrams: bool) -> (i64, u64) {
    let mut score = 0;
    let mut matched = 0;
    if bigrams {
        let (mut sum, mut n) = (0_i64, 0_i64);
        for token in tokens {
            let chars: Vec<char> = word_core(token).chars().collect();
            let mut in_known = vec![false; chars.len()];
            for (i, w) in chars.windows(2).enumerate() {
                n += 1;
                match dict.bigram_millinats(w[0], w[1]) {
                    Some(m) => {
                        sum += i64::from(m);
                        in_known[i] = true;
                        in_known[i + 1] = true;
                    }
                    None => sum += BIGRAM_FLOOR,
                }
            }
            matched += in_known.iter().filter(|&&k| k).count() as u64;
        }
        if n > 0 {
            score = sum.div_euclid(n);
        }
        return (score, matched);
    }
    for token in tokens {
        let core = word_core(token);
        let words: Vec<&str> = if dict.lang() == Lang::Zh {
            core.char_indices()
                .map(|(at, c)| &core[at..at + c.len_utf8()])
                .collect()
        } else if core.is_empty() {
            Vec::new()
        } else {
            vec![core]
        };
        for w in words {
            let len = w.chars().count() as u64;
            if dict.contains(w) {
                score += WORD * len as i64;
                matched += len;
            } else {
                score += PREFIX * dict.longest_prefix(w) as i64;
            }
        }
    }
    (score, matched)
}

fn pairs(s: &str) -> impl Iterator<Item = (char, char)> + '_ {
    s.chars().zip(s.chars().skip(1))
}

// ── scripts ──────────────────────────────────────────────────────────────

/// The script code of letters no range below names.
const UNKNOWN: &str = "Zzzz";
/// The script code of everything that is not a letter.
const COMMON: &str = "Zyyy";

/// The ISO 15924 code of letter `c`'s script, as the index's `scripts` lists
/// them; `None` for digits, punctuation, symbols, spaces, combining marks of
/// no script and `U+FFFD`.
fn script_of(c: char) -> Option<&'static str> {
    const RANGES: [(u32, u32, &str); 23] = [
        (0x0041, 0x024F, "Latn"),
        (0x1E00, 0x1EFF, "Latn"),
        (0x2C60, 0x2C7F, "Latn"),
        (0xA720, 0xA7FF, "Latn"),
        (0xFB00, 0xFB06, "Latn"),
        (0xFF21, 0xFF5A, "Latn"),
        (0x0370, 0x03FF, "Grek"),
        (0x1F00, 0x1FFF, "Grek"),
        (0x0400, 0x052F, "Cyrl"),
        (0x0591, 0x05FF, "Hebr"),
        (0xFB1D, 0xFB4F, "Hebr"),
        (0x0600, 0x06FF, "Arab"),
        (0x0750, 0x077F, "Arab"),
        (0x08A0, 0x08FF, "Arab"),
        (0xFB50, 0xFDFF, "Arab"),
        (0xFE70, 0xFEFF, "Arab"),
        (0x0900, 0x097F, "Deva"),
        (0xA8E0, 0xA8FF, "Deva"),
        (0x2E80, 0x2FDF, "Hani"),
        (0x3005, 0x303B, "Hani"),
        (0x3400, 0x9FFF, "Hani"),
        (0xF900, 0xFAFF, "Hani"),
        (0x20000, 0x3134F, "Hani"),
    ];
    let u = u32::from(c);
    // Combining diacritics belong to the letter they sit on.
    if !c.is_alphabetic() || (0x0300..=0x036F).contains(&u) {
        return None;
    }
    Some(
        RANGES
            .iter()
            .find(|&&(lo, hi, _)| (lo..=hi).contains(&u))
            .map_or(UNKNOWN, |&(_, _, s)| s),
    )
}

// ── coverage, families, ratios ───────────────────────────────────────────

/// Every font of `fonts` with a program (a `.gmap`-only font cannot draw)
/// with the share of `needed` (control characters skipped) its `.gmap` maps,
/// best first, then by id.
fn coverage<'f>(fonts: &'f FontDb, needed: &[u32]) -> Vec<(Ratio, &'f IndexEntry)> {
    let needed: Vec<u32> = needed.iter().copied().filter(|&c| !is_control(c)).collect();
    let mut out: Vec<(Ratio, &IndexEntry)> = fonts
        .inference_gmaps()
        .into_iter()
        .filter_map(|(id, gmap)| {
            let mapped: BTreeSet<u32> = gmap
                .records()
                .iter()
                .map(|r| u32::from(r.unicode()))
                .collect();
            let n = needed.iter().filter(|c| mapped.contains(c)).count();
            let share = if needed.is_empty() {
                ONE
            } else {
                ratio(n as u128, needed.len() as u128)
            };
            Some((share, fonts.entry(id)?))
        })
        .collect();
    out.sort_by(|(a, ae), (b, be)| b.cmp(a).then_with(|| ae.id.cmp(&be.id)));
    out
}

fn is_control(c: u32) -> bool {
    char::from_u32(c).is_some_and(char::is_control)
}

/// The slot's font family: its `/BaseFont` without the leading `/` and a
/// six-capital subset tag, else the slot's resource name.
fn family(slot: &FontSlot) -> String {
    let Some(name) = &slot.base_font else {
        return slot.slot.clone();
    };
    let name = name.strip_prefix('/').unwrap_or(name);
    match name.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()) => {
            rest.to_owned()
        }
        _ => name.to_owned(),
    }
}

const ZERO: Ratio = Ratio { num: 0, den: 1 };
const ONE: Ratio = Ratio { num: 1, den: 1 };

/// `num/den` in lowest terms, halved until both fit in `u64`. `den > 0`.
fn ratio(mut num: u128, mut den: u128) -> Ratio {
    let g = gcd(num, den);
    if g > 1 {
        num /= g;
        den /= g;
    }
    while num > u128::from(u64::MAX) || den > u128::from(u64::MAX) {
        num >>= 1;
        den >>= 1;
    }
    Ratio {
        num: num as u64,
        den: den.max(1) as u64,
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// `max(a − b, 0)`; both are hits, so the result is at most 1.
fn minus_clamped(a: Ratio, b: Ratio) -> Ratio {
    if a <= b {
        return ZERO;
    }
    let (an, ad) = (u128::from(a.num), u128::from(a.den.max(1)));
    let (bn, bd) = (u128::from(b.num), u128::from(b.den.max(1)));
    ratio(an * bd - bn * ad, ad * bd)
}

/// `(a + b) / 2`.
fn mean(a: Ratio, b: Ratio) -> Ratio {
    let (an, ad) = (u128::from(a.num), u128::from(a.den.max(1)));
    let (bn, bd) = (u128::from(b.num), u128::from(b.den.max(1)));
    ratio(an * bd + bn * ad, 2 * ad * bd)
}

fn show(r: Ratio) -> String {
    format!("{}/{}", r.num, r.den)
}

#[cfg(test)]
mod tests;
