//! PDFPundit repairs corrupted PDFs and exports Markdown. The only way in is the
//! cat: the public surface is `run()` plus the PDF model, fixtures, writer and the
//! font-DB builder that `tools/build-templates` calls. The engine facade is
//! crate-private, with no cfg, feature or flag that exposes it (D-047).

pub(crate) mod appdirs;
pub(crate) mod bench;
pub(crate) mod config;
pub(crate) mod engine;
pub(crate) mod jobs;
// The view model (T-20) and the shell (T-23b) are the store's callers; until
// they land only the tests use it. The allow sits here so library.rs carries
// none (D-076). TODO(T-20, T-23b): remove it once both use the store.
#[allow(dead_code)]
pub(crate) mod library;
pub(crate) mod panic_guard;
pub mod pdf;
pub(crate) mod place;
pub(crate) mod ui;

use std::process::ExitCode;

/// Starts PDFPundit. Placeholder: T-23a gives it the terminal shell (refuse a
/// non-terminal, then the cat). Arguments are never read.
pub fn run() -> ExitCode {
    ExitCode::SUCCESS
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
