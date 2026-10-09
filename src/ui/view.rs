//! The view model (T-20): [`view`] derives everything the layouts draw from
//! the [`AppState`], so the full and widget layouts never derive the same
//! state twice. Pure: no clock, no I/O, the same state gives the same model.
//!
//! The idle golden draws LAST CALLERS, "57 files · 91 runs", "history 57
//! files" and the status bar's `node 1 │ idle │ 0 queued │ offline` and
//! `112×38 │ 11:38`, so those are view data too (eng-r3-q4, D-048).
#![cfg_attr(not(test), allow(dead_code))]

use super::color::Rgb;
use super::director::Mood;
use super::state::AppState;
use super::strings;
use super::theme::Roles;
use crate::engine::{
    Finding, FindingKind, FontResolutionKind, InteractionRequest, Location, OutcomeStatus,
    PassOutcome, Ratio, RepairReport, Severity,
};
use crate::jobs::{EntryState, QueueEntry};
use crate::library::{RecentRow, RecentStatus};

/// What the layouts draw.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewModel {
    pub counts: Counts,
    /// The file a job is working on now.
    pub current: Option<Current>,
    /// How far the batch is, weighted by file size; `None` with no files.
    pub batch_progress: Option<Ratio>,
    /// `(files waiting on you, the first one's name)`.
    pub needs_you: Option<(usize, String)>,
    /// Set once every file is finished.
    pub done_summary: Option<Tally>,
    /// One row per dropped file, in drop order.
    pub queue_rows: Vec<QueueRow>,
    /// The selected file's findings (the analysis or result panel).
    pub selected_findings: Vec<FindingRow>,
    /// The selected file's font slots.
    pub font_resolutions: Vec<FontRow>,
    /// The selected file's C9 count line (D-041), when a damaged stream was
    /// found.
    pub c9_line: Option<String>,
    pub mood: Mood,
    /// At most five, newest first.
    pub recent: Vec<RecentRow>,
    /// `(files, runs)` in the history.
    pub history_totals: (u64, u64),
    pub status_bar: StatusBar,
    /// A one-off hint for the hint row.
    pub hint: Option<&'static str>,
    /// The per-file menu is open, the cursor on this item (T-22b).
    pub file_menu: Option<usize>,
}

/// Files per state. Every file is in exactly one of the last six counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    pub total: usize,
    /// Repaired, or clean with nothing to fix.
    pub ok: usize,
    pub partial: usize,
    /// Failed, encrypted or cancelled.
    pub failed: usize,
    /// Parked on a question.
    pub needs_input: usize,
    /// A job is running on it.
    pub working: usize,
    /// Waiting for a job (dropped, or analysed and not yet repaired).
    pub queued: usize,
}

impl Counts {
    /// Finished with an output: the mockup's "5/7 done".
    pub fn done(&self) -> usize {
        self.ok + self.partial
    }
}

/// The file a job is running on.
#[derive(Debug, Clone, PartialEq)]
pub struct Current {
    /// Its index in the queue.
    pub index: usize,
    pub name: String,
    pub activity: Activity,
    /// The job's phase, e.g. "C9 salvage".
    pub phase: Option<&'static str>,
    /// `None` while the job has not said how much there is to do.
    pub progress: Option<Ratio>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Analysing,
    Repairing,
    Exporting,
}

impl Activity {
    pub fn label(self) -> &'static str {
        match self {
            Activity::Analysing => "analysing",
            Activity::Repairing => "repairing",
            Activity::Exporting => "exporting",
        }
    }
}

/// How a finished batch came out: the widget's "6√ 1~ 1×".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tally {
    pub ok: u32,
    pub partial: u32,
    pub failed: u32,
}

/// One file in the queue box: glyph, name, `label · detail`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueRow {
    pub name: String,
    pub kind: RowKind,
    /// "repaired", "queued", "needs input", …
    pub label: &'static str,
    /// "3 fixed", "2 fonts", "C9 salvage", "decrypt first", …
    pub detail: Option<String>,
    /// The result view's shorter form of `detail` (frame 05): the same, except
    /// that a clean file drops "nothing to fix" and reads just "clean", and an
    /// encrypted one drops "decrypt first" (frame 05's last queue row, whose
    /// column 49 shows past the file menu's edge only on that condition).
    /// The result view gives a failed file's full `detail` on the progress
    /// box's bottom edge instead.
    pub short_detail: Option<String>,
    /// The cursor is on this row.
    pub selected: bool,
}

/// What a queue row's glyph shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Ok,
    Partial,
    Failed,
    NeedsInput,
    Working,
    Queued,
}

impl RowKind {
    pub fn glyph(self) -> char {
        match self {
            RowKind::Ok => '√',
            RowKind::Partial => '~',
            RowKind::Failed => '×',
            RowKind::NeedsInput => '‼',
            RowKind::Working => '☼',
            RowKind::Queued => '∙',
        }
    }

    pub fn role(self) -> Role {
        match self {
            RowKind::Ok => Role::Ok,
            RowKind::Partial => Role::Warn,
            RowKind::Failed => Role::Error,
            RowKind::NeedsInput => Role::NeedsInput,
            RowKind::Working => Role::File,
            RowKind::Queued => Role::Dim,
        }
    }

    /// The glyph blinks while something is happening or waiting on you.
    pub fn blinks(self) -> bool {
        matches!(self, RowKind::NeedsInput | RowKind::Working)
    }
}

/// A colour role of the theme (`ui::theme::Roles`), named for what it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// `ok` (G).
    Ok,
    /// `hotkey`, which is also the warning colour (Y).
    Warn,
    /// `error` (R).
    Error,
    /// `needs_input` (M).
    NeedsInput,
    /// `file` (C).
    File,
    /// `body` (w).
    Body,
    /// `dim` (D).
    Dim,
    /// `info` (c).
    Info,
}

impl Role {
    pub fn rgb(self, roles: &Roles) -> Rgb {
        match self {
            Role::Ok => roles.ok,
            Role::Warn => roles.hotkey,
            Role::Error => roles.error,
            Role::NeedsInput => roles.needs_input,
            Role::File => roles.file,
            Role::Body => roles.body,
            Role::Dim => roles.dim,
            Role::Info => roles.info,
        }
    }
}

/// One finding of the selected file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingRow {
    pub level: Level,
    /// The corruption class code ("C9") on error and warning rows.
    pub code: Option<&'static str>,
    pub summary: String,
    pub location: Location,
    /// After a repair: whether the re-diagnosis still finds it. `None` before
    /// a repair and on info rows.
    pub fixed: Option<bool>,
}

/// How a finding is shown: `[ERR]`, `[WRN]` or `[iNF]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Info,
    Warning,
    Error,
}

impl Level {
    pub fn role(self) -> Role {
        match self {
            Level::Info => Role::Info,
            Level::Warning => Role::Warn,
            Level::Error => Role::Error,
        }
    }
}

/// One font slot of the selected file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontRow {
    /// The resource name, e.g. "F3".
    pub slot: String,
    /// The base font without its leading `/`; `None` when unknown.
    pub name: Option<String>,
    pub embedded: bool,
    /// `None`: nothing was done to it (intact, or not resolved yet).
    pub resolution: Option<FontResolutionKind>,
}

