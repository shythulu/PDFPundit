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

// The modals (T-24): the font pick (frame 04) and the theme chooser (frame
// 06). Strings with `{F}` tokens are the canvas's colour markup; the deny-list
// holds their plain text.

/// The font pick's title, before the file's name.
pub const PICK_A_FONT: &str = "PiCK A FONT »";
/// Which of the file's font questions this is: the first `{n}` this one, the
/// second how many.
pub const N_OF_M: &str = "{n} of {n}";
/// The slot line's words: `slot F3 (CIDFont+F1) · first seen p.12 · 418
/// glyph codes · language guess arabic`.
pub const SLOT: &str = "slot";
pub const FIRST_SEEN_P: &str = "first seen p.";
pub const GLYPH_CODES: &str = "glyph codes";
pub const LANGUAGE_GUESS: &str = "language guess";
/// Why the cat asks, when the slot's `/ToUnicode` survived (C7) and when it
/// did not (C8).
pub const WHY_TOUNICODE: &str = "{w}The text decodes through the surviving {W}/ToUnicode{w}, but no bundled font matched by name.";
pub const WHY_NO_TOUNICODE: &str =
    "{w}No {W}/ToUnicode{w} survives, so each preview is decoded through its own candidate.";
/// What the cat asks.
pub const PICK_THE_CANDIDATE: &str = "{w}Pick the candidate whose preview {W}reads correctly{w}.";
/// The candidate table's column heads.
pub const CANDIDATE_COLUMNS: [&str; 4] = ["candidate", "score", "fit", "conf"];
/// The top-ranked candidate's mark.
pub const BEST_GUESS: &str = "◄ best guess";
/// The label before each candidate's preview.
pub const PREVIEW: &str = "preview";
/// The font source row. v1 has only the bundled fonts: system fonts by name
/// wait on fontique (M5, D-035), so `tab` switches nothing yet.
pub const FONT_SOURCE: &str = "{D}font source  {C}(•){W} bundled sister fonts   {D}( ){w} system fonts by name   {D}[{Y}tab{D}]{D} switch";
/// The font pick's buttons.
pub const PICK_BUTTON: &str = "{W/m} ► Pick {/K}";
pub const USE_BEST_BUTTON: &str = "{D}[ {W}Use best for both {D}]";
pub const SKIP_BUTTON: &str = "{D}[ {w}Skip {D}]{D} keeps best guess, marks finding {Y}partial";
/// The font pick's keys.
pub const FONT_PICK_KEYS: &str =
    "{D}↑↓ candidate · enter pick · b best for both · s skip · esc later (file stays parked)";
/// A candidate's language label (T-28's `Lang` labels) as the slot line
/// names it; any other label is shown as it is.
pub const LANGUAGES: [(&str, &str); 6] = [
    ("ar", "arabic"),
    ("en", "english"),
    ("es", "spanish"),
    ("fr", "french"),
    ("hi", "hindi"),
    ("zh", "chinese"),
];
/// The status bar's state while a font question is open; `{file}` is the
/// file's name.
pub const RESOLVING_FILE: &str = "resolving {file}";

/// The theme chooser's title and its note; `{n}` is the number of themes.
pub const THEME_TITLE: &str = "THEME";
pub const N_THEMES: &str = "{n} themes";
pub const LIVE_PREVIEW: &str = "live preview";
/// The mark of the default theme.
pub const DEFAULT_MARK: &str = "★";
/// Under the theme list, with their columns: loading a theme file and
/// copying the current one are not built in v1 (flagged for the user, as
/// D-048's menu items are), so no key answers them yet.
pub const THEME_FILE_LINES: [(i32, &str); 6] = [
    (6, "{D}+ {w}load theme file…"),
    (8, "{D}~/.config/pdfpundit/themes/"),
    (8, "{D}catppuccin-style palette.json"),
    (8, "{D}or *.toml · 16 ansi + ramps"),
    (8, "{D}[{Y}e{D}]{w} copy & edit current"),
    (8, "{Y}★{D} default"),
];
/// One line about each theme, in [`THEME_NAMES`] order.
pub const THEME_DESCS: [&str; 7] = [
    "bog-witch berry · darkest",
    "bog-witch berry · dark",
    "bog-witch berry · dusk",
    "bog-witch berry · light",
    "the original 16 · 1994",
    "lilac · blush · mint",
    "greyscale · low colour",
];
/// What the preview pane says the theme can be drawn in.
pub const COLOUR_SUPPORT: &str =
    "{D}truecolor {G}√ {D}· 256-colour {G}√ {D}· 16-colour fallback {G}√";
