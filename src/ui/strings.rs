//! Every user-visible string, pose, reaction and theme name. [`ALL`] is the
//! deny-list the artefact test (T-14) scans every repaired PDF, report and
//! Markdown export for, so no cat reaches an artefact (goal point 3).
//!
//! Seeded by T-02b with the mockup's fixed set. The UI tickets that draw
//! strings append to [`ALL`]; nothing is ever removed from it.
// The layouts (T-21 on) and the artefact test (T-14) read these.
// TODO(T-14): remove this allow once the artefact test reads the table.
#![allow(dead_code)]

/// The seven themes, default first (UI D3).
pub const THEME_NAMES: [&str; 7] = [
    "DarkBerry Blackwater",
    "DarkBerry Mire",
    "DarkBerry Fen",
    "DarkBerry Wisp",
    "ACiD Classic",
    "Pastel Parlour",
    "Mono Ink",
];

/// The cat's poses, as the mockup names them (`POSES`).
pub const POSE_NAMES: [&str; 3] = ["closed", "open", "happy"];

/// The widget's poses, as the mockup names them (`WPOSES`).
pub const WIDGET_POSE_NAMES: [&str; 4] = ["idle", "working", "needs", "done"];

/// The chomp's named steps beyond "chomp" itself (the mockup's `CHOMP`
/// captions).
pub const CHOMP_STEP_NAMES: [&str; 4] = ["plop", "nom", "gulp", "burp"];

/// The two drop reactions: the chomp everywhere, plus the drag tracking in
/// kitty.
pub const REACTION_NAMES: [&str; 2] = ["chomp", "drag"];

/// The one-line fallback's face (UI:191).
pub const FACE: &str = "=^..^=";

/// The app's name as the status bars spell it.
pub const APP_NAME: &str = "PDFPuNDiT";

/// The widget's status bar while idle: the binary has no network code (D-050).
pub const OFFLINE: &str = "offline";

/// The widget's call to make the tile bigger when a file needs a decision
/// (decisions are never shown in the widget, UI D5).
pub const ZOOM_ME: &str = "zoom me";

/// The widget's status line when the batch is done.
pub const BURP: &str = "burp.";

/// The widget's status bar counts; `{n}` is the count.
pub const N_PDFS: &str = "{n} pdfs";
pub const N_QUEUED: &str = "{n} queued";
pub const N_DONE: &str = "{n} done";

// The full layout (T-22a).

/// The version the status bar shows: `v` and the crate's major.minor.
pub const VERSION: &str = concat!(
    "v",
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR")
);

/// The line under the logo.
pub const TAGLINE: &str = "·∙· f u r e n s i c   p d f   r e p a i r ·∙·";

/// The panel titles.
pub const LAST_CALLERS: &str = "LAST CALLERS";
pub const MENU: &str = "MENU";
pub const HOW_IT_WORKS: &str = "HOW iT WORKS";
pub const SYSTEM: &str = "SYSTEM";

/// LAST CALLERS' footer; the first `{n}` is the files, the second the runs.
pub const N_FILES_N_RUNS: &str = "{n} files · {n} runs";

/// MENU's items: the key, then the label.
pub const MENU_ITEMS: [(char, &str); 6] = [
    ('B', "browse for pdfs"),
    ('H', "history"),
    ('S', "setup"),
    ('T', "theme"),
    ('?', "help"),
    ('Q', "quit"),
];

/// The hotkeys row under the hint: the key, then the label.
pub const HOTKEYS: [(char, &str); 6] = [
    ('B', "browse"),
    ('H', "history"),
    ('S', "setup"),
    ('T', "theme"),
    ('?', "help"),
    ('Q', "quit"),
];

/// MENU's prompt, before its blinking cursor.
pub const MAIN_PROMPT: &str = "main »";

