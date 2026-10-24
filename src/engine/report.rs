//! `RepairReport`: what a repair did, field by field (T-02b fixes the shape;
//! T-14 fills the values).
//!
//! The report is a pure function of the input bytes, the engine version, the
//! settings snapshot and the interactions (D-073). It carries no machine path,
//! no placement tier and no "state reused or rebuilt" field: those are run facts
//! and live in the run record (T-16), so two examiners' reports for the same
//! input, version, settings and replies are byte-identical.

use serde::{Deserialize, Serialize};

use super::{
    AnalysisResult, AnalyzeStats, Answer, FontDb, FontSourcePolicy, InteractionReply, PageSize,
    RepairOptions, Toolpath, UnreproduciblePolicy,
};
use crate::pdf::model::{
    ByteSpan, CorruptionClass, Finding, InteractionKind, ObjId, Ratio, SalvageGrade,
};
use crate::pdf::verify::Verification;

/// One byte replacement in a stream: `(offset, from, to)`.
pub type ByteEdit = (usize, u8, u8);

/// The fixed line for a file that carried a signature (D-052).
pub const SIGNED_NOTE: &str = "the original is digitally signed; the repaired file is not";

/// Every setting the output depends on (goal-r2-q12). No path and no machine
/// name; `threads` and the scratch cap are absent because they never change
/// output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsSnapshot {
    pub salvage_work: u64,
    pub salvage_deep_work: u64,
    pub salvage_deep_pool: u64,
    pub max_search_stream: u64,
    pub passes: Option<Vec<CorruptionClass>>,
    pub font_policy: FontSourcePolicy,
    pub unreproducible: UnreproduciblePolicy,
    pub default_page_size: PageSize,
    pub extract_unplaceable_images: bool,
    pub auto_accept_confidence: Ratio,
    pub max_font_candidates: u32,
}

impl SettingsSnapshot {
    /// The settings `opts` repairs with.
    pub fn of(opts: &RepairOptions) -> Self {
        let budget = &opts.analyze.salvage_budget;
        SettingsSnapshot {
            salvage_work: budget.work,
            salvage_deep_work: budget.deep_work,
            salvage_deep_pool: budget.deep_pool,
            max_search_stream: budget.max_search_stream,
            passes: opts.passes.clone(),
            font_policy: opts.font_policy,
            unreproducible: opts.unreproducible,
            default_page_size: opts.default_page_size,
            extract_unplaceable_images: opts.extract_unplaceable_images,
            auto_accept_confidence: opts.auto_accept_confidence,
            max_font_candidates: opts.max_font_candidates,
        }
    }
}

/// One question a repair asked and the answer it got.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InteractionRecord {
    pub request: InteractionSummary,
    pub reply: InteractionReply,
    pub source: InteractionSource,
}

/// The parts of a request the report keeps: its kind, where, and the ids of
/// the fonts offered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InteractionSummary {
    pub kind: InteractionKind,
    pub page: Option<u32>,
    pub slot: Option<String>,
    pub candidates: Vec<String>,
}

/// Who answered (D-141). Only `User` and `UseBest` say the user saw the
/// question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InteractionSource {
    /// The user answered this question.
    User,
    /// A configured policy answered without asking: the engine's
    /// `unreproducible` policy, or the app's `[fonts] prompt_unresolved =
    /// false`.
    Policy,
    /// The user chose the best guess for this question.
    UseBest,
    /// The app answered without asking, from an answer the user gave another
    /// question: the one for the same font family in the file, "use best
    /// for both" for the file's later questions, or "apply best to all".
    Batched,
}

/// One repair pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassReport {
    pub class: CorruptionClass,
    pub outcome: PassOutcome,
    pub actions: Vec<RepairAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PassOutcome {
    Fixed,
    Partial(String),
    Skipped(String),
}

/// What a pass did to one object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairAction {
    pub object: ObjId,
    pub what: String,
    /// Set for every C9 stream action (D-041).
    pub grade: Option<SalvageGrade>,
}

/// C9 stream counts (D-041): `accepted` repairs are counted apart from `exact`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct C9Summary {
    pub streams_damaged: u32,
    pub repaired: u32,
    pub exact: u32,
    pub accepted: u32,
    pub ambiguous: u32,
    pub unrecoverable: u32,
    pub unsearched: u32,
}

/// One candidate output and how it verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateReport {
    pub toolpath: Toolpath,
    pub verification: Verification,
    pub chosen: bool,
}

