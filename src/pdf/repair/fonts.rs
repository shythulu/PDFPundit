//! The C7 and C8 passes (T-30; TD §5.3, §5.4, §17.4; RR changes #2/#3; SE
//! Q3; D-020, D-027).
//!
//! A damaged font is found by its descriptor (diagnose's C7 or C8 finding);
//! the font the pages name is the descriptor's font, or that font's `/Type0`
//! parent. Every `(page, slot)` whose `/Resources /Font` names it is one of
//! its slots, and the codes it shows are read from each page's content
//! (T-09's `page_content`, T-06's `content_ops`): one [`CodeRun`] per show
//! operator, a token break at each `TJ` offset beyond −250/1000 em.
//!
//! **Only `TemplateAssemble` substitutes.** In the `Resave` candidate both
//! passes keep the font as found and report `Partial`, so `Resave` stays the
//! structure-preserving comparison and asks nothing.
//!
//! **What is substituted.** A `/Type0` `Identity-H` font: its two-byte codes
//! are what the harvested `CIDFontType2` reads. A simple font (one-byte
//! codes) or another `/Encoding` is left as found and the pass is `Partial`.
//!
//! **C7** (the program is lost, `/ToUnicode` survives): each code is decoded
//! through the `/ToUnicode` alone (T-27c's first rung) to its true text; that
//! text is the per-document dictionary T-28's [`infer`] scores with, and the
//! codes' texts are the `/ToUnicode` the output gets. A code whose text is
//! several characters (a ligature, a conjunct, a combining sequence) keeps
//! it whole in the `/ToUnicode`, but the font draws only its first
//! character's glyph, and the pass is `Partial` saying how many. **C8** (the
//! `/ToUnicode` is lost too): the database is tried by name first
//! ([`by_name`]), and a name match is used only when the `CIDFont`'s
//! surviving `/W` confirms that the codes are that font's glyph ids
//! ([`name_hit_holds`]); a rejected match is a `name: … rejected:`
//! provenance line. A `CIDFont+Fn` name never matches. Otherwise the slot
//! goes to [`infer`] over every database font with a program, with the
//! general word lists. The characters are then the chosen font's reading of
//! the codes. A name can match a `.gmap`-only font (G-10, D-010 (b)); its
//! glyphs are then drawn with the program of the font its entry names, the
//! action says so, and the pass is `Partial` when that program lacks some
//! of the characters (they draw as `.notdef`; the `/ToUnicode` keeps them).
//!
//! **Decisions** ([`resolve`], D-009's interim clamp):
//! - `AutoAccept(c)`: substitute `c.font_id`, `Fixed`;
//! - `Ask`: `FontPick` is asked. `Pick(id)` substitutes `id`; `UseBest` the
//!   top candidate, `Partial` (the candidate fell short of auto-accepting,
//!   and nobody looked at it), except on a weak C8 guess (below); `Skip`
//!   the top candidate, `Partial("font substituted without confirmation")`
//!   (TD §5.3); `Substitute(choice)` substitutes the choice; `TextOnly` as
//!   below;
//! - a weak C8 guess (G-02, D-122 (b)): `UseBest` on a C8 `FontPick` whose
//!   top candidate's dictionary hit is below 1/2 substitutes no font. The
//!   font is kept as found and gets a `/ToUnicode` rebuilt from the top
//!   candidate's reading of the codes, so the text is recoverable; the slot
//!   resolves `TextOnly` and the pass is `Partial("text recovered; font not
//!   confirmed")`. A forensic output prefers no new font to a wrong one; a
//!   user who trusts the guess can still `Pick` it. At or above 1/2, and in
//!   C7 (whose `/ToUnicode` already carries the text), `UseBest`
//!   substitutes as above. When the top candidate maps none of the codes
//!   there is nothing to recover and the slot is `TextOnly` as below;
//! - `Unreproducible`: `FontUnreproducible` is asked once per font family per
//!   file, and the answer holds for every font of that family (an answer
//!   given in the C7 pass holds in C8 too). The family is T-28's: the
//!   `/BaseFont` without its subset tag, so each face (`NotoSans-Regular`,
//!   `NotoSans-Bold`) is asked about once. The question lists the family's
//!   slots in the pass that asks it: a C8 font of a family the C7 pass asked
//!   about takes the answer without being listed.
//!   Under `UnreproduciblePolicy::SubstituteGeneric` nothing is asked: the
//!   first option (the best coverage) is the answer, recorded with source
//!   `Policy`. `Substitute(choice)` or `Pick(id)` substitutes that font and is
//!   `Partial`: not every glyph is reproduced. `UseBest` takes the policy
//!   default, the first option (text only when there is none). `Skip` leaves
//!   the font as found;
//! - `TextOnly` (asked, or `UnreproduciblePolicy::TextOnly`, recorded with
//!   source `Policy`): the font is left as found, `Partial("text only: no
//!   reproducible font")`. In C7 its `/ToUnicode` keeps the text
//!   extractable; in C8 the text stays unmapped. Each slot's
//!   [`FontResolution`] is `TextOnly`: the per-page record a Markdown note
//!   can be made from (T-32 has no such note yet).
//!
//! Every question and answer is recorded in [`PassNotes::interactions`]
//! with the source the answer came with (D-141): `Batched` or `Policy` when
//! the app answered on its own, `UseBest` for the user's own `UseBest`
//! reply, else `User`. Request ids are 0:
//! the facade numbers the questions it shows. A substitution is the action
//! `font program substituted: <font_id>` on the descriptor; every slot of
//! the font gets a [`FontResolution`] whose provenance says how it was
//! decided (`name:` for the name path, `inference:` when [`infer`] ran).
//! A font left as found keeps its finding, excused as `Partial`.

use std::collections::{BTreeMap, BTreeSet};

use lopdf::{Dictionary, Object, Stream};

