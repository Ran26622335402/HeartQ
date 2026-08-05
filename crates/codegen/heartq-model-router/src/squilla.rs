//! SquillaRouter V4 strategy via Python sidecar (`squilla-runtime/classify.py`).
//!
//! This avoids linking PyO3/`ort` into the HeartQ binary. The sidecar loads the
//! same OpenSquilla V4 Phase 3 bundle (LightGBM + ONNX BGE/MLP) and speaks JSON
//! over stdin/stdout. On any failure the caller should fall back to heuristic.

use crate::config::{ModelRouterConfig, RolloutPhase, RouterStrategyKind};
use crate::decision::{RouterContext, RoutingDecision};
use crate::postprocess::apply_postprocess;
use crate::features::extract_features;
use crate::strategy::RouterStrategy;
use crate::tiers::{RouteClass, Tier};
use serde::Deserialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

/// OpenSquilla V4 Phase 3 classifier invoked as an external process.
#[derive(Debug, Default)]
pub struct SquillaStrategy;

#[derive(Debug, Deserialize)]
struct SidecarResponse {
    ok: bool,
    #[serde(default)]
    tier: Option<String>,
    #[serde(default)]
    route_class: Option<String>,
    #[serde(default)]
    confidence: f32,
    #[serde(default)]
    difficulty: f32,
    #[serde(default)]
    reasons: Vec<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    source: Option<String>,
}

impl RouterStrategy for SquillaStrategy {
    fn name(&self) -> &'static str {
        "squilla_v4"
    }

    fn decide(&self, ctx: &RouterContext, cfg: &ModelRouterConfig) -> RoutingDecision {
        if !cfg.enabled {
            return RoutingDecision::skipped("router_disabled", cfg.default_tier);
        }
        if ctx.is_subagent {
            return RoutingDecision::skipped("subagent_skip", cfg.default_tier);
        }

        match invoke_sidecar(ctx, cfg) {
            Ok(resp) if resp.ok => map_response(ctx, cfg, resp),
            Ok(resp) => {
                tracing::warn!(
                    error = resp.error.as_deref().unwrap_or("unknown"),
                    source = resp.source.as_deref().unwrap_or("v4_unavailable"),
                    "squilla_v4 sidecar returned ok=false"
                );
                RoutingDecision::skipped(
                    format!(
                        "squilla_v4_unavailable:{}",
                        resp.error.unwrap_or_else(|| "unknown".into())
                    ),
                    cfg.default_tier,
                )
            }
            Err(err) => {
                tracing::warn!(error = %err, "squilla_v4 sidecar invoke failed");
                RoutingDecision::skipped(format!("squilla_v4_error:{err}"), cfg.default_tier)
            }
        }
    }
}

fn map_response(
    ctx: &RouterContext,
    cfg: &ModelRouterConfig,
    resp: SidecarResponse,
) -> RoutingDecision {
    let mut reasons = resp.reasons;
    reasons.push(format!("confidence={:.3}", resp.confidence));
    if let Some(src) = resp.source {
        reasons.push(format!("source={src}"));
    }

    let mut tier = resp
        .tier
        .as_deref()
        .and_then(Tier::from_str_loose)
        .or_else(|| {
            resp.route_class
                .as_deref()
                .and_then(|r| Tier::from_str_loose(r))
        })
        .unwrap_or(cfg.default_tier);

    // Confidence gate (OpenSquilla-compatible): low confidence → default_tier.
    if resp.confidence > 0.0 && resp.confidence < cfg.confidence_threshold {
        reasons.push(format!(
            "confidence_gate:{}<{}",
            resp.confidence, cfg.confidence_threshold
        ));
        tier = cfg.default_tier;
    }

    let feats = extract_features(&ctx.user_text);
    let (mut tier, extra) = apply_postprocess(tier, ctx, cfg, &feats);
    reasons.extend(extra);

    if ctx.has_image {
        let before = tier;
        tier = tier.max(Tier::C2);
        if tier != before {
            reasons.push("image_attachment".into());
        }
    }

    let catalog_model = cfg.model_for_tier(tier).unwrap_or("").to_string();
    let apply = cfg.rollout_phase == RolloutPhase::Full && !catalog_model.is_empty();

    RoutingDecision {
        tier,
        route_class: RouteClass::from_tier(tier),
        catalog_model,
        reasons,
        difficulty: resp.difficulty.clamp(0.0, 3.0) / 3.0,
        apply,
    }
}

fn invoke_sidecar(ctx: &RouterContext, cfg: &ModelRouterConfig) -> Result<SidecarResponse, String> {
    let python = resolve_python(cfg);
    let script = resolve_script(cfg)?;
    let timeout = Duration::from_millis(cfg.squilla_timeout_ms.max(1_000));

    let history: Vec<serde_json::Value> = ctx
        .routing_history
        .iter()
        .map(|h| {
            serde_json::json!({
                "route_class": RouteClass::from_tier(h.tier).to_api_str(),
                "difficulty": 0.0,
                "margin": 0.0,
            })
        })
        .collect();

    let mut req = serde_json::json!({
        "message": ctx.user_text,
        "prev_assistant_text": ctx.prev_assistant_text,
        "history_user_texts": [],
        "routing_history": history,
        "use_aux_head": cfg.squilla_use_aux_head,
    });
    if let Some(bundle) = cfg.squilla_bundle_dir.as_ref().filter(|s| !s.is_empty()) {
        req["bundle_dir"] = serde_json::Value::String(bundle.clone());
    }

    let mut child = Command::new(&python)
        .arg("-u")
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PYTHONWARNINGS", "ignore")
        .spawn()
        .map_err(|e| format!("spawn {python}: {e}"))?;

    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "sidecar stdin missing".to_string())?;
        stdin
            .write_all(req.to_string().as_bytes())
            .map_err(|e| format!("write stdin: {e}"))?;
    }

    let output = wait_with_timeout(&mut child, timeout)?;
    if !output.status.success() && output.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "sidecar exit {:?}: {}",
            output.status.code(),
            stderr.chars().take(400).collect::<String>()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.trim().starts_with('{'))
        .ok_or_else(|| {
            let stderr = String::from_utf8_lossy(&output.stderr);
            format!(
                "no JSON in sidecar stdout; stderr={}",
                stderr.chars().take(400).collect::<String>()
            )
        })?;

    serde_json::from_str(line).map_err(|e| format!("parse sidecar JSON: {e}; line={line}"))
}

