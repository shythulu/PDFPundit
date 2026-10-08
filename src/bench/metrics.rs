//! Pure text-recovery metrics, no I/O (T-25).
//!
//! Scores compare a repaired page's text with the original's (TD §8, SE Q4):
//! both are normalised, tokenised (whitespace runs, or one token per character
//! for Chinese), and compared by their longest common subsequence and as
//! multisets. Every value is an exact [`Ratio`]; there is no float anywhere in
//! this module and no hash container, so a score is the same on every platform.
//! The language label is an input (D-069): nothing here guesses it.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::BTreeMap;

use similar::{Algorithm, DiffOp, TextDiff};
use unicode_normalization::UnicodeNormalization;

use crate::pdf::model::Ratio;

/// The fixed-point scale of [`Mean`]: each value is floored to a billionth.
const MEAN_SCALE: u128 = 1_000_000_000;

/// A page's language, as the corpus harness labels it (T-26 column values).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lang {
    En,
    Fr,
    Es,
    Ar,
    Hi,
    Zh,
    Unknown,
}

impl Lang {
    /// `"en"`, `"fr"`, `"es"`, `"ar"`, `"hi"`, `"zh"`; anything else is `Unknown`.
    pub(crate) fn from_label(label: &str) -> Lang {
        match label {
            "en" => Lang::En,
            "fr" => Lang::Fr,
            "es" => Lang::Es,
            "ar" => Lang::Ar,
            "hi" => Lang::Hi,
            "zh" => Lang::Zh,
            _ => Lang::Unknown,
        }
    }
}

/// NFC, then per-char `to_lowercase` as the casefold, then whitespace runs
/// collapsed to one ASCII space and trimmed (TD §8).
pub(crate) fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut gap = false;
    for c in s.nfc().flat_map(char::to_lowercase) {
        if c.is_whitespace() {
            gap = !out.is_empty();
        } else {
            if gap {
                out.push(' ');
                gap = false;
            }
            out.push(c);
        }
    }
    out
}

/// Splits `text` on whitespace; `Zh` yields one token per non-whitespace
/// `char` (SE Q4). The text is not normalised here.
pub(crate) fn tokens(text: &str, lang: Lang) -> Vec<String> {
    match lang {
        Lang::Zh => text
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(String::from)
            .collect(),
        _ => text.split_whitespace().map(String::from).collect(),
    }
}

/// The scores of one repaired text against its original. A zero denominator
/// gives `Ratio { num: 0, den: 1 }`; the caller records `extract_failed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextScores {
    /// `2·lcs / (len_orig + len_rep)`: the TD's `recovery` (ROUGE-L F1).
    pub(crate) lcs_f1: Ratio,
    /// `lcs / len_orig`.
    pub(crate) recall: Ratio,
    /// `lcs / len_rep`.
    pub(crate) precision: Ratio,
    /// `2·Σ_w min(c_orig(w), c_rep(w)) / (len_orig + len_rep)`, order-insensitive;
    /// `lcs_f1 − bag_f1` is the ordering error.
    pub(crate) bag_f1: Ratio,
    /// The LCS-F1 over the `char`s of the normalised strings.
    pub(crate) char_f1: Ratio,
    /// Tokens in the original.
    pub(crate) len_orig: u64,
    /// Tokens in the repair.
    pub(crate) len_rep: u64,
    /// The longest common token subsequence's length.
    pub(crate) lcs: u64,
}

/// Scores `rep` against `orig`, both normalised first.
pub(crate) fn score(orig: &str, rep: &str, lang: Lang) -> TextScores {
    let (orig, rep) = (normalize(orig), normalize(rep));
    let (orig_tokens, rep_tokens) = (tokens(&orig, lang), tokens(&rep, lang));
    let (len_orig, len_rep) = (count(orig_tokens.len()), count(rep_tokens.len()));
    let lcs = lcs_len(&as_strs(&orig_tokens), &as_strs(&rep_tokens));
    let both = len_orig + len_rep;

    let mut bag = BTreeMap::<String, u64>::new();
    for t in &orig_tokens {
        *bag.entry(t.clone()).or_default() += 1;
    }
    let mut shared = 0;
    for t in &rep_tokens {
        if let Some(c) = bag.get_mut(t).filter(|c| **c > 0) {
            *c -= 1;
            shared += 1;
        }
    }

    let (orig_chars, rep_chars) = (chars(&orig), chars(&rep));
    let char_lcs = lcs_len(&orig_chars, &rep_chars);
    let char_both = count(orig_chars.len()) + count(rep_chars.len());

    TextScores {
        lcs_f1: ratio(2 * lcs, both),
        recall: ratio(lcs, len_orig),
        precision: ratio(lcs, len_rep),
        bag_f1: ratio(2 * shared, both),
        char_f1: ratio(2 * char_lcs, char_both),
        len_orig,
        len_rep,
        lcs,
    }
}