use super::{RepairCtx, RepairPass, font_dict, resolved};
use crate::bench::metrics::Lang;
use crate::engine::{
    FontResolution, FontResolutionKind, FontSlot, InteractionRecord, InteractionReply,
    InteractionRequest, InteractionSource, InteractionSummary, LogLevel, PassOutcome, PassReport,
    RepairAction, SubstituteChoice, ToUnicodeState, Toolpath, UnreproduciblePolicy,
};
use crate::pdf::carver::Body;
use crate::pdf::diagnose::type0_parent;
use crate::pdf::fontdb::FontDb;
use crate::pdf::fontdb::build::IndexEntry;
use crate::pdf::fontdb::decode::decode_codes;
use crate::pdf::fontdb::dict::{Dictionary as WordList, FrequencyList};
use crate::pdf::fontdb::score::{CodeRun, FontDecision, MIN_HIT, infer, resolve};
use crate::pdf::fontdb::template::harvest_from;
use crate::pdf::model::CorruptionClass::C7FontStreamDeleted;
use crate::pdf::model::{CorruptionClass, Finding, InteractionKind, Location, ObjId, Ratio};
use crate::pdf::rebuild::Held;
use crate::pdf::streams::salvage::CarveSource;
use crate::pdf::streams::{DEFAULT_CAP, content_ops};

/// A `TJ` offset beyond this (thousandths of an em, moving right) starts a
/// new token.
const TJ_BREAK: f32 = -250.0;

/// The C7 or C8 pass (module docs).
pub(super) struct FontPrograms(pub(super) CorruptionClass);

impl RepairPass for FontPrograms {
    fn class(&self) -> CorruptionClass {
        self.0
    }

    fn repair(&self, ctx: &mut RepairCtx<'_>, findings: &[Finding]) -> PassReport {
        if ctx.toolpath != Toolpath::TemplateAssemble {
            let what = if self.0 == CorruptionClass::C8FontResourcesDeleted {
                "the font program and its /ToUnicode"
            } else {
                "the font program"
            };
            return PassReport {
                class: self.0,
                outcome: PassOutcome::Partial(format!(
                    "{what} stay damaged: the Resave toolpath keeps the font as found"
                )),
                actions: Vec::new(),
            };
        }
        let c8 = self.0 == CorruptionClass::C8FontResourcesDeleted;
        let targets = targets(ctx, findings, c8);
        let mut actions = Vec::new();
        let mut partial = Vec::new();
        // A descriptor no font names: nothing to substitute.
        for f in findings {
            let Location::Object { id, .. } = f.location else {
                continue;
            };
            if targets
                .iter()
                .any(|t| t.descriptor == id || t.finding.location == f.location)
            {
                continue;
            }
            let why = "not substituted: no font dictionary names this descriptor".to_owned();
            actions.push(RepairAction {
                object: id,
                what: why.clone(),
                grade: None,
            });
            partial.push(why);
            ctx.notes.partial.push((self.0, f.location));
        }

        // Every decision first, so that a family's question lists all of its
        // slots.
        let decided: Vec<(Target<'_>, Decided)> = targets
            .into_iter()
            .map(|t| {
                let d = decide(ctx, &t, c8);
                (t, d)
            })
            .collect();
        let mut family_slots: Vec<(String, Vec<(u32, String)>)> = Vec::new();
        for (t, d) in &decided {
            if let Decided::Decision(FontDecision::Unreproducible(req), _) = d {
                let slots = t.slot_names();
                let key = batch_key(t, &req.family);
                match family_slots.iter_mut().find(|(f, _)| *f == key) {
                    Some((_, all)) => all.extend(slots),
                    None => family_slots.push((key, slots)),
                }
            }
        }

        for (t, d) in decided {
            if ctx.notes.cancelled {
                break;
            }
            let (choice, provenance) = match d {
                Decided::Unsupported(why) => (Choice::Leave(why), Vec::new()),
                Decided::Named(entry, provenance) => (
                    Choice::Font {
                        id: entry,
                        why: None,
                        kind: None,
                    },
                    provenance,
                ),
                Decided::Decision(decision, provenance) => {
                    (answer(ctx, &t, decision, &family_slots, c8), provenance)
                }
            };
            for line in &provenance {
                let msg = format!("{} {} obj: {line}", t.descriptor.0, t.descriptor.1);
                ctx.sink.log(LogLevel::Info, msg);
            }
            let recovers = matches!(choice, Choice::Recover { .. });
            let (what, why, kind) = apply(ctx, &t, choice, c8);
            actions.push(RepairAction {
                object: t.descriptor,
                what,
                grade: None,
            });
            if let Some(why) = why {
                partial.push(why);
                ctx.notes.partial.push((self.0, t.finding.location));
            }
            // The recovered font keeps its lost program beside its new
            // /ToUnicode: the output may re-diagnose it as C7 (repair.rs docs).
            if recovers {
                (ctx.notes.partial_as).push((C7FontStreamDeleted, t.finding.location));
            }
            for (page, slot) in t.slot_names() {
                ctx.notes.resolutions.push((
                    page,
                    slot,
                    FontResolution {
                        kind: kind.clone(),
                        provenance: provenance.clone(),
                    },
                ));
            }
        }
        PassReport {
            class: self.0,
            outcome: if partial.is_empty() {
                PassOutcome::Fixed
            } else {
                PassOutcome::Partial(partial.join("; "))
            },
            actions,
        }
    }
}

// ── the fonts and what they show ─────────────────────────────────────────

/// One damaged font and the slots that show text through it.
struct Target<'f> {
    finding: &'f Finding,
    descriptor: ObjId,
    /// The font the pages name: the `/Type0` parent, or the font itself.
    top: ObjId,
    /// The font that names the descriptor: the `CIDFont` under `top`, or
    /// `top` itself.
    cidfont: ObjId,
    /// `/BaseFont` (else the descriptor's `/FontName`), without the slash.
    base_font: Option<String>,
    subtype: Option<String>,
    /// A `/Type0` `Identity-H` font: two-byte codes.
    identity_h: bool,
    /// The `/ToUnicode` CMap, decoded (C7).
    tounicode: Option<Vec<u8>>,
    /// `(page index, slot)`, in page order.
    slots: Vec<(u32, Vec<u8>)>,
    runs: Vec<CodeRun>,
}

impl Target<'_> {
    fn slot_names(&self) -> Vec<(u32, String)> {
        let names = self.slots.iter();
        names
            .map(|(p, s)| (*p, String::from_utf8_lossy(s).into_owned()))
            .collect()
    }

    fn codes(&self) -> BTreeSet<u16> {
        self.runs
            .iter()
            .flat_map(|r| r.codes.iter().copied())
            .collect()
    }
}