/// The bottom row: `node 1 │ idle │ 0 queued │ offline` … `112×38 │ 11:38`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusBar {
    /// Always 1: the BBS joke.
    pub node: u16,
    /// "idle", "busy" or "needs you".
    pub state: &'static str,
    /// Files still to be worked on (queued or running).
    pub queued: usize,
    /// Always true: the binary has no network code (D-050).
    pub offline: bool,
    pub size: (u16, u16),
    pub clock: Option<(u8, u8)>,
}

/// The view model of `app`.
pub fn view(app: &AppState) -> ViewModel {
    let entries = &app.batch.entries;
    let classes: Vec<Class> = entries.iter().map(class_of).collect();
    let counts = count(&classes);
    let selected = app
        .selected
        .filter(|&i| i < entries.len())
        .or(app.batch.current.filter(|&i| i < entries.len()));
    let selected_entry = selected.map(|i| &entries[i]);
    let batch_progress = batch_progress(entries, &classes);
    let needs_you = entries
        .iter()
        .zip(&classes)
        .find(|(_, c)| **c == Class::NeedsInput)
        .map(|(e, _)| (counts.needs_input, e.name.clone()));
    // A parked file is not finished (`EntryState::is_finished`), so the
    // batch is done only once nothing is working, queued or parked.
    let all_finished =
        !entries.is_empty() && counts.working + counts.queued + counts.needs_input == 0;
    let tally = Tally {
        ok: to_u32(counts.ok),
        partial: to_u32(counts.partial),
        failed: to_u32(counts.failed),
    };
    let mood = if entries.is_empty() {
        Mood::Idle
    } else if counts.needs_input > 0 {
        Mood::NeedsYou
    } else if !all_finished {
        Mood::Working {
            progress: batch_progress.unwrap_or(Ratio { num: 0, den: 1 }),
        }
    } else if counts.failed > 0 && counts.done() == 0 {
        Mood::Failed
    } else {
        Mood::Done {
            ok: tally.ok,
            partial: tally.partial,
            failed: tally.failed,
        }
    };
    let state = if counts.needs_input > 0 {
        strings::STATE_NEEDS_YOU
    } else if counts.working + counts.queued > 0 {
        strings::STATE_BUSY
    } else {
        strings::STATE_IDLE
    };
    ViewModel {
        counts,
        current: current(app),
        batch_progress,
        needs_you,
        done_summary: all_finished.then_some(tally),
        queue_rows: entries
            .iter()
            .enumerate()
            .map(|(i, e)| queue_row(e, selected == Some(i)))
            .collect(),
        selected_findings: selected_entry.map_or_else(Vec::new, finding_rows),
        font_resolutions: selected_entry.map_or_else(Vec::new, font_rows),
        c9_line: selected_entry.and_then(c9_line),
        mood,
        recent: app.history.recent.iter().take(5).cloned().collect(),
        history_totals: (app.history.files, app.history.runs),
        status_bar: StatusBar {
            node: 1,
            state,
            queued: counts.working + counts.queued,
            offline: true,
            size: app.term_size,
            clock: app.clock,
        },
        hint: app.hint,
        file_menu: app.file_menu,
    }
}

/// How a finished repair shows in LAST CALLERS and the history store
/// (`FileEntry::status`), from its `RunRecord`'s report and status.
///
/// Failed or encrypted is `Failed`; then a partial outcome or any partial
/// pass is `Partial`; otherwise `Repaired`. A skipped pass is neutral: the
/// engine skips a pass that has nothing to do (`Skipped("no orphaned
/// font")`), and the outcome status says whether the file came out partial.
/// For a file with no run, see [`recent_status_of`].
pub fn recent_status(report: &RepairReport, status: &OutcomeStatus) -> RecentStatus {
    let encrypted = report
        .findings_before
        .iter()
        .any(|f| f.class == FindingKind::Encrypted);
    let partial_pass = report
        .passes
        .iter()
        .any(|p| matches!(p.outcome, PassOutcome::Partial(_)));
    match status {
        OutcomeStatus::Failed(_) => RecentStatus::Failed,
        _ if encrypted => RecentStatus::Failed,
        OutcomeStatus::Partial(_) => RecentStatus::Partial,
        OutcomeStatus::Ok if partial_pass => RecentStatus::Partial,
        OutcomeStatus::Ok => RecentStatus::Repaired,
    }
}

/// The whole `RecentStatus` table for one queue entry, so the loop and the
/// history writer share one mapping:
///
/// - a finished repair: [`recent_status`] of its report and status;
/// - a failed job, or an encrypted file with no run: `Failed`;
/// - an analysis but no run yet (repair queued, running or parked, or the
///   batch was left before it ran): `Pending`;
/// - nothing analysed yet: `None`, there is nothing to record.
pub fn recent_status_of(e: &QueueEntry) -> Option<RecentStatus> {
    if let Some(run) = &e.run {
        return Some(recent_status(&run.report, &run.status));
    }
    let analysed = e.meta.is_some() || e.state == EntryState::Done;
    match &e.state {
        EntryState::Failed { .. } => Some(RecentStatus::Failed),
        _ if analysed && encrypted(e) => Some(RecentStatus::Failed),
        _ if analysed => Some(RecentStatus::Pending),
        _ => None,
    }
}

impl RecentStatus {
    pub fn glyph(self) -> char {
        match self {
            RecentStatus::Repaired => '√',
            RecentStatus::Partial => '~',
            RecentStatus::Pending => '·',
            RecentStatus::Failed => '×',
        }
    }

    /// The idle golden draws the pending glyph in body text, not dim as the
    /// plan's T-20 text says; the golden is the authority.
    pub fn role(self) -> Role {
        match self {
            RecentStatus::Repaired => Role::Ok,
            RecentStatus::Partial => Role::Warn,
            RecentStatus::Pending => Role::Body,
            RecentStatus::Failed => Role::Error,
        }
    }
}

/// `r` as a whole percentage, rounded down: what the progress rows print.
/// A zero denominator reads as 0.
pub fn percent(r: Ratio) -> u64 {
    if r.den == 0 {
        return 0;
    }
    let p = u128::from(r.num) * 100 / u128::from(r.den);
    u64::try_from(p).unwrap_or(u64::MAX)
}

// ── derivation ───────────────────────────────────────────────────────────

/// Where one file is, for counting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Ok,
    Partial,
    Failed,
    NeedsInput,
    Working,
    Queued,
}

fn class_of(e: &QueueEntry) -> Class {
    match &e.state {
        EntryState::Queued => Class::Queued,
        EntryState::Analyzing { .. }
        | EntryState::Repairing { .. }
        | EntryState::Exporting { .. } => Class::Working,
        EntryState::WaitingOnUser(_) => Class::NeedsInput,
        EntryState::Failed { .. } | EntryState::Cancelled => Class::Failed,
        EntryState::Done => match recent_status_of(e) {
            Some(RecentStatus::Repaired) => Class::Ok,
            Some(RecentStatus::Partial) => Class::Partial,
            Some(RecentStatus::Failed) => Class::Failed,
            // Analysed, repair not started: the runner queues the repair with
            // the analysis (GG §1), so this never lasts and the file counts
            // as still to do.
            Some(RecentStatus::Pending) | None => Class::Queued,
        },
    }
}

