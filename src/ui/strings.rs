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
