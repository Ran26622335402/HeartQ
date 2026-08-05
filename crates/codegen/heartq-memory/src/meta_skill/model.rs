//! Meta-skill data model — all serde-friendly so a run JSON-dumps cleanly to
//! `${HEARTQ_HOME}/meta_runs/<run_id>.json`.
//!
//! Inspired by opensquilla's `meta_skill_runs` and `meta_skill_run_steps`
//! tables (see `migrations/V010__meta_skill_runs.py`). We collapse both tables
//! into a single `MetaSkillRun` struct with a `steps: Vec<...>` because the
//! run only ever reads its own history at replay time; no cross-run queries.

use std::collections::HashMap;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Status of an entire meta-skill run.
///
/// Mirrors opensquilla's `meta_skill_runs.status` enum
/// (`running`/`ok`/`failed`/`cancelled`) plus a `paused` state which
/// HeartQ adds because we want explicit user pause/resume semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// At least one step is queued or in flight; not yet terminal.
    Running,
    /// User explicitly paused; resumable via `RunStatus::Running` again.
    Paused,
    /// Runner needs user input before continuing (clarify stub).
    AwaitingUser,
    /// All steps completed successfully (`StepOutput` populated for each).
    Completed,
    /// At least one step recorded `Failed`; the run is terminal.
    Failed,
    /// User cancelled mid-run; terminal.
    Cancelled,
}

/// Per-step terminal state. Matches opensquilla's
/// `meta_skill_run_steps.status` (`ok`/`failed`/`cancelled`/`substituted`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetaSkillStepStatus {
    /// Not yet started.
    Pending,
    /// Currently invoking the skill.
    Running,
    /// Skill returned successfully; `output` populated.
    Ok,
    /// Skill returned an error; `error` populated.
    Failed,
    /// User asked to skip this step; `output` may be empty.
    Skipped,
    /// Primary step failed and was replaced by an `on_failure` substitute.
    Substituted,
}

/// One conditional branch: first truthy `when` overrides the step skill.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteCase {
    /// Jinja boolean expression evaluated against `inputs` + `outputs`.
    pub when: String,
    /// Skill name to use when `when` is truthy.
    pub to: String,
}

/// Output of a single step. The runner stores a JSON-serializable value
/// so downstream steps can `require` it and resolve via the run's state map.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StepOutput {
    /// Skill-produced text or structured result, JSON-encoded.
    /// `null` if the step produced no output (e.g. skipped).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    /// Free-form human note (e.g. "agent decided to skip").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Static definition of one step inside a `MetaSkill`.
///
/// The runner reads `skill_name` and calls `skill_manage` (Phase D). Inputs
/// are a `HashMap<String, Value>` so steps can be parameterized; output
/// names (`outputs`) are later referenced by downstream steps via
/// `requires`. Timeouts are per-step because some skill calls are slow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaSkillStep {
    /// Human-readable label; shown in `/meta runs` listings.
    pub label: String,

    /// Name of the skill to invoke. Must match a registered skill in
    /// `${HEARTQ_HOME}/skills/`.
    pub skill_name: String,

    /// JSON object passed as inputs to the skill.
    #[serde(default)]
    pub inputs: serde_json::Map<String, serde_json::Value>,

    /// Names under which this step's `StepOutput.value` is registered
    /// in the run's state map. Downstream steps list these in `requires`.
    #[serde(default)]
    pub outputs: Vec<String>,

    /// Other steps' output names this step needs to run. Resolved by
    /// string lookup against the run's state map.
    #[serde(default)]
    pub requires: Vec<String>,

    /// Per-step wall-clock cap. Defaults to 5 minutes if `None`.
    #[serde(default)]
    pub timeout_secs: Option<u32>,

    /// Optional explicit step id. When absent, [`MetaSkillStep::step_id`]
    /// generates one from the step index and label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Step ids (from `id` or generated `step_id`) that must complete
    /// before this step runs. Empty = linear scheduling (spec order).
    #[serde(default)]
    pub depends_on: Vec<String>,

    /// Optional condition expression. Empty / `"always"` = run;
    /// `"never"` = skip. Other values are evaluated via minijinja.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,

    /// Ordered route cases; first truthy `when` overrides [`Self::skill_name`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub route: Vec<RouteCase>,

    /// Step id to run when this step fails. Substitute output is mirrored
    /// under this step's declared `outputs` so downstream deps stay satisfied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_failure: Option<String>,
}