fn encrypted(e: &QueueEntry) -> bool {
    e.findings.iter().any(|f| f.class == FindingKind::Encrypted)
}

fn count(classes: &[Class]) -> Counts {
    let n = |c: Class| classes.iter().filter(|&&x| x == c).count();
    Counts {
        total: classes.len(),
        ok: n(Class::Ok),
        partial: n(Class::Partial),
        failed: n(Class::Failed),
        needs_input: n(Class::NeedsInput),
        working: n(Class::Working),
        queued: n(Class::Queued),
    }
}

fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `done` of `total`, capped at 1; `None` without a total.
fn fraction(done: u64, total: Option<u64>) -> Option<Ratio> {
    match total {
        Some(den) if den > 0 => Some(Ratio {
            num: done.min(den),
            den,
        }),
        _ => None,
    }
}

/// The batch's progress weighted by file size: every finished file counts in
/// full, a running one by its job's fraction, a parked or queued one not at
/// all. With no bytes at all (empty files) every file weighs the same.
fn batch_progress(entries: &[QueueEntry], classes: &[Class]) -> Option<Ratio> {
    if entries.is_empty() {
        return None;
    }
    let total_bytes: u128 = entries.iter().map(|e| u128::from(e.bytes)).sum();
    let weight = |e: &QueueEntry| {
        if total_bytes == 0 {
            1
        } else {
            u128::from(e.bytes)
        }
    };
    let den: u128 = entries.iter().map(weight).sum();
    let num: u128 = entries
        .iter()
        .zip(classes)
        .map(|(e, c)| match c {
            Class::Ok | Class::Partial | Class::Failed => weight(e),
            Class::Working => e
                .state
                .progress()
                .and_then(|(done, total)| fraction(done, total))
                .map_or(0, |r| weight(e) * u128::from(r.num) / u128::from(r.den)),
            Class::NeedsInput | Class::Queued => 0,
        })
        .sum();
    Some(narrow(num, den))
}

/// `num/den` as a [`Ratio`], halving both until they fit in `u64`.
fn narrow(mut num: u128, mut den: u128) -> Ratio {
    while den > u128::from(u64::MAX) {
        num >>= 1;
        den >>= 1;
    }
    Ratio {
        num: u64::try_from(num).unwrap_or(u64::MAX),
        den: u64::try_from(den).unwrap_or(u64::MAX),
    }
}

fn current(app: &AppState) -> Option<Current> {
    let index = app.batch.current?;
    let e = app.batch.entries.get(index)?;
    let (activity, phase, done, total) = match &e.state {
        EntryState::Analyzing { phase, done, total } => (Activity::Analysing, *phase, done, total),
        EntryState::Repairing { phase, done, total } => (Activity::Repairing, *phase, done, total),
        EntryState::Exporting { done, total } => (Activity::Exporting, None, done, total),
        _ => return None,
    };
    Some(Current {
        index,
        name: e.name.clone(),
        activity,
        phase,
        progress: fraction(*done, *total),
    })
}

fn queue_row(e: &QueueEntry, selected: bool) -> QueueRow {
    let RowStatus {
        kind,
        label,
        detail,
        short_detail,
    } = row_status(e);
    QueueRow {
        name: e.name.clone(),
        kind,
        label,
        detail,
        short_detail,
        selected,
    }
}

/// A queue row's status: the long form (FiLE QUEUE) and the short form (the
/// result view's rows), which drops a detail that says nothing.
struct RowStatus {
    kind: RowKind,
    label: &'static str,
    detail: Option<String>,
    short_detail: Option<String>,
}

impl RowStatus {
    /// A status whose detail reads the same in both forms.
    fn new(kind: RowKind, label: &'static str, detail: Option<String>) -> Self {
        RowStatus {
            kind,
            label,
            short_detail: detail.clone(),
            detail,
        }
    }

    /// A clean file: "nothing to fix" in the queue, no detail in the short form.
    fn clean() -> Self {
        RowStatus {
            kind: RowKind::Ok,
            label: "clean",
            detail: Some("nothing to fix".into()),
            short_detail: None,
        }
    }
}

/// `n font` or `n fonts`.
fn fonts(n: usize) -> String {
    let unit = if n == 1 { "font" } else { "fonts" };
    format!("{n} {unit}")
}

fn row_status(e: &QueueEntry) -> RowStatus {
    let finished = e.state.is_finished();
    if finished && encrypted(e) {
        return encrypted_row();
    }
    match &e.state {
        EntryState::Queued => RowStatus::new(RowKind::Queued, "queued", None),
        EntryState::Analyzing { phase, .. } => RowStatus::new(
            RowKind::Working,
            Activity::Analysing.label(),
            phase.map(Into::into),
        ),
        EntryState::Repairing { phase, .. } => RowStatus::new(
            RowKind::Working,
            Activity::Repairing.label(),
            phase.map(Into::into),
        ),
        EntryState::Exporting { .. } => {
            RowStatus::new(RowKind::Working, Activity::Exporting.label(), None)
        }
        EntryState::WaitingOnUser(req) => {
            let n = match req {
                InteractionRequest::FontUnreproducible(r) => r.slots.len(),
                InteractionRequest::FontPick(_) => e
                    .font_resolutions
                    .iter()
                    .filter(|s| s.resolution.is_none() && !s.embedded)
                    .count(),
            }
            .max(1);
            RowStatus::new(RowKind::NeedsInput, "needs input", Some(fonts(n)))
        }
        EntryState::Failed { error, .. } => {
            RowStatus::new(RowKind::Failed, "failed", Some(error.clone()))
        }
        EntryState::Cancelled => RowStatus::new(RowKind::Failed, "cancelled", None),
        EntryState::Done => match &e.run {
            None => RowStatus::new(RowKind::Queued, "analysed", None),
            Some(run) => match &run.status {
                OutcomeStatus::Failed(why) => {
                    RowStatus::new(RowKind::Failed, "failed", Some(why.clone()))
                }
                status => {
                    let fixed = run
                        .report
                        .passes
                        .iter()
                        .filter(|p| p.outcome == PassOutcome::Fixed)
                        .count();
                    // Frame 05: a repair that resolved fonts names them
                    // ("repaired · 2 fonts"), any other names its passes.
                    let resolved = e
                        .font_resolutions
                        .iter()
                        .filter(|s| s.resolution.is_some())
                        .count();
                    match recent_status(&run.report, status) {
                        RecentStatus::Repaired if fixed == 0 => RowStatus::clean(),
                        RecentStatus::Repaired if resolved > 0 => {
                            RowStatus::new(RowKind::Ok, "repaired", Some(fonts(resolved)))
                        }
                        RecentStatus::Repaired => {
                            RowStatus::new(RowKind::Ok, "repaired", Some(format!("{fixed} fixed")))
                        }
                        RecentStatus::Partial => {
                            RowStatus::new(RowKind::Partial, "partial", partial_reason(run))
                        }
                        // With a run that did not fail, `recent_status` says
                        // `Failed` only for a report that found encryption,
                        // and never says `Pending`; both get the failed row so
                        // the row and `class_of`'s count cannot disagree.
                        RecentStatus::Failed | RecentStatus::Pending => encrypted_row(),
                    }
                }
            },
        },
    }
}

