//! Meta-skill auto-trigger for turn prompts.
//!
//! When `[meta_skill_auto_trigger] enabled = true`, each user prompt is matched
//! against meta-skill `triggers`. Hits start a `MetaSkillRunner` with the
//! production [`SkillManagerExecutor`]. Clarify pauses stash a pending run id
//! on the session so the next user message can `resume_from_clarify`.

use std::sync::Arc;

use heartq_memory::meta_skill::{
    match_triggers, MetaSkillRunner, MetaSkillStore, SkillManagerExecutor, StepOutcome,
};
use uuid::Uuid;

/// Result of processing auto-trigger / clarify resume for one user prompt.
#[derive(Debug, Default)]
pub struct MetaAutoTriggerOutcome {
    pub notifications: Vec<String>,
    pub pending_run_id: Option<Uuid>,
}

/// Resume a pending clarify run (if any), then match and run new triggers.
pub fn run_meta_auto_trigger(
    user_text: &str,
    pending_run_id: Option<Uuid>,
    enabled: bool,
) -> MetaAutoTriggerOutcome {
    if !enabled || user_text.trim().is_empty() {
        return MetaAutoTriggerOutcome {
            notifications: Vec::new(),
            pending_run_id,
        };
    }

    let store = MetaSkillStore::new();
    let runner =
        MetaSkillRunner::with_executor(store, Arc::new(SkillManagerExecutor::new()));

    let mut notifications = Vec::new();
    let mut pending = pending_run_id;

    if let Some(id) = pending.take() {
        if let Err(e) = runner.resume_from_clarify_with(id, Some(user_text)) {
            notifications.push(format!("meta-skill resume failed: {e}"));
            return MetaAutoTriggerOutcome {
                notifications,
                pending_run_id: Some(id),
            };
        }
        match runner.run_to_completion(id) {
            Ok(StepOutcome::Paused) => {
                if let Ok(run) = runner.load(id) {
                    if let Some(prompt) = run.clarify_prompt {
                        notifications.push(format!("meta-skill 需要澄清：{prompt}"));
                    } else {
                        notifications.push(format!("meta-skill run {id} 已暂停"));
                    }
                }
                return MetaAutoTriggerOutcome {
                    notifications,
                    pending_run_id: Some(id),
                };
            }
            Ok(StepOutcome::Completed) => {
                notifications.push(format!("meta-skill run {id} 已完成"));
            }
            Ok(StepOutcome::Failed(err)) => {
                notifications.push(format!("meta-skill run {id} 失败：{err}"));
            }
            Ok(_) => {}
            Err(e) => {
                notifications.push(format!("meta-skill run {id} 错误：{e}"));
            }
        }
    }

    let specs = match MetaSkillStore::list_specs() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "meta auto-trigger: list_specs failed");
            return MetaAutoTriggerOutcome {
                notifications,
                pending_run_id: pending,
            };
        }
    };
    let matches = match_triggers(user_text, &specs);
    if matches.is_empty() {
        return MetaAutoTriggerOutcome {
            notifications,
            pending_run_id: pending,
        };
    }

    for m in matches {
        let Some(spec) = specs.iter().find(|s| s.name == m.skill_name).cloned() else {
            continue;
        };
        let mut inputs = serde_json::Map::new();
        inputs.insert("user_text".into(), serde_json::json!(user_text));
        let id = match runner.start_triggered(spec, inputs, "auto_trigger") {
            Ok(id) => id,
            Err(e) => {
                notifications.push(format!("meta-skill `{}` 启动失败：{e}", m.skill_name));
                continue;
            }
        };
        match runner.run_to_completion(id) {
            Ok(StepOutcome::Paused) => {
                if let Ok(run) = runner.load(id) {
                    if let Some(prompt) = run.clarify_prompt {
                        notifications.push(format!(
                            "meta-skill `{}` 需要澄清：{}",
                            m.skill_name, prompt
                        ));
                    } else {
                        notifications.push(format!(
                            "meta-skill `{}` 已暂停 (run {id})",
                            m.skill_name
                        ));
                    }
                }
                pending = Some(id);
                break;
            }
            Ok(StepOutcome::Completed) => {
                notifications.push(format!(
                    "meta-skill `{}` 自动执行完成 (run {id})",
                    m.skill_name
                ));
            }
            Ok(StepOutcome::Failed(err)) => {
                notifications.push(format!(
                    "meta-skill `{}` 自动执行失败：{err}",
                    m.skill_name
                ));
            }
            Ok(_) => {}
            Err(e) => {
                notifications.push(format!("meta-skill `{}` 错误：{e}", m.skill_name));
            }
        }
    }

    MetaAutoTriggerOutcome {
        notifications,
        pending_run_id: pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use heartq_memory::meta_skill::model::{MetaSkillSpec, MetaSkillStep};
    use std::fs;

    fn write_spec(home: &std::path::Path, name: &str, triggers: &[&str], steps: Vec<MetaSkillStep>) {
        let dir = home.join("meta_skills").join(name);
        fs::create_dir_all(&dir).unwrap();
        let spec = MetaSkillSpec {
            name: name.into(),
            version: "1".into(),
            description: "test".into(),
            steps,
            checkpoints: vec![],
            digest: "x".into(),
            triggers: triggers.iter().map(|s| (*s).to_string()).collect(),
            max_parallelism: None,
        };
        fs::write(
            dir.join("spec.json"),
            serde_json::to_string_pretty(&spec).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn match_and_run_soft_step_completes() {
        let tmp = tempfile::TempDir::new().unwrap();
        // SAFETY: test-only isolation of HEARTQ_HOME for meta skill dirs.
        unsafe {
            std::env::set_var("HEARTQ_HOME", tmp.path());
        }
        write_spec(
            tmp.path(),
            "greet-meta",
            &["say hello"],
            vec![MetaSkillStep {
                label: "ack".into(),
                skill_name: "echo".into(),
                inputs: serde_json::Map::new(),
                outputs: vec![],
                requires: vec![],
                timeout_secs: None,
                id: None,
                depends_on: vec![],
                when: None,
                route: vec![],
                on_failure: None,
            }],
        );

        let out = run_meta_auto_trigger("please say hello now", None, true);
        assert!(
            out.notifications.iter().any(|n| n.contains("自动执行完成")),
            "notifications: {:?}",
            out.notifications
        );
        assert!(out.pending_run_id.is_none());
    }

    #[test]
    fn disabled_is_noop() {
        let out = run_meta_auto_trigger("say hello", None, false);
        assert!(out.notifications.is_empty());
        assert!(out.pending_run_id.is_none());
    }

    #[test]
    fn clarify_pauses_then_resume_completes() {
        let tmp = tempfile::TempDir::new().unwrap();
        unsafe {
            std::env::set_var("HEARTQ_HOME", tmp.path());
        }
        let mut clarify_inputs = serde_json::Map::new();
        clarify_inputs.insert("prompt".into(), serde_json::json!("Which env?"));
        write_spec(
            tmp.path(),
            "clarify-meta",
            &["need clarify"],
            vec![
                MetaSkillStep {
                    label: "ask".into(),
                    skill_name: "clarify".into(),
                    inputs: clarify_inputs,
                    outputs: vec![],
                    requires: vec![],
                    timeout_secs: None,
                    id: None,
                    depends_on: vec![],
                    when: None,
                    route: vec![],
                    on_failure: None,
                },
                MetaSkillStep {
                    label: "ack".into(),
                    skill_name: "echo".into(),
                    inputs: serde_json::Map::new(),
                    outputs: vec![],
                    requires: vec![],
                    timeout_secs: None,
                    id: None,
                    depends_on: vec![],
                    when: None,
                    route: vec![],
                    on_failure: None,
                },
            ],
        );

        let paused = run_meta_auto_trigger("please need clarify now", None, true);
        assert!(
            paused.notifications.iter().any(|n| n.contains("澄清")),
            "notifications: {:?}",
            paused.notifications
        );
        let pending = paused.pending_run_id.expect("pending clarify run");

        let resumed = run_meta_auto_trigger("prod", Some(pending), true);
        assert!(
            resumed.notifications.iter().any(|n| n.contains("已完成")),
            "notifications: {:?}",
            resumed.notifications
        );
        assert!(resumed.pending_run_id.is_none());

        // Clarification text must be visible on the resumed run.
        let store = MetaSkillStore::new();
        let run = store.load(pending).unwrap();
        assert_eq!(
            run.state.get("user_clarification"),
            Some(&serde_json::json!("prod"))
        );
        assert_eq!(
            run.inputs.get("clarification"),
            Some(&serde_json::json!("prod"))
        );
    }

    #[test]
    fn skill_manage_step_mutates_skill_on_trigger() {
        let tmp = tempfile::TempDir::new().unwrap();
        unsafe {
            std::env::set_var("HEARTQ_HOME", tmp.path());
            std::env::set_var("HERMES_HOME", tmp.path());
        }
        let skills = tmp.path().join("skills");
        std::fs::create_dir_all(&skills).unwrap();

        let content = "---\nname: auto-demo\ndescription: demo skill for auto trigger\n---\n\n# Auto Demo\n";
        let mut inputs = serde_json::Map::new();
        inputs.insert("action".into(), serde_json::json!("create"));
        inputs.insert("name".into(), serde_json::json!("auto-demo"));
        inputs.insert("content".into(), serde_json::json!(content));

        write_spec(
            tmp.path(),
            "create-demo-meta",
            &["create auto demo"],
            vec![MetaSkillStep {
                label: "create skill".into(),
                skill_name: "skill_manage".into(),
                inputs,
                outputs: vec![],
                requires: vec![],
                timeout_secs: None,
                id: None,
                depends_on: vec![],
                when: None,
                route: vec![],
                on_failure: None,
            }],
        );

        let out = run_meta_auto_trigger("please create auto demo skill", None, true);
        assert!(
            out.notifications.iter().any(|n| n.contains("自动执行完成")),
            "notifications: {:?}",
            out.notifications
        );
        assert!(
            skills.join("auto-demo").join("SKILL.md").is_file(),
            "expected skill file under {}",
            skills.display()
        );
    }
}
