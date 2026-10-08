//! Every user-visible string, pose, reaction and theme name. [`ALL`] is the
//! deny-list the artefact test (T-14) scans every repaired PDF, report and
//! Markdown export for, so no cat reaches an artefact (goal point 3).
//!
//! Seeded by T-02b with the mockup's fixed set. The UI tickets that draw
//! strings append to [`ALL`]; nothing is ever removed from it.
// The layouts (T-21 on) and the artefact test (T-14) read these.
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

/// The two drop reactions: the chomp everywhere, plus the drag tracking in
/// kitty.
pub const REACTION_NAMES: [&str; 2] = ["chomp", "drag"];

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
    // themes
    "DarkBerry Blackwater",
    "DarkBerry Mire",
    "DarkBerry Fen",
    "DarkBerry Wisp",
    "ACiD Classic",
    "Pastel Parlour",
    "Mono Ink",
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
        for s in THEME_NAMES.iter().chain(&POSE_NAMES).chain(&REACTION_NAMES) {
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
