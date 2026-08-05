//! `/meta` slash commands — Day 3 of the opensquilla → HeartQ import plan.
//!
//! Borrowed from `opensquilla docs/features/meta-skills.md`. HeartQ differences:
//!
//! - We read `${HEARTQ_HOME}/meta_skills/*/spec.json` and
//!   `${HEARTQ_HOME}/meta_runs/*.json` directly here rather than depending on
//!   `heartq-memory` from this TUI crate. The persistence format is
//!   `MetaSkillSpec`/`MetaSkillRun` JSON; we parse the same shape with
//!   `serde_json::Value` so this layer doesn't need the full type.
//! - Write operations (`/meta run <name>`) invoke `MetaSkillRunner` from
//!   `heartq-memory` with the production `SkillManagerExecutor` (dispatches
//!   via `skill_manage`).
//!
//! Scope (v1):
//! - `/meta list` — list meta-skills available in `${HEARTQ_HOME}/meta_skills/`
//! - `/meta runs list` — list recent runs in `${HEARTQ_HOME}/meta_runs/`
//! - `/meta runs show <id>` — full plan + per-step state for one run
//! - `/meta runs steps <id>` — compact per-step table for one run

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use heartq_memory::meta_skill::{
    MetaSkillRunner, MetaSkillStore, SkillManagerExecutor, StepOutcome, StoreError,
};
use serde::Deserialize;
use uuid::Uuid;

use super::super::command::{CommandExecCtx, CommandResult, SlashCommand};
// Re-export the canonical pager Action so the `CommandResult::Action(...)`
// return type lines up with `app::actions::Action`.
use crate::app::actions::Action;

/// Resolved `${HEARTQ_HOME}` base. Mirrors `heartq_memory::default_meta_runs_dir`
/// without taking a hard dep on `heartq-memory` (the pager intentionally
/// stays one layer above so TUI changes don't churn the agent crate).
fn heartq_home() -> PathBuf {
    std::env::var("HEARTQ_HOME")
        .or_else(|_| std::env::var("GROK_HOME"))
        .unwrap_or_else(|_| {
            let h = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            format!("{h}/.grok")
        })
        .into()
}

/// Bare-minimum projection of `MetaSkillSpec` — only the fields we render
/// in the TUI. Loaded from `${HEARTQ_HOME}/meta_skills/<name>/spec.json`.
/// The runner uses the full type from `heartq-memory`; here we just need
/// name + version + description for `/meta list`.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct SpecSummary {
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    steps: usize,
}

/// Bare-minimum projection of `MetaSkillRun` — same rationale as
/// `SpecSummary`. We only read `id`, `meta_skill_name`, `started_at`,
/// `ended_at`, `status`, and `steps[].status` for `/meta runs list` and
/// `/meta runs steps`. Anything else in the file is ignored.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct RunSummary {
    id: Uuid,
    meta_skill_name: String,
    #[serde(default)]
    started_at: Option<u64>, // seconds since epoch; SystemTime round-trips
    #[serde(default)]
    ended_at: Option<u64>,
    status: String,
    #[serde(default)]
    current_step: usize,
    #[serde(default)]
    steps: Vec<StepSummary>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct StepSummary {
    index: usize,
    #[serde(default)]
    label: String,
    #[serde(default)]
    skill_name: String,
    status: String,
    #[serde(default)]
    error: Option<String>,
}

/// `/meta` — top-level dispatcher. Subcommands are routed via the args
/// string (the SlashCommand trait gives us a single `args: &str`).
///
/// Recognized forms:
/// - `/meta` (no args) → same as `/meta list`
/// - `/meta list` → list meta-skills
/// - `/meta runs list` → list runs
/// - `/meta runs show <id>` → full run dump
/// - `/meta runs steps <id>` → per-step table
/// - `/meta run <name>` → error (deferred to agent loop)
/// - `/meta runs replay <id>` → error (deferred to agent loop)
pub struct MetaCommand;

impl SlashCommand for MetaCommand {
    fn name(&self) -> &str {
        "meta"
    }

    fn description(&self) -> &str {
        "Inspect meta-skills and their run history"
    }

    fn session_scoped(&self) -> bool {
        false // session-scoped but useful before/after sessions
    }

