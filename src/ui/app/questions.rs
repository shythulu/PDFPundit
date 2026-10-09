//! Parked font questions (F-01): which of a job's questions are already
//! answered, and what the font pick shows for each kind of question.
//!
//! A job asks one question at a time and blocks on it while the batch goes
//! on (GG §1), so a file with several lost fonts asks them one after another.
//! What the user answered is kept per job (one file) for the job's life:
//! - "use best for both" (`b`) and "apply best to all" (`a`) answer every
//!   later question of the file `UseBest`;
//! - any other answer holds for the rest of the font's family in that file.
//!   A family is the `/BaseFont` without its subset tag and style suffix
//!   ([`family`]), so `NotoSans-Regular` and `NotoSans-Bold` ask once. A
//!   pick is carried only to a font the later question lists: the same
//!   font, else its first candidate of the picked font's family. When it
//!   lists neither, the question is asked: the engine grades any pick,
//!   carried or not, as a confirmed font, so carrying a font the scorer did
//!   not rank for the slot would grade an unseen guess as fixed.
//!
//! A question's family comes from the analysis's `/BaseFont` for its slot,
//! never from the request's own name (an unreproducible font without a
//! `/BaseFont` is named after its slot, and slot names such as `F1` repeat
//! across pages). A slot whose base font is not known, or is a `CIDFont+Fn`
//! name, has no family and is asked on its own. The two kinds of question
//! are remembered apart: `Skip` means "keep the best guess" to a font pick
//! and "leave the font as found" to an unreproducible font.
//!
//! An answer given this way reaches the engine as `Batched`, never as the
//! user's (D-141), so the run's interaction record tells it from one the
//! user gave; the app log names each one and the answer it came from.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use crate::engine::{FontCandidate, FontPickRequest, InteractionReply, InteractionRequest, Ratio};
use crate::jobs::{JobId, QueueEntry};

/// What the user has answered, per job.
#[derive(Debug, Default)]
pub(super) struct Answers {
    /// Jobs whose every later question is `UseBest`.
    best: BTreeSet<JobId>,
    /// Each job's answers, by family key.
    given: BTreeMap<JobId, Vec<Given>>,
}

#[derive(Debug)]
struct Given {
    key: String,
    reply: InteractionReply,
    /// A pick's candidate family, to carry the pick to another face.
    picked_family: Option<String>,
    /// The `(page, slot)` the answer was given for, for the log.
    from: (u32, String),
}

/// Where an answer the app gives on its own came from, for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Origin {
    /// "Use best for both" or "apply best to all" for the file.
    BestForFile,
    /// The answer given for the same family: the family and the
    /// `(page, slot)` it was given for.
    Family {
        family: String,
        page: u32,
        slot: String,
    },
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Origin::BestForFile => f.write_str("the best guess the user chose for the whole file"),
            Origin::Family { family, page, slot } => write!(
                f,
                "the answer for slot {slot} on p.{} (family {family})",
                u64::from(*page) + 1
            ),
        }
    }
}

impl Answers {
    /// The answer `request`, asked by job `job` about `entry`, already has,
    /// and where it came from.
    pub(super) fn known(
        &self,
        job: JobId,
        entry: &QueueEntry,
        request: &InteractionRequest,
    ) -> Option<(InteractionReply, Origin)> {
        if self.best.contains(&job) {
            return Some((InteractionReply::UseBest, Origin::BestForFile));
        }
        let key = key_of(entry, request)?;
        let given = self.given.get(&job)?.iter().find(|g| g.key == key)?;
        let (page, slot) = given.from.clone();
        // The key is the kind, a space, then the family.
        let family = key.split_once(' ').map_or(key.as_str(), |(_, f)| f);
        let family = family.to_owned();
        let origin = Origin::Family { family, page, slot };
        Some((carry(given, request)?, origin))
    }

