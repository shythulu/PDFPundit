//! The crate-private engine facade (plan §4): everything a caller must know.
//! The module is `pub(crate)` with no cfg, feature or flag that exposes it
//! (D-047); `pub` below means visible to the crate's other modules and tests.
//!
//! A caller learns [`Engine`] (analyze, plan, repair), [`Progress`],
//! [`Interact`] and the model types. Behind them sit carve, salvage, graph,
//! rebuild, diagnose, plan, emit and verify. Every shape here is fixed by T-02b;
//! T-14 replaces the stub bodies of [`Pdfpundit`] without changing a shape (the
//! contract tests compare the serialised shape with `tests/data/contract/`).
//!
//! Serialised types hold no map with a non-string key (plan §3.1): maps keyed by
//! anything else are `Vec<(K, V)>` sorted by `K`.
#![deny(clippy::iter_over_hash_type)]
// The runner (T-15), the shell (T-23a) and the real bodies (T-14) are the
// consumers; until they land most of this is unused outside the tests.
// TODO(T-14, T-15): remove this allow once the real bodies and the runner use
// the facade, so it stops hiding dead code.
#![allow(dead_code)]

mod report;

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// The facade's vocabulary, re-exported for its callers; unused until they land.
#[allow(unused_imports)]
pub use crate::pdf::model::{
    ByteSpan, CorruptionClass, Evidence, FileMeta, Finding, FindingKind, HexWindow,
    InteractionKind, Location, MetricValue, ObjId, PaintCounts, Ratio, Repairability, SalvageGrade,
    Severity,
};
#[allow(unused_imports)]
pub use crate::pdf::verify::{BaselineKind, Gates, Plausibility, Retention, Verification};
#[allow(unused_imports)]
pub use report::{
    ByteEdit, C9Summary, CandidateReport, ExtractedImage, InteractionRecord, InteractionSource,
    InteractionSummary, PassOutcome, PassReport, RepairAction, RepairReport, SIGNED_NOTE,
    SettingsSnapshot,
};

use crate::pdf::carver::CarveReport;
use crate::pdf::graph::ObjectGraph;
use crate::pdf::streams::salvage::SalvageIndex;

// ── options ──────────────────────────────────────────────────────────────

/// How much C9 search one file may spend. Every figure counts work, never
/// time (D-041): W = input bytes consumed by candidate inflates + 244 × inflate
/// calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SalvageBudget {
    /// W per damaged stream.
    pub work: u64,
    /// W per stream when `work` found no accept, drawn from `deep_pool`.
    pub deep_work: u64,
    /// The per-file pool `deep_work` is drawn from (D-056).
    pub deep_pool: u64,
    /// Raw stream bytes above which no search runs (D-072).
    pub max_search_stream: u64,
}

impl Default for SalvageBudget {
    fn default() -> Self {
        SalvageBudget {
            work: 700_000_000,
            deep_work: 10_000_000_000,
            deep_pool: 80_000_000_000,
            max_search_stream: 4 << 20,
        }
    }
}

/// Options for [`Engine::analyze`]. `threads` and `scratch_cap` change
/// wall-clock and peak memory only, never the output (D-066, D-072), so they are
/// not in the [`SettingsSnapshot`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyzeOptions {
    /// Inputs above this size get a warning (512 MiB).
    pub max_file_bytes: u64,
    pub salvage_budget: SalvageBudget,
    pub threads: NonZeroUsize,
    /// Bytes of search scratch admitted at once (1 GiB).
    pub scratch_cap: u64,
}

impl Default for AnalyzeOptions {
    fn default() -> Self {
        AnalyzeOptions {
            max_file_bytes: 512 << 20,
            salvage_budget: SalvageBudget::default(),
            threads: NonZeroUsize::MIN,
            scratch_cap: 1 << 30,
        }
    }
}

