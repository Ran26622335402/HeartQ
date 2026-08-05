//! Rule post-processing aligned with OpenSquilla engine gates.

use crate::config::ModelRouterConfig;
use crate::decision::RouterContext;
use crate::features::TurnFeatures;
use crate::tiers::Tier;

/// Apply complaint upgrade, context floors, and anti-downgrade.
pub fn apply_postprocess(
    mut tier: Tier,
    ctx: &RouterContext,
    cfg: &ModelRouterConfig,
    feats: &TurnFeatures,
) -> (Tier, Vec<String>) {
    let mut reasons = Vec::new();

    if cfg.complaint_upgrade && feats.is_complaint {
        let before = tier;
        tier = tier.upgrade(cfg.complaint_upgrade_steps.max(1));
        if tier != before {
            reasons.push(format!("complaint_upgrade:{before}->{tier}"));
        }
    }

    if ctx.context_tokens_est >= cfg.large_context_c3_tokens {
        let before = tier;
        tier = tier.max(Tier::C3);
        if tier != before {
            reasons.push("large_context_floor_c3".into());
        }
    } else if ctx.context_tokens_est >= cfg.large_context_c2_tokens {
        let before = tier;
        tier = tier.max(Tier::C2);
        if tier != before {
            reasons.push("large_context_floor_c2".into());
        }
    }

    if cfg.anti_downgrade {
        if let Some(prev) = ctx.routing_history.last() {
            // Within ~10 minutes of a higher-tier turn, do not drop.
            let now = unix_now();
            let window = 600;
            if now.saturating_sub(prev.unix_secs) <= window && prev.tier > tier {
                reasons.push(format!("anti_downgrade:keep_{}", prev.tier));
                tier = prev.tier;
            }
        }
    }

    (tier, reasons)
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::RoutingHistoryEntry;
    use crate::features::TurnFeatures;

    #[test]
    fn context_floor_raises_tier() {
        let cfg = ModelRouterConfig::default();
        let ctx = RouterContext {
            context_tokens_est: 30_000,
            ..Default::default()
        };
        let feats = TurnFeatures::default();
        let (tier, reasons) = apply_postprocess(Tier::C0, &ctx, &cfg, &feats);
        assert_eq!(tier, Tier::C2);
        assert!(reasons.iter().any(|r| r.contains("large_context_floor_c2")));
    }

    #[test]
    fn anti_downgrade_keeps_higher_tier() {
        let cfg = ModelRouterConfig {
            anti_downgrade: true,
            ..Default::default()
        };
        let ctx = RouterContext {
            routing_history: vec![RoutingHistoryEntry {
                tier: Tier::C3,
                catalog_model: "strong".into(),
                unix_secs: unix_now(),
            }],
            ..Default::default()
        };
        let (tier, _) = apply_postprocess(Tier::C0, &ctx, &cfg, &TurnFeatures::default());
        assert_eq!(tier, Tier::C3);
    }
}