    /// The user answered `request` with `reply`; with `whole_file`, every
    /// later question of the file is `UseBest` too.
    pub(super) fn record(
        &mut self,
        job: JobId,
        entry: &QueueEntry,
        request: &InteractionRequest,
        reply: &InteractionReply,
        whole_file: bool,
    ) {
        if whole_file {
            self.best.insert(job);
            return;
        }
        let Some(key) = key_of(entry, request) else {
            return;
        };
        let picked_family = match (request, reply) {
            (InteractionRequest::FontPick(r), InteractionReply::Pick(id)) => r
                .candidates
                .iter()
                .find(|c| &c.font_id == id)
                .map(|c| c.family.clone()),
            _ => None,
        };
        self.given.entry(job).or_default().push(Given {
            key,
            reply: reply.clone(),
            picked_family,
            from: asked_at(request),
        });
    }

    /// Job `job` has ended: its answers go.
    pub(super) fn forget(&mut self, job: JobId) {
        self.best.remove(&job);
        self.given.remove(&job);
    }
}

/// The family key `request` is remembered under, with its kind.
fn key_of(entry: &QueueEntry, request: &InteractionRequest) -> Option<String> {
    match request {
        InteractionRequest::FontPick(r) => {
            let slots = &entry.font_resolutions;
            let slot = slots
                .iter()
                .find(|s| s.slot == r.slot && s.page == r.page)
                .or_else(|| slots.iter().find(|s| s.slot == r.slot))?;
            family(slot.base_font.as_deref()?).map(|f| format!("pick {f}"))
        }
        // `r.family` is the slot's name when the font has no `/BaseFont`,
        // so the key comes from the analysis's base font for its slots.
        InteractionRequest::FontUnreproducible(r) => r
            .slots
            .iter()
            .find_map(|(page, name)| {
                let slots = &entry.font_resolutions;
                slots
                    .iter()
                    .find(|s| &s.slot == name && s.page == *page)?
                    .base_font
                    .as_deref()
            })
            .and_then(family)
            .map(|f| format!("unreproducible {f}")),
    }
}

/// The `(page, slot)` `request` is asked about.
pub(super) fn asked_at(request: &InteractionRequest) -> (u32, String) {
    match request {
        InteractionRequest::FontPick(r) => (r.page, r.slot.clone()),
        InteractionRequest::FontUnreproducible(r) => r
            .slots
            .first()
            .cloned()
            .unwrap_or_else(|| (0, r.family.clone())),
    }
}

/// A remembered answer, made fit for `request`, or `None` when it does not
/// fit: a pick whose font the later question does not list, nor any font
/// of the picked font's family.
fn carry(given: &Given, request: &InteractionRequest) -> Option<InteractionReply> {
    let (InteractionRequest::FontPick(r), InteractionReply::Pick(id)) = (request, &given.reply)
    else {
        return Some(given.reply.clone());
    };
    if r.candidates.iter().any(|c| &c.font_id == id) {
        return Some(given.reply.clone());
    }
    let family = given.picked_family.as_ref()?;
    let c = r.candidates.iter().find(|c| &c.family == family)?;
    Some(InteractionReply::Pick(c.font_id.clone()))
}

/// Style words a PostScript name's suffix is made of (`Bold`, `Italic`,
/// `BoldItalic`, `SemiboldIt`, …), longest first so `italic` wins over `it`.
const STYLE_WORDS: [&str; 18] = [
    "regular", "oblique", "italic", "medium", "normal", "black", "heavy", "light", "roman",
    "extra", "ultra", "plain", "bold", "book", "demi", "semi", "thin", "it",
];

/// Whether `tail` (lower case) is made of style words only.
fn is_style(tail: &str) -> bool {
    let mut rest = tail;
    while !rest.is_empty() {
        match STYLE_WORDS.iter().find(|w| rest.starts_with(*w)) {
            Some(w) => rest = &rest[w.len()..],
            None => return false,
        }
    }
    !tail.is_empty()
}

/// The family a `/BaseFont` name belongs to, as a key: the leading `/` and
/// a six-letter subset tag dropped, then a style suffix after the last `-`
/// or `,` (`-Regular`, `-Bold`, `-Italic`, `-BoldItalic`, `,Bold`,
/// `-BoldMT`, …), then a trailing `MT`; lower case, without spaces.
/// `CIDFont+F1` and an empty name have no family.
pub(super) fn family(base_font: &str) -> Option<String> {
    let name = base_font.strip_prefix('/').unwrap_or(base_font);
    let name = match name.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()) => rest,
        _ => name,
    };
    if name.is_empty() || name.starts_with("CIDFont+") {
        return None;
    }
    let base = match name.rfind(['-', ',']) {
        Some(at) if at > 0 => {
            let tail = name[at + 1..].to_lowercase();
            let tail = tail.strip_suffix("mt").unwrap_or(&tail);
            if is_style(tail) { &name[..at] } else { name }
        }
        _ => name,
    };
    let base = match base.strip_suffix("MT") {
        Some(b) if !b.is_empty() => b,
        _ => base,
    };
    let key: String = base
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    (!key.is_empty()).then_some(key)
}