/// HOW iT WORKS, one line each: the step number (`None` for a step's second
/// line) and the text. Its last line names the output files.
pub const HOW_LINES: [(Option<char>, &str); 8] = [
    (Some('1'), "drop pdfs on the cat"),
    (Some('2'), "it repairs them"),
    (None, "all by itself"),
    (Some('3'), "it only asks when"),
    (None, "it gets stuck"),
    (Some('4'), "fixed copies land"),
    (None, "beside the original"),
    (None, "*.repaired.pdf"),
];

/// SYSTEM's labels, padded to the value column, and its fixed values.
pub const SYS_ENGINE: (&str, &str) = ("engine   ", "pure rust");
pub const SYS_NETWORK: (&str, &str) = ("network  ", "off");
pub const SYS_FONTS: (&str, &str) = ("fonts    ", "bundled");
pub const SYS_THEME: &str = "theme    ";
pub const SYS_HISTORY: &str = "history  ";
pub const SYS_ORIGINALS: (&str, &str) = ("originals", " untouched");

/// SYSTEM's history count.
pub const N_FILES: &str = "{n} files";

/// The status bar's node, and its state while the cat eats a drop.
pub const NODE_N: &str = "node {n}";
pub const EATING_N_PDFS: &str = "eating {n} pdfs";

/// The view model's batch states, as the status bar shows them (T-20).
pub const STATE_BUSY: &str = "busy";
pub const STATE_NEEDS_YOU: &str = "needs you";
/// Listed in [`ALL`] as the widget pose "idle".
pub const STATE_IDLE: &str = "idle";

/// The hint for a MENU key whose screen does not exist yet (D-048): history,
/// setup and help.
pub const NOT_YET: &str = "·∙· not yet ·∙·";

// The full layout's batch and result views (T-22b, frames 03 and 05).

/// The header row's breadcrumb after the app's name: the batch view's two
/// parts, and the result view's.
pub const CRUMB_BATCH: (&str, &str) = ("batch", "auto-repair ON");
pub const CRUMB_RESULTS: &str = "results";

/// The queue box: its title, its counts note (`{n}` the count; the first
/// note part's `{n}` is the done files, the second the total) and its column
/// headings.
pub const QUEUE: &str = "QUEUE";
pub const N_OF_N_DONE: &str = "{n}/{n} done";
/// One file waiting on you, or any other number of them.
pub const N_NEEDS_INPUT: &str = "{n} needs input";
pub const N_NEED_INPUT: &str = "{n} need input";
pub const N_FAILED: &str = "{n} failed";
pub const QUEUE_HEADINGS: &str = "   file                  status";

/// The box asking for a decision. The second line follows the parked file's
/// count of fonts ("2 fonts").
pub const NEEDS_INPUT_TITLE: &str = "‼ NEEDS iNPUT";
pub const CAN_BE_READ_NOT_REPRODUCED: (&str, &str, &str, &str) =
    ("can be", "read", "but not", "reproduced");
pub const PARKED_KEEPS_GOING: &str = "parked · the batch keeps going.";
pub const NEEDS_INPUT_KEYS: [(&str, &str); 2] = [("i", "resolve now"), ("l", "later")];

/// The batch view's line under the cat.
pub const DROP_MORE: &str = "drop more pdfs on the cat to queue them";

/// The analysis panel: its title before the file's name, and its headings.
pub const ANALYSIS: &str = "ANALYSiS »";
pub const FINDINGS: &str = "FiNDiNGS";
pub const STREAMED_AS_FOUND: &str = "streamed as found";
pub const LOG: &str = "LOG";

/// A finding's level tag.
pub const TAG_ERR: &str = "[ERR]";
pub const TAG_WRN: &str = "[WRN]";
pub const TAG_INF: &str = "[iNF]";

/// The two-line progress box: the batch line (`{n}` done, then total) and
/// what follows it while a file is parked or once one failed; a percentage.
pub const BATCH_N_OF_N: &str = "batch {n}/{n}";
pub const N_PARKED: &str = "{n} parked";
pub const N_PERCENT: &str = "{n}%";

