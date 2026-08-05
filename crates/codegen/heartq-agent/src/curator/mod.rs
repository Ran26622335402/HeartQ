//! Background skill-curation pass — Phase F of the Hermes integration.
//!
//! Translated from Hermes Agent's `agent/curator.py::run_curator_review`.
//!
//! ## v1 scope
//!
//! This implementation covers the **deterministic** portion only:
//!
//! - Walk `~/.heartq/skills/` for candidate skills
//! - Read per-skill `use_count` / `last_used_at` metadata (sidecar JSON)
//! - Apply three transitions based on time thresholds:
//!   - `mark_stale` — `last_used_at + stale_after_days < now`
//!   - `archive` — `last_used_at + archive_after_days < now`
//!   - `reactivate` — skill is marked stale but was referenced since
//!
//! The LLM-judged "umbrella consolidation" pass is **explicitly out of
//! scope** for v1 — it requires an LLM-orchestration step. We surface
//! the inputs the LLM would consume (a structured candidate list) so a
//! later pass can be plugged in.
//!
//! ## Configuration
//!
//! | key | type | default | meaning |
//! |---|---|---|---|
//! | `curator.enabled` | bool | `false` | master switch |
//! | `curator.interval_hours` | u32 | `24` | minimum gap between runs |
//! | `curator.min_idle_hours` | f64 | `1.0` | caller idle requirement |
//! | `curator.stale_after_days` | u32 | `30` | mark stale threshold |
//! | `curator.archive_after_days` | u32 | `90` | archive threshold |
//!
//! Defaults preserve the prior behavior (nothing runs).

pub mod llm;
pub mod state;
pub mod transitions;

pub use llm::{
    ConsolidationCandidate, collect_consolidation_candidates, plan_llm_curator_prompt,
    run_llm_curator_review,
};
pub use state::{CuratorState, TransitionStats};
pub use transitions::{apply_automatic_transitions, SkillUsage, run_curator_review};

use std::path::PathBuf;
use std::time::SystemTime;

/// Configuration knobs. All fields are public so a single struct literal
/// works at every call site.
#[derive(Debug, Clone)]
pub struct CuratorConfig {
    pub enabled: bool,
    pub interval_hours: u32,
    pub min_idle_hours: f64,
    pub stale_after_days: u32,
    pub archive_after_days: u32,
    pub skills_root: PathBuf,
    /// When `true`, no transitions are persisted and no `archive/`
    /// subdirectory is created. Reports still record what *would*
    /// happen.
    pub dry_run: bool,
}

impl Default for CuratorConfig {
    fn default() -> Self {
        let home = std::env::var("HEARTQ_HOME")
            .or_else(|_| std::env::var("HERMES_HOME"))
            .unwrap_or_else(|_| {
                let h = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                format!("{h}/.heartq")
            });
        Self {
            enabled: false,
            interval_hours: 24,
            min_idle_hours: 1.0,
            stale_after_days: 30,
            archive_after_days: 90,
            skills_root: PathBuf::from(home).join("skills"),
            dry_run: false,
        }
    }
}

impl CuratorConfig {
    /// Decide whether a curator pass should run **now**. Returns
    /// `ShouldRun::Yes` only when enabled + interval + state all allow.
    pub fn should_run_now(
        &self,
        state: &CuratorState,
        idle_for_seconds: Option<f64>,
        now: SystemTime,
    ) -> ShouldRun {
        if !self.enabled {
            return ShouldRun::Disabled;
        }
        if let Some(idle) = idle_for_seconds {
            if idle < self.min_idle_hours * 3600.0 {
                return ShouldRun::NotIdle;
            }
        }
        if let Some(last) = state.last_run_at {
            let elapsed = now
                .duration_since(last)
                .map(|d| d.as_secs())
                .unwrap_or(u64::MAX);
            if elapsed < (self.interval_hours as u64) * 3600 {
                return ShouldRun::TooSoon;
            }
        }
        ShouldRun::Yes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShouldRun {
    Yes,
    Disabled,
    NotIdle,
    TooSoon,
}