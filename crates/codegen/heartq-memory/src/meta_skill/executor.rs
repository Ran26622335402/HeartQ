//! Production [`SkillExecutor`] that dispatches via `heartq_tools::skill_manager`.
//!
//! Meta-skill steps typically call `skill_manage` (or pass an `action` in
//! inputs). Clarify steps (`clarify` / `ask_user`) return a JSON payload
//! with a `clarify` field so the runner can pause for user input.

use serde_json::{json, Map, Value};

use super::runner::SkillExecutor;

/// Production skill dispatcher used by shell auto-trigger and `/meta run`.
#[derive(Debug, Default, Clone, Copy)]
pub struct SkillManagerExecutor;

impl SkillManagerExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl SkillExecutor for SkillManagerExecutor {
    fn execute(
        &self,
        skill_name: &str,
        inputs: &Map<String, Value>,
        _timeout_secs: u32,
    ) -> Result<Value, String> {
        if skill_name == "clarify" || skill_name == "ask_user" {
            let prompt = inputs
                .get("prompt")
                .or_else(|| inputs.get("clarify_prompt"))
                .or_else(|| inputs.get("question"))
                .and_then(|v| v.as_str())
                .unwrap_or("Please clarify.");
            return Ok(json!({ "clarify": prompt }));
        }

        let action = inputs.get("action").and_then(|v| v.as_str());
        let name = inputs
            .get("name")
            .and_then(|v| v.as_str())
            .or_else(|| {
                // Fall back to top-level skill_name when it isn't the
                // dispatcher itself.
                if skill_name != "skill_manage" {
                    Some(skill_name)
                } else {
                    None
                }
            })
            .unwrap_or("");

        if skill_name == "skill_manage" || action.is_some() {
            let action = action.unwrap_or("edit");
            let content = inputs.get("content").and_then(|v| v.as_str());
            let old = inputs.get("old_string").and_then(|v| v.as_str());
            let new = inputs.get("new_string").and_then(|v| v.as_str());
            let path = inputs.get("path").and_then(|v| v.as_str());
            return heartq_tools::skill_manager::skill_manage_ext(
                action, name, content, old, new, path,
            )
            .map_err(|e| e.to_string());
        }

        // Soft reference: a named skill with no manage action — acknowledge.
        Ok(json!({
            "skill": skill_name,
            "status": "ok",
            "note": "skill referenced (no skill_manage action in inputs)",
            "inputs": inputs,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clarify_returns_prompt_payload() {
        let ex = SkillManagerExecutor::new();
        let mut inputs = Map::new();
        inputs.insert("prompt".into(), json!("Which environment?"));
        let v = ex.execute("clarify", &inputs, 10).unwrap();
        assert_eq!(v["clarify"], "Which environment?");
    }

    #[test]
    fn skill_manage_rejects_invalid_name() {
        let ex = SkillManagerExecutor::new();
        let mut inputs = Map::new();
        inputs.insert("action".into(), json!("create"));
        inputs.insert("name".into(), json!("BAD_NAME"));
        inputs.insert(
            "content".into(),
            json!("---\nname: bad\ndescription: x\n---\n\nbody\n"),
        );
        let err = ex.execute("skill_manage", &inputs, 30).unwrap_err();
        assert!(
            err.contains("lowercase") || err.contains("Invalid") || err.contains("name"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn soft_reference_without_action() {
        let ex = SkillManagerExecutor::new();
        let inputs = Map::new();
        let v = ex.execute("echo", &inputs, 10).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(v["skill"], "echo");
    }
}