/// What the font pick draws for `request`. An unreproducible font's options
/// stand in as candidates: no score, no preview, the first slot as the slot.
pub(super) fn pick_view(request: &InteractionRequest) -> Cow<'_, FontPickRequest> {
    match request {
        InteractionRequest::FontPick(r) => Cow::Borrowed(r),
        InteractionRequest::FontUnreproducible(r) => {
            let zero = Ratio { num: 0, den: 1 };
            let (page, slot) = r
                .slots
                .first()
                .cloned()
                .unwrap_or_else(|| (0, r.family.clone()));
            Cow::Owned(FontPickRequest {
                id: r.id,
                page,
                slot,
                sample_codes: Vec::new(),
                candidates: r
                    .options
                    .iter()
                    .map(|o| FontCandidate {
                        font_id: o.font_id.clone(),
                        family: o.label.clone(),
                        language: String::new(),
                        score: zero,
                        confidence: zero,
                        preview: String::new(),
                    })
                    .collect(),
                preview: String::new(),
            })
        }
    }
}

/// The font pick's answer as `request` takes it: a pick among an
/// unreproducible font's options is that option's `Substitute`.
pub(super) fn reply_for(request: &InteractionRequest, reply: InteractionReply) -> InteractionReply {
    match (request, &reply) {
        (InteractionRequest::FontUnreproducible(r), InteractionReply::Pick(id)) => r
            .options
            .iter()
            .find(|o| &o.font_id == id)
            .map_or(reply.clone(), |o| InteractionReply::Substitute(o.clone())),
        _ => reply,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{
        FontSlot, InteractionRequestId, SubstituteChoice, ToUnicodeState, UnreproducibleRequest,
    };

    #[test]
    fn style_suffixes_and_subset_tags_make_one_family() {
        let same = [
            "/NotoSans-Regular",
            "/ABCDEF+NotoSans-Bold",
            "NotoSans-Italic",
            "NotoSans-BoldItalic",
            "NotoSans-SemiboldIt",
            "NotoSans,Bold",
            "Noto Sans-Oblique",
            "NotoSans",
        ];
        for name in same {
            assert_eq!(family(name).as_deref(), Some("notosans"), "{name}");
        }
        assert_eq!(family("/ArialMT").as_deref(), Some("arial"));
        assert_eq!(family("/Arial-BoldMT").as_deref(), Some("arial"));
        assert_eq!(
            family("TimesNewRomanPSMT"),
            family("TimesNewRomanPS-BoldItalicMT")
        );
        // A suffix that is not a style is part of the family.
        assert_eq!(
            family("NotoSans-Condensed").as_deref(),
            Some("notosans-condensed")
        );
        assert_eq!(family("Foo-Bar").as_deref(), Some("foo-bar"));
        assert_ne!(family("NotoSans-Bold"), family("NotoSerif-Bold"));
        // No family: a Print file's slot name, or nothing.
        assert_eq!(family("/CIDFont+F1"), None);
        assert_eq!(family("/"), None);
        assert_eq!(family(""), None);
        // A tag must be six capitals.
        assert_eq!(family("abcdef+Foo").as_deref(), Some("abcdef+foo"));
    }

    fn slot(page: u32, name: &str, base: Option<&str>) -> FontSlot {
        FontSlot {
            page,
            slot: name.into(),
            base_font: base.map(Into::into),
            subtype: Some("/Type0".into()),
            embedded: false,
            tounicode: ToUnicodeState::Missing,
            glyph_count: 10,
            resolution: None,
        }
    }

    fn candidate(id: &str, family: &str) -> FontCandidate {
        FontCandidate {
            font_id: id.into(),
            family: family.into(),
            language: "en".into(),
            score: Ratio { num: 1, den: 2 },
            confidence: Ratio { num: 1, den: 4 },
            preview: String::new(),
        }
    }

    fn pick(slot: &str, candidates: Vec<FontCandidate>) -> InteractionRequest {
        InteractionRequest::FontPick(FontPickRequest {
            id: InteractionRequestId(0),
            page: 0,
            slot: slot.into(),
            sample_codes: Vec::new(),
            candidates,
            preview: String::new(),
        })
    }

    /// The reply `known` gives, without its origin.
    fn reply_of(
        a: &Answers,
        job: JobId,
        e: &QueueEntry,
        q: &InteractionRequest,
    ) -> Option<InteractionReply> {
        a.known(job, e, q).map(|(reply, _)| reply)
    }

    fn entry() -> QueueEntry {
        let mut e = QueueEntry::queued(JobId(1), "a.pdf".into(), None, 1);
        e.font_resolutions = vec![
            slot(0, "F1", Some("/ABCDEF+NotoSans-Regular")),
            slot(0, "F2", Some("/NotoSans-Bold")),
            slot(0, "F3", Some("/CIDFont+F1")),
            slot(0, "F4", Some("/CIDFont+F2")),
            slot(0, "F5", Some("/NotoSerif-Bold")),
            slot(3, "F7", Some("/Garamond-Regular")),
            slot(4, "F7", Some("/ABCDEF+Garamond-Italic")),
            slot(5, "F1", None),
            slot(6, "F1", None),
        ];
        e
    }

    #[test]
    fn an_answer_holds_for_the_rest_of_the_family_in_that_file() {
        let e = entry();
        let mut a = Answers::default();
        let one = JobId(1);
        let regular = pick("F1", vec![candidate("noto-sans-regular", "Noto Sans")]);
        let bold = pick(
            "F2",
            vec![
                candidate("noto-serif-bold", "Noto Serif"),
                candidate("noto-sans-bold", "Noto Sans"),
            ],
        );
        assert_eq!(reply_of(&a, one, &e, &regular), None);
        a.record(
            one,
            &e,
            &regular,
            &InteractionReply::Pick("noto-sans-regular".into()),
            false,
        );
        // The bold face takes the pick, carried to its own Noto Sans face.
        assert_eq!(
            reply_of(&a, one, &e, &bold),
            Some(InteractionReply::Pick("noto-sans-bold".into()))
        );
        // Another family, another file, or a nameless slot still asks.
        assert_eq!(reply_of(&a, one, &e, &pick("F5", Vec::new())), None);
        assert_eq!(reply_of(&a, JobId(2), &e, &bold), None);
        assert_eq!(reply_of(&a, one, &e, &pick("F9", Vec::new())), None);
        a.record(
            one,
            &e,
            &pick("F3", Vec::new()),
            &InteractionReply::Skip,
            false,
        );
        assert_eq!(reply_of(&a, one, &e, &pick("F4", Vec::new())), None);
        // A skip is carried as it is.
        a.record(
            one,
            &e,
            &pick("F5", Vec::new()),
            &InteractionReply::Skip,
            false,
        );
        let serif = pick("F5", vec![candidate("x", "X")]);
        assert_eq!(reply_of(&a, one, &e, &serif), Some(InteractionReply::Skip));
        a.forget(one);
        assert_eq!(
            reply_of(&a, one, &e, &bold),
            None,
            "forgotten when the job ends"
        );
    }

    #[test]
    fn a_pick_is_carried_only_to_a_font_the_later_question_lists() {
        let e = entry();
        let mut a = Answers::default();
        let one = JobId(1);
        let regular = pick("F1", vec![candidate("noto-sans", "Noto Sans")]);
        a.record(
            one,
            &e,
            &regular,
            &InteractionReply::Pick("noto-sans".into()),
            false,
        );
        let listed = pick(
            "F2",
            vec![
                candidate("noto-sans-bold", "Noto Sans"),
                candidate("noto-sans", "Noto Sans"),
            ],
        );
        assert_eq!(
            reply_of(&a, one, &e, &listed),
            Some(InteractionReply::Pick("noto-sans".into()))
        );
        // Neither the font nor its family is listed: the scorer did not
        // rank the pick for this slot, so the user is asked.
        let unlisted = pick(
            "F2",
            vec![
                candidate("noto-serif-bold", "Noto Serif"),
                candidate("liberation-sans-bold", "Liberation Sans"),
            ],
        );
        assert_eq!(reply_of(&a, one, &e, &unlisted), None);
        // The origin names the slot the answer was given for.
        assert_eq!(
            a.known(one, &e, &listed).map(|(_, o)| o),
            Some(Origin::Family {
                family: "notosans".into(),
                page: 0,
                slot: "F1".into(),
            })
        );
    }

    fn unreproducible(family: &str, page: u32, slot: &str) -> InteractionRequest {
        InteractionRequest::FontUnreproducible(UnreproducibleRequest {
            id: InteractionRequestId(0),
            family: family.into(),
            slots: vec![(page, slot.into())],
            reason: "no font covers it".into(),
            options: vec![SubstituteChoice {
                font_id: "noto-sans".into(),
                label: "Noto Sans".into(),
            }],
        })
    }

    #[test]
    fn best_for_the_file_answers_every_later_question() {
        let e = entry();
        let mut a = Answers::default();
        a.record(
            JobId(1),
            &e,
            &pick("F3", Vec::new()),
            &InteractionReply::UseBest,
            true,
        );
        for q in [pick("F4", Vec::new()), unreproducible("Whatever", 9, "F9")] {
            assert_eq!(
                a.known(JobId(1), &e, &q),
                Some((InteractionReply::UseBest, Origin::BestForFile))
            );
        }
        assert_eq!(reply_of(&a, JobId(2), &e, &pick("F4", Vec::new())), None);
    }

    #[test]
    fn the_two_kinds_are_remembered_apart() {
        let e = entry();
        let mut a = Answers::default();
        a.record(
            JobId(1),
            &e,
            &unreproducible("Garamond-Regular", 3, "F7"),
            &InteractionReply::Skip,
            false,
        );
        assert_eq!(
            reply_of(
                &a,
                JobId(1),
                &e,
                &unreproducible("Garamond-Italic", 4, "F7")
            ),
            Some(InteractionReply::Skip)
        );
        let garamond = pick("F7", Vec::new());
        assert_eq!(reply_of(&a, JobId(1), &e, &garamond), None);
    }

    /// A font without a `/BaseFont` is named after its slot; two such fonts
    /// in slots called `F1` on different pages are asked about apart.
    #[test]
    fn nameless_unreproducible_fonts_are_asked_on_their_own() {
        let e = entry();
        let mut a = Answers::default();
        let first = unreproducible("F1", 5, "F1");
        let second = unreproducible("F1", 6, "F1");
        assert_eq!(reply_of(&a, JobId(1), &e, &first), None);
        a.record(JobId(1), &e, &first, &InteractionReply::Skip, false);
        assert_eq!(reply_of(&a, JobId(1), &e, &second), None);
        // Nor does the request's own name stand in for a slot the analysis
        // does not list.
        let unlisted = unreproducible("Garamond-Regular", 8, "F8");
        assert_eq!(reply_of(&a, JobId(1), &e, &unlisted), None);
    }

    #[test]
    fn an_unreproducible_font_is_shown_as_its_options() {
        let q = unreproducible("Garamond", 3, "F7");
        let view = pick_view(&q);
        assert_eq!((view.page, view.slot.as_str()), (3, "F7"));
        assert_eq!(view.candidates.len(), 1);
        assert_eq!(view.candidates[0].family, "Noto Sans");
        assert_eq!(
            reply_for(&q, InteractionReply::Pick("noto-sans".into())),
            InteractionReply::Substitute(SubstituteChoice {
                font_id: "noto-sans".into(),
                label: "Noto Sans".into(),
            })
        );
        assert_eq!(
            reply_for(&q, InteractionReply::Skip),
            InteractionReply::Skip
        );
        let p = pick("F1", Vec::new());
        assert!(matches!(pick_view(&p), Cow::Borrowed(_)));
        assert_eq!(
            reply_for(&p, InteractionReply::Pick("x".into())),
            InteractionReply::Pick("x".into())
        );
    }
}