/// The fonts `findings` (descriptors) belong to, once each, in finding order.
fn targets<'f>(ctx: &RepairCtx<'_>, findings: &'f [Finding], c8: bool) -> Vec<Target<'f>> {
    let mut out: Vec<Target<'f>> = Vec::new();
    for f in findings {
        let Location::Object { id: descriptor, .. } = f.location else {
            continue;
        };
        let user = ctx
            .graph
            .referrers(descriptor)
            .iter()
            .find(|e| e.path.0.len() == 1 && e.path.first_key() == Some(&b"FontDescriptor"[..]))
            .map(|e| e.from);
        let Some(user) = user else {
            continue;
        };
        let top = type0_parent(ctx.graph, user).unwrap_or(user);
        if out.iter().any(|t| t.top == top) {
            continue;
        }
        let Some(dict) = font_dict(ctx, top) else {
            continue;
        };
        let name = |d: &Dictionary, key: &[u8]| {
            d.get(key)
                .ok()
                .and_then(|v| v.as_name().ok())
                .map(|n| String::from_utf8_lossy(n).into_owned())
        };
        let base_font = name(dict, b"BaseFont").or_else(|| {
            let d = font_dict(ctx, descriptor)?;
            name(d, b"FontName")
        });
        let subtype = name(dict, b"Subtype");
        let identity_h = subtype.as_deref() == Some("Type0")
            && name(dict, b"Encoding").as_deref() == Some("Identity-H");
        let tounicode = if c8 {
            None
        } else {
            match dict.get(b"ToUnicode") {
                Ok(Object::Reference(id)) => decoded(ctx, *id),
                _ => None,
            }
        };
        let slots = slots_of(ctx, top);
        let runs = runs_of(ctx, top, &slots, identity_h);
        out.push(Target {
            finding: f,
            descriptor,
            top,
            cidfont: user,
            base_font,
            subtype,
            identity_h,
            tounicode,
            slots,
            runs,
        });
    }
    out
}

/// Stream `id`'s data through its filters (the C9 salvage when it has one).
fn decoded(ctx: &RepairCtx<'_>, id: ObjId) -> Option<Vec<u8>> {
    let source = CarveSource::new(ctx.carve, ctx.bytes);
    let bytes = ctx.salvage.decoded(&source, id, DEFAULT_CAP).ok()?;
    Some(bytes.into_owned())
}

/// The dictionary `v` is, or the carved dictionary it refers to.
fn dict_of(ctx: &RepairCtx<'_>, v: &Object) -> Option<Dictionary> {
    match v {
        Object::Dictionary(d) => Some(d.clone()),
        Object::Reference(id) => match ctx.remap.target(*id)? {
            Held::Object(at) => match &ctx.carve.objects.get(at)?.body {
                Body::Dict(d) | Body::Stream { dict: d, .. } => Some(d.clone()),
                _ => None,
            },
            Held::Orphan(_) => None,
        },
        _ => None,
    }
}

/// The slots of `resources` (a resources dictionary or a reference to one)
/// whose `/Font` entry names `font`.
fn slots_naming(ctx: &RepairCtx<'_>, resources: &Object, font: ObjId) -> Vec<Vec<u8>> {
    let target = ctx.remap.target(font);
    let Some(fonts) = dict_of(ctx, resources).and_then(|r| dict_of(ctx, r.get(b"Font").ok()?))
    else {
        return Vec::new();
    };
    fonts
        .iter()
        .filter(|(_, v)| matches!(v, Object::Reference(id) if ctx.remap.target(*id) == target))
        .map(|(k, _)| k.clone())
        .collect()
}

/// Every `(page index, slot)` of the output's pages whose resources name
/// `font`, in page order: what the page tree has, with the C6 pass's
/// re-links (in this candidate) over it.
fn slots_of(ctx: &RepairCtx<'_>, font: ObjId) -> Vec<(u32, Vec<u8>)> {
    let target = ctx.remap.target(font);
    let mut out = Vec::new();
    for (index, page) in ctx.page_tree.pages.iter().enumerate() {
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        let relinked = ctx.doc.relinked(page.id);
        let elsewhere = |s: &Vec<u8>| relinked.is_some_and(|m| m.contains_key(s));
        let mut slots: BTreeSet<Vec<u8>> = (page.resources.as_ref())
            .map(|r| slots_naming(ctx, r, font))
            .unwrap_or_default()
            .into_iter()
            .filter(|s| !elsewhere(s))
            .collect();
        if let Some(m) = relinked {
            slots.extend(
                m.iter()
                    .filter(|&(_, &id)| ctx.remap.target(id) == target)
                    .map(|(s, _)| s.clone()),
            );
        }
        out.extend(slots.into_iter().map(|s| (index, s)));
    }
    out
}

/// The input id of the page at `index` of the flat tree.
fn page_input_id(ctx: &RepairCtx<'_>, index: u32) -> Option<ObjId> {
    let page = ctx.page_tree.pages.get(usize::try_from(index).ok()?)?;
    let numbered = ctx.remap.objects();
    let i = numbered.binary_search_by_key(&page.id, |&(n, _)| n).ok()?;
    match numbered[i].1 {
        Held::Object(at) => Some(ctx.carve.objects.get(at)?.declared_id),
        Held::Orphan(_) => None,
    }
}

