//! Persistent state for the curator pass.

use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// On-disk curator state. Persisted at
/// `${HEARTQ_HOME}/.curator_state.json` (or wherever the caller puts it).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CuratorState {
    /// Last run timestamp (system time).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<SystemTime>,
    /// Total pass count (real runs only — dry-runs don't bump it).
    #[serde(default)]
    pub run_count: u64,
    /// Human-readable summary of the most recent run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_summary: Option<String>,
    /// Path to the most recent report (markdown).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_report_path: Option<PathBuf>,
    /// Counter of skills marked stale since the state was created.
    #[serde(default)]
    pub cumulative_marked_stale: u64,
    /// Counter of skills archived since the state was created.
    #[serde(default)]
    pub cumulative_archived: u64,
}

/// Result counters for a single transition pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TransitionStats {
    pub checked: usize,
    pub marked_stale: usize,
    pub archived: usize,
    pub reactivated: usize,
}

impl TransitionStats {
    pub fn is_empty(&self) -> bool {
        self.marked_stale == 0 && self.archived == 0 && self.reactivated == 0
    }
}

impl CuratorState {
    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    }
}