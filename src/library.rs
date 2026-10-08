//! `HistoryStore` and its JSON backend (T-16).
// The store (T-16) builds `HistorySummary` and the view model (T-20) reads it.
// TODO(T-16, T-20): remove this allow once both use it.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// What the idle screen shows of the history (eng-r3-q4): totals and the five
/// newest files. T-16's store builds it; the view model reads it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HistorySummary {
    pub files: u64,
    pub runs: u64,
    /// At most 5, newest first.
    pub recent: Vec<RecentRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentRow {
    pub name: String,
    pub status: RecentStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecentStatus {
    Repaired,
    Partial,
    Pending,
    Failed,
}