/// The codes the pages of `slots` show through `font`: one run per show
/// operator, in content order (module docs).
fn runs_of(
    ctx: &RepairCtx<'_>,
    font: ObjId,
    slots: &[(u32, Vec<u8>)],
    two_byte: bool,
) -> Vec<CodeRun> {
    let source = CarveSource::new(ctx.carve, ctx.bytes);
    let decode = |id: ObjId| ctx.salvage.decoded(&source, id, DEFAULT_CAP).ok();
    let pages: BTreeSet<u32> = slots.iter().map(|(p, _)| *p).collect();
    let mut runs = Vec::new();
    for index in pages {
        let Some(page) = page_input_id(ctx, index) else {
            continue;
        };
        // A page's /Contents pieces are one stream: a font set in one
        // governs text in the next. A form starts afresh.
        let mut page_state = FontState::default();
        for piece in ctx.graph.page_content(ctx.carve, page, decode) {
            let Some(bytes) = decode(piece.stream) else {
                continue;
            };
            // The page's own content selects through the page's slots (its
            // re-links included); a form through its own resources.
            let ours: Vec<Vec<u8>> = if piece.via.is_empty() {
                let own = slots.iter().filter(|(p, _)| *p == index);
                own.map(|(_, s)| s.clone()).collect()
            } else {
                let resources = Object::Dictionary(piece.resources.dict.clone());
                slots_naming(ctx, &resources, font)
            };
            let mut form_state = FontState::default();
            let state = if piece.via.is_empty() {
                &mut page_state
            } else {
                &mut form_state
            };
            for op in content_ops(&bytes) {
                let on = state.on;
                let mut run = CodeRun {
                    codes: Vec::new(),
                    breaks: Vec::new(),
                };
                match (op.op, op.operands.last()) {
                    (b"Tf", _) => {
                        if let [.., Object::Name(s), _] = op.operands.as_slice() {
                            state.on = ours.contains(s);
                        }
                        continue;
                    }
                    (b"q", _) => {
                        state.saved.push(state.on);
                        continue;
                    }
                    (b"Q", _) => {
                        // An unbalanced Q leaves the state as it is.
                        if let Some(on) = state.saved.pop() {
                            state.on = on;
                        }
                        continue;
                    }
                    (b"Tj" | b"'" | b"\"", Some(Object::String(t, _))) if on => {
                        push_codes(&mut run.codes, t, two_byte);
                    }
                    (b"TJ", Some(Object::Array(items))) if on => {
                        for item in items {
                            let offset = match item {
                                Object::String(t, _) => {
                                    push_codes(&mut run.codes, t, two_byte);
                                    continue;
                                }
                                Object::Integer(n) => *n as f32,
                                Object::Real(r) => *r,
                                _ => continue,
                            };
                            if offset < TJ_BREAK && !run.codes.is_empty() {
                                run.breaks.push(run.codes.len());
                            }
                        }
                    }
                    _ => continue,
                }
                if !run.codes.is_empty() {
                    runs.push(run);
                }
            }
        }
    }
    runs
}

/// Whether the current text font is one of the target's slots, and the
/// values `q` saved (the font is part of the graphics state).
#[derive(Default)]
struct FontState {
    on: bool,
    saved: Vec<bool>,
}

/// The codes of string `t`: big-endian pairs (a trailing odd byte dropped),
/// or single bytes.
fn push_codes(out: &mut Vec<u16>, t: &[u8], two_byte: bool) {
    if two_byte {
        out.extend(t.as_chunks::<2>().0.iter().map(|&p| u16::from_be_bytes(p)));
    } else {
        out.extend(t.iter().map(|&b| u16::from(b)));
    }
}

// ── deciding ─────────────────────────────────────────────────────────────

/// What a font's resolution came to before anyone is asked.
enum Decided {
    /// The font cannot be substituted in this version: why.
    Unsupported(String),
    /// C8's name path matched this database font.
    Named(String, Vec<String>),
    /// T-28's policy, and its provenance.
    Decision(FontDecision, Vec<String>),
}

fn decide(ctx: &RepairCtx<'_>, t: &Target<'_>, c8: bool) -> Decided {
    if !t.identity_h {
        let (subtype, codes) = match t.subtype.as_deref() {
            Some("Type0") => ("/Type0", "another /Encoding's"),
            Some(other) => (other, "one-byte"),
            None => ("untyped", "one-byte"),
        };
        return Decided::Unsupported(format!(
            "not substituted: this version substitutes /Type0 Identity-H fonts only, and this \
             is a {subtype} font with {codes} codes"
        ));
    }
    if t.slots.is_empty() {
        return Decided::Unsupported(
            "not substituted: no page shows text through this font".to_owned(),
        );
    }
    let codes = t.codes();
    let mut rejected = Vec::new();
    if c8 && let Some((entry, how)) = t.base_font.as_deref().and_then(|n| by_name(ctx.fonts, n)) {
        let named = format!(
            "name: /BaseFont {} matches {} by its {how}",
            t.base_font.as_deref().unwrap_or_default(),
            entry.id
        );
        match name_hit_holds(ctx, t, &entry.id, &codes) {
            Ok(checked) => {
                let provenance = vec![format!("{named}; {checked}")];
                return Decided::Named(entry.id.clone(), provenance);
            }
            Err(why) => rejected.push(format!("{named}, rejected: {why}")),
        }
    }

    let (first_page, first_slot) = t.slot_names().swap_remove(0);
    let slot = FontSlot {
        page: first_page,
        slot: first_slot,
        base_font: t.base_font.clone(),
        subtype: t.subtype.clone(),
        embedded: true,
        tounicode: if c8 {
            ToUnicodeState::Missing
        } else {
            ToUnicodeState::Present
        },
        glyph_count: u32::try_from(codes.len()).unwrap_or(u32::MAX),
        resolution: None,
    };
    let gmaps = ctx.fonts.inference_gmaps();
    let lookup = |id: &str| gmaps.iter().find(|(i, _)| *i == id).map(|(_, g)| g);
    let entries = ctx.fonts.entries();
    let text = (!c8).then(|| true_text(t));
    let per_doc = text
        .as_ref()
        .map(|t| FrequencyList::from_text(Lang::Unknown, &t.words));
    let (cands, trace) = infer(
        &t.runs,
        &entries,
        &lookup,
        ctx.dicts,
        per_doc.as_ref().map(|d| d as &dyn WordList),
    );
    // C7: what the slot's text needs; C8: what the top candidate reads.
    let needed: Vec<u32> = match &text {
        Some(t) => t.used.values().map(|&c| u32::from(c)).collect(),
        None => cands
            .first()
            .and_then(|c| lookup(&c.font_id))
            .map(|g| {
                codes
                    .iter()
                    .filter_map(|&c| g.unicode(c))
                    .map(u32::from)
                    .collect()
            })
            .unwrap_or_default(),
    };
    let (decision, provenance) =
        resolve(&cands, &trace, ctx.opts, &slot, &needed, &t.runs, ctx.fonts);
    rejected.extend(provenance);
    Decided::Decision(decision, rejected)
}

