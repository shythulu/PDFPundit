//! Core types (T-02): what the engine finds in a PDF and how it reports it.
//!
//! Every type here is `serde` + `Debug + Clone + PartialEq + Send + Sync` (the
//! `const` block below checks it). Nothing here holds a map, so no serialised
//! map has a non-string key (plan §3.1), and every fraction is an integer
//! [`Ratio`], never a float.

use std::cmp::Ordering;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// An object's identity: `(number, generation)`, the same pair as
/// `lopdf::ObjectId`. Serialises as a two-element array.
pub type ObjId = (u32, u16);

/// The ten corruption classes, C1 to C10. The derived order is [`Self::ALL`]'s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CorruptionClass {
    C1Header,
    C2XrefMissing,
    C3TrailerDamaged,
    C4PageTreeBroken,
    C5ObjectTagStripped,
    C6FontMapLost,
    C7FontStreamDeleted,
    C8FontResourcesDeleted,
    C9ZlibTampered,
    C10Truncated,
}

impl CorruptionClass {
    /// Every class, C1 first, C10 last.
    pub const ALL: [CorruptionClass; 10] = [
        CorruptionClass::C1Header,
        CorruptionClass::C2XrefMissing,
        CorruptionClass::C3TrailerDamaged,
        CorruptionClass::C4PageTreeBroken,
        CorruptionClass::C5ObjectTagStripped,
        CorruptionClass::C6FontMapLost,
        CorruptionClass::C7FontStreamDeleted,
        CorruptionClass::C8FontResourcesDeleted,
        CorruptionClass::C9ZlibTampered,
        CorruptionClass::C10Truncated,
    ];

    /// The order repair passes run in. C1 to C3 have no pass of their own: the
    /// re-emission of the file fixes them.
    pub const REPAIR_ORDER: [CorruptionClass; 7] = [
        CorruptionClass::C9ZlibTampered,
        CorruptionClass::C10Truncated,
        CorruptionClass::C5ObjectTagStripped,
        CorruptionClass::C4PageTreeBroken,
        CorruptionClass::C6FontMapLost,
        CorruptionClass::C7FontStreamDeleted,
        CorruptionClass::C8FontResourcesDeleted,
    ];

    /// `"C1"` to `"C10"`; also the prefix of a corruption finding's id.
    pub fn code(self) -> &'static str {
        match self {
            CorruptionClass::C1Header => "C1",
            CorruptionClass::C2XrefMissing => "C2",
            CorruptionClass::C3TrailerDamaged => "C3",
            CorruptionClass::C4PageTreeBroken => "C4",
            CorruptionClass::C5ObjectTagStripped => "C5",
            CorruptionClass::C6FontMapLost => "C6",
            CorruptionClass::C7FontStreamDeleted => "C7",
            CorruptionClass::C8FontResourcesDeleted => "C8",
            CorruptionClass::C9ZlibTampered => "C9",
            CorruptionClass::C10Truncated => "C10",
        }
    }

    /// A short lower-case description, as the result view shows it.
    pub fn label(self) -> &'static str {
        match self {
            CorruptionClass::C1Header => "header damaged",
            CorruptionClass::C2XrefMissing => "xref table missing",
            CorruptionClass::C3TrailerDamaged => "trailer missing",
            CorruptionClass::C4PageTreeBroken => "page tree broken",
            CorruptionClass::C5ObjectTagStripped => "object tags stripped",
            CorruptionClass::C6FontMapLost => "font mapping lost",
            CorruptionClass::C7FontStreamDeleted => "font stream deleted",
            CorruptionClass::C8FontResourcesDeleted => "font resources deleted",
            CorruptionClass::C9ZlibTampered => "zlib stream damaged",
            CorruptionClass::C10Truncated => "file truncated",
        }
    }
}

