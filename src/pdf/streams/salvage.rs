//! C9 salvage ladder with a work budget (T-08).

/// Every Flate stream's salvage outcome. In memory only, never serialised.
/// Stub until T-08: it holds nothing.
#[derive(Debug, Default)]
pub(crate) struct SalvageIndex {}

impl SalvageIndex {
    /// Capacity of every retained `data` and `survivors` vector, in bytes.
    pub(crate) fn heap_bytes(&self) -> u64 {
        0
    }
}
