//! T-13a acceptance for the planner: one test per row of the Layer A table,
//! the prior, escalations from interactive findings, and the
//! `RepairOptions.passes` override.

use super::*;
use crate::engine::{EscalationKind, RepairOptions, Toolpath};
use crate::pdf::model::{
    Evidence, FindingKind, InteractionKind, Location, Repairability, Severity,
};

use CorruptionClass::{
    C1Header, C2XrefMissing, C3TrailerDamaged, C4PageTreeBroken, C5ObjectTagStripped,
    C6FontMapLost, C7FontStreamDeleted, C8FontResourcesDeleted, C9ZlibTampered, C10Truncated,
};
use Toolpath::{Resave, TemplateAssemble};

fn finding(class: FindingKind, n: u32) -> Finding {
    Finding {
        id: format!("X-{n:03}"),
        class,
        severity: Severity::Error,
        location: Location::File,
        summary: "something".to_owned(),
        evidence: Vec::new(),
        repair: Repairability::Auto,
    }
}

fn corruption(classes: &[CorruptionClass]) -> Vec<Finding> {
    classes
        .iter()
        .zip(1..)
        .map(|(&c, n)| finding(FindingKind::Corruption(c), n))
        .collect()
}

fn plan_with(findings: &[Finding], passes: Option<Vec<CorruptionClass>>) -> RepairPlan {
    let opts = RepairOptions {
        passes,
        ..RepairOptions::default()
    };
    plan(
        findings,
        &CarveReport::default(),
        &ObjectGraph::default(),
        &opts,
    )
}

fn candidates(classes: &[CorruptionClass]) -> Vec<Toolpath> {
    plan_with(&corruption(classes), None).candidates
}

// ── one test per row ─────────────────────────────────────────────────────

#[test]
fn row_encrypted_builds_nothing() {
    let mut findings = corruption(&[C2XrefMissing, C4PageTreeBroken]);
    findings.push(finding(FindingKind::Encrypted, 1));
    let plan = plan_with(&findings, None);
    assert_eq!(PlanRow::of(&findings, None), PlanRow::Encrypted);
    assert!(plan.candidates.is_empty());
    assert_eq!(plan.prior, Resave);
}

#[test]
fn row_nothing_to_repair_builds_nothing() {
    assert!(candidates(&[]).is_empty());
    // Info findings are not corruption.
    let signed = vec![finding(FindingKind::Signed { fields: 1 }, 1)];
    assert_eq!(PlanRow::of(&signed, None), PlanRow::NothingToRepair);
    assert!(plan_with(&signed, None).candidates.is_empty());
}

#[test]
fn row_re_emission_resaves_c1_to_c3() {
    for classes in [
        &[C1Header][..],
        &[C2XrefMissing],
        &[C3TrailerDamaged],
        &[C1Header, C2XrefMissing, C3TrailerDamaged],
    ] {
        assert_eq!(
            PlanRow::of(&corruption(classes), None),
            PlanRow::ReEmission,
            "{classes:?}"
        );
        assert_eq!(candidates(classes), [Resave], "{classes:?}");
    }
}

#[test]
fn row_re_emission_takes_c9_as_orthogonal() {
    // C9 adds no toolpath: its salvage is swapped into whichever is built.
    for classes in [&[C9ZlibTampered][..], &[C9ZlibTampered, C2XrefMissing]] {
        assert_eq!(PlanRow::of(&corruption(classes), None), PlanRow::ReEmission);
        assert_eq!(candidates(classes), [Resave], "{classes:?}");
    }
    assert_eq!(
        candidates(&[C9ZlibTampered, C4PageTreeBroken]),
        [Resave, TemplateAssemble]
    );
}

#[test]
fn row_structural_builds_both_toolpaths() {
    for classes in [
        &[C4PageTreeBroken][..],
        &[C5ObjectTagStripped],
        &[C10Truncated],
        &[C2XrefMissing, C5ObjectTagStripped],
        &[C1Header, C4PageTreeBroken, C10Truncated],
    ] {
        assert_eq!(
            PlanRow::of(&corruption(classes), None),
            PlanRow::Structural,
            "{classes:?}"
        );
        assert_eq!(
            candidates(classes),
            [Resave, TemplateAssemble],
            "{classes:?}"
        );
    }
}

#[test]
fn row_font_track_assembles_first_then_resaves() {
    for classes in [
        &[C7FontStreamDeleted][..],
        &[C8FontResourcesDeleted],
        &[C4PageTreeBroken, C7FontStreamDeleted],
        &[C6FontMapLost, C8FontResourcesDeleted],
    ] {
        assert_eq!(
            PlanRow::of(&corruption(classes), None),
            PlanRow::FontTrack,
            "{classes:?}"
        );
        assert_eq!(
            candidates(classes),
            [TemplateAssemble, Resave],
            "{classes:?}"
        );
    }
}