/// What a [`Finding`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindingKind {
    Corruption(CorruptionClass),
    /// `/Encrypt` present: unrepairable until decrypted.
    Encrypted,
    /// A `/Type /Sig` dict or a `/ByteRange` array is present; the repaired file
    /// carries no valid signature (D-052). Info only.
    Signed {
        fields: u32,
    },
    /// Text drawn as filled paths. Info only.
    OutlinedText {
        glyph_runs: u32,
        contours: u32,
    },
    /// Text drawn by a Type 3 font. Info only.
    Type3Text {
        font: ObjId,
    },
}

/// Ordered `Info < Warning < Error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// A half-open byte range `start..end` of the original file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ByteSpan {
    pub start: u64,
    pub end: u64,
}

/// Where in the file a finding was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Location {
    File,
    Span(ByteSpan),
    Object {
        id: ObjId,
        span: Option<ByteSpan>,
    },
    /// `index` counts from 0 in document order.
    Page {
        index: u32,
        obj: Option<ObjId>,
    },
}

/// Up to [`HexWindow::MAX_BYTES`] bytes of the original file starting at `at`.
/// The cap holds for every value: [`HexWindow::new`] truncates and
/// deserialising a longer window fails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawHexWindow")]
pub struct HexWindow {
    at: u64,
    bytes: Vec<u8>,
}

#[derive(Deserialize)]
struct RawHexWindow {
    at: u64,
    bytes: Vec<u8>,
}

impl TryFrom<RawHexWindow> for HexWindow {
    type Error = String;

    fn try_from(raw: RawHexWindow) -> Result<Self, Self::Error> {
        if raw.bytes.len() > HexWindow::MAX_BYTES {
            return Err(format!(
                "a hex window holds at most {} bytes, got {}",
                HexWindow::MAX_BYTES,
                raw.bytes.len()
            ));
        }
        Ok(HexWindow {
            at: raw.at,
            bytes: raw.bytes,
        })
    }
}

impl HexWindow {
    pub const MAX_BYTES: usize = 64;

    /// The window over `slice`, which starts at file offset `at`; bytes past the
    /// first 64 are dropped.
    pub fn new(at: u64, slice: &[u8]) -> Self {
        let keep = slice.len().min(Self::MAX_BYTES);
        HexWindow {
            at,
            bytes: slice[..keep].to_vec(),
        }
    }

    pub fn at(&self) -> u64 {
        self.at
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// The value of an [`Evidence::Metric`]: a signed count, offset or delta; an
/// exact fraction; or a name such as a salvage outcome. Serialises untagged
/// (`12`, `{"num":7,"den":10}`, `"Repaired"`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MetricValue {
    Int(i64),
    Ratio(Ratio),
    Text(String),
}

/// What a finding rests on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Evidence {
    Text(String),
    HexWindow(HexWindow),
    ObjectRef(ObjId),
    Metric { name: String, value: MetricValue },
}

/// One diagnosis. `id` is `<code>-<nnn>` (e.g. `C2-001`), numbered by diagnose
/// in byte order, so it is stable for the same input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub class: FindingKind,
    pub severity: Severity,
    pub location: Location,
    pub summary: String,
    pub evidence: Vec<Evidence>,
    pub repair: Repairability,
}

/// What repairing a finding takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Repairability {
    Auto,
    /// Needs an answer from the user before the pass runs (D-020).
    Interactive(InteractionKind),
    /// Best effort; the string says what is lost.
    Partial(String),
    Unrepairable(String),
    /// Info findings: nothing to repair.
    NotApplicable,
}

/// The questions a repair can ask (D-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InteractionKind {
    /// Which font a damaged font slot should be rebuilt from.
    FontPick,
    /// No font can reproduce the slot's glyphs: substitute, text only, or skip.
    FontUnreproducible,
}

/// What a carved object is, from `/Type`/`/Subtype` or its content.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ObjectKind {
    Catalog,
    Pages,
    Page,
    Font,
    FontDescriptor,
    FontFile,
    ToUnicode,
    ContentStream,
    Image,
    Form,
    XRefStream,
    ObjStm,
    Sig,
    /// Any other kind, named by its `/Type` (or `/Subtype`).
    Other(String),
}

