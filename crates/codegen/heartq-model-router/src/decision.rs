//! Routing decision types and context.

use crate::tiers::{RouteClass, Tier};
use serde::{Deserialize, Serialize};

/// One historical routing outcome kept for anti-downgrade / sticky behavior.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutingHistoryEntry {
    pub tier: Tier,
    pub catalog_model: String,
    pub unix_secs: u64,
}

/// Inputs for a single routing decision.
#[derive(Debug, Clone, Default)]
pub struct RouterContext {
    pub user_text: String,
    pub has_image: bool,
    /// Estimated tokens already in context (conversation + tools).
    pub context_tokens_est: u64,
    pub is_subagent: bool,
    /// Recent decisions (newest last), typically ≤5 entries.
    pub routing_history: Vec<RoutingHistoryEntry>,
    pub prev_assistant_text: String,
}

/// Result of `RouterStrategy::decide`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutingDecision {
    pub tier: Tier,
    pub route_class: RouteClass,
    /// Catalog key / model slug to apply when rollout is `full`.
    pub catalog_model: String,
    /// Human-readable reasons (flags, upgrades, floors).
    pub reasons: Vec<String>,
    /// Heuristic score in \[0, 1\] (higher = more complex).
    pub difficulty: f32,
    /// Whether the router recommends changing the session model.
    pub apply: bool,
}

impl RoutingDecision {
    pub fn skipped(reason: impl Into<String>, default_tier: Tier) -> Self {
        Self {
            tier: default_tier,
            route_class: RouteClass::from_tier(default_tier),
            catalog_model: String::new(),
            reasons: vec![reason.into()],
            difficulty: 0.0,
            apply: false,
        }
    }
}
