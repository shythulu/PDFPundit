//! UI state (T-20): everything the screen is drawn from. The loop owns one
//! [`AppState`] and fills it from job events, the history store, `Resize` and
//! the wall clock; [`super::view::view`] turns it into what the layouts draw.
// The loop (T-23a), the modals (T-24) and the picker (T-37) build these.
// TODO(T-23a, T-24, T-37): remove this allow once they do.
#![allow(dead_code)]

use crate::jobs::BatchState;
use crate::library::HistorySummary;

/// The app's state, as the loop keeps it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AppState {
    /// The dropped files, in drop order.
    pub batch: BatchState,
    /// From `HistoryStore::summary()`, refilled at start and after every run
    /// record (eng-r3-q4).
    pub history: HistorySummary,
    /// Columns × rows, from every `Resize`.
    pub term_size: (u16, u16),
    /// `(hour, minute)`, set by the loop from the wall clock; `None` until the
    /// first reading.
    pub clock: Option<(u8, u8)>,
    pub screen: Screen,
    /// The queue row the cursor is on. `None` follows the runner's current
    /// file; an index past the end is ignored.
    pub selected: Option<usize>,
    /// A one-off hint for the hint row (D-048's "not yet", D-064's refusal).
    pub hint: Option<&'static str>,
    /// The per-file menu (D-048, frame 05) is open over the selected file,
    /// the cursor on item `n` of `strings::FILE_MENU_ITEMS` (T-22b).
    pub file_menu: Option<usize>,
}

/// Which screen is up.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    Main,
    /// The `browse…` picker (T-37).
    Browse(BrowseState),
    /// The font-pick modal (T-24) for the parked queue entry `entry`, the
    /// cursor on candidate `selected`.
    FontPick { entry: usize, selected: usize },
    /// The theme chooser (T-24), the cursor on theme `selected`.
    Themes { selected: usize },
}

/// The browse picker's state. A placeholder: T-37 gives it its fields
/// (`cwd`, `entries`, `cursor`, `selected`, `filter`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BrowseState {}

#[cfg(test)]
pub(crate) mod fixtures {
    //! The mockup's sample data (generate.py), so the goldens test layout, not
    //! product content (D-048, eng-r3-q4).

    use std::path::PathBuf;

    use super::AppState;
    use crate::engine::{
        AnalysisResult, AnalysisStateUse, AnalyzeStats, CarveSummary, CorruptionClass, FileMeta,
        Finding, FindingKind, FontCandidate, FontDb, FontPickRequest, FontResolution,
        FontResolutionKind, FontSlot, InteractionRequest, InteractionRequestId, Location,
        OutcomeStatus, PassOutcome, PassReport, Ratio, RepairOptions, RepairReport, Repairability,
        Severity, StateHandle, SubstituteChoice, ToUnicodeState,
    };
    use crate::jobs::{BatchState, EntryState, JobId, Placed, QueueEntry, RepairRun};
    use crate::library::{HistorySummary, RecentRow, RecentStatus};

    impl AppState {
        /// Frame 01, the idle screen: an empty queue, the mockup's history
        /// (five recent files, 57 files, 91 runs), 112 × 38 at 11:38.
        pub(crate) fn mockup_idle() -> AppState {
            let recent = [
                ("thesis_ar.pdf", RecentStatus::Repaired),
                ("minutes_q3.pdf", RecentStatus::Repaired),
                ("invoice_scan.pdf", RecentStatus::Partial),
                ("contract_signed.pdf", RecentStatus::Pending),
                ("payroll_locked.pdf", RecentStatus::Failed),
            ];
            AppState {
                history: HistorySummary {
                    files: 57,
                    runs: 91,
                    recent: recent
                        .iter()
                        .map(|&(name, status)| RecentRow {
                            name: name.into(),
                            status,
                        })
                        .collect(),
                },
                term_size: (112, 38),
                clock: Some((11, 38)),
                ..AppState::default()
            }
        }