/// How the carver settled where a stream's data ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LengthSource {
    /// A direct `/Length` that lands on `endstream`.
    Declared,
    /// An indirect `/Length`, read from this object.
    DeclaredIndirect(ObjId),
    /// The end of a complete inflate.
    InflateProbe,
    /// The next `endstream` keyword.
    ScannedEndstream,
    /// The file ended inside the stream. The carver also uses it when no
    /// `endstream` comes before the next object header and the data is cut
    /// there; its `NoEndstream` note marks that case.
    TruncatedAtEof,
}

/// What the file says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FileMeta {
    /// The header version, e.g. `"1.7"`; `None` when there is no readable header.
    pub version: Option<String>,
    pub pages: u32,
    pub title: Option<String>,
    /// `(width, height)` in whole points, one per page in document order.
    pub page_sizes: Vec<(u32, u32)>,
}

/// An exact non-negative fraction `num/den`. Comparison is by value through
/// `u128` cross-multiplication, never through a float, so `2/4 == 1/2`.
/// A zero denominator is still totally ordered: `0/0` equals zero and `n/0`
/// (`n > 0`) is above every finite ratio and equal to every other `n/0`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Ratio {
    pub num: u64,
    pub den: u64,
}

impl Ratio {
    /// `(num, den)` with `0/0` as `0/1` and every `n/0` as `1/0`.
    fn canonical(self) -> (u64, u64) {
        match (self.num, self.den) {
            (0, 0) => (0, 1),
            (_, 0) => (1, 0),
            pair => pair,
        }
    }
}

impl Ord for Ratio {
    fn cmp(&self, other: &Self) -> Ordering {
        let (an, ad) = self.canonical();
        let (bn, bd) = other.canonical();
        match (ad, bd) {
            (0, 0) => Ordering::Equal,
            (0, _) => Ordering::Greater,
            (_, 0) => Ordering::Less,
            _ => (u128::from(an) * u128::from(bd)).cmp(&(u128::from(bn) * u128::from(ad))),
        }
    }
}

impl PartialOrd for Ratio {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Ratio {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Ratio {}

/// Per-page counts of what the content stream paints (filled by T-36). These
/// come from interpreted content, never from rendered pixels (D-059).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PaintCounts {
    pub path_fills: u32,
    pub path_strokes: u32,
    pub images: u32,
    pub glyph_runs_visible: u32,
    pub glyph_runs_invisible: u32,
    /// Glyphs whose text is not whitespace.
    pub glyphs_inked: u32,
    pub clips: u32,
}

impl PaintCounts {
    /// Fills + strokes + images + visible glyph runs; `0` means a blank page.
    pub fn paint_ops(&self) -> u64 {
        u64::from(self.path_fills)
            + u64::from(self.path_strokes)
            + u64::from(self.images)
            + u64::from(self.glyph_runs_visible)
    }
}

/// How sure a C9 stream repair is, as the report states it (D-041).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SalvageGrade {
    /// The search window was exhausted and one distinct output survived.
    Exact,
    /// A repair was accepted when the budget ran out; another output may exist.
    Accepted,
    /// Two or more distinct outputs survived; the pinned order chose one.
    Ambiguous,
    /// No search ran (the stream was over the size limit).
    Unsearched,
}

const fn assert_model_type<
    T: Serialize + DeserializeOwned + std::fmt::Debug + Clone + PartialEq + Send + Sync,
>() {
}