/// An image extracted for export. The bytes travel in `RepairOutcome.images`,
/// never in the report (D-073).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedImage {
    pub name: String,
    pub sha256: [u8; 32],
    pub len: u64,
}

/// The record of one repair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairReport {
    /// [`RepairReport::SCHEMA_VERSION`].
    pub schema_version: u32,
    pub engine_version: String,
    pub font_db_sha256: [u8; 32],
    pub input_sha256: [u8; 32],
    pub settings: SettingsSnapshot,
    /// Every request and reply, in order.
    pub interactions: Vec<InteractionRecord>,
    pub findings_before: Vec<Finding>,
    pub findings_after: Vec<Finding>,
    pub passes: Vec<PassReport>,
    pub c9_summary: C9Summary,
    pub candidates: Vec<CandidateReport>,
    pub chosen: Option<Toolpath>,
    /// Per searched stream, every surviving edit list; sorted by `ObjId` (a map
    /// keyed by `ObjId` cannot be written as JSON).
    pub c9_survivors: Vec<(ObjId, Vec<Vec<ByteEdit>>)>,
    /// `(object, filter before, filter after)`.
    pub filter_rewrites: Vec<(ObjId, String, String)>,
    /// Reals that do not survive `f32` on re-emission (D-075).
    pub reals_narrowed: u32,
    pub shadows: Vec<(ObjId, ByteSpan)>,
    /// [`SIGNED_NOTE`] when the original was signed (D-052).
    pub signed_note: Option<String>,
    pub extracted_images: Vec<ExtractedImage>,
    pub partial_reasons: Vec<String>,
    pub stats: AnalyzeStats,
}

impl RepairReport {
    pub const SCHEMA_VERSION: u32 = 1;

    /// A report that records the inputs and nothing done: the shape every
    /// repair starts from.
    pub fn default_for(analysis: &AnalysisResult, opts: &RepairOptions, fonts: &FontDb) -> Self {
        RepairReport {
            schema_version: Self::SCHEMA_VERSION,
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            font_db_sha256: fonts.sha256(),
            input_sha256: analysis.input_sha256,
            settings: SettingsSnapshot::of(opts),
            interactions: Vec::new(),
            findings_before: analysis.findings.clone(),
            findings_after: Vec::new(),
            passes: Vec::new(),
            c9_summary: C9Summary::default(),
            candidates: Vec::new(),
            chosen: None,
            c9_survivors: Vec::new(),
            filter_rewrites: Vec::new(),
            reals_narrowed: 0,
            shadows: Vec::new(),
            signed_note: None,
            extracted_images: Vec::new(),
            partial_reasons: Vec::new(),
            stats: analysis.stats.clone(),
        }
    }

    /// The answers that replay this report's questions, in order, each with
    /// the source it was recorded with (goal-r2-q12, D-141): every record
    /// but the engine's own policy answers, which a repair with the same
    /// settings gives again without asking. Those are the `Policy` records of
    /// an unreproducible font under a policy other than `Ask`; a `Policy`
    /// record of a question the engine asked (the app's `[fonts]
    /// prompt_unresolved = false`) is replayed like any other.
    // A replay is run by the tests; the runner replays its own answers.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn replay_answers(&self) -> Vec<Answer> {
        let unasked = |r: &&InteractionRecord| {
            r.source == InteractionSource::Policy
                && r.request.kind == InteractionKind::FontUnreproducible
                && self.settings.unreproducible != UnreproduciblePolicy::Ask
        };
        (self.interactions.iter())
            .filter(|r| !unasked(r))
            .map(|r| Answer {
                reply: r.reply.clone(),
                source: r.source,
            })
            .collect()
    }

    /// The fixed report lines: the signature note (D-052), the C9 count line
    /// when a damaged stream was found (D-041) and the narrowed-reals line
    /// (D-075).
    // The result view draws its own (T-22b); the artefact test reads these.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(note) = &self.signed_note {
            lines.push(note.clone());
        }
        let c9 = &self.c9_summary;
        if c9.streams_damaged > 0 {
            lines.push(format!(
                "{} streams repaired; {} of them unique within the searched window; \
                 {} accepted without a uniqueness check; {} ambiguous; \
                 {} unrecoverable, written as found",
                c9.repaired, c9.exact, c9.accepted, c9.ambiguous, c9.unrecoverable
            ));
        }
        if self.reals_narrowed > 0 {
            lines.push(format!(
                "{} numbers were narrowed to single precision on re-emission",
                self.reals_narrowed
            ));
        }
        lines
    }
}