/// Where a DarkBerry flavour comes from; the palette's version follows.
pub const PALETTE_SOURCE: &str = "{D}source {C}darkberry.slacklab.ca {D}· palette.json v";
/// The preview pane's headings.
pub const ROLES: &str = "ROLES";
pub const GRADIENTS: &str = "GRADIENTS";
pub const ANSI_0_15: &str = "ANSi 0–15";
pub const SAMPLE: &str = "SAMPLE";
/// The two columns of roles: the slot letter, then what it colours.
pub const ROLE_LABELS: [[(char, &str); 5]; 2] = [
    [
        ('W', "headings"),
        ('w', "body text"),
        ('C', "file names"),
        ('Y', "hotkeys · warn"),
        ('D', "dim · shadows"),
    ],
    [
        ('R', "errors"),
        ('G', "repaired · ok"),
        ('M', "needs input"),
        ('c', "info"),
        ('b', "lightbar · status"),
    ],
];
/// The gradients' labels: border, logo, progress, attention (the modals) and
/// batch.
pub const GRADIENT_LABELS: [&str; 5] = ["border", "logo", "progress", "attention", "batch"];
/// The ANSI rows' labels and the eight colours' names.
pub const ANSI_ROWS: [&str; 2] = ["0–7", "8–15"];
pub const ANSI_NAMES: [&str; 8] = ["blk", "red", "grn", "yel", "blu", "mag", "cyn", "wht"];
/// The sample queue the preview pane draws in the theme: its title, its note
/// and its rows (the first on the lightbar), then its bar's percentage.
pub const SAMPLE_TITLE: &str = "QUEUE";
pub const SAMPLE_NOTE: &str = "{G}3{w}/7 {D}·{M} 1 needs input";
pub const SAMPLE_ROWS: [&str; 4] = [
    "{C/b}☼  {W}invoice_scan.pdf     repairing · C9 salvage",
    "{G}√  {C}report_2024.pdf      {w}repaired · 3 fixed",
    "{M}‼  {C}thesis_ar.pdf        {M}needs input · 2 fonts",
    "{R}[ERR] {w}C9 zlib stream   {Y}[WRN] {w}C3 trailer   {c}[iNF] {w}header ok",
];
pub const SAMPLE_PCT: &str = "{W} 71%";
/// The theme chooser's keys.
pub const THEME_KEYS: &str = "{D}↑↓ preview (screen recolours live) · enter apply · esc cancel";
/// The status bar's state while the theme chooser is open.
pub const CHOOSING_THEME: &str = "choosing theme";

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
    // the modals (T-24): their phrases as drawn, markup and button brackets
    // removed; lone column heads and labels ("score", "errors", "partial")
    // are left out, so an ordinary report word is not a hit
    PICK_A_FONT,
    "first seen p.{n}",
    "{n} glyph codes",
    "language guess",
    "The text decodes through the surviving /ToUnicode, but no bundled font matched by name.",
    "No /ToUnicode survives, so each preview is decoded through its own candidate.",
    "Pick the candidate whose preview reads correctly.",
    BEST_GUESS,
    "font source  (•) bundled sister fonts   ( ) system fonts by name   [tab] switch",
    " ► Pick ",
    "Use best for both",
    "keeps best guess, marks finding partial",
    "↑↓ candidate · enter pick · b best for both · s skip · esc later (file stays parked)",
    RESOLVING_FILE,
    "{n} themes · live preview",
    "+ load theme file…",
    "~/.config/pdfpundit/themes/",
    "catppuccin-style palette.json",
    "or *.toml · 16 ansi + ramps",
    "copy & edit current",
    "★ default",
    "bog-witch berry · darkest",
    "bog-witch berry · dark",
    "bog-witch berry · dusk",
    "bog-witch berry · light",
    "the original 16 · 1994",
    "lilac · blush · mint",
    "greyscale · low colour",
    "truecolor √ · 256-colour √ · 16-colour fallback √",
    "source darkberry.slacklab.ca · palette.json v",
    ANSI_0_15,
    "↑↓ preview (screen recolours live) · enter apply · esc cancel",
    CHOOSING_THEME,
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
        let keyed: Vec<String> = MENU_ITEMS
            .iter()
            .chain(&HOTKEYS)
            .map(|(k, l)| format!("[{k}] {l}"))
            .collect();
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