/// Options for [`Engine::plan`] and [`Engine::repair`].
///
/// Three fields go beyond plan §4's five (a recorded T-02b deviation):
/// `analyze` is what `repair` re-analyses with when the analysis carries no
/// usable state (`repair` takes no [`AnalyzeOptions`] of its own), and its
/// salvage budget is what the report's [`SettingsSnapshot`] records;
/// `auto_accept_confidence` and `max_font_candidates` are the two `[repair]`
/// knobs the snapshot must also record (D-017).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairOptions {
    /// `None`: the planner decides (GG §1).
    pub passes: Option<Vec<CorruptionClass>>,
    pub font_policy: FontSourcePolicy,
    pub unreproducible: UnreproduciblePolicy,
    pub default_page_size: PageSize,
    pub extract_unplaceable_images: bool,
    /// `[repair] auto_accept_confidence` (0.35).
    pub auto_accept_confidence: Ratio,
    /// `[repair] max_font_candidates` (5).
    pub max_font_candidates: u32,
    pub analyze: AnalyzeOptions,
}

impl Default for RepairOptions {
    fn default() -> Self {
        RepairOptions {
            passes: None,
            font_policy: FontSourcePolicy::Bundled,
            unreproducible: UnreproduciblePolicy::Ask,
            default_page_size: PageSize::A4,
            extract_unplaceable_images: true,
            auto_accept_confidence: Ratio { num: 35, den: 100 },
            max_font_candidates: 5,
            analyze: AnalyzeOptions::default(),
        }
    }
}

/// Where replacement fonts may come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FontSourcePolicy {
    Bundled,
    /// M5.
    BundledThenSystem,
}

/// What to do when no font can reproduce a slot's glyphs (SE Q3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnreproduciblePolicy {
    Ask,
    SubstituteGeneric,
    TextOnly,
}

/// The page size used when a page has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageSize {
    A4,
    Letter,
    Custom { w_pt: u32, h_pt: u32 },
}

// ── analysis ─────────────────────────────────────────────────────────────

/// What [`Engine::analyze`] found. `state` is never serialised: a result
/// reloaded from history has none, and `repair` then re-analyses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub meta: FileMeta,
    pub findings: Vec<Finding>,
    pub carve: CarveSummary,
    pub font_slots: Vec<FontSlot>,
    pub stats: AnalyzeStats,
    pub input_sha256: [u8; 32],
    #[serde(skip)]
    pub state: StateHandle,
}

/// One font resource of one page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontSlot {
    pub page: u32,
    /// The resource name, e.g. `"F1"`.
    pub slot: String,
    pub base_font: Option<String>,
    pub subtype: Option<String>,
    pub embedded: bool,
    pub tounicode: ToUnicodeState,
    pub glyph_count: u32,
    pub resolution: Option<FontResolution>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToUnicodeState {
    Present,
    Missing,
    Unparsable,
}

/// How a damaged font slot was resolved, and the steps that led there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontResolution {
    pub kind: FontResolutionKind,
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FontResolutionKind {
    /// Re-linked to a carved font object.
    Relinked(ObjId),
    Picked {
        font_id: String,
        confidence: Ratio,
    },
    Substituted(SubstituteChoice),
    TextOnly,
    Skipped,
}

/// The carve in numbers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarveSummary {
    pub objects: u32,
    pub orphans: u32,
    pub streams: u32,
    pub shadows: u32,
    pub xref_kind: XrefKind,
    pub header_offset: Option<u64>,
    pub truncated: bool,
    /// Which carve caps were reached.
    pub caps_hit: Vec<String>,
}