/// The batch view's hotkeys, and the hotkeys while the file menu is open.
pub const BATCH_KEYS: [(&str, &str); 6] = [
    ("↑↓", "select"),
    ("enter", "file menu"),
    ("i", "resolve"),
    ("+", "add files"),
    ("p", "pause"),
    ("q", "quit"),
];
pub const MENU_KEYS: [(&str, &str); 4] = [
    ("↑↓", "move"),
    ("enter", "choose"),
    ("esc", "close menu"),
    ("q", "quit"),
];

/// The result panel: its title before the file's name and its headings.
pub const RESULT: &str = "RESULT »";
pub const BEFORE_AFTER: &str = "before after";
pub const RECOVERY: &str = "RECOVERY";
pub const FONTS: &str = "FONTS";

/// A font row's words: where the font came from, then what became of it.
/// The intact and bundled rows are frame 05's; a re-linked, text-only or
/// skipped slot has no frame yet, so these are T-22b's words until one
/// exists. [`FONT_UNKNOWN`] stands in for a name the file does not give; it
/// is not in [`ALL`], because the engine's reports say "unknown" of things.
pub const FONT_EMBEDDED: &str = "embedded";
pub const FONT_BUNDLED: &str = "bundled";
pub const FONT_INTACT: &str = "intact";
pub const FONT_RELINKED: &str = "relinked";
pub const FONT_TEXT_ONLY: &str = "text only";
pub const FONT_LEFT_AS_FOUND: &str = "left as found";
pub const FONT_UNKNOWN: &str = "unknown";

/// The result panel's actions row.
pub const RESULT_KEYS: [(&str, &str); 4] = [
    ("o", "open"),
    ("e", "export .md"),
    ("d", "re-diagnose"),
    ("c", "copy"),
];

/// The result panel's sign-off and the cat's line under it.
pub const CASE_CLOSED: &str = "─── case closed · the cat has inspected this pdf ───";
pub const CAT_SATISFIED: (&str, &str) = ("the cat is satisfied.", "burp.");

/// The per-file menu (D-048): its title and items, `None` a separator. The
/// export item is DA:472's `Export → <name>.md` action, labelled as frame 05
/// draws it.
pub const FILE_MENU: &str = "FiLE";
pub const FILE_MENU_ITEMS: [Option<(&str, char)>; 8] = [
    Some(("Open repaired PDF", 'o')),
    Some(("Reveal in folder", 'f')),
    Some((EXPORT_MARKDOWN, 'e')),
    Some(("Re-diagnose", 'd')),
    None,
    Some(("Repair options…", 'R')),
    Some(("Copy report", 'c')),
    Some(("Remove from queue", 'x')),
];
pub const EXPORT_MARKDOWN: &str = "Export → Markdown";

/// The last row of a findings or fonts list that has more entries than rows:
/// how many are not shown (`{n}`).
pub const N_MORE: &str = "… {n} more";

/// A finding's location in the analysis panel's detail column: an object
/// (`{n}` its number, then its generation) or a page (`{n}` counting from 1).
/// Neither is in [`ALL`]: "obj" is in every PDF, so the artefact scan would
/// flag every repaired file.
pub const LOC_OBJ: &str = "obj {n} {n}";
pub const LOC_PAGE: &str = "p.{n}";