        /// Frame 03, the batch view at 11:42: seven files, three repaired or
        /// clean, `thesis_ar.pdf` parked on a font question,
        /// `invoice_scan.pdf` 71% through its C9 salvage, `board_deck.pdf`
        /// queued and `payroll_locked.pdf` encrypted. The file sizes put the
        /// batch at 52% here and at 79% in [`AppState::mockup_result`].
        pub(crate) fn mockup_batch() -> AppState {
            let mut thesis = entry(2, "thesis_ar.pdf", THESIS_BYTES);
            thesis.meta = Some(meta("1.6", 214));
            thesis.findings = thesis_findings();
            thesis.font_resolutions = thesis_fonts(None, None);
            thesis.state = EntryState::WaitingOnUser(InteractionRequest::FontPick(font_question()));

            let mut invoice = entry(4, "invoice_scan.pdf", INVOICE_BYTES);
            invoice.meta = Some(meta("1.4", 6));
            invoice.findings = invoice_findings();
            invoice.state = EntryState::Repairing {
                phase: Some("C9 salvage"),
                done: 71,
                total: Some(100),
            };

            let entries = vec![
                report_2024(),
                contract_signed(),
                thesis,
                minutes_q3(),
                invoice,
                entry(5, "board_deck.pdf", BOARD_BYTES),
                payroll_locked(),
            ];
            AppState {
                batch: BatchState {
                    entries,
                    current: Some(4),
                },
                term_size: (112, 38),
                clock: Some((11, 42)),
                ..AppState::default()
            }
        }

        /// Frame 05, the result view at 11:46: `thesis_ar.pdf` selected and
        /// repaired, `invoice_scan.pdf` partial (88% of a stream salvaged) and
        /// `board_deck.pdf` 34% through its repair.
        pub(crate) fn mockup_result() -> AppState {
            let mut thesis = entry(2, "thesis_ar.pdf", THESIS_BYTES);
            thesis.meta = Some(meta("1.6", 214));
            thesis.findings = thesis_findings();
            let picked = FontResolutionKind::Picked {
                font_id: "noto-naskh-arabic".into(),
                confidence: Ratio { num: 31, den: 100 },
            };
            let substituted = FontResolutionKind::Substituted(SubstituteChoice {
                font_id: "noto-sans".into(),
                label: "Noto Sans".into(),
            });
            thesis.font_resolutions = thesis_fonts(Some(picked), Some(substituted));
            let passes = [
                CorruptionClass::C2XrefMissing,
                CorruptionClass::C8FontResourcesDeleted,
                CorruptionClass::C6FontMapLost,
            ];
            thesis.state = EntryState::Done;
            thesis.run = Some(run(&thesis, fixed(&passes), OutcomeStatus::Ok));

            let mut invoice = entry(4, "invoice_scan.pdf", INVOICE_BYTES);
            invoice.meta = Some(meta("1.4", 6));
            invoice.findings = invoice_findings();
            let mut c9 = fixed(&[CorruptionClass::C3TrailerDamaged]);
            c9.push(PassReport {
                class: CorruptionClass::C9ZlibTampered,
                outcome: PassOutcome::Partial("88% salvaged".into()),
                actions: Vec::new(),
            });
            invoice.state = EntryState::Done;
            invoice.run = Some(run(
                &invoice,
                c9,
                OutcomeStatus::Partial(vec!["88% salvaged".into()]),
            ));

            let mut board = entry(5, "board_deck.pdf", BOARD_BYTES);
            board.state = EntryState::Repairing {
                phase: None,
                done: 34,
                total: Some(100),
            };

            let entries = vec![
                report_2024(),
                contract_signed(),
                thesis,
                minutes_q3(),
                invoice,
                board,
                payroll_locked(),
            ];
            AppState {
                batch: BatchState {
                    entries,
                    current: Some(5),
                },
                term_size: (112, 38),
                clock: Some((11, 46)),
                selected: Some(2),
                ..AppState::default()
            }
        }

        /// The widget's working state (frame 07): frame 03's batch with
        /// `thesis_ar.pdf` back in the queue and `invoice_scan.pdf` 30%
        /// through its salvage, so three of seven files are done and the
        /// byte-weighted progress (1837/4400) fills ten of the bar's 24 cells,
        /// as the mockup's 43% does.
        pub(crate) fn mockup_widget_working() -> AppState {
            let mut app = AppState::mockup_batch();
            app.batch.entries[2].state = EntryState::Queued;
            app.batch.entries[4].state = EntryState::Repairing {
                phase: Some("C9 salvage"),
                done: 30,
                total: Some(100),
            };
            app.term_size = (32, 16);
            app
        }

