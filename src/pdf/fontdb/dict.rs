//! Word lists the inference scorer matches decoded text against (T-28, TD
//! §18.2, FR-05).
//!
//! [`bundled`] holds the general lists the C8 pass scores with (G-09,
//! D-011): `assets/dicts/{en,fr,es}.txt`, the 50,000 most frequent words of
//! the Leipzig Corpora Collection's 1M-sentence news corpora (CC BY 4.0;
//! `THIRD_PARTY_NOTICES.md`), cut by `tools/cut-wordlist.py` and pinned by
//! `fetch-assets.sh`. [`FrequencyList`] reads the FR-05 format those files
//! are in, and [`FrequencyList::from_text`] turns a surviving `/ToUnicode`'s
//! true text into the per-document dictionary (TD §5.2). A database with no
//! lists scores with [`EmptyDictionary`].
//!
//! Every word is stored the way the scorer looks it up: [`normalize`]d (NFC,
//! lowercase) and stripped of leading and trailing characters that are not
//! letters or digits ([`word_core`]).

// T-30 (the C7/C8 passes) is the first caller outside the tests.
#![cfg_attr(not(test), allow(dead_code))]

use std::sync::OnceLock;

use crate::bench::metrics::{Lang, normalize};

/// The bundled lists' files, in the order [`bundled`] returns them.
const BUNDLED: [(Lang, &[u8]); 3] = [
    (Lang::En, include_bytes!("../../../assets/dicts/en.txt")),
    (Lang::Fr, include_bytes!("../../../assets/dicts/fr.txt")),
    (Lang::Es, include_bytes!("../../../assets/dicts/es.txt")),
];

/// The lines of each bundled file (the cut's size).
const BUNDLED_WORDS: usize = 50_000;

/// The bundled English, French and Spanish lists, in that order, parsed once
/// per process.
pub(crate) fn bundled() -> &'static [FrequencyList] {
    static LISTS: OnceLock<Vec<FrequencyList>> = OnceLock::new();
    LISTS.get_or_init(|| {
        BUNDLED
            .iter()
            .map(|&(lang, bytes)| {
                // The tests check the committed files parse.
                FrequencyList::from_bytes(lang, bytes).expect("the bundled word lists parse")
            })
            .collect()
    })
}

/// A word list for one language.
pub(crate) trait Dictionary {
    fn lang(&self) -> Lang;
    /// Whether `w`, a normalised word, is in the list.
    fn contains(&self, w: &str) -> bool;
    /// The length in `char`s of the longest proper prefix of `w` that is a
    /// word of the list, 0 when none is.
    fn longest_prefix(&self, w: &str) -> usize;
    /// `zh`: the log-probability of `b` following `a`, in thousandths of a
    /// nat (≤ 0), when the list knows the pair.
    fn bigram_millinats(&self, a: char, b: char) -> Option<i32>;
}

/// No words: never contains, prefix 0, no bigrams. What a database with no
/// word lists scores with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct EmptyDictionary;

impl Dictionary for EmptyDictionary {
    fn lang(&self) -> Lang {
        Lang::Unknown
    }

    fn contains(&self, _w: &str) -> bool {
        false
    }

    fn longest_prefix(&self, _w: &str) -> usize {
        0
    }

    fn bigram_millinats(&self, _a: char, _b: char) -> Option<i32> {
        None
    }
}

/// Why bytes are not an FR-05 word list. Lines count from 1.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum DictError {
    #[error("word list line {0} is not UTF-8")]
    NotUtf8(usize),
    #[error("word list line {0} is not `word count`")]
    Malformed(usize),
}

/// A sorted word list searched by binary search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FrequencyList {
    lang: Lang,
    words: Vec<String>,
}