impl MetaSkillStep {
    /// Stable identity for checkpoint / log correlation.
    pub fn step_id(&self, index: usize) -> String {
        format!("step-{index:02}-{}", slugify(&self.label))
    }

    /// Resolved step id: explicit `id` or generated [`step_id`].
    pub fn resolved_id(&self, index: usize) -> String {
        self.id
            .clone()
            .unwrap_or_else(|| self.step_id(index))
    }
}

/// Static definition of an entire meta-skill (loaded from disk by the runner).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaSkillSpec {
    /// Canonical name; matches the directory under
    /// `${HEARTQ_HOME}/meta_skills/<name>/spec.json`.
    pub name: String,

    /// Version for audit / replay fidelity. Bumping the version on a
    /// spec invalidates in-flight runs (we snapshot the spec at run start
    /// so already-running runs keep using the old version).
    pub version: String,

    /// Free-form description shown in `/meta list` and used by
    /// `meta-skill-creator` when proposing new workflows.
    #[serde(default)]
    pub description: String,

    /// Step definitions, in execution order.
    pub steps: Vec<MetaSkillStep>,

    /// Step indices at which to checkpoint (auto-persist current state).
    /// Empty = no checkpointing; the run lives only in memory.
    #[serde(default)]
    pub checkpoints: Vec<usize>,

    /// SHA-256 digest of the canonicalized spec at run start. Used to
    /// detect drift between proposal and execution.
    pub digest: String,

    /// Substrings / phrases that trigger this meta-skill when matched
    /// against user text (see `meta_skill::trigger::match_triggers`).
    #[serde(default)]
    pub triggers: Vec<String>,

    /// Max concurrent ready steps when using the DAG scheduler. Default 4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_parallelism: Option<u32>,
}

/// Default parallelism when [`MetaSkillSpec::max_parallelism`] is unset.
pub const DEFAULT_MAX_PARALLELISM: u32 = 4;

impl MetaSkillSpec {
    /// Effective max parallelism (at least 1).
    pub fn effective_max_parallelism(&self) -> usize {
        self.max_parallelism
            .unwrap_or(DEFAULT_MAX_PARALLELISM)
            .max(1) as usize
    }
}

/// Validate DAG fields (`depends_on`, `on_failure`, route targets) at start.
///
/// Mirrors OpenSquilla parser rules for failover substitutes.
pub fn validate_dag_spec(spec: &MetaSkillSpec) -> Result<(), String> {
    let id_map: HashMap<String, usize> = spec
        .steps
        .iter()
        .enumerate()
        .map(|(i, s)| (s.resolved_id(i), i))
        .collect();

    // Detect duplicate ids.
    if id_map.len() != spec.steps.len() {
        return Err("duplicate step ids".into());
    }

    for (i, step) in spec.steps.iter().enumerate() {
        let sid = step.resolved_id(i);
        for dep in &step.depends_on {
            if !id_map.contains_key(dep) {
                return Err(format!("step `{sid}` depends_on unknown `{dep}`"));
            }
        }
        for case in &step.route {
            if case.when.trim().is_empty() || case.to.trim().is_empty() {
                return Err(format!("step `{sid}` has empty route when/to"));
            }
        }
    }

    let mut substitute_owners: HashMap<String, String> = HashMap::new();
    for (i, step) in spec.steps.iter().enumerate() {
        let sid = step.resolved_id(i);
        let Some(sub_id) = step.on_failure.as_deref().filter(|s| !s.is_empty()) else {
            continue;
        };
        if sub_id == sid {
            return Err(format!("step `{sid}` on_failure cannot point to itself"));
        }
        let Some(&sub_idx) = id_map.get(sub_id) else {
            return Err(format!("step `{sid}` on_failure unknown target `{sub_id}`"));
        };
        let sub = &spec.steps[sub_idx];
        if sub.on_failure.as_deref().is_some_and(|s| !s.is_empty()) {
            return Err(format!(
                "substitute `{sub_id}` cannot itself declare on_failure"
            ));
        }
        if !sub.depends_on.is_empty() {
            return Err(format!(
                "substitute `{sub_id}` must not declare depends_on"
            ));
        }
        if let Some(owner) = substitute_owners.insert(sub_id.to_string(), sid.clone()) {
            return Err(format!(
                "substitute `{sub_id}` referenced by both `{owner}` and `{sid}`"
            ));
        }
    }
    Ok(())
}

