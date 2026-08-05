//! `[model_router]` configuration.

use crate::tiers::Tier;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How aggressively the router mutates the live session model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RolloutPhase {
    /// Record decisions only; keep the session model unchanged.
    #[default]
    Observe,
    /// Apply the routed catalog model for the current turn (ephemeral).
    Full,
}

/// Which classifier implementation to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouterStrategyKind {
    /// Rule / feature heuristic (always available, no Python).
    #[default]
    Heuristic,
    /// OpenSquilla SquillaRouter V4 Phase 3 via Python sidecar.
    SquillaV4,
}

/// Per-tier mapping to a HeartQ catalog key (`[model.<name>]` section name).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TierModelConfig {
    /// Catalog key or API model slug resolved via ModelsManager.
    pub model: String,
}

impl Default for TierModelConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
        }
    }
}

/// Full `[model_router]` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelRouterConfig {
    /// Master switch. Default off so existing deployments are unchanged.
    pub enabled: bool,
    pub rollout_phase: RolloutPhase,
    /// `heuristic` (default) or `squilla_v4` (Python sidecar + V4 bundle).
    pub strategy: RouterStrategyKind,
    pub default_tier: Tier,
    /// Confidence gate for squilla_v4 (below → default_tier).
    pub confidence_threshold: f32,
    /// Soft budget for heuristic path (milliseconds).
    pub timeout_ms: u64,
    pub complaint_upgrade: bool,
    pub complaint_upgrade_steps: u8,
    pub anti_downgrade: bool,
    /// Estimated context-token floors (OpenSquilla-inspired).
    pub large_context_c2_tokens: u64,
    pub large_context_c3_tokens: u64,
    /// Python interpreter for the Squilla sidecar (default: crate `.venv` or `python3`).
    pub squilla_python: Option<String>,
    /// Path to `classify.py` (default: crate `squilla-runtime/classify.py`).
    pub squilla_script: Option<String>,
    /// Override V4 bundle directory (`HEARTQ_SQUILLA_BUNDLE` also works).
    pub squilla_bundle_dir: Option<String>,
    /// Sidecar wall-clock timeout (model load + predict). Default 60s.
    pub squilla_timeout_ms: u64,
    /// When squilla_v4 fails, fall back to heuristic instead of skipping.
    pub squilla_fallback_heuristic: bool,
    /// Forwarded to V4 InferenceCore (`use_aux_head`).
    pub squilla_use_aux_head: bool,
    /// Tier → catalog model. Missing tiers fall back to `default_tier`'s model,
    /// then to an empty string (caller keeps current session model).
    pub tiers: BTreeMap<String, TierModelConfig>,
}

impl Default for ModelRouterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            rollout_phase: RolloutPhase::Observe,
            strategy: RouterStrategyKind::Heuristic,
            default_tier: Tier::C1,
            confidence_threshold: 0.5,
            timeout_ms: 50,
            complaint_upgrade: true,
            complaint_upgrade_steps: 1,
            anti_downgrade: true,
            large_context_c2_tokens: 25_000,
            large_context_c3_tokens: 80_000,
            squilla_python: None,
            squilla_script: None,
            squilla_bundle_dir: None,
            squilla_timeout_ms: 60_000,
            squilla_fallback_heuristic: true,
            squilla_use_aux_head: true,
            tiers: BTreeMap::new(),
        }
    }
}

impl ModelRouterConfig {
    /// Resolve the catalog model key for a tier.
    pub fn model_for_tier(&self, tier: Tier) -> Option<&str> {
        let key = tier.as_str();
        if let Some(entry) = self.tiers.get(key) {
            let m = entry.model.trim();
            if !m.is_empty() {
                return Some(m);
            }
        }
        // Fallback: default_tier mapping
        let def_key = self.default_tier.as_str();
        self.tiers
            .get(def_key)
            .map(|e| e.model.trim())
            .filter(|m| !m.is_empty())
    }

}
