//! Object carver (T-05, T-07).

/// Everything the carve found. Stub until T-05: it holds nothing.
#[derive(Debug, Default)]
pub(crate) struct CarveReport {}

impl CarveReport {
    /// Capacity of every owned `Vec` and `String`, in bytes.
    pub(crate) fn heap_bytes(&self) -> u64 {
        0
    }
}