/// Whether database font `id`, which the slot's `/BaseFont` names, is the
/// font the codes were written for (C8). The name alone does not say that a
/// subset kept the font's glyph ids, or that the font is the same version.
/// The `CIDFont`'s surviving `/W` is the check, with no inference: every
/// code must be a glyph of `id`, at least one code must have a `/W` width,
/// each `/W` width must be the glyph's within [`WIDTH_SLACK`], and the
/// widths checked must not all be equal (equal widths, as in a monospaced
/// font, cannot tell one glyph order from another). `Ok`: what was checked;
/// `Err`: why the name is not trusted.
fn name_hit_holds(
    ctx: &RepairCtx<'_>,
    t: &Target<'_>,
    id: &str,
    codes: &BTreeSet<u16>,
) -> Result<String, String> {
    let resolve = |v| resolved(ctx, v);
    let cidfont = font_dict(ctx, t.cidfont);
    widths_confirm(ctx.fonts, id, codes, cidfont, &resolve)
}

/// [`name_hit_holds`]'s check of `codes` against font `id` and the `/W` of
/// `cidfont`, whose references `resolve` follows.
fn widths_confirm<'c>(
    fonts: &FontDb,
    id: &str,
    codes: &BTreeSet<u16>,
    cidfont: Option<&'c Dictionary>,
    resolve: &dyn Fn(&'c Object) -> Option<&'c Object>,
) -> Result<String, String> {
    let Some(gmap) = fonts.gmap(id) else {
        return Err(format!("{id} has no glyph map"));
    };
    let missing = codes.iter().filter(|&&c| gmap.unicode(c).is_none()).count();
    if missing > 0 {
        return Err(format!(
            "{missing} of {} codes are not glyphs of {id}",
            codes.len()
        ));
    }
    let widths = cidfont
        .map(|d| cid_widths(resolve, d, codes))
        .unwrap_or_default();
    if widths.is_empty() {
        return Err("the font's /W gives no width to check the glyphs against".to_owned());
    }
    let off = |(&c, &w): (&u16, &i64)| {
        gmap.width(c)
            .is_none_or(|g| w.abs_diff(i64::from(g)) > WIDTH_SLACK)
    };
    let differ = widths.iter().filter(|&e| off(e)).count();
    if differ > 0 {
        return Err(format!(
            "{differ} of {} /W widths differ from {id}'s glyphs'",
            widths.len()
        ));
    }
    let distinct: BTreeSet<i64> = widths.values().copied().collect();
    if distinct.len() < 2 {
        return Err(
            "the /W widths checked are all equal, so they cannot confirm the glyphs".into(),
        );
    }
    Ok(format!("its glyphs agree with {} /W widths", widths.len()))
}

/// What C8's name path makes of a `CIDFont` whose `/BaseFont` is
/// `base_font`, with its references followed by `resolve`: the database
/// font the name matches, and whether its glyphs agree with every code but
/// `.notdef` (0) that the `/W` gives a width: the corpus harness's stand-in
/// for the codes the pages show, since producers give `.notdef` a width it
/// never draws (G-10).
#[cfg(test)]
pub(crate) fn name_match<'c>(
    fonts: &FontDb,
    base_font: &str,
    cidfont: &'c Dictionary,
    resolve: &dyn Fn(&'c Object) -> Option<&'c Object>,
) -> Option<(String, Result<String, String>)> {
    let (entry, _) = by_name(fonts, base_font)?;
    let all: BTreeSet<u16> = (1..=u16::MAX).collect();
    let codes: BTreeSet<u16> = cid_widths(resolve, cidfont, &all).into_keys().collect();
    let checked = widths_confirm(fonts, &entry.id, &codes, Some(cidfont), resolve);
    Some((entry.id.clone(), checked))
}

/// How far a `/W` width may be from the `.gmap`'s (both 1000 units per em):
/// a producer may round where the `.gmap` truncates.
const WIDTH_SLACK: u64 = 1;

/// The `/W` width of each of `codes` that `cidfont`'s `/W` gives, truncated
/// to an integer. `/DW` is not read: a default says nothing about a glyph.
fn cid_widths<'c>(
    resolve: &dyn Fn(&'c Object) -> Option<&'c Object>,
    cidfont: &'c Dictionary,
    codes: &BTreeSet<u16>,
) -> BTreeMap<u16, i64> {
    let mut out = BTreeMap::new();
    let Some(Object::Array(w)) = cidfont.get(b"W").ok().and_then(resolve) else {
        return out;
    };
    let number = |v: &'c Object| match resolve(v)? {
        Object::Integer(n) => Some(*n),
        // Truncation toward zero: a cast, no float method.
        Object::Real(r) if r.is_finite() => Some(*r as i64),
        _ => None,
    };
    let code = |n: i64| u16::try_from(n).ok();
    let mut items = w.iter();
    while let Some(first) = items.next() {
        let Some(first) = number(first) else {
            break;
        };
        match items.next().and_then(resolve) {
            Some(Object::Array(list)) => {
                for (i, v) in list.iter().enumerate() {
                    let at = i64::try_from(i).ok().and_then(|i| first.checked_add(i));
                    let Some(c) = at.and_then(code) else {
                        break;
                    };
                    if codes.contains(&c)
                        && let Some(width) = number(v)
                    {
                        out.entry(c).or_insert(width);
                    }
                }
            }
            Some(last) => {
                let (Some(last), Some(width)) = (number(last), items.next().and_then(number))
                else {
                    break;
                };
                let (Some(lo), Some(hi)) = (code(first), code(last.min(0xFFFF))) else {
                    continue;
                };
                if lo <= hi {
                    for &c in codes.range(lo..=hi) {
                        out.entry(c).or_insert(width);
                    }
                }
            }
            None => break,
        }
    }
    out
}

