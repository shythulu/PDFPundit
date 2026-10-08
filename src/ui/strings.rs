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