/// Step ids that are only started via `on_failure` (never auto-scheduled).
pub fn substitute_only_ids(spec: &MetaSkillSpec) -> std::collections::HashSet<String> {
    spec.steps
        .iter()
        .filter_map(|s| s.on_failure.clone())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Public alias so other modules can refer to the spec without writing
/// out the longer path.
pub type MetaSkill = MetaSkillSpec;

/// Live runtime state of one meta-skill execution.
///
/// One `MetaSkillRun` per `opensquilla meta runs create` invocation;
/// persisted as a single JSON file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaSkillRun {
    /// Unique run ID (UUID v4). Matches opensquilla's `run_id`.
    pub id: Uuid,

    /// Which meta-skill was invoked.
    pub meta_skill_name: String,

    /// Snapshot of the spec at run start, so replay reproduces the
    /// original plan even after the spec on disk is updated. Matches
    /// opensquilla's `plan_snapshot_json`.
    pub spec_snapshot: MetaSkillSpec,

    /// Trigger source (mirrors opensquilla's `triggered_by` column).
    #[serde(default = "default_trigger")]
    pub triggered_by: String,

    /// Wall-clock start time.
    pub started_at: SystemTime,

    /// Wall-clock end time (`None` while running).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<SystemTime>,

    /// Current step pointer (0-based). `steps.len()` = done.
    pub current_step: usize,

    /// Per-step state, in spec order.
    pub steps: Vec<RunStep>,

    /// Intermediates: every step's `outputs` are recorded here under their
    /// declared output names. Downstream steps look up by `requires`.
    #[serde(default)]
    pub state: HashMap<String, serde_json::Value>,

    /// Current run status. `Paused` is mid-flight; the rest are terminal.
    pub status: RunStatus,

    /// Snapshot of user inputs captured at run start (mirrors opensquilla's
    /// `inputs_json`).
    #[serde(default)]
    pub inputs: serde_json::Map<String, serde_json::Value>,

    /// Final text output if the run completes successfully. Mirrors
    /// `final_text`; `None` until then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_text: Option<String>,

    /// If the run failed: the step index that failed and the error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Prompt shown to the user when status is [`RunStatus::AwaitingUser`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clarify_prompt: Option<String>,
}

impl MetaSkillRun {
    /// Index of the next step to execute, or `None` if all done.
    pub fn next_step_index(&self) -> Option<usize> {
        if self.status != RunStatus::Running {
            return None;
        }
        self.steps.iter().position(|s| {
            matches!(s.status, MetaSkillStepStatus::Pending | MetaSkillStepStatus::Running)
        })
    }

    /// Compact one-line summary for `/meta runs list`.
    pub fn summary_line(&self) -> String {
        let total = self.steps.len();
        let done = self
            .steps
            .iter()
            .filter(|s| matches!(s.status, MetaSkillStepStatus::Ok))
            .count();
        format!(
            "{} | {} | {}/{} steps | {:?}",
            self.id,
            self.meta_skill_name,
            done,
            total,
            self.status,
        )
    }
}

/// Per-step live state inside a `MetaSkillRun`. Mirrors opensquilla's
/// `meta_skill_run_steps` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStep {
    /// Stable identity (e.g. `step-02-decode-cargo-lock`).
    pub step_id: String,

    /// Index in the spec this run step corresponds to.
    pub index: usize,

    /// Human-readable label, copied from `MetaSkillStep::label`.
    pub label: String,

    /// Skill invoked at this step.
    pub skill_name: String,

    /// Status. `Pending` until the runner picks it up.
    pub status: MetaSkillStepStatus,

    /// Wall-clock start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<SystemTime>,

    /// Wall-clock end (set on terminal statuses).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<SystemTime>,

    /// Skill output (terminal states only).
    #[serde(default)]
    pub output: StepOutput,

    /// Error message if status == Failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn default_trigger() -> String {
    "soft_meta_invoke".into()
}

fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_id_is_stable_and_human_readable() {
        let step = MetaSkillStep {
            label: "Decode Cargo.lock!".into(),
            skill_name: "cargo".into(),
            inputs: serde_json::Map::new(),
            outputs: vec![],
            requires: vec![],
            timeout_secs: None,
            id: None,
            depends_on: vec![],
            when: None,
            route: vec![],
            on_failure: None,
        };
        // `slugify` maps non-alphanumeric to '-' and then trims surrounding
        // dashes, so the trailing '!' collapses cleanly (no trailing dash).
        assert_eq!(step.step_id(2), "step-02-decode-cargo-lock");
    }

    #[test]
    fn next_step_index_skips_completed_steps() {
        let mut run = make_run(3);
        run.steps[0].status = MetaSkillStepStatus::Ok;
        run.steps[1].status = MetaSkillStepStatus::Running;
        assert_eq!(run.next_step_index(), Some(1));
        run.steps[1].status = MetaSkillStepStatus::Ok;
        assert_eq!(run.next_step_index(), Some(2));
        run.steps[2].status = MetaSkillStepStatus::Ok;
        assert_eq!(run.next_step_index(), None);
    }

    #[test]
    fn run_status_terminal_states_have_no_next_step() {
        for terminal in [
            RunStatus::Completed,
            RunStatus::Failed,
            RunStatus::Cancelled,
        ] {
            let mut run = make_run(2);
            run.status = terminal;
            assert_eq!(run.next_step_index(), None);
        }
    }

    #[test]
    fn serde_round_trip() {
        let run = make_run(2);
        let json = serde_json::to_string(&run).unwrap();
        let back: MetaSkillRun = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, run.id);
        assert_eq!(back.steps.len(), 2);
    }

    fn make_run(steps: usize) -> MetaSkillRun {
        MetaSkillRun {
            id: Uuid::new_v4(),
            meta_skill_name: "test".into(),
            spec_snapshot: MetaSkillSpec {
                name: "test".into(),
                version: "1".into(),
                description: String::new(),
                steps: vec![],
                checkpoints: vec![],
                digest: "deadbeef".into(),
                triggers: vec![],
                max_parallelism: None,
            },
            triggered_by: "soft_meta_invoke".into(),
            started_at: SystemTime::now(),
            ended_at: None,
            current_step: 0,
            steps: (0..steps)
                .map(|i| RunStep {
                    step_id: format!("step-{i:02}"),
                    index: i,
                    label: format!("step {i}"),
                    skill_name: "noop".into(),
                    status: MetaSkillStepStatus::Pending,
                    started_at: None,
                    ended_at: None,
                    output: StepOutput::default(),
                    error: None,
                })
                .collect(),
            state: HashMap::new(),
            status: RunStatus::Running,
            inputs: serde_json::Map::new(),
            final_text: None,
            error: None,
            clarify_prompt: None,
        }
    }

    #[test]
    fn validate_on_failure_rejects_self_ref() {
        let mut spec = MetaSkillSpec {
            name: "t".into(),
            version: "1".into(),
            description: String::new(),
            steps: vec![MetaSkillStep {
                label: "a".into(),
                skill_name: "s".into(),
                inputs: serde_json::Map::new(),
                outputs: vec![],
                requires: vec![],
                timeout_secs: None,
                id: Some("a".into()),
                depends_on: vec![],
                when: None,
                route: vec![],
                on_failure: Some("a".into()),
            }],
            checkpoints: vec![],
            digest: "x".into(),
            triggers: vec![],
            max_parallelism: None,
        };
        assert!(validate_dag_spec(&spec).is_err());
        spec.steps[0].on_failure = Some("missing".into());
        assert!(validate_dag_spec(&spec).is_err());
    }
}