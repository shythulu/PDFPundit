//! Verification gates V0, V1 and V2 (T-12b).
//!
//! The record shapes are fixed by T-02b so T-12b implements against them. Every
//! value is a `bool`, a count or an integer [`Ratio`] of interpreted or carved
//! data: no number read from rendered pixels reaches verification (D-059).

use serde::{Deserialize, Serialize};

use crate::pdf::model::Ratio;

/// One candidate output's verification, against the input's baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verification {
    pub baseline: BaselineKind,
    pub v0: Gates,
    pub v1: Retention,
    pub v2: Plausibility,
}

/// What the input's text baseline came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BaselineKind {
    /// Text extracted from the input (T-36).
    Extracted,
    /// Show-operator counts over the carved content streams, when the input
    /// does not load.
    CarveProxy,
    None,
}

/// V0: pass/fail gates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gates {
    pub lopdf_reload: bool,
    pub hayro_render_all_pages: bool,
    pub rediagnose_clean: bool,
    pub page_count_ok: bool,
    pub root_and_pages_present: bool,
}

/// V1: what the output kept, as fractions of the baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retention {
    pub text_ops: Ratio,
    pub path_fills: Ratio,
    pub images: Ratio,
    pub content_bytes: Ratio,
    /// Pages blank in the output but not in the baseline, from paint counts.
    pub blank_pages: u32,
    pub glyph_count: Ratio,
}

/// V2: whether the output's text reads as text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plausibility {
    pub unmapped_glyph: Ratio,
    pub fffd: Ratio,
    pub script_consistency: Ratio,
    /// `None` without a dictionary (D-011).
    pub dictionary_hit: Option<Ratio>,
}