/// C7: each code's character through the `/ToUnicode` alone, and the
/// slot's text with a space at every token break.
fn true_text(t: &Target<'_>) -> TrueText {
    let Some(cmap) = &t.tounicode else {
        return TrueText::default();
    };
    // Only the first rung of the ladder: the dictionary holds nothing else.
    let mut font = Dictionary::new();
    font.set("Subtype", Object::Name(b"Type0".to_vec()));
    font.set("Encoding", Object::Name(b"Identity-H".to_vec()));
    font.set(
        "ToUnicode",
        Object::Stream(Stream::new(Dictionary::new(), cmap.clone())),
    );
    let mut out = TrueText::default();
    let mut several = BTreeSet::new();
    for run in &t.runs {
        let decoded = decode_codes(&font, &run.codes);
        // Both ascending: a cursor, not a search per code.
        let mut breaks = run.breaks.iter().peekable();
        for (i, (code, d)) in run.codes.iter().zip(&decoded).enumerate() {
            while breaks.next_if(|&&b| b < i).is_some() {}
            if breaks.next_if(|&&b| b == i).is_some() {
                out.words.push(' ');
            }
            let mut chars = d.text.chars();
            if let Some(c) = chars.next() {
                out.used.entry(*code).or_insert(c);
                out.text.entry(*code).or_insert_with(|| d.text.clone());
                if chars.next().is_some() {
                    several.insert(*code);
                }
            }
            out.words.push_str(&d.text);
        }
        out.words.push(' ');
    }
    out.several = several.len();
    out
}

/// C7's reading of a slot through its surviving `/ToUnicode`.
#[derive(Default)]
struct TrueText {
    /// Each code's character: the first of its text, the one the
    /// substituted font draws.
    used: BTreeMap<u16, char>,
    /// Each code's whole text: what the output's `/ToUnicode` maps it to.
    text: BTreeMap<u16, String>,
    /// The slot's text, a space at every token break and after every run.
    words: String,
    /// How many codes' text is several characters (a ligature, a conjunct,
    /// a combining sequence): the substituted font draws only the first.
    several: usize,
}

/// The database font `/BaseFont` `name` names (RR change #3): the subset
/// tag dropped, an entry whose id or PostScript name equals it, else one
/// that lists it as an alias, else one whose family equals it, all ignoring
/// case and spaces. A `CIDFont+Fn` name matches nothing. With `how`, what
/// matched. The ticket's rule is the family alone; a Save-As `/BaseFont` is
/// a PostScript name (`NotoSans-Bold`), which names one face where a family
/// names several, so the id, PostScript name and alias are tried first. No
/// part of a name stands for the whole: `Arial-BoldMT` never matches the
/// family `Arial`. A match is a candidate only: [`name_hit_holds`] checks
/// it against the slot's widths.
fn by_name<'f>(fonts: &'f FontDb, name: &str) -> Option<(&'f IndexEntry, &'static str)> {
    let name = strip_subset(name.strip_prefix('/').unwrap_or(name));
    if name.starts_with("CIDFont+") {
        return None;
    }
    let key = |s: &str| -> String {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let want = key(name);
    let entries = fonts.entries();
    let find = |test: &dyn Fn(&IndexEntry) -> bool| entries.iter().copied().find(|e| test(e));
    find(&|e| key(&e.id) == want || key(&e.postscript_name) == want)
        .map(|e| (e, "PostScript name"))
        .or_else(|| {
            find(&|e| e.base_font_aliases.iter().any(|a| key(a) == want)).map(|e| (e, "alias"))
        })
        .or_else(|| find(&|e| key(&e.family) == want).map(|e| (e, "family")))
}

/// `ABCDEF+Name` → `Name`.
fn strip_subset(name: &str) -> &str {
    match name.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()) => rest,
        _ => name,
    }
}

// ── asking ───────────────────────────────────────────────────────────────

/// What becomes of one font.
enum Choice {
    /// Substitute database font `id`; `why` makes the pass `Partial`; `kind`
    /// overrides the resolution `Picked` would be.
    Font {
        id: String,
        why: Option<String>,
        kind: Option<FontResolutionKind>,
    },
    /// Keep the font as found; its `/ToUnicode` text stays.
    TextOnly,
    /// Keep the font as found and give it a `/ToUnicode` over `used`: the
    /// codes as database font `id` reads them, a guess whose hit `hit` is
    /// below 1/2 (C8 under `UseBest`, module docs).
    Recover {
        id: String,
        hit: Ratio,
        used: BTreeMap<u16, char>,
    },
    /// Keep the font as found, for this reason.
    Leave(String),
}