        /// The widget's needs-you state (frame 07): frame 03's batch with
        /// `invoice_scan.pdf` repaired, so four of seven are done and
        /// `thesis_ar.pdf` waits on its font question.
        pub(crate) fn mockup_widget_needs() -> AppState {
            let mut app = AppState::mockup_batch();
            let invoice = &mut app.batch.entries[4];
            invoice.state = EntryState::Done;
            invoice.run = Some(run(
                invoice,
                fixed(&[CorruptionClass::C9ZlibTampered]),
                OutcomeStatus::Ok,
            ));
            app.batch.current = None;
            app.term_size = (32, 16);
            app
        }

        /// The finished batch the widget's done state shows: six repaired,
        /// one partial, one failed ("6√ 1~ 1×", "7 done").
        pub(crate) fn mockup_done() -> AppState {
            let mut entries: Vec<QueueEntry> = (0..6)
                .map(|i| {
                    let mut e = entry(i, &format!("file_{i}.pdf"), 100_000);
                    e.state = EntryState::Done;
                    let passes = fixed(&[CorruptionClass::C2XrefMissing]);
                    e.run = Some(run(&e, passes, OutcomeStatus::Ok));
                    e
                })
                .collect();
            let mut partial = entry(7, "invoice_scan.pdf", INVOICE_BYTES);
            partial.state = EntryState::Done;
            partial.run = Some(run(
                &partial,
                Vec::new(),
                OutcomeStatus::Partial(vec!["88% salvaged".into()]),
            ));
            entries.push(partial);
            entries.push(payroll_locked());
            AppState {
                batch: BatchState {
                    entries,
                    current: None,
                },
                term_size: (32, 16),
                clock: Some((11, 52)),
                ..AppState::default()
            }
        }
    }

    // Sizes chosen so the batch's byte-weighted progress is exactly 52% in
    // frame 03 and 79% in frame 05; the four small files make up the rest.
    const INVOICE_BYTES: u64 = 1_100_000;
    const THESIS_BYTES: u64 = 393_000;
    const BOARD_BYTES: u64 = 1_400_000;

    fn entry(job: u64, name: &str, bytes: u64) -> QueueEntry {
        let path = PathBuf::from("evidence").join(name);
        QueueEntry::queued(JobId(job), name.into(), Some(path), bytes)
    }

    fn meta(version: &str, pages: u32) -> FileMeta {
        FileMeta {
            version: Some(version.into()),
            pages,
            title: None,
            page_sizes: Vec::new(),
        }
    }

    pub(crate) fn finding(
        id: &str,
        class: FindingKind,
        severity: Severity,
        summary: &str,
    ) -> Finding {
        Finding {
            id: id.into(),
            class,
            severity,
            location: Location::File,
            summary: summary.into(),
            evidence: Vec::new(),
            repair: match severity {
                Severity::Info => Repairability::NotApplicable,
                _ => Repairability::Auto,
            },
        }
    }

    fn corruption(class: CorruptionClass, severity: Severity) -> Finding {
        let id = format!("{}-001", class.code());
        finding(&id, FindingKind::Corruption(class), severity, class.label())
    }

    fn invoice_findings() -> Vec<Finding> {
        let mut c9 = corruption(CorruptionClass::C9ZlibTampered, Severity::Error);
        c9.location = Location::Object {
            id: (14, 0),
            span: None,
        };
        vec![
            c9,
            corruption(CorruptionClass::C3TrailerDamaged, Severity::Warning),
            info("header intact"),
        ]
    }

    fn thesis_findings() -> Vec<Finding> {
        vec![
            corruption(CorruptionClass::C2XrefMissing, Severity::Error),
            corruption(CorruptionClass::C8FontResourcesDeleted, Severity::Error),
            corruption(CorruptionClass::C6FontMapLost, Severity::Warning),
            info("header intact"),
        ]
    }

    fn info(summary: &str) -> Finding {
        finding(
            "C1-001",
            FindingKind::Corruption(CorruptionClass::C1Header),
            Severity::Info,
            summary,
        )
    }