#[test]
fn row_relink_resaves_c6() {
    for classes in [&[C6FontMapLost][..], &[C6FontMapLost, C4PageTreeBroken]] {
        assert_eq!(
            PlanRow::of(&corruption(classes), None),
            PlanRow::Relink,
            "{classes:?}"
        );
        assert_eq!(candidates(classes), [Resave], "{classes:?}");
    }
}

#[test]
fn the_prior_is_resave_outside_the_font_track() {
    for classes in [
        &[][..],
        &[C2XrefMissing],
        &[C4PageTreeBroken],
        &[C6FontMapLost],
    ] {
        assert_eq!(plan_with(&corruption(classes), None).prior, Resave);
    }
    for classes in [&[C7FontStreamDeleted][..], &[C8FontResourcesDeleted]] {
        assert_eq!(
            plan_with(&corruption(classes), None).prior,
            TemplateAssemble
        );
    }
}

// ── escalations ──────────────────────────────────────────────────────────

#[test]
fn interactive_findings_become_escalations() {
    let mut c6 = finding(FindingKind::Corruption(C6FontMapLost), 1);
    c6.id = "C6-001".to_owned();
    c6.summary = "page 2 selects font /F2, which its resources do not map".to_owned();
    c6.location = Location::Page {
        index: 1,
        obj: Some((4, 0)),
    };
    c6.evidence = vec![Evidence::Text("slots: F2".to_owned())];
    c6.repair = Repairability::Interactive(InteractionKind::FontPick);
    let mut c8 = finding(FindingKind::Corruption(C8FontResourcesDeleted), 2);
    c8.id = "C8-001".to_owned();
    c8.location = Location::Object {
        id: (7, 0),
        span: None,
    };
    c8.repair = Repairability::Interactive(InteractionKind::FontPick);
    let auto = finding(FindingKind::Corruption(C4PageTreeBroken), 3);

    let plan = plan_with(&[c6, auto, c8.clone()], None);
    assert_eq!(
        plan.escalations,
        [
            Escalation {
                kind: EscalationKind::Interaction(InteractionKind::FontPick),
                page: Some(1),
                slot: Some("F2".to_owned()),
                note: "C6-001: page 2 selects font /F2, which its resources do not map".to_owned(),
            },
            Escalation {
                kind: EscalationKind::Interaction(InteractionKind::FontPick),
                page: None,
                slot: None,
                note: format!("C8-001: {}", c8.summary),
            },
        ]
    );
}

// ── the passes override ──────────────────────────────────────────────────

#[test]
fn passes_restrict_the_plan_to_the_chosen_classes() {
    let findings = corruption(&[C1Header, C5ObjectTagStripped]);
    assert_eq!(
        plan_with(&findings, None).candidates,
        [Resave, TemplateAssemble]
    );
    // Only C1 chosen: the table sees a C1-only file.
    let only_c1 = plan_with(&findings, Some(vec![C1Header]));
    assert_eq!(only_c1.candidates, [Resave]);
    assert_eq!(
        PlanRow::of(&findings, Some(&[C1Header])),
        PlanRow::ReEmission
    );
    // C5 chosen: both toolpaths.
    assert_eq!(
        plan_with(&findings, Some(vec![C5ObjectTagStripped])).candidates,
        [Resave, TemplateAssemble]
    );
    // Nothing chosen that the file has: nothing to build.
    assert!(
        plan_with(&findings, Some(vec![C8FontResourcesDeleted]))
            .candidates
            .is_empty()
    );
    assert!(plan_with(&findings, Some(Vec::new())).candidates.is_empty());
}

#[test]
fn passes_drop_the_escalations_of_unchosen_classes() {
    let mut c6 = finding(FindingKind::Corruption(C6FontMapLost), 1);
    c6.repair = Repairability::Interactive(InteractionKind::FontPick);
    let findings = [c6, finding(FindingKind::Corruption(C2XrefMissing), 2)];
    assert_eq!(plan_with(&findings, None).escalations.len(), 1);
    let plan = plan_with(&findings, Some(vec![C2XrefMissing]));
    assert!(plan.escalations.is_empty());
    assert_eq!(plan.candidates, [Resave]);
}

#[test]
fn passes_never_unlock_an_encrypted_file() {
    let mut findings = corruption(&[C2XrefMissing]);
    findings.push(finding(FindingKind::Encrypted, 1));
    assert!(
        plan_with(&findings, Some(vec![C2XrefMissing]))
            .candidates
            .is_empty()
    );
}