/// The choice `decision` comes to, asking when it must (module docs).
fn answer(
    ctx: &mut RepairCtx<'_>,
    t: &Target<'_>,
    decision: FontDecision,
    family_slots: &[(String, Vec<(u32, String)>)],
    c8: bool,
) -> Choice {
    match decision {
        FontDecision::AutoAccept(c) => Choice::Font {
            kind: Some(picked(&c.font_id, c.confidence)),
            id: c.font_id,
            why: None,
        },
        FontDecision::Ask(req) => {
            let summary = InteractionSummary {
                kind: InteractionKind::FontPick,
                page: Some(req.page),
                slot: Some(req.slot.clone()),
                candidates: req.candidates.iter().map(|c| c.font_id.clone()).collect(),
            };
            let top = req
                .candidates
                .first()
                .map(|c| (c.font_id.clone(), c.confidence));
            // T-28's `resolve` puts each candidate's dictionary hit in
            // `score`.
            let top_hit = req.candidates.first().map_or(ZERO, |c| c.score);
            let confidence_of = |id: &str| {
                req.candidates
                    .iter()
                    .find(|c| c.font_id == id)
                    .map_or(ZERO, |c| c.confidence)
            };
            let Some(reply) = ask(ctx, InteractionRequest::FontPick(req.clone()), summary) else {
                return Choice::Leave("not substituted: the question was cancelled".to_owned());
            };
            let best = |why: Option<String>| match &top {
                Some((id, confidence)) => Choice::Font {
                    id: id.clone(),
                    why,
                    kind: Some(picked(id, *confidence)),
                },
                None => Choice::Leave("not substituted: no candidate font".to_owned()),
            };
            match reply {
                InteractionReply::Pick(id) if ctx.fonts.entry(&id).is_some() => Choice::Font {
                    kind: Some(picked(&id, confidence_of(&id))),
                    id,
                    why: None,
                },
                InteractionReply::Pick(id) => best(Some(format!(
                    "the picked font {id} is not in the font database; the best candidate \
                     was substituted"
                ))),
                // A weak C8 guess: recover the text, substitute nothing.
                InteractionReply::UseBest if c8 && top_hit < MIN_HIT => match &top {
                    Some((id, _)) => {
                        let used = read_through(ctx.fonts, id, &t.codes());
                        if used.is_empty() {
                            Choice::TextOnly
                        } else {
                            Choice::Recover {
                                id: id.clone(),
                                hit: top_hit,
                                used,
                            }
                        }
                    }
                    None => Choice::Leave("not substituted: no candidate font".to_owned()),
                },
                // `Ask` means the top candidate fell short of auto-accepting:
                // taking it unseen is a guess, never `Fixed` (a C8 guess
                // reads the codes through a font nothing confirmed).
                InteractionReply::UseBest => {
                    let c = top.as_ref().map_or(ZERO, |(_, c)| *c);
                    best(Some(format!(
                        "best candidate substituted unconfirmed: it fell short of \
                         auto-accepting (confidence {}/{})",
                        c.num, c.den
                    )))
                }
                InteractionReply::Skip => {
                    best(Some("font substituted without confirmation".to_owned()))
                }
                InteractionReply::Substitute(choice) => Choice::Font {
                    id: choice.font_id.clone(),
                    why: None,
                    kind: Some(FontResolutionKind::Substituted(choice)),
                },
                InteractionReply::TextOnly => Choice::TextOnly,
            }
        }
        FontDecision::Unreproducible(mut req) => {
            let key = batch_key(t, &req.family);
            let known = (ctx.notes.family_replies.iter())
                .find(|(f, _)| *f == key)
                .map(|(_, r)| r.clone());
            let reply = match known {
                Some(reply) => reply,
                None => {
                    if let Some((_, slots)) = family_slots.iter().find(|(f, _)| *f == key) {
                        req.slots = slots.clone();
                    }
                    let (page, slot) = req.slots.first().cloned().unzip();
                    let summary = InteractionSummary {
                        kind: InteractionKind::FontUnreproducible,
                        page,
                        slot,
                        candidates: req.options.iter().map(|o| o.font_id.clone()).collect(),
                    };
                    let reply = if ctx.opts.unreproducible == UnreproduciblePolicy::Ask {
                        let request = InteractionRequest::FontUnreproducible(req.clone());
                        let Some(reply) = ask(ctx, request, summary) else {
                            return Choice::Leave(
                                "not substituted: the question was cancelled".to_owned(),
                            );
                        };
                        reply
                    } else {
                        let reply = generic(&req.options);
                        ctx.notes.interactions.push(InteractionRecord {
                            request: summary,
                            reply: reply.clone(),
                            source: InteractionSource::Policy,
                        });
                        reply
                    };
                    ctx.notes.family_replies.push((key, reply.clone()));
                    reply
                }
            };
            let reply = match reply {
                InteractionReply::UseBest => generic(&req.options),
                other => other,
            };
            let why = || {
                Some(
                    "generic font substituted: no database font maps every character of the \
                     slot"
                        .to_owned(),
                )
            };
            match reply {
                InteractionReply::Substitute(choice) => Choice::Font {
                    id: choice.font_id.clone(),
                    why: why(),
                    kind: Some(FontResolutionKind::Substituted(choice)),
                },
                InteractionReply::Pick(id) => Choice::Font {
                    kind: Some(FontResolutionKind::Substituted(SubstituteChoice {
                        label: ctx
                            .fonts
                            .entry(&id)
                            .map_or_else(|| id.clone(), |e| e.family.clone()),
                        font_id: id.clone(),
                    })),
                    id,
                    why: why(),
                },
                InteractionReply::Skip => Choice::Leave(format!(
                    "font left as found: the question for {} was skipped",
                    req.family
                )),
                InteractionReply::TextOnly | InteractionReply::UseBest => Choice::TextOnly,
            }
        }
        FontDecision::TextOnly => {
            let family = batch_key(t, &family_of(t));
            if !ctx.notes.family_replies.iter().any(|(f, _)| *f == family) {
                let (page, slot) = t.slot_names().first().cloned().unzip();
                ctx.notes.interactions.push(InteractionRecord {
                    request: InteractionSummary {
                        kind: InteractionKind::FontUnreproducible,
                        page,
                        slot,
                        candidates: Vec::new(),
                    },
                    reply: InteractionReply::TextOnly,
                    source: InteractionSource::Policy,
                });
                ctx.notes
                    .family_replies
                    .push((family, InteractionReply::TextOnly));
            }
            Choice::TextOnly
        }
    }
}

/// The family T-28 names in an [`UnreproducibleRequest`](crate::engine::UnreproducibleRequest):
/// `/BaseFont` without its subset tag, else the first slot's name.
fn family_of(t: &Target<'_>) -> String {
    match &t.base_font {
        Some(name) => strip_subset(name).to_owned(),
        None => t
            .slot_names()
            .first()
            .map(|(_, s)| s.clone())
            .unwrap_or_default(),
    }
}

/// What one `FontUnreproducible` answer is shared by: `family` for a font
/// with a name, else the slot name and the font's object, so two nameless
/// fonts that sit in slots of one name on different pages are asked about
/// apart.
fn batch_key(t: &Target<'_>, family: &str) -> String {
    match t.base_font {
        Some(_) => family.to_owned(),
        None => format!("{family}@{} {}", t.top.0, t.top.1),
    }
}

/// The policy's answer to `FontUnreproducible`: the first option, else text
/// only.
fn generic(options: &[SubstituteChoice]) -> InteractionReply {
    options.first().map_or(InteractionReply::TextOnly, |o| {
        InteractionReply::Substitute(o.clone())
    })
}

/// Asks `request`, records the answer and who gave it (D-141), and gives
/// the reply; `None` (and the run marked cancelled) when the question was
/// cancelled.
fn ask(
    ctx: &mut RepairCtx<'_>,
    request: InteractionRequest,
    summary: InteractionSummary,
) -> Option<InteractionReply> {
    match ctx.ask.ask(request) {
        Ok(answer) => {
            ctx.notes.interactions.push(InteractionRecord {
                request: summary,
                reply: answer.reply.clone(),
                source: answer.recorded_source(),
            });
            Some(answer.reply)
        }
        Err(_) => {
            ctx.notes.cancelled = true;
            None
        }
    }
}

