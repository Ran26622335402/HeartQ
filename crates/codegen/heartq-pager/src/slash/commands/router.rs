//! `/router` — show multi-model router status and last decision.

use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// Display HeartQ model_router configuration and last decision snapshot.
pub struct RouterCommand;

impl SlashCommand for RouterCommand {
    fn name(&self) -> &str {
        "router"
    }

    fn description(&self) -> &str {
        "Show multi-model router status"
    }

    fn usage(&self) -> &str {
        "/router"
    }

    fn run(&self, _ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        CommandResult::Message(format_router_status())
    }
}

fn format_router_status() -> String {
    let mut lines = Vec::new();
    lines.push("HeartQ multi-model router".to_string());

    let config_path = dirs::home_dir().map(|h| h.join(".heartq").join("config.toml"));
    match config_path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(text) => {
            if let Ok(value) = text.parse::<toml::Value>() {
                match value.get("model_router") {
                    Some(table) => {
                        lines.push(format!(
                            "config: {}",
                            config_path
                                .as_ref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default()
                        ));
                        if let Some(v) = table.get("enabled") {
                            lines.push(format!("  enabled = {v}"));
                        }
                        if let Some(v) = table.get("strategy") {
                            lines.push(format!("  strategy = {v}"));
                        }
                        if let Some(v) = table.get("rollout_phase") {
                            lines.push(format!("  rollout_phase = {v}"));
                        }
                        if let Some(v) = table.get("default_tier") {
                            lines.push(format!("  default_tier = {v}"));
                        }
                        if let Some(tiers) = table.get("tiers").and_then(|t| t.as_table()) {
                            for (k, v) in tiers {
                                let model = v
                                    .get("model")
                                    .and_then(|m| m.as_str())
                                    .unwrap_or("?");
                                lines.push(format!("  tier {k} → {model}"));
                            }
                        }
                    }
                    None => lines.push(
                        "config: [model_router] not set (default enabled=false)".into(),
                    ),
                }
            } else {
                lines.push("config: failed to parse ~/.heartq/config.toml".into());
            }
        }
        None => lines.push("config: ~/.heartq/config.toml not found".into()),
    }

    let last_path = dirs::home_dir().map(|h| h.join(".heartq").join("model_router_last.json"));
    match last_path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(text) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(v) => {
                lines.push("last decision:".into());
                if let Some(s) = v.get("status").and_then(|x| x.as_str()) {
                    lines.push(s.to_string());
                } else {
                    lines.push(format!("  {text}"));
                }
            }
            Err(_) => lines.push("last decision: (unreadable snapshot)".into()),
        },
        None => lines.push(
            "last decision: (none yet — send a turn with model_router.enabled=true)".into(),
        ),
    }

    lines.join("\n")
}