impl Default for CarveSummary {
    fn default() -> Self {
        CarveSummary {
            objects: 0,
            orphans: 0,
            streams: 0,
            shadows: 0,
            xref_kind: XrefKind::None,
            header_offset: None,
            truncated: false,
            caps_hit: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum XrefKind {
    Classic,
    Stream,
    None,
}

/// Analysis counters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyzeStats {
    pub bytes: u64,
    pub pages_discovered: u32,
    /// W spent on C9 search; also the parked-job eviction key (T-15, D-005).
    pub salvage_work_total: u64,
    /// Stream count per salvage outcome name (`"Clean"`, `"Repaired"`, …).
    pub streams_by_salvage: BTreeMap<String, u32>,
    pub baseline_kind: BaselineKind,
    /// Per page, from interpreted content (T-36).
    pub paint_counts: Vec<PaintCounts>,
}

impl Default for AnalyzeStats {
    fn default() -> Self {
        AnalyzeStats {
            bytes: 0,
            pages_discovered: 0,
            salvage_work_total: 0,
            streams_by_salvage: BTreeMap::new(),
            baseline_kind: BaselineKind::None,
            paint_counts: Vec::new(),
        }
    }
}

/// What analysis built and repair reuses: the carve, the object graph, the
/// salvage index and the hash of the input they came from (D-006). Never
/// serialised, never persisted.
#[derive(Debug)]
pub struct AnalysisState {
    pub(crate) carve: CarveReport,
    pub(crate) graph: ObjectGraph,
    pub(crate) salvage: SalvageIndex,
    pub(crate) input_sha256: [u8; 32],
    /// Added to [`Self::heap_bytes`] so the runner's memory cap is testable
    /// with a fake engine (T-15).
    #[cfg(test)]
    pub(crate) scripted_heap_bytes: u64,
}

impl AnalysisState {
    pub(crate) fn new(
        carve: CarveReport,
        graph: ObjectGraph,
        salvage: SalvageIndex,
        input_sha256: [u8; 32],
    ) -> Self {
        AnalysisState {
            carve,
            graph,
            salvage,
            input_sha256,
            #[cfg(test)]
            scripted_heap_bytes: 0,
        }
    }

    /// Bytes this state holds on the heap: carve + graph + salvage index, never
    /// the index alone (D-005, lead-r4-fr1).
    pub(crate) fn heap_bytes(&self) -> u64 {
        let total = self.carve.heap_bytes() + self.graph.heap_bytes() + self.salvage.heap_bytes();
        #[cfg(test)]
        let total = total + self.scripted_heap_bytes;
        total
    }
}

/// A shared handle to an [`AnalysisState`]. Equal when both are empty or both
/// point at the same allocation: two states with identical contents in
/// different allocations are different handles.
#[derive(Debug, Clone, Default)]
pub struct StateHandle(pub(crate) Option<Arc<AnalysisState>>);

impl StateHandle {
    pub(crate) fn new(state: AnalysisState) -> Self {
        StateHandle(Some(Arc::new(state)))
    }
}

impl PartialEq for StateHandle {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

// ── plan and repair ──────────────────────────────────────────────────────

/// How a candidate output is built (TD §17.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Toolpath {
    Resave,
    TemplateAssemble,
}

/// The candidates repair will build, the planner's prior, and what needs a
/// human or a note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairPlan {
    pub candidates: Vec<Toolpath>,
    pub prior: Toolpath,
    pub escalations: Vec<Escalation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Escalation {
    pub kind: EscalationKind,
    pub page: Option<u32>,
    pub slot: Option<String>,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EscalationKind {
    Interaction(InteractionKind),
    CandidatesDisagree,
    OutOfDistribution,
    NoCandidatePassed,
}

/// What [`Engine::repair`] returns. Image bytes travel here, to the export
/// path; the report lists them by name, hash and length only (D-073). Whether
/// the analysis state was reused is a run fact, recorded by the runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairOutcome {
    pub output: Option<Vec<u8>>,
    pub report: RepairReport,
    pub status: OutcomeStatus,
    pub images: Vec<(String, Vec<u8>)>,
    pub analysis_state: AnalysisStateUse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutcomeStatus {
    Ok,
    Partial(Vec<String>),
    Failed(String),
}

/// Whether repair reused the analysis state or rebuilt it, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnalysisStateUse {
    Reused,
    Rebuilt { reason: String },
}

// ── the engine ───────────────────────────────────────────────────────────

/// The synchronous pipeline. The runner (T-15) wraps it in threads; tests
/// drive it directly.
pub trait Engine: Send + Sync {
    fn analyze(
        &self,
        bytes: &[u8],
        opts: &AnalyzeOptions,
        sink: &mut dyn Progress,
    ) -> Result<AnalysisResult, Cancelled>;

    fn plan(&self, analysis: &AnalysisResult, opts: &RepairOptions) -> RepairPlan;

    #[allow(clippy::too_many_arguments)]
    fn repair(
        &self,
        bytes: &[u8],
        analysis: &AnalysisResult,
        plan: &RepairPlan,
        opts: &RepairOptions,
        fonts: &FontDb,
        ask: &mut dyn Interact,
        sink: &mut dyn Progress,
    ) -> Result<RepairOutcome, Cancelled>;
}

/// The real engine. Stub bodies until T-14: analysis reports no findings and
/// reads only the header version; repair fails with "engine not built".
#[derive(Debug, Clone, Copy, Default)]
pub struct Pdfpundit;

impl Engine for Pdfpundit {
    fn analyze(
        &self,
        bytes: &[u8],
        _opts: &AnalyzeOptions,
        sink: &mut dyn Progress,
    ) -> Result<AnalysisResult, Cancelled> {
        if sink.cancelled() {
            return Err(Cancelled);
        }
        Ok(AnalysisResult {
            meta: FileMeta {
                version: header_version(bytes),
                pages: 0,
                title: None,
                page_sizes: Vec::new(),
            },
            findings: Vec::new(),
            carve: CarveSummary::default(),
            font_slots: Vec::new(),
            stats: AnalyzeStats {
                bytes: bytes.len() as u64,
                ..AnalyzeStats::default()
            },
            input_sha256: Sha256::digest(bytes).into(),
            state: StateHandle::default(),
        })
    }

    fn plan(&self, _analysis: &AnalysisResult, _opts: &RepairOptions) -> RepairPlan {
        RepairPlan {
            candidates: vec![Toolpath::Resave],
            prior: Toolpath::Resave,
            escalations: Vec::new(),
        }
    }

    fn repair(
        &self,
        _bytes: &[u8],
        analysis: &AnalysisResult,
        _plan: &RepairPlan,
        opts: &RepairOptions,
        fonts: &FontDb,
        _ask: &mut dyn Interact,
        _sink: &mut dyn Progress,
    ) -> Result<RepairOutcome, Cancelled> {
        Ok(RepairOutcome {
            output: None,
            report: RepairReport::default_for(analysis, opts, fonts),
            status: OutcomeStatus::Failed("engine not built".into()),
            images: Vec::new(),
            analysis_state: AnalysisStateUse::Rebuilt {
                reason: "stub".into(),
            },
        })
    }
}

/// The `x.y` of the first `%PDF-x.y` in the first KiB, if any.
fn header_version(bytes: &[u8]) -> Option<String> {
    const MARKER: &[u8] = b"%PDF-";
    let head = &bytes[..bytes.len().min(1024)];
    let at = head.windows(MARKER.len()).position(|w| w == MARKER)?;
    match bytes.get(at + MARKER.len()..at + MARKER.len() + 3)? {
        [major, b'.', minor] if major.is_ascii_digit() && minor.is_ascii_digit() => {
            Some(format!("{}.{}", *major as char, *minor as char))
        }
        _ => None,
    }
}

/// [`Engine::analyze`] on [`Pdfpundit`].
pub fn analyze(
    bytes: &[u8],
    opts: &AnalyzeOptions,
    sink: &mut dyn Progress,
) -> Result<AnalysisResult, Cancelled> {
    Pdfpundit.analyze(bytes, opts, sink)
}

/// [`Engine::plan`] on [`Pdfpundit`].
pub fn plan(analysis: &AnalysisResult, opts: &RepairOptions) -> RepairPlan {
    Pdfpundit.plan(analysis, opts)
}

/// [`Engine::repair`] on [`Pdfpundit`].
pub fn repair(
    bytes: &[u8],
    analysis: &AnalysisResult,
    plan: &RepairPlan,
    opts: &RepairOptions,
    fonts: &FontDb,
    ask: &mut dyn Interact,
    sink: &mut dyn Progress,
) -> Result<RepairOutcome, Cancelled> {
    Pdfpundit.repair(bytes, analysis, plan, opts, fonts, ask, sink)
}

// ── progress and interaction ─────────────────────────────────────────────

/// Where the engine reports what it is doing, and asks whether to stop.
pub trait Progress {
    fn phase(&mut self, name: &'static str, index: u32, total: u32);
    fn progress(&mut self, done: u64, total: Option<u64>);
    fn finding(&mut self, f: &Finding);
    fn log(&mut self, level: LogLevel, msg: String);
    fn cancelled(&self) -> bool;
}

/// Ignores everything and never cancels.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullProgress;

impl Progress for NullProgress {
    fn phase(&mut self, _name: &'static str, _index: u32, _total: u32) {}
    fn progress(&mut self, _done: u64, _total: Option<u64>) {}
    fn finding(&mut self, _f: &Finding) {}
    fn log(&mut self, _level: LogLevel, _msg: String) {}
    fn cancelled(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

/// The job was cancelled; no artefact is produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("cancelled")]
pub struct Cancelled;

/// Where a repair asks its questions (D-020).
pub trait Interact {
    fn ask(&mut self, req: InteractionRequest) -> Result<InteractionReply, Cancelled>;
}

/// Answers every question with [`InteractionReply::UseBest`]. Reachable only
/// from the tests and the runner.
#[derive(Debug, Clone, Copy, Default)]
pub struct UseBest;

impl Interact for UseBest {
    fn ask(&mut self, _req: InteractionRequest) -> Result<InteractionReply, Cancelled> {
        Ok(InteractionReply::UseBest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InteractionRequest {
    FontPick(FontPickRequest),
    FontUnreproducible(UnreproducibleRequest),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InteractionReply {
    /// A `font_id`.
    Pick(String),
    UseBest,
    Skip,
    Substitute(SubstituteChoice),
    TextOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct InteractionRequestId(pub u64);

/// Which font a damaged slot should be rebuilt from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontPickRequest {
    pub id: InteractionRequestId,
    pub page: u32,
    pub slot: String,
    /// At most 64.
    pub sample_codes: Vec<u32>,
    /// At most 5 (TD:424).
    pub candidates: Vec<FontCandidate>,
    /// At most 200 chars, decoded through the top candidate.
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontCandidate {
    pub font_id: String,
    pub family: String,
    pub language: String,
    pub score: Ratio,
    pub confidence: Ratio,
    /// At most 200 chars.
    pub preview: String,
}

/// No font reproduces these slots' glyphs: substitute, text only, or skip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnreproducibleRequest {
    pub id: InteractionRequestId,
    pub family: String,
    /// `(page, slot)`.
    pub slots: Vec<(u32, String)>,
    pub reason: String,
    pub options: Vec<SubstituteChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubstituteChoice {
    pub font_id: String,
    pub label: String,
}

// ── fonts ────────────────────────────────────────────────────────────────

/// The font database repair draws replacement fonts from (T-28): its
/// loading, hashing and lookups live in `pdf::fontdb`.
pub(crate) use crate::pdf::fontdb::FontDb;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum FontDbError {
    #[error("bad font index: {0}")]
    BadIndex(String),
    #[error("missing font blob: {0}")]
    MissingBlob(String),
    #[error("font blob {name} does not match its hash")]
    HashMismatch { name: String },
}

#[cfg(test)]
mod artefact_tests;
#[cfg(test)]
mod contract_tests;
#[cfg(test)]
mod tests;