/// An encrypted file: "decrypt first" in the queue, no detail in the short
/// form.
fn encrypted_row() -> RowStatus {
    RowStatus {
        kind: RowKind::Failed,
        label: "encrypted",
        detail: Some("decrypt first".into()),
        short_detail: None,
    }
}

/// The first reason the repair gives for being partial.
fn partial_reason(run: &crate::jobs::RepairRun) -> Option<String> {
    let from_status = match &run.status {
        OutcomeStatus::Partial(reasons) => reasons.first().cloned(),
        _ => None,
    };
    from_status.or_else(|| {
        run.report.passes.iter().find_map(|p| match &p.outcome {
            PassOutcome::Partial(why) => Some(why.clone()),
            PassOutcome::Fixed | PassOutcome::Skipped(_) => None,
        })
    })
}

/// Signatures, outlined text, Type 3 text and fonts never embedded are
/// notes, whatever severity they carry; encryption is always an error.
fn level_of(f: &Finding) -> Level {
    match f.class {
        FindingKind::Signed { .. }
        | FindingKind::OutlinedText { .. }
        | FindingKind::Type3Text { .. }
        | FindingKind::FontNotEmbedded { .. } => Level::Info,
        FindingKind::Encrypted => Level::Error,
        FindingKind::Corruption(_) => match f.severity {
            Severity::Info => Level::Info,
            Severity::Warning => Level::Warning,
            Severity::Error => Level::Error,
        },
    }
}

fn finding_rows(e: &QueueEntry) -> Vec<FindingRow> {
    // Only a repair that produced an output was re-diagnosed; a failed one
    // has an empty `findings_after` that must not read as everything fixed.
    let after = e
        .run
        .as_ref()
        .filter(|r| !matches!(r.status, OutcomeStatus::Failed(_)))
        .map(|r| &r.report.findings_after);
    e.findings
        .iter()
        .map(|f| {
            let level = level_of(f);
            let code = match (&f.class, level) {
                (FindingKind::Corruption(c), Level::Warning | Level::Error) => Some(c.code()),
                _ => None,
            };
            // Ids are renumbered when the output is re-diagnosed, so a finding
            // survives the repair when one of the same kind is still there.
            let fixed = match (after, level) {
                (Some(after), Level::Warning | Level::Error) => {
                    Some(!after.iter().any(|a| a.class == f.class))
                }
                _ => None,
            };
            FindingRow {
                level,
                code,
                summary: f.summary.clone(),
                location: f.location,
                fixed,
            }
        })
        .collect()
}

fn font_rows(e: &QueueEntry) -> Vec<FontRow> {
    e.font_resolutions
        .iter()
        .map(|s| FontRow {
            slot: s.slot.clone(),
            name: s
                .base_font
                .as_ref()
                .map(|n| n.strip_prefix('/').unwrap_or(n).to_string()),
            embedded: s.embedded,
            resolution: s.resolution.as_ref().map(|r| r.kind.clone()),
        })
        .collect()
}