impl FrequencyList {
    /// Parses FR-05's format: UTF-8, one `word count` line per entry (one
    /// ASCII space, a decimal count), LF line endings, lowercase and sorted by
    /// count descending. A trailing CR, a final newline and empty lines are
    /// tolerated; anything else is [`DictError`]. The counts and their order
    /// are not kept: only the words, normalised and sorted.
    pub(crate) fn from_bytes(lang: Lang, bytes: &[u8]) -> Result<FrequencyList, DictError> {
        let mut words = Vec::new();
        for (i, line) in bytes.split(|&b| b == b'\n').enumerate() {
            let n = i + 1;
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.is_empty() {
                continue;
            }
            let line = std::str::from_utf8(line).map_err(|_| DictError::NotUtf8(n))?;
            let (word, count) = line.split_once(' ').ok_or(DictError::Malformed(n))?;
            let count_ok = !count.is_empty() && count.bytes().all(|b| b.is_ascii_digit());
            if word.is_empty() || word.chars().any(char::is_whitespace) || !count_ok {
                return Err(DictError::Malformed(n));
            }
            words.push(word);
        }
        Ok(FrequencyList::from_words(lang, words))
    }

    /// The per-document dictionary of a surviving `/ToUnicode`: every
    /// whitespace-separated word of `text`, normalised as the scorer
    /// normalises decoded text.
    pub(crate) fn from_text(lang: Lang, text: &str) -> FrequencyList {
        let text = normalize(text);
        FrequencyList::from_words(lang, text.split(' '))
    }

    fn from_words<'w>(lang: Lang, words: impl IntoIterator<Item = &'w str>) -> FrequencyList {
        let mut words: Vec<String> = words
            .into_iter()
            .map(|w| word_core(&normalize(w)).to_owned())
            .filter(|w| !w.is_empty())
            .collect();
        words.sort();
        words.dedup();
        FrequencyList { lang, words }
    }

    pub(crate) fn len(&self) -> usize {
        self.words.len()
    }
}

impl Dictionary for FrequencyList {
    fn lang(&self) -> Lang {
        self.lang
    }

    fn contains(&self, w: &str) -> bool {
        self.words.binary_search_by(|x| x.as_str().cmp(w)).is_ok()
    }

    fn longest_prefix(&self, w: &str) -> usize {
        // Proper prefixes, longest first; each is one binary search.
        let ends: Vec<usize> = w.char_indices().map(|(at, _)| at).skip(1).collect();
        ends.iter()
            .rev()
            .find(|&&end| self.contains(&w[..end]))
            .map_or(0, |&end| w[..end].chars().count())
    }

    fn bigram_millinats(&self, _a: char, _b: char) -> Option<i32> {
        None
    }
}