/// The artefact deny-list. Several entries are ordinary words ("open",
/// "closed", "happy", "drag", "idle", "working", "needs", "done", "nom"), so
/// T-14's scan must match whole words (else "nom" flags "nominal"), and the
/// engine's fixed report and export text must avoid these words.
pub const ALL: &[&str] = &[
    // the one-line fallback's face and the needs-you mark
    "=^..^=",
    "‼",
    "feed me",
    "feed me a pdf",
    // reactions
    "chomp",
    "drag",
    // poses
    "closed",
    "open",
    "happy",
    // widget poses
    "idle",
    "working",
    "needs",
    "done",
    // chomp steps
    "plop",
    "nom",
    "gulp",
    "burp",
    // themes
    "DarkBerry Blackwater",
    "DarkBerry Mire",
    "DarkBerry Fen",
    "DarkBerry Wisp",
    "ACiD Classic",
    "Pastel Parlour",
    "Mono Ink",
    // the director's hints, full then widget (T-19); `{n}` is the file count
    "·∙· drop a pdf on the cat ·∙·",
    "·∙· the cat has noticed something ·∙·",
    "» release to feed the cat «",
    "·∙· plop · {n} pdfs landed on the cat ·∙·",
    "» the cat is eating your pdfs «",
    "» c h o m p «",
    "·∙· nom ·∙·",
    "·∙· nom nom nom ·∙·",
    "·∙· gulp ·∙·",
    "·∙· burp. {n} pdfs queued for repair ·∙·",
    "·∙ feed me a pdf ∙·",
    "·∙ the cat has noticed ∙·",
    "» release to feed «",
    "·∙ plop · {n} pdfs ∙·",
    "» nom time «",
    "·∙ nom ∙·",
    "·∙ nom nom nom ∙·",
    "·∙ gulp ∙·",
    "·∙ burp. {n} queued ∙·",
    // the director's captions and effects (T-19)
    "the meme: file enters",
    "ears up",
    "turns",
    "looks up",
    "tracks it",
    "jaw drops",
    "wider",
    "ready to eat",
    "the drop: plop",
    "nom nom",
    "ears down",
    "CHOMP!",
    // the widget and the one-line fallback (T-21)
    APP_NAME,
    OFFLINE,
    ZOOM_ME,
    BURP,
    N_PDFS,
    N_QUEUED,
    N_DONE,
    // the full layout (T-22a): every word it draws; a MENU or hotkey label
    // with its key, so a lone "help" or "history" in an artefact is not a hit
    VERSION,
    TAGLINE,
    LAST_CALLERS,
    MENU,
    HOW_IT_WORKS,
    SYSTEM,
    N_FILES_N_RUNS,
    "[B] browse for pdfs",
    "[H] history",
    "[S] setup",
    "[T] theme",
    "[?] help",
    "[Q] quit",
    "[B] browse",
    MAIN_PROMPT,
    "1 drop pdfs on the cat",
    "2 it repairs them",
    "all by itself",
    "3 it only asks when",
    "it gets stuck",
    "4 fixed copies land",
    "beside the original",
    "*.repaired.pdf",
    "engine   pure rust",
    "network  off",
    "fonts    bundled",
    "history  {n} files",
    "originals untouched",
    NODE_N,
    EATING_N_PDFS,
    STATE_BUSY,
    STATE_NEEDS_YOU,
    NOT_YET,
    // the full layout's batch and result views (T-22b), as drawn: the
    // breadcrumbs behind their chevron, keyed labels with their key, and the
    // font words in the phrases a row draws them in
    "» batch · auto-repair ON",
    "» results",
    QUEUE,
    N_OF_N_DONE,
    N_NEEDS_INPUT,
    N_NEED_INPUT,
    N_FAILED,
    QUEUE_HEADINGS,
    NEEDS_INPUT_TITLE,
    "can be read but not reproduced.",
    PARKED_KEEPS_GOING,
    "[i] resolve now",
    "[l] later",
    DROP_MORE,
    ANALYSIS,
    "FiNDiNGS streamed as found",
    LOG,
    TAG_ERR,
    TAG_WRN,
    TAG_INF,
    BATCH_N_OF_N,
    N_PARKED,
    N_PERCENT,
    "[↑↓] select",
    "[enter] file menu",
    "[i] resolve",
    "[+] add files",
    "[p] pause",
    "[q] quit",
    "[↑↓] move",
    "[enter] choose",
    "[esc] close menu",
    RESULT,
    "FiNDiNGS before after",
    RECOVERY,
    FONTS,
    "embedded intact",
    "embedded relinked",
    FONT_TEXT_ONLY,
    FONT_LEFT_AS_FOUND,
    "[o] open",
    "[e] export .md",
    "[d] re-diagnose",
    "[c] copy",
    CASE_CLOSED,
    "the cat is satisfied. burp.",
    FILE_MENU,
    "Open repaired PDF",
    "Reveal in folder",
    EXPORT_MARKDOWN,
    "Re-diagnose",
    "Repair options…",
    "Copy report",
    "Remove from queue",
    N_MORE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_holds_the_seeded_set() {
        assert!(!ALL.is_empty());
        for s in ["=^..^=", "‼", "feed me", "chomp"] {
            assert!(ALL.contains(&s), "{s:?} missing from ALL");
        }
        for s in THEME_NAMES
            .iter()
            .chain(&POSE_NAMES)
            .chain(&WIDGET_POSE_NAMES)
            .chain(&CHOMP_STEP_NAMES)
            .chain(&REACTION_NAMES)
        {
            assert!(ALL.contains(s), "{s:?} missing from ALL");
        }
        assert_eq!(
            THEME_NAMES[0], "DarkBerry Blackwater",
            "the default theme first"
        );
    }

    /// The full layout's hand-written entries are its arrays as drawn: each
    /// `[K] label`, numbered step and SYSTEM row is in `ALL`, and every such
    /// entry in `ALL` is one of them, so a renamed label leaves no stale one.
    #[test]
    fn the_full_layouts_entries_follow_its_arrays() {
        let mut keyed: Vec<String> = MENU_ITEMS
            .iter()
            .chain(&HOTKEYS)
            .map(|(k, l)| format!("[{k}] {l}"))
            .collect();
        keyed.extend(
            BATCH_KEYS
                .iter()
                .chain(&MENU_KEYS)
                .chain(&RESULT_KEYS)
                .chain(&NEEDS_INPUT_KEYS)
                .map(|(k, l)| format!("[{k}] {l}")),
        );
        keyed.extend([TAG_ERR, TAG_WRN, TAG_INF].map(String::from));
        let how: Vec<String> = HOW_LINES
            .iter()
            .map(|(n, t)| n.map_or_else(|| (*t).to_string(), |n| format!("{n} {t}")))
            .collect();
        let sys: Vec<String> = [
            SYS_ENGINE,
            SYS_NETWORK,
            SYS_FONTS,
            (SYS_HISTORY, N_FILES),
            SYS_ORIGINALS,
        ]
        .iter()
        .map(|(l, v)| format!("{l}{v}"))
        .collect();
        for s in keyed.iter().chain(&how).chain(&sys) {
            assert!(ALL.contains(&s.as_str()), "{s:?} missing from ALL");
        }
        let numbered = |s: &str| {
            let mut c = s.chars();
            c.next().is_some_and(|d| d.is_ascii_digit()) && c.next() == Some(' ')
        };
        let sys_label = |s: &str| {
            [
                SYS_ENGINE.0,
                SYS_NETWORK.0,
                SYS_FONTS.0,
                SYS_HISTORY,
                SYS_ORIGINALS.0,
            ]
            .iter()
            .any(|l| s.starts_with(l.trim_end()) && s.len() > l.trim_end().len())
        };
        for &s in ALL {
            if s.starts_with('[') {
                assert!(keyed.iter().any(|k| k == s), "stale {s:?} in ALL");
            }
            if numbered(s) {
                assert!(how.iter().any(|h| h == s), "stale {s:?} in ALL");
            }
            if sys_label(s) {
                assert!(sys.iter().any(|r| r == s), "stale {s:?} in ALL");
            }
        }
    }

    /// The per-file menu's export item (D-048, DA:472) and every other item
    /// are in the deny-list as drawn.
    #[test]
    fn the_file_menus_labels_are_in_all() {
        assert!(ALL.contains(&EXPORT_MARKDOWN));
        assert!(FILE_MENU_ITEMS.contains(&Some((EXPORT_MARKDOWN, 'e'))));
        for (label, _) in FILE_MENU_ITEMS.iter().flatten() {
            assert!(ALL.contains(label), "{label:?} missing from ALL");
        }
    }

    #[test]
    fn all_has_no_empty_or_duplicate_entry() {
        // An empty entry would match every artefact.
        assert!(ALL.iter().all(|s| !s.trim().is_empty()));
        let mut sorted = ALL.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ALL.len());
    }
}