const ZERO: Ratio = Ratio { num: 0, den: 1 };
const ONE: Ratio = Ratio { num: 1, den: 1 };

fn picked(id: &str, confidence: Ratio) -> FontResolutionKind {
    FontResolutionKind::Picked {
        font_id: id.to_owned(),
        confidence,
    }
}

// ── applying ─────────────────────────────────────────────────────────────

/// Carries `choice` out on `ctx.doc`: the action's text, the `Partial`
/// reason if any, and the resolution.
fn apply(
    ctx: &mut RepairCtx<'_>,
    t: &Target<'_>,
    choice: Choice,
    c8: bool,
) -> (String, Option<String>, FontResolutionKind) {
    let pages = || {
        let names: Vec<String> = t
            .slot_names()
            .iter()
            .map(|(p, s)| format!("page {} /{s}", p + 1))
            .collect();
        names.join(", ")
    };
    match choice {
        Choice::Font { id, why, kind } => {
            let (used, text, several) = if c8 {
                (read_through(ctx.fonts, &id, &t.codes()), None, 0)
            } else {
                let text = true_text(t);
                let whole = (text.several > 0).then_some(text.text);
                (text.used, whole, text.several)
            };
            // A C7 code whose text is several characters keeps all of them.
            let harvested = harvest_from(ctx.fonts, &id, &used).map(|h| match &text {
                Some(t) => h.with_text(t),
                None => h,
            });
            // The input's widths lay the text out where it was.
            let widths = t.slots.first().and_then(|(page, slot)| {
                (ctx.doc).input_widths(ctx.carve, ctx.page_tree, *page, slot)
            });
            let harvested = harvested.map(|h| match widths {
                Some(w) => h.with_widths(w),
                None => h,
            });
            match harvested {
                Ok(font) => {
                    ctx.doc.substitute(t.slots.clone(), font);
                    let program = ctx.fonts.program_of(&id).unwrap_or(&id).to_owned();
                    let mut what = if program == id {
                        format!("font program substituted: {id}")
                    } else {
                        format!(
                            "font program substituted: {program} for {id} (the font database \
                             holds only its glyph map)"
                        )
                    };
                    if c8 {
                        what.push_str(&format!("; /ToUnicode rebuilt over {} codes", used.len()));
                    }
                    let mut why = why;
                    // A `.gmap`-only font is drawn with another font's
                    // program, which may lack some of its characters.
                    let missing = if program == id {
                        0
                    } else {
                        undrawn(ctx.fonts, &program, &used)
                    };
                    if missing > 0 {
                        let lost = format!(
                            "{missing} of {} characters have no glyph in {program} and are \
                             drawn as .notdef",
                            used.len()
                        );
                        what.push_str(&format!("; {lost}"));
                        why = Some(match why {
                            Some(w) => format!("{w}; {lost}"),
                            None => lost,
                        });
                    }
                    // A code whose /ToUnicode text is several characters
                    // keeps it whole, but its glyph is its first character's
                    // (`TrueText::several`).
                    if several > 0 {
                        let lost = format!(
                            "{several} codes whose /ToUnicode text is several characters are \
                             drawn with the glyph of their first; the /ToUnicode keeps their \
                             text whole"
                        );
                        let msg = format!("{} {} obj: {lost}", t.descriptor.0, t.descriptor.1);
                        ctx.sink.log(LogLevel::Warn, msg);
                        what.push_str(&format!("; {lost}"));
                        why = Some(match why {
                            Some(w) => format!("{w}; {lost}"),
                            None => lost,
                        });
                    }
                    let kind = kind.unwrap_or_else(|| picked(&id, ONE));
                    (what, why, kind)
                }
                Err(e) => {
                    let why = format!("not substituted: font {id} could not be harvested: {e}");
                    (why.clone(), Some(why), FontResolutionKind::Skipped)
                }
            }
        }
        Choice::TextOnly => (
            if c8 {
                format!(
                    "text only: no reproducible font; the font is kept as found for {}, and \
                     with no /ToUnicode its text stays unmapped",
                    pages()
                )
            } else {
                format!(
                    "text only: no reproducible font; the font is kept as found for {}, and \
                     its /ToUnicode keeps the text extractable",
                    pages()
                )
            },
            Some("text only: no reproducible font".to_owned()),
            FontResolutionKind::TextOnly,
        ),
        Choice::Recover { id, hit, used } => {
            let (mapped, all) = (used.len(), t.codes().len());
            ctx.doc.set_tounicode(t.top, used);
            (
                format!(
                    "text recovered, no font substituted: /ToUnicode rebuilt over {mapped} of \
                     {all} codes as {id} reads them, a guess nothing confirmed (hit {}/{}, \
                     below 1/2); the font is kept as found for {}",
                    hit.num,
                    hit.den,
                    pages()
                ),
                Some(RECOVERED.to_owned()),
                FontResolutionKind::TextOnly,
            )
        }
        Choice::Leave(why) => (why.clone(), Some(why), FontResolutionKind::Skipped),
    }
}

/// How many of the characters of `used` database font `program` has no
/// glyph for.
fn undrawn(fonts: &FontDb, program: &str, used: &BTreeMap<u16, char>) -> usize {
    let Some(gmap) = fonts.gmap(program) else {
        return used.len();
    };
    let drawn: BTreeSet<char> = gmap.records().iter().map(|r| r.unicode()).collect();
    used.values().filter(|c| !drawn.contains(c)).count()
}

/// The `Partial` reason of a weak C8 guess whose text was recovered (module
/// docs).
const RECOVERED: &str = "text recovered; font not confirmed";

/// C8: each code's character through font `id`'s `.gmap` (the code is the
/// glyph id); a code the `.gmap` does not map is left out.
fn read_through(fonts: &FontDb, id: &str, codes: &BTreeSet<u16>) -> BTreeMap<u16, char> {
    let Some(gmap) = fonts.gmap(id) else {
        return BTreeMap::new();
    };
    codes
        .iter()
        .filter_map(|&c| Some((c, gmap.unicode(c)?)))
        .collect()
}