    fn usage(&self) -> &str {
        "/meta [list | runs [list | show <id> | steps <id> | resume <id>] | run <name>]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn args_required(&self) -> bool {
        false
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("list | runs ...")
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let args = args.trim();
        let mut parts = args.split_whitespace();
        let first = parts.next().unwrap_or("list");
        match first {
            // Bare `/meta` or `/meta list`
            "list" | "" => {
                let home = heartq_home();
                let skills_dir = home.join("meta_skills");
                // Treat a missing directory as "no meta-skills yet" rather
                // than an error — the user just hasn't created any.
                let entries = match fs::read_dir(&skills_dir) {
                    Ok(e) => e.flatten().collect::<Vec<_>>(),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                    Err(e) => {
                        return CommandResult::Error(format!(
                            "could not read {}: {}",
                            skills_dir.display(),
                            e
                        ))
                    }
                };
                if entries.is_empty() {
                    return CommandResult::Action(Action::MetaList(MetaListPayload {
                        skills: Vec::new(),
                        note: format!("(no meta-skills in {})", skills_dir.display()),
                    }));
                }
                let mut summaries = Vec::new();
                for entry in entries {
                    let spec_path = entry.path().join("spec.json");
                    let raw = match fs::read_to_string(&spec_path) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    let parsed: Result<SpecSummary, _> = serde_json::from_str(&raw);
                    if let Ok(s) = parsed {
                        summaries.push(s);
                    }
                }
                summaries.sort_by(|a, b| a.name.cmp(&b.name));
                let n = summaries.len();
                CommandResult::Action(Action::MetaList(MetaListPayload {
                    skills: summaries,
                    note: format!("(read {n} from {})", skills_dir.display()),
                }))
            }

            "runs" => match parts.next() {
                None | Some("list") => list_runs(),
                Some("show") => match parts.next() {
                    Some(id_str) => show_run(id_str),
                    None => CommandResult::Error("usage: /meta runs show <id>".into()),
                },
                Some("steps") => match parts.next() {
                    Some(id_str) => show_steps(id_str),
                    None => CommandResult::Error("usage: /meta runs steps <id>".into()),
                },
                Some("resume") => match parts.next() {
                    Some(id_str) => resume_run(id_str),
                    None => CommandResult::Error("usage: /meta runs resume <id>".into()),
                },
                Some("replay") => CommandResult::Error(
                    "/meta runs replay <id> is not implemented in v1; use `/meta runs resume <id>` \
                     for paused/clarify runs, or `/meta run <name>` to start fresh"
                        .into(),
                ),
                Some(other) => {
                    CommandResult::Error(format!("unknown /meta runs subcommand: {other:?}"))
                }
            },

            "run" => match parts.next() {
                Some(name) if !name.is_empty() => run_meta_skill(name),
                _ => CommandResult::Error("usage: /meta run <name>".into()),
            },

            other => CommandResult::Error(format!(
                "unknown /meta subcommand: {other:?}\nusage: /meta [list | runs ...]"
            )),
        }
    }
}

// ---- helpers ----

fn run_meta_skill(name: &str) -> CommandResult {
    let spec = match MetaSkillStore::load_spec(name) {
        Ok(s) => s,
        Err(StoreError::SpecNotFound(n)) => {
            return CommandResult::Error(format!(
                "meta-skill `{n}` not found under {}/meta_skills/{n}/spec.json",
                heartq_home().display()
            ));
        }
        Err(e) => return CommandResult::Error(format!("failed to load meta-skill `{name}`: {e}")),
    };

    let store = MetaSkillStore::new();
    let runner =
        MetaSkillRunner::with_executor(store, Arc::new(SkillManagerExecutor::new()));
    let run_id = match runner.start(spec.clone(), serde_json::Map::new()) {
        Ok(id) => id,
        Err(e) => return CommandResult::Error(format!("failed to start meta-skill run: {e}")),
    };

    match runner.run_to_completion(run_id) {
        Ok(StepOutcome::Completed) => {
            let run = runner.load(run_id).unwrap();
            CommandResult::Message(format!(
                "meta-skill `{}` completed (run {})\nstatus: {:?}\nfinal: {}",
                spec.name,
                run_id,
                run.status,
                run.final_text.unwrap_or_else(|| "(none)".into())
            ))
        }
        Ok(StepOutcome::Failed(err)) => CommandResult::Error(format!(
            "meta-skill `{}` run {run_id} failed: {err}",
            spec.name
        )),
        Ok(StepOutcome::Paused) => {
            let prompt = runner
                .load(run_id)
                .ok()
                .and_then(|r| r.clarify_prompt)
                .unwrap_or_else(|| "(no prompt)".into());
            CommandResult::Message(format!(
                "meta-skill `{}` run {run_id} paused awaiting input\nclarify: {prompt}",
                spec.name
            ))
        }
        Ok(StepOutcome::Ok | StepOutcome::Skipped) => CommandResult::Message(format!(
            "meta-skill `{}` run {run_id} stopped mid-flight",
            spec.name
        )),
        Err(e) => CommandResult::Error(format!("meta-skill `{}` run error: {e}", spec.name)),
    }
}

