//! Rule / heuristic strategy: features → R0–R3.

use crate::decision::{RouterContext, RoutingDecision};
use crate::features::extract_features;
use crate::postprocess::apply_postprocess;
use crate::strategy::RouterStrategy;
use crate::tiers::{RouteClass, Tier};
use crate::config::{ModelRouterConfig, RolloutPhase};

/// OpenSquilla-inspired heuristic classifier (no ML weights).
#[derive(Debug, Default)]
pub struct HeuristicStrategy;

impl RouterStrategy for HeuristicStrategy {
    fn name(&self) -> &'static str {
        "heuristic"
    }

    fn decide(&self, ctx: &RouterContext, cfg: &ModelRouterConfig) -> RoutingDecision {
        if !cfg.enabled {
            return RoutingDecision::skipped("router_disabled", cfg.default_tier);
        }
        if ctx.is_subagent {
            return RoutingDecision::skipped("subagent_skip", cfg.default_tier);
        }
        if ctx.has_image {
            let tier = Tier::C2.max(cfg.default_tier);
            return finalize(ctx, cfg, RouteClass::from_tier(tier), 0.7, vec![
                "image_attachment".into(),
            ]);
        }

        let feats = extract_features(&ctx.user_text);
        let mut reasons = Vec::new();
        let mut score: f32 = 0.15;

        if feats.is_trivial {
            reasons.push("trivial_ack".into());
            return finalize(ctx, cfg, RouteClass::R0, 0.05, reasons);
        }

        score += (feats.approx_tokens as f32 / 800.0).min(0.35);
        if feats.has_code_fence {
            score += 0.18;
            reasons.push("code_fence".into());
        }
        if feats.has_path_or_file {
            score += 0.08;
            reasons.push("path_or_file".into());
        }
        if feats.flag_debug {
            score += 0.22;
            reasons.push("flag_debug".into());
        }
        if feats.flag_architecture {
            score += 0.28;
            reasons.push("flag_architecture".into());
        }
        if feats.flag_high_risk {
            score += 0.30;
            reasons.push("flag_high_risk".into());
        }
        if feats.flag_planning {
            score += 0.16;
            reasons.push("flag_planning".into());
        }
        if feats.flag_long_form {
            score += 0.20;
            reasons.push("flag_long_form".into());
        }
        if feats.chinese_ratio > 0.4 && feats.char_len > 80 {
            score += 0.05;
        }

        let route = match score {
            s if s < 0.28 => RouteClass::R0,
            s if s < 0.48 => RouteClass::R1,
            s if s < 0.72 => RouteClass::R2,
            _ => RouteClass::R3,
        };
        reasons.push(format!("score={score:.2}"));
        finalize(ctx, cfg, route, score.clamp(0.0, 1.0), reasons)
    }
}

fn finalize(
    ctx: &RouterContext,
    cfg: &ModelRouterConfig,
    route: RouteClass,
    difficulty: f32,
    mut reasons: Vec<String>,
) -> RoutingDecision {
    let feats = extract_features(&ctx.user_text);
    let (tier, extra) = apply_postprocess(route.to_tier(), ctx, cfg, &feats);
    reasons.extend(extra);

    let catalog_model = cfg
        .model_for_tier(tier)
        .unwrap_or("")
        .to_string();
    let apply = cfg.rollout_phase == RolloutPhase::Full && !catalog_model.is_empty();

    RoutingDecision {
        tier,
        route_class: RouteClass::from_tier(tier),
        catalog_model,
        reasons,
        difficulty,
        apply,
    }
}
