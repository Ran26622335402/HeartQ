//! Per-turn model router integration (heartq-model-router).

use super::*;
use crate::agent::config::{resolve_credentials, sampling_config_for_model};
use heartq_model_router::{RouterContext, RoutingDecision};

impl SessionActor {
    /// Run the smart router once at the start of a user turn.
    ///
    /// - `observe`: log decision only
    /// - `full`: ephemeral session model switch via [`Self::handle_set_session_model`]
    pub(super) async fn maybe_apply_model_router(&self) {
        if !self.model_router.enabled() {
            return;
        }
        if self.startup_hints.is_subagent {
            return;
        }

        let user_text = self
            .chat_state_handle
            .get_last_user_query_text()
            .await
            .unwrap_or_default();
        if user_text.trim().is_empty() {
            return;
        }

        let prev_assistant = self
            .chat_state_handle
            .get_last_assistant_text()
            .await
            .unwrap_or_default();
        let context_tokens_est = self.chat_state_handle.get_total_tokens().await;

        let ctx = RouterContext {
            user_text,
            has_image: false,
            context_tokens_est,
            is_subagent: self.startup_hints.is_subagent,
            routing_history: Vec::new(),
            prev_assistant_text: prev_assistant,
        };

        let decision = self.model_router.decide(ctx);
        self.log_router_decision(&decision);

        if !decision.apply || decision.catalog_model.is_empty() {
            return;
        }

        if let Err(err) = self.apply_routed_model(&decision.catalog_model).await {
            tracing::warn!(
                error = %err,
                model = %decision.catalog_model,
                "model_router: failed to apply routed model"
            );
        }
    }

    fn log_router_decision(&self, decision: &RoutingDecision) {
        let payload = serde_json::json!({
            "session_id": self.session_info.id.0.as_ref(),
            "tier": decision.tier.as_str(),
            "route_class": format!("{:?}", decision.route_class).to_ascii_lowercase(),
            "catalog_model": decision.catalog_model,
            "apply": decision.apply,
            "difficulty": decision.difficulty,
            "reasons": decision.reasons,
            "rollout_phase": format!("{:?}", self.model_router.config.rollout_phase),
            "status": self.model_router.status_text(),
        });
        heartq_telemetry::unified_log::info(
            "model_router.decision",
            Some(self.session_info.id.0.as_ref()),
            Some(payload.clone()),
        );
        tracing::info!(
            target: "heartq_model_router",
            tier = %decision.tier,
            model = %decision.catalog_model,
            apply = decision.apply,
            difficulty = decision.difficulty,
            reasons = ?decision.reasons,
            "model_router decision"
        );
        // Best-effort snapshot for `/router` slash (pager reads this file).
        if let Some(home) = dirs::home_dir() {
            let path = home.join(".heartq").join("model_router_last.json");
            if let Ok(bytes) = serde_json::to_vec_pretty(&payload) {
                let _ = std::fs::create_dir_all(path.parent().unwrap_or(home.as_path()));
                let _ = std::fs::write(path, bytes);
            }
        }
    }

    async fn apply_routed_model(&self, catalog_key: &str) -> Result<(), String> {
        let models = self.models_manager.models();
        let entry = models
            .get(catalog_key)
            .or_else(|| {
                models
                    .values()
                    .find(|e| e.info().model.as_str() == catalog_key)
            })
            .cloned()
            .ok_or_else(|| format!("catalog model not found: {catalog_key}"))?;

        let current = self
            .chat_state_handle
            .get_sampling_config()
            .await
            .ok_or_else(|| "no current sampling config".to_string())?;
        if current.model == entry.info().model {
            tracing::debug!(
                model = %current.model,
                "model_router: already on routed model"
            );
            return Ok(());
        }

        let session_key = self
            .auth_manager
            .as_ref()
            .and_then(|am| am.current_or_expired().map(|a| a.key));
        let credentials = resolve_credentials(&entry, session_key.as_deref());
        let mut sampling = sampling_config_for_model(
            &entry,
            credentials,
            None,
            None,
            None,
            None,
        );
        // Preserve session-scoped callbacks / client identity from current config
        // by reconstructing through handle_set_session_model paths that already
        // know how to merge auth. Keep origin fields unset; sampler rebuild fills them.
        sampling.attribution_callback = self.attribution_callback.clone();
        sampling.idle_timeout_secs = Some(self.inference_idle_timeout.as_secs());
        sampling.doom_loop_recovery = self.doom_loop_recovery;

        let threshold = self.compaction.threshold_percent.get();
        let use_concise = entry.info().use_concise;
        self.handle_set_session_model(
            sampling,
            use_concise,
            /* apply_prompt_override */ false,
            /* skip_prompt_rewrite */ true,
            threshold,
        )
        .await
        .map_err(|e| format!("set_session_model failed: {e:?}"))?;

        tracing::info!(
            session_id = %self.session_info.id.0,
            from = %current.model,
            to = %entry.info().model,
            "model_router: applied routed model for turn"
        );
        Ok(())
    }
}