fn resume_run(id_str: &str) -> CommandResult {
    let id: Uuid = match id_str.parse() {
        Ok(u) => u,
        Err(_) => return CommandResult::Error(format!("invalid uuid: {id_str:?}")),
    };
    let store = MetaSkillStore::new();
    let runner =
        MetaSkillRunner::with_executor(store, Arc::new(SkillManagerExecutor::new()));
    if let Err(e) = runner.resume_from_clarify(id) {
        // Also try plain resume for Paused (non-clarify).
        if let Err(e2) = runner.resume(id) {
            return CommandResult::Error(format!("resume failed: {e}; {e2}"));
        }
    }
    match runner.run_to_completion(id) {
        Ok(StepOutcome::Completed) => {
            let run = runner.load(id).unwrap();
            CommandResult::Message(format!(
                "meta-skill run {id} completed\nfinal: {}",
                run.final_text.unwrap_or_else(|| "(none)".into())
            ))
        }
        Ok(StepOutcome::Failed(err)) => {
            CommandResult::Error(format!("meta-skill run {id} failed: {err}"))
        }
        Ok(StepOutcome::Paused) => {
            let prompt = runner
                .load(id)
                .ok()
                .and_then(|r| r.clarify_prompt)
                .unwrap_or_else(|| "(no prompt)".into());
            CommandResult::Message(format!(
                "meta-skill run {id} still paused\nclarify: {prompt}"
            ))
        }
        Ok(_) => CommandResult::Message(format!("meta-skill run {id} stopped mid-flight")),
        Err(e) => CommandResult::Error(format!("meta-skill run {id} error: {e}")),
    }
}

fn list_runs() -> CommandResult {
    let runs_dir = heartq_home().join("meta_runs");
    // Treat a missing directory as "no runs yet" rather than an error.
    let entries = match fs::read_dir(&runs_dir) {
        Ok(e) => e.flatten().collect::<Vec<_>>(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            return CommandResult::Error(format!(
                "could not read {}: {}",
                runs_dir.display(),
                e
            ))
        }
    };
    let mut runs = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let raw = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if let Ok(s) = serde_json::from_str::<RunSummary>(&raw) {
            runs.push(s);
        }
    }
    // Newest first (sort by started_at desc, fall back to id).
    runs.sort_by(|a, b| b.started_at.cmp(&a.started_at).then_with(|| b.id.cmp(&a.id)));
    let n = runs.len();
    CommandResult::Action(Action::MetaRunsList(MetaRunsListPayload {
        runs,
        note: format!("(read {n} from {})", runs_dir.display()),
    }))
}

fn show_run(id_str: &str) -> CommandResult {
    let id: Uuid = match id_str.parse() {
        Ok(u) => u,
        Err(_) => return CommandResult::Error(format!("invalid uuid: {id_str:?}")),
    };
    let path = heartq_home().join("meta_runs").join(format!("{id}.json"));
    let raw = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            return CommandResult::Error(format!("could not read {}: {}", path.display(), e))
        }
    };
    // Pretty-print the entire JSON to the user — this is a debug-grade view.
    let value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            return CommandResult::Error(format!(
                "{} is corrupt: {}",
                path.display(),
                e
            ))
        }
    };
    let pretty = serde_json::to_string_pretty(&value).unwrap_or_default();
    CommandResult::Action(Action::MetaRunShow(MetaRunShowPayload {
        id,
        body: pretty,
    }))
}