/// `num/den`, or `0/1` when `den` is zero.
fn ratio(num: u64, den: u64) -> Ratio {
    if den == 0 {
        Ratio { num: 0, den: 1 }
    } else {
        Ratio { num, den }
    }
}

fn count(n: usize) -> u64 {
    u64::try_from(n).expect("a length fits in u64")
}

fn as_strs(tokens: &[String]) -> Vec<&str> {
    tokens.iter().map(String::as_str).collect()
}

/// Each `char` of `s` as its own slice.
fn chars(s: &str) -> Vec<&str> {
    s.char_indices()
        .map(|(i, c)| &s[i..i + c.len_utf8()])
        .collect()
}

/// The LCS length: the `Equal` items of a shortest edit script. `RawMyers`,
/// because similar's default `Myers` may give up minimality for bounded work,
/// which would undercount; no deadline is set, so the result never depends on
/// time.
fn lcs_len(a: &[&str], b: &[&str]) -> u64 {
    let diff = TextDiff::configure()
        .algorithm(Algorithm::RawMyers)
        .diff_slices(a, b);
    diff.ops()
        .iter()
        .map(|op| match *op {
            DiffOp::Equal { len, .. } => count(len),
            _ => 0,
        })
        .sum()
}

/// A running mean of `lcs_f1` values in `u128` fixed point: each row adds its
/// value ×10⁹ (floored) to `sum_num` and 10⁹ to `sum_den`, so the mean is
/// `sum_num / sum_den`. It is exact when every value's denominator divides 10⁹
/// and otherwise off by under 10⁻⁹; reading it is integer division only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Mean {
    pub(crate) sum_num: u128,
    pub(crate) sum_den: u128,
    pub(crate) n: u64,
}

/// Mean `lcs_f1` per group key, iterated in key order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Aggregate {
    pub(crate) groups: BTreeMap<String, Mean>,
}

/// Groups `rows` by key and averages each group's `lcs_f1`.
pub(crate) fn aggregate<'a>(rows: impl Iterator<Item = (&'a str, &'a TextScores)>) -> Aggregate {
    let mut groups = BTreeMap::<String, Mean>::new();
    for (key, scores) in rows {
        let mean = groups.entry(key.to_owned()).or_default();
        let Ratio { num, den } = ratio(scores.lcs_f1.num, scores.lcs_f1.den);
        mean.sum_num += u128::from(num) * MEAN_SCALE / u128::from(den);
        mean.sum_den += MEAN_SCALE;
        mean.n += 1;
    }
    Aggregate { groups }
}