fn wait_with_timeout(child: &mut std::process::Child, timeout: Duration) -> Result<Output, String> {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_end(&mut stdout);
                }
                if let Some(mut err) = child.stderr.take() {
                    let _ = err.read_to_end(&mut stderr);
                }
                return Ok(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "sidecar timed out after {}ms",
                        timeout.as_millis()
                    ));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(format!("try_wait: {e}")),
        }
    }
}

fn resolve_python(cfg: &ModelRouterConfig) -> String {
    if let Some(p) = cfg.squilla_python.as_ref().filter(|s| !s.is_empty()) {
        return p.clone();
    }
    if let Ok(p) = std::env::var("HEARTQ_SQUILLA_PYTHON") {
        if !p.is_empty() {
            return p;
        }
    }
    // Dev-layout default next to this crate.
    let candidate = crate_runtime_dir().join(".venv/bin/python");
    if candidate.is_file() {
        return candidate.display().to_string();
    }
    "python3".to_string()
}

fn resolve_script(cfg: &ModelRouterConfig) -> Result<PathBuf, String> {
    if let Some(p) = cfg.squilla_script.as_ref().filter(|s| !s.is_empty()) {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!("squilla_script not found: {p}"));
    }
    if let Ok(p) = std::env::var("HEARTQ_SQUILLA_SCRIPT") {
        let path = PathBuf::from(&p);
        if path.is_file() {
            return Ok(path);
        }
    }
    let candidate = crate_runtime_dir().join("classify.py");
    if candidate.is_file() {
        return Ok(candidate);
    }
    Err(format!(
        "classify.py not found at {} (set model_router.squilla_script or HEARTQ_SQUILLA_SCRIPT)",
        candidate.display()
    ))
}

fn crate_runtime_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR works in tests / when built from source tree.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("squilla-runtime")
}

impl RouteClass {
    fn to_api_str(self) -> &'static str {
        match self {
            RouteClass::R0 => "R0",
            RouteClass::R1 => "R1",
            RouteClass::R2 => "R2",
            RouteClass::R3 => "R3",
        }
    }
}

/// Decide with squilla_v4, falling back to heuristic when the sidecar fails.
pub fn decide_with_fallback(ctx: &RouterContext, cfg: &ModelRouterConfig) -> RoutingDecision {
    use crate::heuristic::HeuristicStrategy;

    match cfg.strategy {
        RouterStrategyKind::Heuristic => HeuristicStrategy.decide(ctx, cfg),
        RouterStrategyKind::SquillaV4 => {
            let decision = SquillaStrategy.decide(ctx, cfg);
            let failed = decision.reasons.iter().any(|r| {
                r.starts_with("squilla_v4_error")
                    || r.starts_with("squilla_v4_unavailable")
            });
            if failed && cfg.squilla_fallback_heuristic {
                tracing::info!("squilla_v4 failed; falling back to heuristic strategy");
                let mut fb = HeuristicStrategy.decide(ctx, cfg);
                fb.reasons.insert(0, "fallback_heuristic".into());
                fb
            } else {
                decision
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TierModelConfig;

    fn cfg() -> ModelRouterConfig {
        let mut cfg = ModelRouterConfig {
            enabled: true,
            strategy: RouterStrategyKind::SquillaV4,
            rollout_phase: RolloutPhase::Observe,
            squilla_timeout_ms: 120_000,
            ..Default::default()
        };
        cfg.tiers.insert(
            "c0".into(),
            TierModelConfig {
                model: "cheap".into(),
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
    fn map_response_sets_tier_and_catalog() {
        let ctx = RouterContext {
            user_text: "x".into(),
            ..Default::default()
        };
        let d = map_response(
            &ctx,
            &cfg(),
            SidecarResponse {
                ok: true,
                tier: Some("c0".into()),
                route_class: Some("R0".into()),
                confidence: 0.9,
                difficulty: 0.1,
                reasons: vec!["squilla_v4".into()],
                error: None,
                source: Some("v4_phase3".into()),
            },
        );
        assert_eq!(d.tier, Tier::C0);
        assert_eq!(d.catalog_model, "cheap");
        assert!(!d.apply); // observe
    }

    #[test]
    #[ignore = "requires squilla-runtime venv + V4 bundle weights"]
    fn live_sidecar_trivial_is_c0() {
        let d = SquillaStrategy.decide(
            &RouterContext {
                user_text: "thanks".into(),
                ..Default::default()
            },
            &cfg(),
        );
        assert!(
            d.reasons.iter().any(|r| r.contains("squilla")),
            "{:?}",
            d.reasons
        );
        assert_eq!(d.tier, Tier::C0, "{:?}", d);
    }
}
