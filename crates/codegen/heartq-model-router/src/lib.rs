//! HeartQ multi-model smart router (OpenSquilla-compatible c0–c3 tiers).
//!
//! - **v1** [`HeuristicStrategy`]: rule / feature classifier (always available)
//! - **v2** [`SquillaStrategy`]: OpenSquilla SquillaRouter V4 via Python sidecar
//!   (`squilla-runtime/classify.py` + LightGBM/ONNX bundle)

pub mod config;
pub mod decision;
pub mod features;
pub mod heuristic;
pub mod postprocess;
pub mod squilla;
pub mod strategy;
pub mod tiers;

pub use config::{ModelRouterConfig, RolloutPhase, RouterStrategyKind, TierModelConfig};
pub use decision::{RouterContext, RoutingDecision, RoutingHistoryEntry};
pub use heuristic::HeuristicStrategy;
pub use squilla::SquillaStrategy;
pub use strategy::RouterStrategy;
pub use tiers::{RouteClass, Tier};

use std::sync::Mutex;

/// Per-session router runtime: config, history, last decision.
#[derive(Debug, Default)]
pub struct ModelRouterState {
    pub config: ModelRouterConfig,
    history: Mutex<Vec<RoutingHistoryEntry>>,
    last: Mutex<Option<RoutingDecision>>,
}

impl ModelRouterState {
    pub fn new(config: ModelRouterConfig) -> Self {
        Self {
            config,
            history: Mutex::new(Vec::new()),
            last: Mutex::new(None),
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn history_snapshot(&self) -> Vec<RoutingHistoryEntry> {
        self.history.lock().map(|h| h.clone()).unwrap_or_default()
    }

    pub fn last_decision(&self) -> Option<RoutingDecision> {
        self.last.lock().ok().and_then(|g| g.clone())
    }

    /// Run the configured strategy (heuristic or squilla_v4) and record history.
    pub fn decide(&self, mut ctx: RouterContext) -> RoutingDecision {
        ctx.routing_history = self.history_snapshot();
        let decision = squilla::decide_with_fallback(&ctx, &self.config);
        if let Ok(mut last) = self.last.lock() {
            *last = Some(decision.clone());
        }
        if self.config.enabled && !decision.reasons.iter().any(|r| r == "router_disabled") {
            if let Ok(mut hist) = self.history.lock() {
                hist.push(RoutingHistoryEntry {
                    tier: decision.tier,
                    catalog_model: decision.catalog_model.clone(),
                    unix_secs: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                });
                if hist.len() > 5 {
                    let drain = hist.len() - 5;
                    hist.drain(0..drain);
                }
            }
        }
        decision
    }

    /// Human-readable status for `/router`.
    pub fn status_text(&self) -> String {
        let mut lines = vec![
            format!("model_router.enabled = {}", self.config.enabled),
            format!("model_router.rollout_phase = {:?}", self.config.rollout_phase),
            format!("model_router.default_tier = {}", self.config.default_tier),
            format!("model_router.strategy = {:?}", self.config.strategy),
        ];
        for tier in Tier::ALL {
            let model = self.config.model_for_tier(tier).unwrap_or("(unset)");
            lines.push(format!("  tier {tier} → {model}"));
        }
        match self.last_decision() {
            Some(d) => {
                lines.push(format!(
                    "last: tier={} model={} apply={} difficulty={:.2}",
                    d.tier, d.catalog_model, d.apply, d.difficulty
                ));
                if !d.reasons.is_empty() {
                    lines.push(format!("  reasons: {}", d.reasons.join(", ")));
                }
            }
            None => lines.push("last: (none)".into()),
        }
        lines.join("\n")
    }
}

/// Convenience: decide with an ephemeral config (no session state).
pub fn decide_once(ctx: &RouterContext, cfg: &ModelRouterConfig) -> RoutingDecision {
    squilla::decide_with_fallback(ctx, cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with_tiers() -> ModelRouterConfig {
        let mut cfg = ModelRouterConfig {
            enabled: true,
            rollout_phase: RolloutPhase::Full,
            ..Default::default()
        };
        cfg.tiers.insert(
            "c0".into(),
            TierModelConfig {
                model: "cheap".into(),
            },
        );
        cfg.tiers.insert(
            "c1".into(),
            TierModelConfig {
                model: "mid".into(),
            },
        );
        cfg.tiers.insert(
            "c2".into(),
            TierModelConfig {
                model: "strong".into(),
            },
        );
        cfg.tiers.insert(
            "c3".into(),
            TierModelConfig {
                model: "frontier".into(),
            },
        );
        cfg
    }

    #[test]
    fn disabled_skips() {
        let cfg = ModelRouterConfig::default();
        let d = decide_once(
            &RouterContext {
                user_text: "hello".into(),
                ..Default::default()
            },
            &cfg,
        );
        assert!(!d.apply);
        assert!(d.reasons.iter().any(|r| r.contains("disabled")));
    }

    #[test]
    fn trivial_routes_c0() {
        let cfg = cfg_with_tiers();
        let d = decide_once(
            &RouterContext {
                user_text: "thanks".into(),
                ..Default::default()
            },
            &cfg,
        );
        assert_eq!(d.tier, Tier::C0);
        assert_eq!(d.catalog_model, "cheap");
        assert!(d.apply);
    }

    #[test]
    fn architecture_routes_high() {
        let cfg = cfg_with_tiers();
        let d = decide_once(
            &RouterContext {
                user_text: "Please redesign the microservice architecture and migrate the auth system step by step.".into(),
                ..Default::default()
            },
            &cfg,
        );
        assert!(d.tier >= Tier::C2, "tier was {}", d.tier);
    }

    #[test]
    fn complaint_upgrades() {
        let cfg = cfg_with_tiers();
        let d = decide_once(
            &RouterContext {
                user_text: "that's wrong, try again".into(),
                ..Default::default()
            },
            &cfg,
        );
        assert!(
            d.reasons.iter().any(|r| r.contains("complaint")),
            "{:?}",
            d.reasons
        );
        assert!(d.tier >= Tier::C1);
    }

    #[test]
    fn observe_does_not_apply() {
        let mut cfg = cfg_with_tiers();
        cfg.rollout_phase = RolloutPhase::Observe;
        let d = decide_once(
            &RouterContext {
                user_text: "thanks".into(),
                ..Default::default()
            },
            &cfg,
        );
        assert!(!d.apply);
        assert_eq!(d.catalog_model, "cheap");
    }

    #[test]
    fn state_keeps_history_and_status() {
        let state = ModelRouterState::new(cfg_with_tiers());
        let _ = state.decide(RouterContext {
            user_text: "ok".into(),
            ..Default::default()
        });
        assert!(state.last_decision().is_some());
        assert!(!state.history_snapshot().is_empty());
        let status = state.status_text();
        assert!(status.contains("model_router.enabled"));
        assert!(status.contains("last:"));
    }
}