impl Mean {
    /// The mean as a reduced ratio; `0/1` for an empty group.
    pub(crate) fn ratio(&self) -> Ratio {
        if self.sum_den == 0 {
            return Ratio { num: 0, den: 1 };
        }
        let g = gcd(self.sum_num, self.sum_den);
        let (mut num, mut den) = (self.sum_num / g, self.sum_den / g);
        // Only past ~1.8e10 rows; halving both moves the value by under 2⁻⁶³.
        while u64::try_from(num.max(den)).is_err() {
            num >>= 1;
            den >>= 1;
        }
        Ratio {
            num: num as u64,
            den: den as u64,
        }
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

impl Ratio {
    /// The value in thousandths, `"0.750"`.
    pub(crate) fn display_permille(&self) -> String {
        match (self.num, self.den) {
            (0, _) => "0.000".to_owned(),
            (_, 0) => "inf".to_owned(),
            (num, den) => {
                let permille = u128::from(num) * 1000 / u128::from(den);
                format!("{}.{:03}", permille / 1000, permille % 1000)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};

    fn r(num: u64, den: u64) -> Ratio {
        Ratio { num, den }
    }

    const ONE: Ratio = Ratio { num: 1, den: 1 };
    const ZERO: Ratio = Ratio { num: 0, den: 1 };

    fn all_ratios(s: &TextScores) -> [Ratio; 5] {
        [s.lcs_f1, s.recall, s.precision, s.bag_f1, s.char_f1]
    }

    /// The exact pair, not just the value (`Ratio`'s `==` is by value).
    fn same(a: Ratio, b: Ratio) -> bool {
        (a.num, a.den) == (b.num, b.den)
    }

    #[test]
    fn identical_texts_score_one_everywhere() {
        let text = "The cat sat on the mat. The end.";
        let s = score(text, text, Lang::En);
        assert_eq!(all_ratios(&s), [ONE; 5]);
        assert_eq!(s.lcs, s.len_orig);
        assert_eq!(s.len_orig, 8);
        assert_eq!(s.len_rep, 8);
    }

    #[test]
    fn disjoint_texts_score_zero() {
        let s = score("alpha beta", "xyz", Lang::En);
        assert_eq!(all_ratios(&s), [ZERO; 5]);
        assert_eq!(s.lcs, 0);
    }

    #[test]
    fn swapped_paragraphs_keep_the_bag_but_not_the_order() {
        let p1 = "the first paragraph talks about cats";
        let p2 = "a second one is about dogs instead";
        let s = score(&format!("{p1}\n\n{p2}"), &format!("{p2}\n\n{p1}"), Lang::En);
        assert_eq!(s.bag_f1, ONE);
        assert!(s.lcs_f1 < ONE);
        assert!(s.lcs_f1 > ZERO);
    }

    #[test]
    fn a_duplicated_word_never_exceeds_one_and_lowers_precision() {
        let s = score("red green blue", "red red green blue", Lang::En);
        for v in all_ratios(&s) {
            assert!(v <= ONE, "{v:?}");
        }
        assert_eq!(s.recall, ONE);
        assert!(s.precision < ONE);
        assert!(same(s.precision, r(3, 4)));
    }

    #[test]
    fn zh_is_scored_per_character() {
        assert_eq!(tokens("你好 世界", Lang::Zh), ["你", "好", "世", "界"]);
        let s = score("你好世界", "你好世民", Lang::Zh);
        assert_eq!(s.len_orig, 4);
        assert_eq!(s.lcs, 3);
        assert_eq!(s.char_f1, r(3, 4));
        assert_eq!(s.lcs_f1, r(3, 4));
        // The same string as one whitespace token per side scores nothing.
        assert_eq!(score("你好世界", "你好世民", Lang::En).lcs_f1, ZERO);
    }

    #[test]
    fn composed_and_decomposed_forms_and_case_compare_equal() {
        assert_eq!(normalize("caf\u{e9}"), normalize("cafe\u{301}"));
        assert_eq!(normalize("\u{dc}ber"), normalize("\u{fc}ber"));
        assert_eq!(normalize("U\u{308}BER"), "\u{fc}ber");
        let s = score("Caf\u{e9} \u{dc}ber", "cafe\u{301} u\u{308}ber", Lang::Fr);
        assert_eq!(all_ratios(&s), [ONE; 5]);
    }

    #[test]
    fn whitespace_runs_collapse_and_trim() {
        assert_eq!(normalize("a\t b\n\nc"), "a b c");
        assert_eq!(normalize("  \u{a0}x\u{2003} y \r\n"), "x y");
        assert_eq!(normalize(""), "");
        assert_eq!(tokens("a  b\tc", Lang::En), ["a", "b", "c"]);
        assert!(tokens("   ", Lang::Ar).is_empty());
    }

    #[test]
    fn labels_parse_and_anything_else_is_unknown() {
        let labels = [
            ("en", Lang::En),
            ("fr", Lang::Fr),
            ("es", Lang::Es),
            ("ar", Lang::Ar),
            ("hi", Lang::Hi),
            ("zh", Lang::Zh),
            ("unknown", Lang::Unknown),
            ("", Lang::Unknown),
            ("de", Lang::Unknown),
        ];
        for (label, lang) in labels {
            assert_eq!(Lang::from_label(label), lang, "{label:?}");
        }
    }

    #[test]
    fn an_empty_original_gives_zero_without_panicking() {
        let s = score("", "some repaired text", Lang::En);
        assert_eq!(all_ratios(&s), [ZERO; 5]);
        assert!(same(s.recall, ZERO));
        let s = score("", "", Lang::Unknown);
        for v in all_ratios(&s) {
            assert!(same(v, ZERO), "{v:?}");
        }
        assert_eq!((s.len_orig, s.len_rep, s.lcs), (0, 0, 0));
        let s = score(" \n\t", "", Lang::Zh);
        assert!(same(s.char_f1, ZERO));
    }

    /// The textbook O(n·m) table, as an oracle for the diff-based LCS.
    fn lcs_table(a: &[String], b: &[String]) -> u64 {
        let mut prev = vec![0u64; b.len() + 1];
        for x in a {
            let mut cur = vec![0u64; b.len() + 1];
            for (j, y) in b.iter().enumerate() {
                cur[j + 1] = if x == y {
                    prev[j] + 1
                } else {
                    prev[j + 1].max(cur[j])
                };
            }
            prev = cur;
        }
        prev[b.len()]
    }

    #[test]
    fn lcs_is_the_longest_common_subsequence() {
        // A fixed LCG over a three-word alphabet: many near-misses, all repeatable.
        let mut state: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = move || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state >> 33
        };
        for _ in 0..200 {
            let mut text = || -> String {
                let len = next() % 160;
                (0..len)
                    .map(|_| ["x", "y", "z"][(next() % 3) as usize])
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let (a, b) = (text(), text());
            let s = score(&a, &b, Lang::En);
            let oracle = lcs_table(&tokens(&a, Lang::En), &tokens(&b, Lang::En));
            assert_eq!(s.lcs, oracle, "{a:?} vs {b:?}");
        }
    }

    fn scores_with_lcs_f1(num: u64, den: u64) -> TextScores {
        TextScores {
            lcs_f1: r(num, den),
            recall: ZERO,
            precision: ZERO,
            bag_f1: ZERO,
            char_f1: ZERO,
            len_orig: 0,
            len_rep: 0,
            lcs: 0,
        }
    }

    #[test]
    fn aggregate_gives_the_exact_mean_per_group_in_key_order() {
        let a = scores_with_lcs_f1(1, 2);
        let b = scores_with_lcs_f1(3, 4);
        let c = scores_with_lcs_f1(4, 4);
        let d = scores_with_lcs_f1(2, 5);
        let rows = [
            ("saveas", &a),
            ("print", &d),
            ("saveas", &b),
            ("saveas", &c),
        ];
        let agg = aggregate(rows.into_iter());
        let keys: Vec<&str> = agg.groups.keys().map(String::as_str).collect();
        assert_eq!(keys, ["print", "saveas"]);
        // (1/2 + 3/4 + 1) / 3 = 3/4, exactly.
        let saveas = agg.groups["saveas"];
        assert_eq!(saveas.n, 3);
        assert!(same(saveas.ratio(), r(3, 4)));
        assert_eq!(saveas.ratio().display_permille(), "0.750");
        let print = agg.groups["print"];
        assert_eq!(print.n, 1);
        assert!(same(print.ratio(), r(2, 5)));
    }

    #[test]
    fn aggregate_floors_each_value_to_a_billionth() {
        let third = scores_with_lcs_f1(1, 3);
        let zero_den = scores_with_lcs_f1(0, 1);
        let agg = aggregate([("g", &third), ("g", &zero_den)].into_iter());
        let g = agg.groups["g"];
        assert_eq!((g.sum_num, g.sum_den, g.n), (333_333_333, 2_000_000_000, 2));
        assert_eq!(g.ratio().display_permille(), "0.166");
        assert!(aggregate(std::iter::empty()).groups.is_empty());
        assert!(same(Mean::default().ratio(), ZERO));
    }

    #[test]
    fn display_permille_floors_to_thousandths() {
        assert_eq!(r(3, 4).display_permille(), "0.750");
        assert_eq!(r(1, 1).display_permille(), "1.000");
        assert_eq!(r(2, 3).display_permille(), "0.666");
        assert_eq!(r(0, 1).display_permille(), "0.000");
        assert_eq!(r(0, 0).display_permille(), "0.000");
        assert_eq!(r(1, 0).display_permille(), "inf");
        assert_eq!(
            r(u64::MAX, 1).display_permille(),
            format!("{}.000", u64::MAX)
        );
    }

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Golden {
        about: String,
        cases: Vec<GoldenCase>,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct GoldenCase {
        name: String,
        lang: String,
        orig: String,
        rep: String,
        len_orig: u64,
        len_rep: u64,
        lcs: u64,
        lcs_f1: Ratio,
        recall: Ratio,
        precision: Ratio,
        bag_f1: Ratio,
        char_f1: Ratio,
    }

    const GOLDEN: &str = include_str!("../../tests/data/metrics_golden.json");

    /// sha256 of the golden's re-serialised form (compact `serde_json`), which
    /// does not depend on the checkout's line endings. Changing it is a
    /// deliberate re-baseline.
    const GOLDEN_SHA256: &str = "94e5cf8dc31551b625bfe5c67f4058ca479821b12a712fd26fb1ea7b56480257";

    #[test]
    fn golden_table_matches_exactly() {
        let golden: Golden = serde_json::from_str(GOLDEN).expect("golden parses");
        assert_eq!(golden.cases.len(), 10);
        for case in &golden.cases {
            let s = score(&case.orig, &case.rep, Lang::from_label(&case.lang));
            let got = [
                ("lcs_f1", s.lcs_f1, case.lcs_f1),
                ("recall", s.recall, case.recall),
                ("precision", s.precision, case.precision),
                ("bag_f1", s.bag_f1, case.bag_f1),
                ("char_f1", s.char_f1, case.char_f1),
            ];
            for (column, actual, expected) in got {
                assert!(
                    same(actual, expected),
                    "{} {column}: got {actual:?}, golden {expected:?}",
                    case.name
                );
            }
            assert_eq!(
                (s.len_orig, s.len_rep, s.lcs),
                (case.len_orig, case.len_rep, case.lcs),
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn golden_table_hash_is_pinned() {
        let golden: Golden = serde_json::from_str(GOLDEN).expect("golden parses");
        let bytes = serde_json::to_vec(&golden).expect("golden serialises");
        let hex: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(hex, GOLDEN_SHA256);
    }
}