/// `w` without its leading and trailing characters that are not letters or
/// digits: `«cœur»,` → `cœur`, `l’été.` → `l’été`.
pub(crate) fn word_core(w: &str) -> &str {
    w.trim_matches(|c: char| !c.is_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FR-05's documented format, with its observed quirks: apostrophe forms,
    /// digits, a single letter, one uppercase token.
    const FR05_SAMPLE: &[u8] = b"you 22484400\nI 19975318\nthe 17594291\nt' 130\n2nd 313\na 12\n";

    #[test]
    fn parses_the_fr05_format() {
        let list = FrequencyList::from_bytes(Lang::En, FR05_SAMPLE).unwrap();
        assert_eq!(list.lang(), Lang::En);
        assert_eq!(list.len(), 6);
        for w in ["you", "i", "the", "t", "2nd", "a"] {
            assert!(list.contains(w), "{w}");
        }
        assert!(!list.contains("yo"));
        assert!(!list.contains("I"), "lookups are of normalised words");
    }

    #[test]
    fn tolerates_crlf_and_blank_lines() {
        let list =
            FrequencyList::from_bytes(Lang::Fr, b"\xc3\xa9t\xc3\xa9 4\r\n\nmang\xc3\xa9 3\n\n")
                .unwrap();
        assert!(list.contains("été"));
        assert!(list.contains("mangé"));
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn rejects_malformed_lines() {
        for (bytes, err) in [
            (&b"you 1\nthe\n"[..], DictError::Malformed(2)),
            (b"you one\n", DictError::Malformed(1)),
            (b"you 1 2\n", DictError::Malformed(1)),
            (b" 12\n", DictError::Malformed(1)),
            (b"you \n", DictError::Malformed(1)),
            (b"you -1\n", DictError::Malformed(1)),
            (b"you\t1\n", DictError::Malformed(1)),
            (b"ok 1\n\xff\xfe 2\n", DictError::NotUtf8(2)),
        ] {
            assert_eq!(FrequencyList::from_bytes(Lang::En, bytes), Err(err));
        }
    }

    #[test]
    fn longest_prefix_is_the_longest_listed_proper_prefix() {
        let list = FrequencyList::from_bytes(Lang::En, b"walk 3\nwal 2\nw 1\n").unwrap();
        assert_eq!(list.longest_prefix("walked"), 4);
        assert_eq!(list.longest_prefix("walk"), 3, "a proper prefix only");
        assert_eq!(list.longest_prefix("wx"), 1);
        assert_eq!(list.longest_prefix("xyz"), 0);
        assert_eq!(list.longest_prefix(""), 0);
        let fr = FrequencyList::from_text(Lang::Fr, "mangé");
        assert_eq!(fr.longest_prefix("mangées"), 5, "counted in chars");
    }

    #[test]
    fn from_text_keeps_the_true_words() {
        let list = FrequencyList::from_text(Lang::Fr, "Le garçon a mangé la crème « au cœur ».");
        for w in ["le", "garçon", "a", "mangé", "la", "crème", "au", "cœur"] {
            assert!(list.contains(w), "{w}");
        }
        assert_eq!(list.len(), 8, "the guillemets are no words");
        assert_eq!(list.bigram_millinats('a', 'b'), None);
    }

    #[test]
    fn empty_dictionary_knows_nothing() {
        let d = EmptyDictionary;
        assert_eq!(d.lang(), Lang::Unknown);
        assert!(!d.contains("the"));
        assert_eq!(d.longest_prefix("the"), 0);
        assert_eq!(d.bigram_millinats('的', '一'), None);
    }

    #[test]
    fn the_bundled_lists_are_english_french_and_spanish() {
        let lists = bundled();
        assert!(std::ptr::eq(lists, bundled()), "parsed once");
        let langs: Vec<Lang> = lists.iter().map(Dictionary::lang).collect();
        assert_eq!(langs, [Lang::En, Lang::Fr, Lang::Es]);
        for ((lang, bytes), list) in BUNDLED.iter().zip(lists) {
            let text = std::str::from_utf8(bytes).unwrap();
            let words: Vec<&str> = text.lines().map(|l| l.split_once(' ').unwrap().0).collect();
            assert_eq!(words.len(), BUNDLED_WORDS, "{lang:?}");
            for w in &words {
                assert_eq!(normalize(w), *w, "{lang:?}: {w} is not lowercase NFC");
            }
            // Edge apostrophes are stripped on load (`l'` is `l`), so a few
            // words merge.
            assert!(
                list.len() <= BUNDLED_WORDS && list.len() > 49_000,
                "{}",
                list.len()
            );
        }
        let [en, fr, es] = lists else { panic!() };
        for w in ["the", "quick", "brown", "jumps", "lazy", "golden", "page"] {
            assert!(en.contains(w), "en: {w}");
        }
        for w in ["le", "garçon", "mangé", "crème", "cœur", "été", "c’est"] {
            assert!(fr.contains(w), "fr: {w}");
        }
        for w in ["el", "niño", "año", "también"] {
            assert!(es.contains(w), "es: {w}");
        }
    }

    /// `fetch-assets.sh` re-downloads and re-cuts the lists and checks them
    /// against these hashes; the committed files must be the pinned ones.
    #[test]
    fn fetch_assets_pins_the_bundled_lists() {
        use sha2::{Digest, Sha256};
        const SCRIPT: &str = include_str!("../../../fetch-assets.sh");
        for (lang, bytes) in BUNDLED {
            let hex: String = Sha256::digest(bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            assert!(SCRIPT.contains(&hex), "{lang:?}: {hex} is not pinned");
        }
    }

    #[test]
    fn word_core_strips_edge_punctuation_only() {
        assert_eq!(word_core("«cœur»,"), "cœur");
        assert_eq!(word_core("l’été."), "l’été");
        assert_eq!(word_core("«"), "");
        assert_eq!(word_core("2nd"), "2nd");
    }
}
