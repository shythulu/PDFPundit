//! PDFPundit repairs corrupted PDFs and exports Markdown. The only way in is the
//! cat: the public surface is `run()` plus the PDF model, fixtures, writer and the
//! font-DB builder that `tools/build-templates` calls. The engine facade is
//! crate-private, with no cfg, feature or flag that exposes it (D-047).

pub(crate) mod appdirs;
pub(crate) mod bench;
pub(crate) mod config;
pub(crate) mod engine;
pub(crate) mod jobs;
// The shell records runs and reads the summary (T-23a); listing a file's runs
// and deleting a file wait on the history screen, which has no frame yet
// (D-048). A temporary dead-code allow only, not a disallowed-type or clock
// allow: library.rs needs none (D-076).
// TODO(D-048): remove it once the history screen uses the store.
#[allow(dead_code)]
pub(crate) mod library;
pub(crate) mod panic_guard;
pub mod pdf;
pub(crate) mod place;
pub(crate) mod ui;

use std::process::ExitCode;

/// Starts PDFPundit: refuses a non-terminal stdin or stdout with exit code 2,
/// otherwise draws the cat and runs until quit. Arguments are never read.
pub fn run() -> ExitCode {
    ui::app::run()
}

#[cfg(test)]
mod surface_tests;

#[cfg(test)]
mod tests {
    #[test]
    fn export_feature_is_on_by_default() {
        // black_box: a test that fails at run time, not a const assertion.
        assert!(std::hint::black_box(cfg!(feature = "export")));
    }
}
