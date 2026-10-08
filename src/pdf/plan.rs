//! Repair planner (T-13a; SE Q1 Layer A, D-008): which toolpaths a repair
//! builds, from the findings alone, as a fixed table.
//!
//! | Row | Selected findings | Candidates |
//! |---|---|---|
//! | [`PlanRow::Encrypted`] | an `Encrypted` finding (whatever `passes` says) | none: decrypt first |
//! | [`PlanRow::NothingToRepair`] | no corruption finding | none |
//! | [`PlanRow::FontTrack`] | any of C7, C8 | `TemplateAssemble`, `Resave` |
//! | [`PlanRow::Relink`] | C6 (and neither C7 nor C8) | `Resave` |
//! | [`PlanRow::Structural`] | any of C4, C5, C10 (and no font class) | `Resave`, `TemplateAssemble` |
//! | [`PlanRow::ReEmission`] | only C1–C3 and C9 | `Resave` |
//!
//! C9 adds no toolpath of its own: the salvage is swapped into whichever
//! toolpath is built (SE Q1, "orthogonal"). C6's re-link (T-13b) works
//! inside `Resave`. The font track (T-30) builds `TemplateAssemble`, which
//! substitutes the damaged fonts, and keeps `Resave` beside it as the
//! structure-preserving comparison (map #2): `Resave` keeps the broken font,
//! and wins only when template assembly fails V0. Repair skips a
//! `TemplateAssemble` with no font to substitute (the structural row) with a
//! log: it would equal `Resave`. The candidate set is bounded by this table,
//! never by time.
//!
//! `prior` (SE Q2 V4: the rule prior, a tie-break only; the TD's 0.60
//! reachability constant is no more than that, map #12) is `Resave`, except
//! in the font track, where it is `TemplateAssemble`: a C7 font whose
//! `/ToUnicode` survives extracts the same text from both outputs, so V0–V3
//! tie and only the prior can prefer the substituted font. Every selected
//! finding whose repair is `Interactive` becomes an [`Escalation`].
//!
//! `RepairOptions.passes`: `None` lets the planner decide (GG §1); `Some`
//! is the user's checklist and the table sees only the findings of the
//! classes it names. Repair logs the classes it leaves out.
// T-14 is the first caller outside the tests.
#![cfg_attr(not(test), allow(dead_code))]

#[cfg(test)]
mod tests;

use crate::engine::{Escalation, EscalationKind, RepairOptions, RepairPlan, Toolpath};
use crate::pdf::carver::CarveReport;
use crate::pdf::graph::ObjectGraph;
use crate::pdf::model::{CorruptionClass, Evidence, Finding, FindingKind, Location, Repairability};

use CorruptionClass::{
    C4PageTreeBroken, C5ObjectTagStripped, C6FontMapLost, C7FontStreamDeleted,
    C8FontResourcesDeleted, C10Truncated,
};

/// The row of the Layer A table a file falls in (module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanRow {
    Encrypted,
    NothingToRepair,
    FontTrack,
    Relink,
    Structural,
    ReEmission,
}

impl PlanRow {
    /// The row for `findings`, seeing only the classes `passes` names when
    /// it is `Some`.
    pub(crate) fn of(findings: &[Finding], passes: Option<&[CorruptionClass]>) -> PlanRow {
        if findings.iter().any(|f| f.class == FindingKind::Encrypted) {
            return PlanRow::Encrypted;
        }
        let classes: Vec<CorruptionClass> =
            selected(findings, passes).filter_map(class_of).collect();
        let any = |of: &[CorruptionClass]| classes.iter().any(|c| of.contains(c));
        if classes.is_empty() {
            PlanRow::NothingToRepair
        } else if any(&[C7FontStreamDeleted, C8FontResourcesDeleted]) {
            PlanRow::FontTrack
        } else if any(&[C6FontMapLost]) {
            PlanRow::Relink
        } else if any(&[C4PageTreeBroken, C5ObjectTagStripped, C10Truncated]) {
            PlanRow::Structural
        } else {
            PlanRow::ReEmission
        }
    }

    /// The toolpaths this row builds, in build order.
    pub(crate) fn candidates(self) -> Vec<Toolpath> {
        match self {
            PlanRow::Encrypted | PlanRow::NothingToRepair => Vec::new(),
            PlanRow::FontTrack => vec![Toolpath::TemplateAssemble, Toolpath::Resave],
            PlanRow::Structural => vec![Toolpath::Resave, Toolpath::TemplateAssemble],
            PlanRow::Relink | PlanRow::ReEmission => vec![Toolpath::Resave],
        }
    }

    /// The row's prior (module docs).
    pub(crate) fn prior(self) -> Toolpath {
        match self {
            PlanRow::FontTrack => Toolpath::TemplateAssemble,
            _ => Toolpath::Resave,
        }
    }
}

/// The repair plan for `findings` (module docs). `carve` and `graph` come
/// from the same input; the v1 table reads the findings only.
pub(crate) fn plan(
    findings: &[Finding],
    _carve: &CarveReport,
    _graph: &ObjectGraph,
    opts: &RepairOptions,
) -> RepairPlan {
    let passes = opts.passes.as_deref();
    let row = PlanRow::of(findings, passes);
    let escalations = match row {
        PlanRow::Encrypted => Vec::new(),
        _ => selected(findings, passes).filter_map(escalation).collect(),
    };
    RepairPlan {
        candidates: row.candidates(),
        prior: row.prior(),
        escalations,
    }
}

/// The corruption findings of the classes `passes` names (all of them when
/// it is `None`), in their order. Info, `Encrypted` and `Signed` findings
/// are never selected.
pub(crate) fn selected<'f>(
    findings: &'f [Finding],
    passes: Option<&'f [CorruptionClass]>,
) -> impl Iterator<Item = &'f Finding> {
    findings.iter().filter(move |f| {
        class_of(f).is_some_and(|c| passes.is_none_or(|chosen| chosen.contains(&c)))
    })
}

fn class_of(f: &Finding) -> Option<CorruptionClass> {
    match f.class {
        FindingKind::Corruption(c) => Some(c),
        _ => None,
    }
}

/// An `Interactive` finding's escalation: its page, its slot (a C6
/// finding's `slots:` evidence) and its id and summary as the note.
fn escalation(f: &Finding) -> Option<Escalation> {
    let Repairability::Interactive(kind) = f.repair else {
        return None;
    };
    let page = match f.location {
        Location::Page { index, .. } => Some(index),
        _ => None,
    };
    let slot = f.evidence.iter().find_map(|e| match e {
        Evidence::Text(t) => t.strip_prefix("slots: ").map(str::to_owned),
        _ => None,
    });
    Some(Escalation {
        kind: EscalationKind::Interaction(kind),
        page,
        slot,
        note: format!("{}: {}", f.id, f.summary),
    })
}