const _: () = {
    assert_model_type::<ObjId>();
    assert_model_type::<CorruptionClass>();
    assert_model_type::<FindingKind>();
    assert_model_type::<Severity>();
    assert_model_type::<ByteSpan>();
    assert_model_type::<Location>();
    assert_model_type::<HexWindow>();
    assert_model_type::<MetricValue>();
    assert_model_type::<Evidence>();
    assert_model_type::<Finding>();
    assert_model_type::<Repairability>();
    assert_model_type::<InteractionKind>();
    assert_model_type::<ObjectKind>();
    assert_model_type::<LengthSource>();
    assert_model_type::<FileMeta>();
    assert_model_type::<Ratio>();
    assert_model_type::<PaintCounts>();
    assert_model_type::<SalvageGrade>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    // Exhaustive matches: adding a variant breaks the build here until the
    // fixture below carries it.
    fn class_index(c: CorruptionClass) -> usize {
        match c {
            CorruptionClass::C1Header => 0,
            CorruptionClass::C2XrefMissing => 1,
            CorruptionClass::C3TrailerDamaged => 2,
            CorruptionClass::C4PageTreeBroken => 3,
            CorruptionClass::C5ObjectTagStripped => 4,
            CorruptionClass::C6FontMapLost => 5,
            CorruptionClass::C7FontStreamDeleted => 6,
            CorruptionClass::C8FontResourcesDeleted => 7,
            CorruptionClass::C9ZlibTampered => 8,
            CorruptionClass::C10Truncated => 9,
        }
    }

    fn kind_index(k: &FindingKind) -> usize {
        match k {
            FindingKind::Corruption(_) => 0,
            FindingKind::Encrypted => 1,
            FindingKind::Signed { .. } => 2,
            FindingKind::OutlinedText { .. } => 3,
            FindingKind::Type3Text { .. } => 4,
        }
    }

    fn severity_index(s: Severity) -> usize {
        match s {
            Severity::Info => 0,
            Severity::Warning => 1,
            Severity::Error => 2,
        }
    }

    fn location_index(l: &Location) -> usize {
        match l {
            Location::File => 0,
            Location::Span(_) => 1,
            Location::Object { .. } => 2,
            Location::Page { .. } => 3,
        }
    }

    fn evidence_index(e: &Evidence) -> usize {
        match e {
            Evidence::Text(_) => 0,
            Evidence::HexWindow(_) => 1,
            Evidence::ObjectRef(_) => 2,
            Evidence::Metric { value, .. } => match value {
                MetricValue::Int(_) => 3,
                MetricValue::Ratio(_) => 4,
                MetricValue::Text(_) => 5,
            },
        }
    }

    fn repair_index(r: &Repairability) -> usize {
        match r {
            Repairability::Auto => 0,
            Repairability::Interactive(InteractionKind::FontPick) => 1,
            Repairability::Interactive(InteractionKind::FontUnreproducible) => 2,
            Repairability::Partial(_) => 3,
            Repairability::Unrepairable(_) => 4,
            Repairability::NotApplicable => 5,
        }
    }

    fn object_kind_index(k: &ObjectKind) -> usize {
        match k {
            ObjectKind::Catalog => 0,
            ObjectKind::Pages => 1,
            ObjectKind::Page => 2,
            ObjectKind::Font => 3,
            ObjectKind::FontDescriptor => 4,
            ObjectKind::FontFile => 5,
            ObjectKind::ToUnicode => 6,
            ObjectKind::ContentStream => 7,
            ObjectKind::Image => 8,
            ObjectKind::Form => 9,
            ObjectKind::XRefStream => 10,
            ObjectKind::ObjStm => 11,
            ObjectKind::Sig => 12,
            ObjectKind::Other(_) => 13,
        }
    }

    fn length_source_index(l: &LengthSource) -> usize {
        match l {
            LengthSource::Declared => 0,
            LengthSource::DeclaredIndirect(_) => 1,
            LengthSource::InflateProbe => 2,
            LengthSource::ScannedEndstream => 3,
            LengthSource::TruncatedAtEof => 4,
        }
    }

    fn grade_index(g: SalvageGrade) -> usize {
        match g {
            SalvageGrade::Exact => 0,
            SalvageGrade::Accepted => 1,
            SalvageGrade::Ambiguous => 2,
            SalvageGrade::Unsearched => 3,
        }
    }

    fn covers(indexes: impl IntoIterator<Item = usize>, n: usize) -> bool {
        let mut seen = vec![false; n];
        for i in indexes {
            seen[i] = true;
        }
        seen.into_iter().all(|s| s)
    }

    /// Everything a report can carry, every enum variant at least once.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Fixture {
        findings: Vec<Finding>,
        kinds: Vec<ObjectKind>,
        lengths: Vec<LengthSource>,
        grades: Vec<SalvageGrade>,
        meta: FileMeta,
        paint: PaintCounts,
        ratio: Ratio,
    }

    fn hundred_bytes() -> Vec<u8> {
        (0u8..100).collect()
    }

    fn fixture() -> Fixture {
        let mut findings: Vec<Finding> = CorruptionClass::ALL
            .iter()
            .enumerate()
            .map(|(i, &class)| Finding {
                id: format!("{}-{:03}", class.code(), i + 1),
                class: FindingKind::Corruption(class),
                severity: Severity::Error,
                location: Location::Span(ByteSpan {
                    start: 10 * i as u64,
                    end: 10 * i as u64 + 5,
                }),
                summary: class.label().to_string(),
                evidence: vec![Evidence::Text(format!("class {}", class.code()))],
                repair: Repairability::Auto,
            })
            .collect();
        findings.push(Finding {
            id: "ENC-001".into(),
            class: FindingKind::Encrypted,
            severity: Severity::Error,
            location: Location::File,
            summary: "encrypted".into(),
            evidence: vec![Evidence::HexWindow(HexWindow::new(1024, &hundred_bytes()))],
            repair: Repairability::Unrepairable("decrypt first".into()),
        });
        findings.push(Finding {
            id: "SIG-001".into(),
            class: FindingKind::Signed { fields: 2 },
            severity: Severity::Info,
            location: Location::Object {
                id: (12, 0),
                span: Some(ByteSpan {
                    start: 400,
                    end: 512,
                }),
            },
            summary: "the original is digitally signed; a repaired file is not".into(),
            evidence: vec![
                Evidence::ObjectRef((12, 0)),
                Evidence::Metric {
                    name: "fields".into(),
                    value: MetricValue::Int(2),
                },
            ],
            repair: Repairability::NotApplicable,
        });
        findings.push(Finding {
            id: "OUTLINE-001".into(),
            class: FindingKind::OutlinedText {
                glyph_runs: 9,
                contours: 41,
            },
            severity: Severity::Info,
            location: Location::Page {
                index: 3,
                obj: Some((7, 0)),
            },
            summary: "text drawn as outlines".into(),
            evidence: vec![
                Evidence::Metric {
                    name: "startxref_delta".into(),
                    value: MetricValue::Int(-3),
                },
                Evidence::Metric {
                    name: "kept_fraction".into(),
                    value: MetricValue::Ratio(Ratio { num: 7, den: 10 }),
                },
                Evidence::Metric {
                    name: "salvage".into(),
                    value: MetricValue::Text("Repaired".into()),
                },
            ],
            repair: Repairability::Interactive(InteractionKind::FontPick),
        });
        findings.push(Finding {
            id: "TYPE3-001".into(),
            class: FindingKind::Type3Text { font: (21, 1) },
            severity: Severity::Warning,
            location: Location::Page {
                index: 0,
                obj: None,
            },
            summary: "type 3 text".into(),
            evidence: vec![],
            repair: Repairability::Interactive(InteractionKind::FontUnreproducible),
        });
        findings.push(Finding {
            id: "C10-099".into(),
            class: FindingKind::Corruption(CorruptionClass::C10Truncated),
            severity: Severity::Error,
            location: Location::Object {
                id: (3, 0),
                span: None,
            },
            summary: "truncated".into(),
            evidence: vec![],
            repair: Repairability::Partial("kept 70%".into()),
        });

        Fixture {
            findings,
            kinds: vec![
                ObjectKind::Catalog,
                ObjectKind::Pages,
                ObjectKind::Page,
                ObjectKind::Font,
                ObjectKind::FontDescriptor,
                ObjectKind::FontFile,
                ObjectKind::ToUnicode,
                ObjectKind::ContentStream,
                ObjectKind::Image,
                ObjectKind::Form,
                ObjectKind::XRefStream,
                ObjectKind::ObjStm,
                ObjectKind::Sig,
                ObjectKind::Other("/Annot".into()),
            ],
            lengths: vec![
                LengthSource::Declared,
                LengthSource::DeclaredIndirect((5, 0)),
                LengthSource::InflateProbe,
                LengthSource::ScannedEndstream,
                LengthSource::TruncatedAtEof,
            ],
            grades: vec![
                SalvageGrade::Exact,
                SalvageGrade::Accepted,
                SalvageGrade::Ambiguous,
                SalvageGrade::Unsearched,
            ],
            meta: FileMeta {
                version: Some("1.7".into()),
                pages: 2,
                title: Some("Quarterly figures".into()),
                page_sizes: vec![(612, 792), (595, 842)],
            },
            paint: PaintCounts {
                path_fills: 1,
                path_strokes: 2,
                images: 3,
                glyph_runs_visible: 4,
                glyph_runs_invisible: 5,
                glyphs_inked: 6,
                clips: 7,
            },
            ratio: Ratio { num: 2, den: 3 },
        }
    }

    #[test]
    fn fixture_carries_every_variant() {
        let f = fixture();
        let classes = f.findings.iter().filter_map(|x| match x.class {
            FindingKind::Corruption(c) => Some(class_index(c)),
            _ => None,
        });
        assert!(covers(classes, 10));
        assert!(covers(f.findings.iter().map(|x| kind_index(&x.class)), 5));
        assert!(covers(
            f.findings.iter().map(|x| severity_index(x.severity)),
            3
        ));
        assert!(covers(
            f.findings.iter().map(|x| location_index(&x.location)),
            4
        ));
        let evidence = f.findings.iter().flat_map(|x| x.evidence.iter());
        assert!(covers(evidence.map(evidence_index), 6));
        assert!(covers(
            f.findings.iter().map(|x| repair_index(&x.repair)),
            6
        ));
        assert!(covers(f.kinds.iter().map(object_kind_index), 14));
        assert!(covers(f.lengths.iter().map(length_source_index), 5));
        assert!(covers(f.grades.iter().copied().map(grade_index), 4));
    }

    #[test]
    fn every_type_round_trips_through_json() {
        let f = fixture();
        let json = serde_json::to_string(&f).expect("serialise");
        let back: Fixture = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back, f);
        // Ratio's equality is by value, so also check the bytes come back the same.
        assert_eq!(serde_json::to_string(&back).expect("serialise again"), json);
    }

    #[test]
    fn hex_window_keeps_the_first_64_bytes() {
        let bytes = hundred_bytes();
        let w = HexWindow::new(1024, &bytes);
        assert_eq!(w.at(), 1024);
        assert_eq!(w.bytes(), &bytes[..64]);
        assert_eq!(HexWindow::MAX_BYTES, 64);

        let short = HexWindow::new(0, &bytes[..3]);
        assert_eq!(short.bytes(), &[0, 1, 2]);

        // The 64 that survived come back through serde.
        let f = fixture();
        let json = serde_json::to_string(&f).expect("serialise");
        let back: Fixture = serde_json::from_str(&json).expect("deserialise");
        let found = back
            .findings
            .iter()
            .flat_map(|x| x.evidence.iter())
            .find_map(|e| match e {
                Evidence::HexWindow(w) => Some(w.clone()),
                _ => None,
            })
            .expect("a hex window in the fixture");
        assert_eq!(found.bytes().len(), 64);
        assert_eq!(found, w);
    }

    #[test]
    fn hex_window_over_64_bytes_does_not_deserialise() {
        let ok = format!(r#"{{"at":5,"bytes":{:?}}}"#, vec![1u8; 64]);
        let w: HexWindow = serde_json::from_str(&ok).expect("64 bytes is a window");
        assert_eq!(w.bytes().len(), 64);
        let bad = format!(r#"{{"at":5,"bytes":{:?}}}"#, vec![1u8; 65]);
        assert!(serde_json::from_str::<HexWindow>(&bad).is_err());
    }

    #[test]
    fn all_lists_c1_to_c10_in_order() {
        let codes: Vec<&str> = CorruptionClass::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            ["C1", "C2", "C3", "C4", "C5", "C6", "C7", "C8", "C9", "C10"]
        );
        for (i, c) in CorruptionClass::ALL.iter().enumerate() {
            assert_eq!(class_index(*c), i);
        }
        // Derived Ord follows ALL.
        assert!(CorruptionClass::ALL.windows(2).all(|w| w[0] < w[1]));
        let mut labels: Vec<&str> = CorruptionClass::ALL.iter().map(|c| c.label()).collect();
        assert!(labels.iter().all(|l| !l.is_empty()));
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), 10, "labels are distinct");
    }

    #[test]
    fn repair_order_is_c9_c10_c5_c4_c6_c7_c8() {
        use CorruptionClass::*;
        assert_eq!(
            CorruptionClass::REPAIR_ORDER,
            [
                C9ZlibTampered,
                C10Truncated,
                C5ObjectTagStripped,
                C4PageTreeBroken,
                C6FontMapLost,
                C7FontStreamDeleted,
                C8FontResourcesDeleted,
            ]
        );
    }

    #[test]
    fn severity_orders_info_warning_error() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
    }

    #[test]
    fn ratio_compares_by_value_without_floats() {
        let r = |num, den| Ratio { num, den };
        assert!(r(1, 3) < r(1, 2));
        assert_eq!(r(2, 4), r(1, 2));
        assert_eq!(r(2, 4).cmp(&r(1, 2)), Ordering::Equal);
        assert!(r(3, 4) > r(2, 3));
        assert_eq!(r(0, 5), r(0, 1));

        // f64 rounds both of these to 1.0; the integer comparison does not.
        let a = r(u64::MAX, u64::MAX - 1);
        let b = r(u64::MAX - 1, u64::MAX - 2);
        assert_eq!(
            a.num as f64 / a.den as f64,
            b.num as f64 / b.den as f64,
            "the floats cannot tell them apart"
        );
        assert!(a < b);
        assert!(a > r(1, 1));
        assert!(r(u64::MAX, 1) > r(u64::MAX - 1, 1));
    }

    #[test]
    fn ratio_with_zero_denominator_has_a_total_order() {
        let r = |num, den| Ratio { num, den };
        // 0/0 is zero; n/0 with n > 0 is above every finite ratio and equal to
        // every other n/0.
        assert_eq!(r(0, 0), r(0, 1));
        assert!(r(0, 0) < r(1, u64::MAX));
        assert!(r(1, 0) > r(u64::MAX, 1));
        assert_eq!(r(1, 0), r(9, 0));
        assert!(r(1, 0) > r(0, 0));

        let mut v = [r(1, 0), r(1, 2), r(0, 0), r(u64::MAX, 1), r(1, 3), r(2, 4)];
        v.sort();
        let ranks: Vec<(u64, u64)> = v.iter().map(|x| (x.num, x.den)).collect();
        assert_eq!(
            ranks,
            [(0, 0), (1, 3), (1, 2), (2, 4), (u64::MAX, 1), (1, 0)]
        );
    }

    #[test]
    fn paint_ops_counts_visible_paint_only() {
        let p = PaintCounts {
            path_fills: 1,
            path_strokes: 2,
            images: 3,
            glyph_runs_visible: 4,
            glyph_runs_invisible: 50,
            glyphs_inked: 60,
            clips: 70,
        };
        assert_eq!(p.paint_ops(), 10);
        assert_eq!(PaintCounts::default().paint_ops(), 0);
        let max = PaintCounts {
            path_fills: u32::MAX,
            path_strokes: u32::MAX,
            images: u32::MAX,
            glyph_runs_visible: u32::MAX,
            ..PaintCounts::default()
        };
        assert_eq!(max.paint_ops(), 4 * u64::from(u32::MAX));
    }

    #[test]
    fn object_ids_serialise_as_number_generation_pairs() {
        let json = serde_json::to_string(&Evidence::ObjectRef((12, 3))).expect("serialise");
        assert_eq!(json, r#"{"ObjectRef":[12,3]}"#);
    }
}
