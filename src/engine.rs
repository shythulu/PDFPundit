//! The crate-private engine facade (plan §4). The module is `pub(crate)` with no
//! cfg, feature or flag that exposes it (D-047).
#![deny(clippy::iter_over_hash_type)]

#[cfg(test)]
mod artefact_tests;
#[cfg(test)]
mod tests;
