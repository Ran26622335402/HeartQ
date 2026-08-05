//! Pluggable router strategies (heuristic today; ML later).

use crate::config::ModelRouterConfig;
use crate::decision::{RouterContext, RoutingDecision};

/// Strategy that maps a [`RouterContext`] to a [`RoutingDecision`].
pub trait RouterStrategy: Send + Sync {
    fn name(&self) -> &'static str;
    fn decide(&self, ctx: &RouterContext, cfg: &ModelRouterConfig) -> RoutingDecision;
}