fn show_steps(id_str: &str) -> CommandResult {
    let id: Uuid = match id_str.parse() {
        Ok(u) => u,
        Err(_) => return CommandResult::Error(format!("invalid uuid: {id_str:?}")),
    };
    let path = heartq_home().join("meta_runs").join(format!("{id}.json"));
    let raw = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            return CommandResult::Error(format!("could not read {}: {}", path.display(), e))
        }
    };
    let run: RunSummary = match serde_json::from_str(&raw) {
        Ok(r) => r,
        Err(e) => {
            return CommandResult::Error(format!(
                "{} is corrupt: {}",
                path.display(),
                e
            ))
        }
    };
    CommandResult::Action(Action::MetaRunSteps(MetaRunStepsPayload {
        meta_skill_name: run.meta_skill_name,
        id: run.id,
        status: run.status,
        steps: run.steps,
    }))
}

// ---- payloads (rendered by the TUI view layer) ----

#[derive(Debug, Clone)]
pub struct MetaListPayload {
    pub skills: Vec<SpecSummary>,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct MetaRunsListPayload {
    pub runs: Vec<RunSummary>,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct MetaRunShowPayload {
    pub id: Uuid,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct MetaRunStepsPayload {
    pub meta_skill_name: String,
    pub id: Uuid,
    pub status: String,
    pub steps: Vec<StepSummary>,
}

// ---- Payloads (consumed by app/actions.rs Action variants) ----
//
// The four Action variants `MetaList`/`MetaRunsList`/`MetaRunShow`/
// `MetaRunSteps` are declared in `app/actions.rs` and carry the payloads
// below. The view layer (slash_dropdown / dashboard) renders them.

#[cfg(test)]
mod tests {
    use super::*;

    fn write_run(id: Uuid, started_at: u64, status: &str, n_steps: usize) {
        let dir = heartq_home().join("meta_runs");
        fs::create_dir_all(&dir).unwrap();
        let body = serde_json::json!({
            "id": id,
            "meta_skill_name": "demo",
            "started_at": started_at,
            "status": status,
            "current_step": n_steps,
            "steps": (0..n_steps).map(|i| serde_json::json!({
                "index": i,
                "label": format!("step {i}"),
                "skill_name": "noop",
                "status": if i == 0 { "ok" } else { "pending" },
            })).collect::<Vec<_>>(),
            "spec_snapshot": {
                "name": "demo",
                "version": "1",
                "steps": [],
                "digest": "x",
            },
        });
        fs::write(dir.join(format!("{id}.json")), body.to_string()).unwrap();
    }

    fn clear_dir() {
        let _ = fs::remove_dir_all(heartq_home().join("meta_runs"));
        let _ = fs::remove_dir_all(heartq_home().join("meta_skills"));
    }

    #[test]
    fn parses_uuid_string() {
        let id: Uuid = "550e8400-e29b-41d4-a716-446655440000".parse().unwrap();
        assert_eq!(id.to_string(), "550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn list_runs_returns_empty_when_dir_absent() {
        clear_dir();
        let result = list_runs();
        // We don't match on the variant here (Action is private to this
        // module); we just ensure it doesn't error.
        match result {
            CommandResult::Action(Action::MetaRunsList(p)) => assert!(p.runs.is_empty()),
            other => panic!("expected MetaRunsList, got {other:?}"),
        }
    }

    #[test]
    fn list_runs_sorts_newest_first() {
        clear_dir();
        let older = Uuid::new_v4();
        let newer = Uuid::new_v4();
        write_run(older, 1_000, "completed", 2);
        write_run(newer, 2_000, "running", 1);
        let result = list_runs();
        match result {
            CommandResult::Action(Action::MetaRunsList(p)) => {
                assert_eq!(p.runs.len(), 2);
                assert_eq!(p.runs[0].id, newer);
                assert_eq!(p.runs[1].id, older);
            }
            other => panic!("expected MetaRunsList, got {other:?}"),
        }
    }

    #[test]
    fn show_run_rejects_invalid_uuid() {
        let result = show_run("not-a-uuid");
        match result {
            CommandResult::Error(msg) => assert!(msg.contains("not-a-uuid")),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn show_run_reports_missing_file() {
        let id = Uuid::new_v4();
        let result = show_run(&id.to_string());
        match result {
            CommandResult::Error(_) => {}
            other => panic!("expected Error for missing run, got {other:?}"),
        }
    }
}