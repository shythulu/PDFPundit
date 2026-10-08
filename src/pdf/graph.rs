//! Object graph (T-09).

/// References between carved objects. Stub until T-09: it holds nothing.
#[derive(Debug, Default)]
pub(crate) struct ObjectGraph {}

impl ObjectGraph {
    /// Capacity of the edge maps, in bytes.
    pub(crate) fn heap_bytes(&self) -> u64 {
        0
    }
}