/// D-041's fixed count line, when the repair found a damaged C9 stream.
fn c9_line(e: &QueueEntry) -> Option<String> {
    let c9 = &e.run.as_ref()?.report.c9_summary;
    (c9.streams_damaged > 0).then(|| {
        format!(
            "{} streams repaired; {} of them unique within the searched window; \
             {} accepted without a uniqueness check",
            c9.repaired, c9.exact, c9.accepted
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{C9Summary, CorruptionClass};
    use crate::jobs::{BatchState, JobId};
    use crate::ui::state::fixtures::finding;

    fn ratio(num: u64, den: u64) -> Ratio {
        Ratio { num, den }
    }

    /// `label · detail`, as the batch view prints a queue row.
    fn row_text(r: &QueueRow) -> String {
        match &r.detail {
            Some(d) => format!("{} · {d}", r.label),
            None => r.label.to_string(),
        }
    }

    /// `label · short_detail`, as the result view prints a queue row.
    fn short_text(r: &QueueRow) -> String {
        match &r.short_detail {
            Some(d) => format!("{} · {d}", r.label),
            None => r.label.to_string(),
        }
    }

    /// The characters of `golden`'s row `y` from column `x0` up to `x1`,
    /// trailing blanks trimmed.
    fn golden_text(golden: &crate::ui::goldens::Golden, x0: u16, x1: u16, y: u16) -> String {
        use crate::ui::goldens::GoldenCell;
        let s: String = (x0..x1)
            .map(|x| match golden.cell(x, y) {
                GoldenCell::Text { ch, .. } => ch,
                _ => '?',
            })
            .collect();
        s.trim_end().to_string()
    }

    #[test]
    fn idle_fixture_gives_the_mockups_idle_screen() {
        let vm = view(&AppState::mockup_idle());
        assert_eq!(vm.counts, Counts::default());
        assert_eq!(vm.current, None);
        assert_eq!(vm.batch_progress, None);
        assert_eq!(vm.needs_you, None);
        assert_eq!(vm.done_summary, None);
        assert!(vm.queue_rows.is_empty());
        assert!(vm.selected_findings.is_empty());
        assert!(vm.font_resolutions.is_empty());
        assert_eq!(vm.c9_line, None);
        assert_eq!(vm.mood, Mood::Idle);
        let recent: Vec<(&str, char, Role)> = vm
            .recent
            .iter()
            .map(|r| (r.name.as_str(), r.status.glyph(), r.status.role()))
            .collect();
        assert_eq!(
            recent,
            [
                ("thesis_ar.pdf", '√', Role::Ok),
                ("minutes_q3.pdf", '√', Role::Ok),
                ("invoice_scan.pdf", '~', Role::Warn),
                ("contract_signed.pdf", '·', Role::Body),
                ("payroll_locked.pdf", '×', Role::Error),
            ]
        );
        assert_eq!(vm.history_totals, (57, 91));
        assert_eq!(
            vm.status_bar,
            StatusBar {
                node: 1,
                state: "idle",
                queued: 0,
                offline: true,
                size: (112, 38),
                clock: Some((11, 38)),
            }
        );
        assert_eq!(vm.hint, None);
    }

    #[test]
    fn batch_fixture_gives_frame_03() {
        let vm = view(&AppState::mockup_batch());
        assert_eq!(
            vm.counts,
            Counts {
                total: 7,
                ok: 3,
                partial: 0,
                failed: 1,
                needs_input: 1,
                working: 1,
                queued: 1,
            }
        );
        // "3/7 done · 1 needs input · 1 failed"
        assert_eq!(vm.counts.done(), 3);
        assert_eq!(
            vm.current,
            Some(Current {
                index: 4,
                name: "invoice_scan.pdf".into(),
                activity: Activity::Repairing,
                phase: Some("C9 salvage"),
                progress: Some(ratio(71, 100)),
            })
        );
        assert_eq!(
            vm.current.as_ref().and_then(|c| c.progress).map(percent),
            Some(71)
        );
        assert_eq!(vm.batch_progress, Some(ratio(52, 100)));
        assert_eq!(vm.batch_progress.map(percent), Some(52));
        assert_eq!(vm.needs_you, Some((1, "thesis_ar.pdf".into())));
        assert_eq!(vm.done_summary, None);

        let rows: Vec<(char, &str, String, bool)> = vm
            .queue_rows
            .iter()
            .map(|r| (r.kind.glyph(), r.name.as_str(), row_text(r), r.selected))
            .collect();
        let want = [
            ('√', "report_2024.pdf", "repaired · 3 fixed", false),
            ('√', "contract_signed.pdf", "clean · nothing to fix", false),
            ('‼', "thesis_ar.pdf", "needs input · 2 fonts", false),
            ('√', "minutes_q3.pdf", "repaired · 1 fixed", false),
            ('☼', "invoice_scan.pdf", "repairing · C9 salvage", true),
            ('∙', "board_deck.pdf", "queued", false),
            (
                '×',
                "payroll_locked.pdf",
                "encrypted · decrypt first",
                false,
            ),
        ];
        let want: Vec<(char, &str, String, bool)> = want
            .iter()
            .map(|&(g, n, t, s)| (g, n, t.to_string(), s))
            .collect();
        assert_eq!(rows, want);
        let blinking: Vec<&str> = vm
            .queue_rows
            .iter()
            .filter(|r| r.kind.blinks())
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(blinking, ["thesis_ar.pdf", "invoice_scan.pdf"]);

        // The analysis panel follows the current file.
        let findings: Vec<(Level, Option<&str>, &str, Option<bool>)> = vm
            .selected_findings
            .iter()
            .map(|f| (f.level, f.code, f.summary.as_str(), f.fixed))
            .collect();
        assert_eq!(
            findings,
            [
                (Level::Error, Some("C9"), "zlib stream damaged", None),
                (Level::Warning, Some("C3"), "trailer missing", None),
                (Level::Info, None, "header intact", None),
            ]
        );
        assert_eq!(
            vm.selected_findings[0].location,
            Location::Object {
                id: (14, 0),
                span: None
            }
        );
        assert!(vm.font_resolutions.is_empty());
        assert_eq!(vm.c9_line, None);
        assert_eq!(vm.mood, Mood::NeedsYou);
        assert_eq!(vm.status_bar.state, "needs you");
        assert_eq!(vm.status_bar.queued, 2);
        assert_eq!(vm.status_bar.clock, Some((11, 42)));
    }

    #[test]
    fn result_fixture_gives_frame_05() {
        let vm = view(&AppState::mockup_result());
        // "5/7 done · 1 failed"
        assert_eq!(
            vm.counts,
            Counts {
                total: 7,
                ok: 4,
                partial: 1,
                failed: 1,
                needs_input: 0,
                working: 1,
                queued: 0,
            }
        );
        assert_eq!(vm.counts.done(), 5);
        let current = vm.current.as_ref().expect("board_deck.pdf is repairing");
        assert_eq!(
            (
                current.name.as_str(),
                current.activity.label(),
                current.phase
            ),
            ("board_deck.pdf", "repairing", None)
        );
        assert_eq!(current.progress.map(percent), Some(34));
        assert_eq!(vm.batch_progress, Some(ratio(79, 100)));
        assert_eq!(vm.needs_you, None);
        assert_eq!(vm.done_summary, None);

        let rows: Vec<(char, String, bool)> = vm
            .queue_rows
            .iter()
            .map(|r| (r.kind.glyph(), row_text(r), r.selected))
            .collect();
        let want = [
            ('√', "repaired · 3 fixed", false),
            ('√', "clean · nothing to fix", false),
            ('√', "repaired · 2 fonts", true),
            ('√', "repaired · 1 fixed", false),
            ('~', "partial · 88% salvaged", false),
            ('☼', "repairing", false),
            ('×', "encrypted · decrypt first", false),
        ];
        let want: Vec<(char, String, bool)> = want
            .iter()
            .map(|&(g, t, s)| (g, t.to_string(), s))
            .collect();
        assert_eq!(rows, want);
        // The result view's short form drops the clean and encrypted files'
        // details.
        let short: Vec<String> = vm.queue_rows.iter().map(short_text).collect();
        assert_eq!(
            short,
            [
                "repaired · 3 fixed",
                "clean",
                "repaired · 2 fonts",
                "repaired · 1 fixed",
                "partial · 88% salvaged",
                "repairing",
                "encrypted",
            ]
        );

        // The result panel shows the selected file, before and after.
        let findings: Vec<(Level, Option<&str>, &str, Option<bool>)> = vm
            .selected_findings
            .iter()
            .map(|f| (f.level, f.code, f.summary.as_str(), f.fixed))
            .collect();
        assert_eq!(
            findings,
            [
                (Level::Error, Some("C2"), "xref table missing", Some(true)),
                (
                    Level::Error,
                    Some("C8"),
                    "font resources deleted",
                    Some(true)
                ),
                (Level::Warning, Some("C6"), "font mapping lost", Some(true)),
                (Level::Info, None, "header intact", None),
            ]
        );
        let fonts: Vec<(&str, Option<&str>, bool, bool)> = vm
            .font_resolutions
            .iter()
            .map(|f| {
                (
                    f.slot.as_str(),
                    f.name.as_deref(),
                    f.embedded,
                    f.resolution.is_some(),
                )
            })
            .collect();
        assert_eq!(
            fonts,
            [
                ("F1", Some("TimesNewRomanPSMT"), true, false),
                ("F3", Some("CIDFont+F1"), false, true),
                ("F7", None, false, true),
            ]
        );
        assert!(matches!(
            vm.font_resolutions[1].resolution,
            Some(FontResolutionKind::Picked { .. })
        ));
        assert_eq!(vm.c9_line, None);
        assert_eq!(
            vm.mood,
            Mood::Working {
                progress: ratio(79, 100)
            }
        );
        assert_eq!(vm.status_bar.state, "busy");
        assert_eq!(vm.status_bar.queued, 1);
    }

    #[test]
    fn done_fixture_gives_the_tally() {
        let vm = view(&AppState::mockup_done());
        let tally = Tally {
            ok: 6,
            partial: 1,
            failed: 1,
        };
        assert_eq!(vm.done_summary, Some(tally));
        assert_eq!(
            vm.mood,
            Mood::Done {
                ok: 6,
                partial: 1,
                failed: 1
            }
        );
        // "7 done"
        assert_eq!(vm.counts.done(), 7);
        assert_eq!(vm.current, None);
        assert_eq!(vm.batch_progress, Some(ratio(1, 1)));
        assert_eq!(vm.status_bar.state, "idle");
        assert_eq!(vm.status_bar.queued, 0);
        assert_eq!(vm.status_bar.size, (32, 16));
    }

    // ── mood ─────────────────────────────────────────────────────────────

    fn state_of(entries: Vec<QueueEntry>) -> AppState {
        AppState {
            batch: BatchState {
                entries,
                current: None,
            },
            ..AppState::default()
        }
    }

    fn with_state(job: u64, state: EntryState) -> QueueEntry {
        let mut e = QueueEntry::queued(JobId(job), format!("f{job}.pdf"), None, 100);
        e.state = state;
        e
    }

    fn finished(job: u64, status: OutcomeStatus) -> QueueEntry {
        let src = AppState::mockup_done();
        let mut e = src.batch.entries[0].clone();
        e.job = JobId(job);
        e.bytes = 100;
        if let Some(run) = &mut e.run {
            run.status = status;
        }
        e
    }

    fn parked(job: u64) -> QueueEntry {
        let src = AppState::mockup_batch();
        let mut e = src.batch.entries[2].clone();
        e.job = JobId(job);
        e
    }

    fn repairing(done: u64) -> EntryState {
        EntryState::Repairing {
            phase: None,
            done,
            total: Some(100),
        }
    }

    #[test]
    fn mood_derivation_table() {
        let failed = || OutcomeStatus::Failed("no".into());
        let partial = || OutcomeStatus::Partial(vec!["half".into()]);
        let table: Vec<(&str, Vec<QueueEntry>, Mood)> = vec![
            ("no files", vec![], Mood::Idle),
            (
                "one queued",
                vec![with_state(1, EntryState::Queued)],
                Mood::Working {
                    progress: ratio(0, 1),
                },
            ),
            (
                "half done",
                vec![finished(1, OutcomeStatus::Ok), with_state(2, repairing(50))],
                Mood::Working {
                    progress: ratio(3, 4),
                },
            ),
            (
                "parked beats working",
                vec![parked(1), with_state(2, repairing(10))],
                Mood::NeedsYou,
            ),
            (
                "parked with the rest finished",
                vec![finished(1, OutcomeStatus::Ok), parked(2)],
                Mood::NeedsYou,
            ),
            (
                "all finished",
                vec![
                    finished(1, OutcomeStatus::Ok),
                    finished(2, partial()),
                    finished(3, failed()),
                ],
                Mood::Done {
                    ok: 1,
                    partial: 1,
                    failed: 1,
                },
            ),
            (
                "everything failed",
                vec![
                    finished(1, failed()),
                    with_state(
                        2,
                        EntryState::Failed {
                            error: "boom".into(),
                            panicked: true,
                        },
                    ),
                    with_state(3, EntryState::Cancelled),
                ],
                Mood::Failed,
            ),
            (
                "analysed, repair not started",
                vec![
                    finished(1, OutcomeStatus::Ok),
                    with_state(2, EntryState::Done),
                ],
                Mood::Working {
                    progress: ratio(1, 2),
                },
            ),
        ];
        for (what, entries, mood) in table {
            assert_eq!(view(&state_of(entries)).mood, mood, "{what}");
        }
    }

    #[test]
    fn status_bar_state_follows_the_batch() {
        let cases = [
            (vec![], "idle", 0),
            (vec![with_state(1, EntryState::Queued)], "busy", 1),
            (
                vec![parked(1), with_state(2, EntryState::Queued)],
                "needs you",
                1,
            ),
            (vec![finished(1, OutcomeStatus::Ok)], "idle", 0),
        ];
        for (entries, state, queued) in cases {
            let bar = view(&state_of(entries)).status_bar;
            assert_eq!((bar.state, bar.queued, bar.offline), (state, queued, true));
        }
    }

    #[test]
    fn a_parked_file_keeps_the_batch_from_being_done() {
        let vm = view(&state_of(vec![finished(1, OutcomeStatus::Ok), parked(2)]));
        assert_eq!(vm.done_summary, None);
        assert_eq!(vm.mood, Mood::NeedsYou);
    }

    #[test]
    fn the_done_tally_covers_every_file() {
        let vm = view(&state_of(vec![
            finished(1, OutcomeStatus::Ok),
            finished(2, OutcomeStatus::Partial(vec!["half".into()])),
            finished(3, OutcomeStatus::Failed("no".into())),
            with_state(4, EntryState::Cancelled),
        ]));
        let tally = vm.done_summary.expect("a finished batch has a tally");
        let sum = tally.ok + tally.partial + tally.failed;
        assert_eq!(usize::try_from(sum).ok(), Some(vm.counts.total));
    }

    #[test]
    fn an_encrypted_report_gives_the_failed_row_and_count() {
        // The report found encryption the entry's own findings miss: the row
        // and the count must still agree.
        let mut e = finished(1, OutcomeStatus::Ok);
        e.findings.retain(|f| f.class != FindingKind::Encrypted);
        if let Some(run) = &mut e.run {
            run.report = report_with(&[PassOutcome::Fixed], true);
        }
        let vm = view(&state_of(vec![e]));
        assert_eq!(vm.queue_rows[0].kind, RowKind::Failed);
        assert_eq!(vm.queue_rows[0].label, "encrypted");
        assert_eq!(vm.counts.failed, 1);
    }

    #[test]
    fn a_clean_row_drops_its_detail_only_in_the_short_form() {
        let mut e = finished(1, OutcomeStatus::Ok);
        if let Some(run) = &mut e.run {
            run.report = report_with(&[], false);
        }
        let vm = view(&state_of(vec![e]));
        let row = &vm.queue_rows[0];
        assert_eq!(row.label, "clean");
        assert_eq!(row.detail.as_deref(), Some("nothing to fix"));
        assert_eq!(row.short_detail, None);
    }

    // ── RecentStatus ─────────────────────────────────────────────────────

    fn report_with(passes: &[PassOutcome], encrypted: bool) -> RepairReport {
        let src = AppState::mockup_done();
        let mut report = src.batch.entries[0]
            .run
            .as_ref()
            .expect("a run")
            .report
            .clone();
        report.passes = passes
            .iter()
            .map(|outcome| crate::engine::PassReport {
                class: CorruptionClass::C2XrefMissing,
                outcome: outcome.clone(),
                actions: Vec::new(),
            })
            .collect();
        if encrypted {
            report.findings_before.push(finding(
                "ENC-001",
                FindingKind::Encrypted,
                Severity::Error,
                "encrypted",
            ));
        }
        report
    }

    #[test]
    fn recent_status_table() {
        use PassOutcome::{Fixed, Partial as P, Skipped};
        use RecentStatus::{Failed, Partial, Repaired};
        let ok = OutcomeStatus::Ok;
        let partial = OutcomeStatus::Partial(vec!["88% salvaged".into()]);
        let failed = OutcomeStatus::Failed("no candidate passed".into());
        let p = || P("ambiguous: 2 candidate repairs".into());
        let s = || Skipped("no orphaned font".into());
        let table: Vec<(&str, Vec<PassOutcome>, bool, &OutcomeStatus, RecentStatus)> = vec![
            ("all fixed, ok", vec![Fixed, Fixed], false, &ok, Repaired),
            ("clean, ok", vec![], false, &ok, Repaired),
            ("a partial pass", vec![Fixed, p()], false, &ok, Partial),
            ("partial outcome", vec![Fixed], false, &partial, Partial),
            (
                "a skipped pass is neutral",
                vec![Fixed, s()],
                false,
                &ok,
                Repaired,
            ),
            (
                "skipped, partial outcome",
                vec![s()],
                false,
                &partial,
                Partial,
            ),
            ("failed", vec![], false, &failed, Failed),
            ("failed beats partial", vec![p()], false, &failed, Failed),
            ("encrypted", vec![], true, &failed, Failed),
            ("encrypted beats ok", vec![Fixed], true, &ok, Failed),
            ("encrypted beats partial", vec![p()], true, &partial, Failed),
        ];
        for (what, passes, encrypted, status, want) in table {
            let report = report_with(&passes, encrypted);
            assert_eq!(recent_status(&report, status), want, "{what}");
        }
    }

    /// The table's per-entry rows: a run maps through `recent_status`, an
    /// analysis with no run is `Pending`, nothing analysed records nothing.
    #[test]
    fn recent_status_of_an_entry() {
        let analysed = |state: EntryState| {
            let mut e = with_state(1, state);
            e.meta = AppState::mockup_batch().batch.entries[2].meta.clone();
            e
        };
        let mut locked = analysed(EntryState::Done);
        locked.findings = AppState::mockup_batch().batch.entries[6].findings.clone();
        let table: Vec<(&str, QueueEntry, Option<RecentStatus>)> = vec![
            ("queued", with_state(1, EntryState::Queued), None),
            (
                "analysing",
                with_state(
                    1,
                    EntryState::Analyzing {
                        phase: None,
                        done: 0,
                        total: None,
                    },
                ),
                None,
            ),
            (
                "analysis only",
                analysed(EntryState::Done),
                Some(RecentStatus::Pending),
            ),
            (
                "analysed, repairing",
                analysed(repairing(10)),
                Some(RecentStatus::Pending),
            ),
            ("parked", parked(1), Some(RecentStatus::Pending)),
            ("encrypted, no run", locked, Some(RecentStatus::Failed)),
            (
                "job failed",
                with_state(
                    1,
                    EntryState::Failed {
                        error: "boom".into(),
                        panicked: false,
                    },
                ),
                Some(RecentStatus::Failed),
            ),
            (
                "repaired",
                finished(1, OutcomeStatus::Ok),
                Some(RecentStatus::Repaired),
            ),
            (
                "partial",
                finished(1, OutcomeStatus::Partial(vec!["half".into()])),
                Some(RecentStatus::Partial),
            ),
            (
                "failed run",
                finished(1, OutcomeStatus::Failed("no".into())),
                Some(RecentStatus::Failed),
            ),
        ];
        for (what, e, want) in table {
            assert_eq!(recent_status_of(&e), want, "{what}");
        }
    }

    #[test]
    fn recent_status_glyphs_and_roles() {
        let table = [
            (RecentStatus::Repaired, '√', Role::Ok),
            (RecentStatus::Partial, '~', Role::Warn),
            (RecentStatus::Pending, '·', Role::Body),
            (RecentStatus::Failed, '×', Role::Error),
        ];
        for (status, glyph, role) in table {
            assert_eq!((status.glyph(), status.role()), (glyph, role), "{status:?}");
        }
    }

    #[test]
    fn row_and_level_roles_match_the_mockup() {
        // generate.py's QUEUE rows and finding tags.
        let rows = [
            (RowKind::Ok, Role::Ok, false),
            (RowKind::Partial, Role::Warn, false),
            (RowKind::Failed, Role::Error, false),
            (RowKind::NeedsInput, Role::NeedsInput, true),
            (RowKind::Working, Role::File, true),
            (RowKind::Queued, Role::Dim, false),
        ];
        for (kind, role, blinks) in rows {
            assert_eq!((kind.role(), kind.blinks()), (role, blinks), "{kind:?}");
        }
        let levels = [
            (Level::Error, Role::Error),
            (Level::Warning, Role::Warn),
            (Level::Info, Role::Info),
        ];
        for (level, role) in levels {
            assert_eq!(level.role(), role, "{level:?}");
        }
    }

    #[test]
    fn roles_pick_the_themes_colours() {
        let roles = &crate::ui::theme::Theme::default_theme().roles;
        let table = [
            (Role::Ok, roles.ok),
            (Role::Warn, roles.hotkey),
            (Role::Error, roles.error),
            (Role::NeedsInput, roles.needs_input),
            (Role::File, roles.file),
            (Role::Body, roles.body),
            (Role::Dim, roles.dim),
            (Role::Info, roles.info),
        ];
        for (role, rgb) in table {
            assert_eq!(role.rgb(roles), rgb, "{role:?}");
        }
    }

    #[test]
    fn recent_glyphs_and_roles_match_the_idle_golden() {
        use crate::ui::goldens::{self, GoldenCell};
        let golden = goldens::load("01-idle");
        let roles = &crate::ui::theme::Theme::default_theme().roles;
        // LAST CALLERS: one row per recent file from (3, 10), glyph first.
        for (i, row) in view(&AppState::mockup_idle()).recent.iter().enumerate() {
            let y = 10 + u16::try_from(i).expect("five rows");
            let GoldenCell::Text { ch, fg, .. } = golden.cell(3, y) else {
                panic!("LAST CALLERS row {i} is not text");
            };
            let want = (row.status.glyph(), row.status.role().rgb(roles).0);
            assert_eq!((ch, fg.0), want, "{}", row.name);
        }
    }

    #[test]
    fn queue_glyphs_and_roles_match_the_batch_and_result_goldens() {
        use crate::ui::goldens::{self, GoldenCell};
        let roles = &crate::ui::theme::Theme::default_theme().roles;
        for (frame, app) in [
            ("03-batch", AppState::mockup_batch()),
            ("05-result", AppState::mockup_result()),
        ] {
            let golden = goldens::load(frame);
            // The QUEUE box's rows start at (3, 4), glyph first.
            for (i, row) in view(&app).queue_rows.iter().enumerate() {
                let y = 4 + u16::try_from(i).expect("seven rows");
                let GoldenCell::Text { ch, fg, .. } = golden.cell(3, y) else {
                    panic!("{frame}: QUEUE row {i} is not text");
                };
                let want = (row.kind.glyph(), row.kind.role().rgb(roles).0);
                assert_eq!((ch, fg.0), want, "{frame}: {}", row.name);
                let blinks = golden.blink.contains(&(3, y));
                assert_eq!(blinks, row.kind.blinks(), "{frame}: {} blink", row.name);
            }
        }
    }

    #[test]
    fn queue_text_matches_the_batch_golden() {
        let golden = crate::ui::goldens::load("03-batch");
        // Name from column 6, `label · detail` from column 28, inside the box.
        for (i, row) in view(&AppState::mockup_batch())
            .queue_rows
            .iter()
            .enumerate()
        {
            let y = 4 + u16::try_from(i).expect("seven rows");
            assert_eq!(golden_text(&golden, 6, 27, y), row.name, "row {i}");
            assert_eq!(golden_text(&golden, 28, 64, y), row_text(row), "row {i}");
        }
    }

    #[test]
    fn queue_text_matches_the_result_golden() {
        let golden = crate::ui::goldens::load("05-result");
        let vm = view(&AppState::mockup_result());
        // Name from column 6, `label · short_detail` from column 27. The FiLE
        // menu covers rows 7 on from column 17, so only the first eleven
        // characters of their names show.
        for (i, row) in vm.queue_rows.iter().enumerate() {
            let y = 4 + u16::try_from(i).expect("seven rows");
            if y < 7 {
                assert_eq!(golden_text(&golden, 6, 26, y), row.name, "row {i}");
                assert_eq!(golden_text(&golden, 27, 49, y), short_text(row), "row {i}");
            } else {
                let prefix: String = row.name.chars().take(11).collect();
                assert_eq!(golden_text(&golden, 6, 17, y), prefix, "row {i}");
            }
        }
    }

    #[test]
    fn recent_is_capped_at_five() {
        let mut app = AppState::mockup_idle();
        let extra = app.history.recent[0].clone();
        app.history.recent.push(extra);
        assert_eq!(view(&app).recent.len(), 5);
    }

    // ── findings ─────────────────────────────────────────────────────────

    #[test]
    fn info_kinds_render_as_info_not_errors() {
        let kinds = [
            FindingKind::Signed { fields: 2 },
            FindingKind::OutlinedText {
                glyph_runs: 40,
                contours: 900,
            },
            FindingKind::Type3Text { font: (12, 0) },
            FindingKind::FontNotEmbedded {
                font: (5, 0),
                base_font: "Arial".into(),
            },
        ];
        let mut app = AppState::mockup_batch();
        let invoice = &mut app.batch.entries[4];
        // Even when a finding carries a higher severity than its kind allows.
        invoice.findings = kinds
            .into_iter()
            .map(|k| finding("X-001", k, Severity::Error, "note"))
            .collect();
        invoice.findings.push(finding(
            "ENC-001",
            FindingKind::Encrypted,
            Severity::Error,
            "encrypted",
        ));
        let rows = view(&app).selected_findings;
        let levels: Vec<(Level, Option<&str>)> = rows.iter().map(|r| (r.level, r.code)).collect();
        assert_eq!(
            levels,
            [
                (Level::Info, None),
                (Level::Info, None),
                (Level::Info, None),
                (Level::Info, None),
                (Level::Error, None),
            ]
        );
    }

    #[test]
    fn a_finding_still_present_after_the_repair_is_not_fixed() {
        let mut app = AppState::mockup_result();
        let thesis = &mut app.batch.entries[2];
        let still = thesis.findings[2].clone();
        thesis
            .run
            .as_mut()
            .expect("a run")
            .report
            .findings_after
            .push(still);
        let fixed: Vec<Option<bool>> = view(&app)
            .selected_findings
            .iter()
            .map(|f| f.fixed)
            .collect();
        assert_eq!(fixed, [Some(true), Some(true), Some(false), None]);
    }

    #[test]
    fn a_failed_repair_fixes_nothing() {
        // payroll_locked.pdf: its run failed and wrote nothing to re-diagnose.
        let mut app = AppState::mockup_result();
        app.selected = Some(6);
        let rows = view(&app).selected_findings;
        let fixed: Vec<(Level, Option<bool>)> = rows.iter().map(|f| (f.level, f.fixed)).collect();
        assert_eq!(fixed, [(Level::Error, None)]);

        // Any failed run, whatever it found.
        let mut app = AppState::mockup_result();
        let thesis = &mut app.batch.entries[2];
        thesis.run.as_mut().expect("a run").status = OutcomeStatus::Failed("no".into());
        assert!(
            view(&app)
                .selected_findings
                .iter()
                .all(|f| f.fixed.is_none())
        );
    }

    // ── c9_line ──────────────────────────────────────────────────────────

    #[test]
    fn c9_line_is_none_without_a_damaged_stream() {
        // No run yet, and a run that touched no C9 stream.
        assert_eq!(view(&AppState::mockup_batch()).c9_line, None);
        assert_eq!(view(&AppState::mockup_result()).c9_line, None);
    }

    #[test]
    fn c9_line_carries_the_three_counts() {
        let mut app = AppState::mockup_result();
        app.selected = Some(4);
        let invoice = &mut app.batch.entries[4];
        invoice.run.as_mut().expect("a run").report.c9_summary = C9Summary {
            streams_damaged: 4,
            repaired: 3,
            exact: 2,
            accepted: 1,
            ambiguous: 0,
            unrecoverable: 1,
            unsearched: 0,
        };
        assert_eq!(
            view(&app).c9_line.as_deref(),
            Some(
                "3 streams repaired; 2 of them unique within the searched window; \
                 1 accepted without a uniqueness check"
            )
        );
    }

    // ── selection and progress ───────────────────────────────────────────

    #[test]
    fn a_selection_past_the_end_follows_the_current_file() {
        let mut app = AppState::mockup_batch();
        app.selected = Some(99);
        let vm = view(&app);
        let selected: Vec<&str> = vm
            .queue_rows
            .iter()
            .filter(|r| r.selected)
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(selected, ["invoice_scan.pdf"]);
        assert_eq!(vm.selected_findings.len(), 3);
    }

    #[test]
    fn nothing_is_selected_with_no_current_file_and_no_cursor() {
        let vm = view(&AppState::mockup_done());
        assert!(vm.queue_rows.iter().all(|r| !r.selected));
        assert!(vm.selected_findings.is_empty());
    }

    #[test]
    fn progress_without_a_total_counts_as_nothing_done() {
        let entries = vec![
            finished(1, OutcomeStatus::Ok),
            with_state(
                2,
                EntryState::Analyzing {
                    phase: Some("carving"),
                    done: 7,
                    total: None,
                },
            ),
        ];
        let vm = view(&state_of(entries));
        assert_eq!(vm.current, None, "no current index was set");
        assert_eq!(vm.batch_progress, Some(ratio(1, 2)));
        let row = &vm.queue_rows[1];
        assert_eq!(
            (row.kind, row_text(row)),
            (RowKind::Working, "analysing · carving".into())
        );
    }

    #[test]
    fn empty_files_weigh_by_count() {
        let mut a = finished(1, OutcomeStatus::Ok);
        a.bytes = 0;
        let b = with_state(2, EntryState::Queued);
        let mut b = b;
        b.bytes = 0;
        assert_eq!(
            view(&state_of(vec![a, b])).batch_progress,
            Some(ratio(1, 2))
        );
    }

    #[test]
    fn percent_rounds_down() {
        assert_eq!(percent(ratio(2, 3)), 66);
        assert_eq!(percent(ratio(1, 1)), 100);
        assert_eq!(percent(ratio(0, 0)), 0);
    }

    #[test]
    fn view_is_a_pure_function_of_the_state() {
        for app in [
            AppState::mockup_idle(),
            AppState::mockup_batch(),
            AppState::mockup_result(),
            AppState::mockup_done(),
        ] {
            assert_eq!(view(&app), view(&app.clone()));
        }
    }

    #[test]
    fn the_hint_passes_through() {
        let mut app = AppState::mockup_idle();
        app.hint = Some("not yet");
        assert_eq!(view(&app).hint, Some("not yet"));
    }
}