    /// F1 embedded and intact; F3 and F7 lost their fonts.
    fn thesis_fonts(
        f3: Option<FontResolutionKind>,
        f7: Option<FontResolutionKind>,
    ) -> Vec<FontSlot> {
        let slot = |slot: &str,
                    base: Option<&str>,
                    embedded: bool,
                    kind: Option<FontResolutionKind>| FontSlot {
            page: 11,
            slot: slot.into(),
            base_font: base.map(Into::into),
            subtype: Some("/Type0".into()),
            embedded,
            tounicode: ToUnicodeState::Present,
            glyph_count: 418,
            resolution: kind.map(|kind| FontResolution {
                kind,
                provenance: Vec::new(),
            }),
        };
        vec![
            slot("F1", Some("/TimesNewRomanPSMT"), true, None),
            slot("F3", Some("/CIDFont+F1"), false, f3),
            slot("F7", None, false, f7),
        ]
    }

    fn font_question() -> FontPickRequest {
        FontPickRequest {
            id: InteractionRequestId(1),
            page: 11,
            slot: "F3".into(),
            sample_codes: Vec::new(),
            candidates: vec![FontCandidate {
                font_id: "noto-naskh-arabic".into(),
                family: "Noto Naskh Arabic".into(),
                language: "ar".into(),
                score: Ratio { num: 64, den: 100 },
                confidence: Ratio { num: 31, den: 100 },
                preview: String::new(),
            }],
            preview: String::new(),
        }
    }

    fn fixed(classes: &[CorruptionClass]) -> Vec<PassReport> {
        classes
            .iter()
            .map(|&class| PassReport {
                class,
                outcome: PassOutcome::Fixed,
                actions: Vec::new(),
            })
            .collect()
    }

    /// A finished repair of `e` with `passes`, re-diagnosed clean.
    fn run(e: &QueueEntry, passes: Vec<PassReport>, status: OutcomeStatus) -> RepairRun {
        let analysis = AnalysisResult {
            meta: e.meta.clone().unwrap_or_else(|| meta("1.4", 1)),
            findings: e.findings.clone(),
            carve: CarveSummary::default(),
            font_slots: e.font_resolutions.clone(),
            stats: AnalyzeStats {
                bytes: e.bytes,
                ..AnalyzeStats::default()
            },
            input_sha256: [0; 32],
            state: StateHandle::default(),
        };
        let mut report =
            RepairReport::default_for(&analysis, &RepairOptions::default(), &FontDb::empty());
        report.passes = passes;
        let output_path = e.path.as_ref().map(|p| p.with_extension("repaired.pdf"));
        RepairRun {
            report,
            status,
            output_path,
            placed: Some(Placed::Atomic),
            analysis_state: AnalysisStateUse::Reused,
        }
    }

    fn repaired(job: u64, name: &str, bytes: u64, classes: &[CorruptionClass]) -> QueueEntry {
        let mut e = entry(job, name, bytes);
        e.findings = classes
            .iter()
            .map(|&c| corruption(c, Severity::Error))
            .collect();
        e.state = EntryState::Done;
        e.run = Some(run(&e, fixed(classes), OutcomeStatus::Ok));
        e
    }

    fn report_2024() -> QueueEntry {
        repaired(
            0,
            "report_2024.pdf",
            512_000,
            &[
                CorruptionClass::C2XrefMissing,
                CorruptionClass::C3TrailerDamaged,
                CorruptionClass::C4PageTreeBroken,
            ],
        )
    }

    fn contract_signed() -> QueueEntry {
        let mut e = repaired(1, "contract_signed.pdf", 245_000, &[]);
        e.findings = vec![finding(
            "SIG-001",
            FindingKind::Signed { fields: 1 },
            Severity::Info,
            "digitally signed",
        )];
        e
    }

    fn minutes_q3() -> QueueEntry {
        repaired(
            3,
            "minutes_q3.pdf",
            180_000,
            &[CorruptionClass::C3TrailerDamaged],
        )
    }

    fn payroll_locked() -> QueueEntry {
        let mut e = entry(6, "payroll_locked.pdf", 570_000);
        e.findings = vec![finding(
            "ENC-001",
            FindingKind::Encrypted,
            Severity::Error,
            "encrypted",
        )];
        e.state = EntryState::Done;
        e.run = Some(run(
            &e,
            Vec::new(),
            OutcomeStatus::Failed("encrypted".into()),
        ));
        e
    }
}
