//! Meta-skill runner — drives `MetaSkillRun` through its step lifecycle.
//!
//! OpenSquilla-aligned DAG semantics:
//! - `when` (minijinja) / `route` / `depends_on` ready-set parallelism /
//!   `on_failure` failover with output mirroring.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use thiserror::Error;
use uuid::Uuid;

use super::model::{
    substitute_only_ids, validate_dag_spec, MetaSkillRun, MetaSkillSpec, MetaSkillStep,
    MetaSkillStepStatus, RunStatus, RunStep, StepOutput,
};
use super::store::{MetaSkillStore, StoreError};
use super::templating::{effective_skill, eval_when, render_inputs};

/// Outcome of running a single step / batch. Mirrors opensquilla's
/// `MetaSkillRunResult` (step succeeded / failed / paused).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    /// Step(s) succeeded; recorded in the run.
    Ok,
    /// Step failed; the run is now `Failed`. `error` carries the message.
    Failed(String),
    /// Step was skipped by user request; recorded as `Skipped`.
    Skipped,
    /// Run was paused before/during this step; runner should stop.
    Paused,
    /// All steps finished successfully; run transitioned to `Completed`.
    Completed,
}

/// All the ways a runner operation can fail.
#[derive(Debug, Error)]
pub enum MetaSkillError {
    #[error("meta-skill storage error: {0}")]
    Store(#[from] StoreError),

    #[error("meta-skill run {0} not found")]
    NotFound(Uuid),

    #[error("meta-skill run {0} is already in terminal state {1:?}")]
    Terminal(Uuid, RunStatus),

    #[error("meta-skill run {0} step {1} skill `{2}` failed: {3}")]
    SkillFailed(Uuid, usize, String, String),

    #[error("meta-skill run {0} step {1} missing required output: {2}")]
    MissingRequiredOutput(Uuid, usize, String),

    #[error("meta-skill run {0} step dependency cycle: {1}")]
    DependencyCycle(Uuid, String),

    #[error("meta-skill run {0} step {1} timed out after {2}s")]
    Timeout(Uuid, usize, u32),

    #[error("meta-skill invalid spec: {0}")]
    InvalidSpec(String),
}

/// Abstraction over the actual skill-dispatch mechanism.
pub trait SkillExecutor: Send + Sync {
    fn execute(
        &self,
        skill_name: &str,
        inputs: &serde_json::Map<String, serde_json::Value>,
        timeout_secs: u32,
    ) -> Result<serde_json::Value, String>;
}

/// Pure-in-memory mock for tests.
#[derive(Debug, Default)]
pub struct MockSkillExecutor {
    responses: std::sync::Mutex<HashMap<String, Result<serde_json::Value, String>>>,
}

impl MockSkillExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on(
        &self,
        skill_name: &str,
        _inputs: &serde_json::Map<String, serde_json::Value>,
        response: Result<serde_json::Value, String>,
    ) {
        self.responses
            .lock()
            .unwrap()
            .insert(skill_name.to_string(), response);
    }
}

impl SkillExecutor for MockSkillExecutor {
    fn execute(
        &self,
        skill_name: &str,
        _inputs: &serde_json::Map<String, serde_json::Value>,
        _timeout_secs: u32,
    ) -> Result<serde_json::Value, String> {
        self.responses
            .lock()
            .unwrap()
            .get(skill_name)
            .cloned()
            .unwrap_or(Err(format!("no mock response for `{skill_name}`")))
    }
}

/// The runner itself.
#[derive(Clone)]
pub struct MetaSkillRunner {
    store: MetaSkillStore,
    executor: Arc<dyn SkillExecutor>,
}

pub type RunOutcome = Result<Uuid, MetaSkillError>;

impl MetaSkillRunner {
    pub fn new(store: MetaSkillStore) -> Self {
        Self {
            store,
            executor: Arc::new(MockSkillExecutor::new()),
        }
    }

    pub fn with_executor(store: MetaSkillStore, executor: Arc<dyn SkillExecutor>) -> Self {
        Self { store, executor }
    }

    pub fn load(&self, id: Uuid) -> Result<MetaSkillRun, MetaSkillError> {
        Ok(self.store.load(id)?)
    }

    pub fn start(
        &self,
        spec: MetaSkillSpec,
        inputs: serde_json::Map<String, serde_json::Value>,
    ) -> RunOutcome {
        self.start_triggered(spec, inputs, "soft_meta_invoke")
    }

    pub fn start_triggered(
        &self,
        spec: MetaSkillSpec,
        inputs: serde_json::Map<String, serde_json::Value>,
        triggered_by: &str,
    ) -> RunOutcome {
        validate_dag_spec(&spec).map_err(MetaSkillError::InvalidSpec)?;
        let id = Uuid::new_v4();
        if uses_dag(&spec) {
            detect_dependency_cycle(&spec)
                .map_err(|msg| MetaSkillError::DependencyCycle(id, msg))?;
        }
        let run = MetaSkillRun {
            id,
            meta_skill_name: spec.name.clone(),
            spec_snapshot: spec.clone(),
            triggered_by: triggered_by.into(),
            started_at: SystemTime::now(),
            ended_at: None,
            current_step: 0,
            steps: spec
                .steps
                .iter()
                .enumerate()
                .map(|(i, s)| RunStep {
                    step_id: s.resolved_id(i),
                    index: i,
                    label: s.label.clone(),
                    skill_name: s.skill_name.clone(),
                    status: MetaSkillStepStatus::Pending,
                    started_at: None,
                    ended_at: None,
                    output: StepOutput::default(),
                    error: None,
                })
                .collect(),
            state: HashMap::new(),
            status: RunStatus::Running,
            inputs,
            final_text: None,
            error: None,
            clarify_prompt: None,
        };
        self.store.save(&run)?;
        Ok(run.id)
    }

    /// Execute the next pending step, or a ready-set batch when using the
    /// advanced DAG scheduler.
    pub fn run_step(&self, id: Uuid) -> Result<StepOutcome, MetaSkillError> {
        let mut run = self.store.load(id)?;
        if !matches!(run.status, RunStatus::Running) {
            return Ok(match run.status {
                RunStatus::Completed => StepOutcome::Completed,
                RunStatus::Failed => {
                    StepOutcome::Failed(run.error.unwrap_or_else(|| "unknown".into()))
                }
                RunStatus::Cancelled => StepOutcome::Skipped,
                RunStatus::Paused | RunStatus::AwaitingUser => StepOutcome::Paused,
                RunStatus::Running => unreachable!(),
            });
        }

        self.auto_skip_disabled_steps(&mut run)?;

        if uses_advanced_scheduler(&run.spec_snapshot) {
            self.run_ready_batch(id, &mut run)
        } else {
            self.run_one_linear(id, &mut run)
        }
    }

    pub fn run_to_completion(&self, id: Uuid) -> Result<StepOutcome, MetaSkillError> {
        loop {
            match self.run_step(id)? {
                StepOutcome::Completed | StepOutcome::Failed(_) => {
                    break Ok(self
                        .load(id)?
                        .status
                        .terminal_outcome()
                        .expect("terminal run yields outcome"))
                }
                StepOutcome::Ok => continue,
                StepOutcome::Paused | StepOutcome::Skipped => {
                    break Ok(StepOutcome::Paused);
                }
            }
        }
    }

    pub fn pause(&self, id: Uuid) -> Result<(), MetaSkillError> {
        let mut run = self.store.load(id)?;
        if !matches!(run.status, RunStatus::Running) {
            return Err(MetaSkillError::Terminal(id, run.status));
        }
        run.status = RunStatus::Paused;
        self.store.save(&run)?;
        Ok(())
    }

    pub fn resume(&self, id: Uuid) -> Result<(), MetaSkillError> {
        let mut run = self.store.load(id)?;
        if matches!(
            run.status,
            RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled
        ) {
            return Err(MetaSkillError::Terminal(id, run.status));
        }
        if matches!(run.status, RunStatus::Paused) {
            run.status = RunStatus::Running;
            self.store.save(&run)?;
        }
        Ok(())
    }

    pub fn cancel(&self, id: Uuid) -> Result<(), MetaSkillError> {
        let mut run = self.store.load(id)?;
        if matches!(
            run.status,
            RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled
        ) {
            return Err(MetaSkillError::Terminal(id, run.status));
        }
        run.status = RunStatus::Cancelled;
        run.ended_at = Some(SystemTime::now());
        self.store.save(&run)?;
        Ok(())
    }

    pub fn skip_step(&self, id: Uuid) -> Result<(), MetaSkillError> {
        let mut run = self.store.load(id)?;
        if !matches!(run.status, RunStatus::Running) {
            return Err(MetaSkillError::Terminal(id, run.status));
        }
        let idx = next_executable_step_index(&run).ok_or(MetaSkillError::NotFound(id))?;
        let step = &mut run.steps[idx];
        step.status = MetaSkillStepStatus::Skipped;
        step.ended_at = Some(SystemTime::now());
        step.output.note = Some("user-skipped".into());
        self.store.save(&run)?;
        Ok(())
    }

    pub fn pause_for_clarify(&self, id: Uuid, prompt: String) -> Result<(), MetaSkillError> {
        let mut run = self.store.load(id)?;
        if !matches!(run.status, RunStatus::Running) {
            return Err(MetaSkillError::Terminal(id, run.status));
        }
        run.status = RunStatus::AwaitingUser;
        run.clarify_prompt = Some(prompt);
        self.store.save(&run)?;
        Ok(())
    }

    pub fn resume_from_clarify(&self, id: Uuid) -> Result<(), MetaSkillError> {
        self.resume_from_clarify_with(id, None)
    }

    /// Resume from clarify, optionally injecting the user's reply into
    /// `run.inputs["clarification"]` and `run.state["user_clarification"]`.
    pub fn resume_from_clarify_with(
        &self,
        id: Uuid,
        user_text: Option<&str>,
    ) -> Result<(), MetaSkillError> {
        let mut run = self.store.load(id)?;
        if matches!(run.status, RunStatus::AwaitingUser) {
            if let Some(text) = user_text.filter(|t| !t.trim().is_empty()) {
                run.inputs
                    .insert("clarification".into(), serde_json::json!(text));
                run.state
                    .insert("user_clarification".into(), serde_json::json!(text));
            }
            run.status = RunStatus::Running;
            run.clarify_prompt = None;
            self.store.save(&run)?;
        } else if matches!(run.status, RunStatus::Paused) {
            run.status = RunStatus::Running;
            self.store.save(&run)?;
        } else if matches!(
            run.status,
            RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled
        ) {
            return Err(MetaSkillError::Terminal(id, run.status));
        }
        Ok(())
    }

    // ---- internal ----

    fn auto_skip_disabled_steps(&self, run: &mut MetaSkillRun) -> Result<(), MetaSkillError> {
        let mut changed = false;
        let inputs = run.inputs.clone();
        let state = run.state.clone();
        let sub_only = substitute_only_ids(&run.spec_snapshot);
        for (i, step_spec) in run.spec_snapshot.steps.iter().enumerate() {
            if !matches!(run.steps[i].status, MetaSkillStepStatus::Pending) {
                continue;
            }
            let sid = step_spec.resolved_id(i);
            if sub_only.contains(&sid) {
                continue;
            }
            if !eval_when(step_spec.when.as_deref(), &inputs, &state) {
                run.steps[i].status = MetaSkillStepStatus::Skipped;
                run.steps[i].ended_at = Some(SystemTime::now());
                run.steps[i].output.note = Some("when=false".into());
                // Empty output still satisfies downstream depends_on.
                for name in &step_spec.outputs {
                    run.state
                        .entry(name.clone())
                        .or_insert(serde_json::Value::Null);
                }
                changed = true;
            }
        }
        if changed {
            self.store.save(run)?;
        }
        Ok(())
    }

    fn run_one_linear(
        &self,
        id: Uuid,
        run: &mut MetaSkillRun,
    ) -> Result<StepOutcome, MetaSkillError> {
        let idx = match next_executable_step_index(run) {
            Some(i) => i,
            None if all_steps_terminal(run) => {
                return Ok(self.finalize(run.clone(), StepOutcome::Completed));
            }
            None => {
                return Err(MetaSkillError::DependencyCycle(
                    id,
                    "no ready steps while run still has pending work".into(),
                ));
            }
        };
        let step_spec = run.spec_snapshot.steps[idx].clone();
        for req in &step_spec.requires {
            if !run.state.contains_key(req) {
                return Err(MetaSkillError::MissingRequiredOutput(
                    id,
                    idx,
                    req.clone(),
                ));
            }
        }

        let prepared = prepare_step_invocation(run, idx, &step_spec);
        run.steps[idx].status = MetaSkillStepStatus::Running;
        run.steps[idx].started_at = Some(SystemTime::now());
        run.steps[idx].skill_name = prepared.skill.clone();
        run.current_step = idx;
        self.store.save(run)?;

        let result = self
            .executor
            .execute(&prepared.skill, &prepared.inputs, prepared.timeout);

        apply_step_result(run, idx, &step_spec, result, None);
        self.store.save(run)?;

        if matches!(run.status, RunStatus::AwaitingUser) {
            return Ok(StepOutcome::Paused);
        }
        if matches!(run.status, RunStatus::Failed) {
            return Ok(StepOutcome::Failed(
                run.error.clone().unwrap_or_else(|| "unknown".into()),
            ));
        }
        Ok(if next_executable_step_index(run).is_none() && all_steps_terminal(run) {
            self.finalize(run.clone(), StepOutcome::Completed)
        } else {
            StepOutcome::Ok
        })
    }

    fn run_ready_batch(
        &self,
        id: Uuid,
        run: &mut MetaSkillRun,
    ) -> Result<StepOutcome, MetaSkillError> {
        let ready = collect_ready_indices(run);
        if ready.is_empty() {
            if all_steps_terminal(run) {
                return Ok(self.finalize(run.clone(), StepOutcome::Completed));
            }
            // Pending substitutes waiting for failover are OK — treated as
            // terminal-for-now only if nothing else pending that isn't sub-only.
            let sub_only = substitute_only_ids(&run.spec_snapshot);
            let stuck = run.steps.iter().enumerate().any(|(i, s)| {
                matches!(s.status, MetaSkillStepStatus::Pending)
                    && !sub_only.contains(&run.spec_snapshot.steps[i].resolved_id(i))
            });
            if stuck {
                return Err(MetaSkillError::DependencyCycle(
                    id,
                    "no ready steps while run still has pending work".into(),
                ));
            }
            // Only substitute-only pending left without a primary trigger —
            // mark them skipped and complete.
            for (i, s) in run.steps.iter_mut().enumerate() {
                if matches!(s.status, MetaSkillStepStatus::Pending)
                    && sub_only.contains(&run.spec_snapshot.steps[i].resolved_id(i))
                {
                    s.status = MetaSkillStepStatus::Skipped;
                    s.ended_at = Some(SystemTime::now());
                    s.output.note = Some("unused substitute".into());
                }
            }
            self.store.save(run)?;
            return Ok(self.finalize(run.clone(), StepOutcome::Completed));
        }

        let limit = run.spec_snapshot.effective_max_parallelism();
        let batch: Vec<usize> = ready.into_iter().take(limit).collect();

        // Pre-flight requires + mark Running.
        let mut prepared: Vec<(usize, MetaSkillStep, PreparedInvocation)> = Vec::new();
        for &idx in &batch {
            let step_spec = run.spec_snapshot.steps[idx].clone();
            for req in &step_spec.requires {
                if !run.state.contains_key(req) {
                    return Err(MetaSkillError::MissingRequiredOutput(
                        id,
                        idx,
                        req.clone(),
                    ));
                }
            }
            let prep = prepare_step_invocation(run, idx, &step_spec);
            run.steps[idx].status = MetaSkillStepStatus::Running;
            run.steps[idx].started_at = Some(SystemTime::now());
            run.steps[idx].skill_name = prep.skill.clone();
            run.current_step = idx;
            prepared.push((idx, step_spec, prep));
        }
        self.store.save(run)?;

        // Parallel execute.
        let executor = Arc::clone(&self.executor);
        let results: Vec<(usize, MetaSkillStep, Result<serde_json::Value, String>)> =
            std::thread::scope(|scope| {
                let mut handles = Vec::new();
                for (idx, step_spec, prep) in prepared {
                    let exec = Arc::clone(&executor);
                    handles.push(scope.spawn(move || {
                        let result = exec.execute(&prep.skill, &prep.inputs, prep.timeout);
                        (idx, step_spec, result)
                    }));
                }
                handles
                    .into_iter()
                    .map(|h| h.join().expect("step thread panicked"))
                    .collect()
            });

        // Apply results. Track failover primary indices and clarify.
        let mut any_clarify: Option<String> = None;
        let mut hard_fail: Option<String> = None;
        let mut failover_queue: Vec<(usize, String)> = Vec::new(); // (primary_idx, sub_id)

        for (idx, step_spec, result) in results {
            match result {
                Ok(ref value) => {
                    if let Some(prompt) = clarify_prompt_from(value) {
                        any_clarify = Some(prompt);
                    }
                    apply_step_result(run, idx, &step_spec, result, None);
                }
                Err(ref err) => {
                    if let Some(sub) = step_spec
                        .on_failure
                        .as_deref()
                        .filter(|s| !s.is_empty())
                    {
                        // Mark substituted; queue failover.
                        run.steps[idx].status = MetaSkillStepStatus::Substituted;
                        run.steps[idx].ended_at = Some(SystemTime::now());
                        run.steps[idx].error = Some(err.clone());
                        run.steps[idx].output.note =
                            Some(format!("failover → {sub}"));
                        failover_queue.push((idx, sub.to_string()));
                    } else {
                        let err_owned = err.clone();
                        apply_step_result(run, idx, &step_spec, result, None);
                        hard_fail = Some(err_owned);
                    }
                }
            }
        }

        // Run failover substitutes sequentially (output mirrored to primary).
        for (primary_idx, sub_id) in failover_queue {
            let id_map = step_id_map(&run.spec_snapshot);
            let Some(&sub_idx) = id_map.get(&sub_id) else {
                hard_fail = Some(format!("on_failure target `{sub_id}` missing"));
                continue;
            };
            if !matches!(run.steps[sub_idx].status, MetaSkillStepStatus::Pending) {
                continue;
            }
            let sub_spec = run.spec_snapshot.steps[sub_idx].clone();
            let prep = prepare_step_invocation(run, sub_idx, &sub_spec);
            run.steps[sub_idx].status = MetaSkillStepStatus::Running;
            run.steps[sub_idx].started_at = Some(SystemTime::now());
            run.steps[sub_idx].skill_name = prep.skill.clone();
            let result = self
                .executor
                .execute(&prep.skill, &prep.inputs, prep.timeout);
            let primary_outputs = run.spec_snapshot.steps[primary_idx].outputs.clone();
            match result {
                Ok(value) => {
                    if let Some(prompt) = clarify_prompt_from(&value) {
                        any_clarify = Some(prompt);
                    }
                    run.steps[sub_idx].status = MetaSkillStepStatus::Ok;
                    run.steps[sub_idx].ended_at = Some(SystemTime::now());
                    run.steps[sub_idx].output.value = Some(value.clone());
                    // Mirror onto substitute's own outputs AND primary's outputs.
                    for name in &sub_spec.outputs {
                        run.state.insert(name.clone(), value.clone());
                    }
                    for name in &primary_outputs {
                        run.state.insert(name.clone(), value.clone());
                    }
                }
                Err(err) => {
                    run.steps[sub_idx].status = MetaSkillStepStatus::Failed;
                    run.steps[sub_idx].ended_at = Some(SystemTime::now());
                    run.steps[sub_idx].error = Some(err.clone());
                    hard_fail = Some(err);
                }
            }
        }

        if let Some(prompt) = any_clarify {
            run.status = RunStatus::AwaitingUser;
            run.clarify_prompt = Some(prompt);
            self.store.save(run)?;
            return Ok(StepOutcome::Paused);
        }

        if let Some(err) = hard_fail {
            run.status = RunStatus::Failed;
            run.error = Some(err.clone());
            run.ended_at = Some(SystemTime::now());
            self.store.save(run)?;
            return Ok(StepOutcome::Failed(err));
        }

        self.store.save(run)?;
        Ok(
            if collect_ready_indices(run).is_empty() && all_steps_terminal_or_unused_sub(run) {
                // Skip unused substitutes then finalize.
                let sub_only = substitute_only_ids(&run.spec_snapshot);
                for (i, s) in run.steps.iter_mut().enumerate() {
                    if matches!(s.status, MetaSkillStepStatus::Pending)
                        && sub_only.contains(&run.spec_snapshot.steps[i].resolved_id(i))
                    {
                        s.status = MetaSkillStepStatus::Skipped;
                        s.ended_at = Some(SystemTime::now());
                        s.output.note = Some("unused substitute".into());
                    }
                }
                if all_steps_terminal(run) {
                    self.finalize(run.clone(), StepOutcome::Completed)
                } else {
                    StepOutcome::Ok
                }
            } else {
                StepOutcome::Ok
            },
        )
    }

    fn finalize(&self, mut run: MetaSkillRun, outcome: StepOutcome) -> StepOutcome {
        if matches!(outcome, StepOutcome::Completed) {
            run.status = RunStatus::Completed;
            run.ended_at = Some(SystemTime::now());
            run.final_text = run
                .steps
                .iter()
                .rev()
                .find(|s| {
                    matches!(
                        s.status,
                        MetaSkillStepStatus::Ok | MetaSkillStepStatus::Substituted
                    ) && s.output.value.is_some()
                })
                .and_then(|s| s.output.value.as_ref())
                .and_then(|v| v.as_str().map(str::to_string))
                .or_else(|| {
                    run.steps
                        .iter()
                        .rev()
                        .find_map(|s| s.output.value.as_ref())
                        .and_then(|v| v.as_str().map(str::to_string))
                })
                .or_else(|| Some("(no output)".into()));
            let _ = self.store.save(&run);
        }
        outcome
    }
}

impl RunStatus {
    pub fn terminal_outcome(&self) -> Option<StepOutcome> {
        match self {
            RunStatus::Completed => Some(StepOutcome::Completed),
            RunStatus::Failed => Some(StepOutcome::Failed("(see run.error)".into())),
            RunStatus::Cancelled => Some(StepOutcome::Skipped),
            RunStatus::Running | RunStatus::Paused | RunStatus::AwaitingUser => None,
        }
    }
}

struct PreparedInvocation {
    skill: String,
    inputs: serde_json::Map<String, serde_json::Value>,
    timeout: u32,
}

fn prepare_step_invocation(
    run: &MetaSkillRun,
    _idx: usize,
    step_spec: &MetaSkillStep,
) -> PreparedInvocation {
    let skill = effective_skill(step_spec, &run.inputs, &run.state);
    let mut rendered = render_inputs(step_spec, &run.inputs, &run.state);
    for req in &step_spec.requires {
        if let Some(v) = run.state.get(req) {
            rendered.insert(req.clone(), v.clone());
        }
    }
    PreparedInvocation {
        skill,
        inputs: rendered,
        timeout: step_spec.timeout_secs.unwrap_or(300),
    }
}

fn apply_step_result(
    run: &mut MetaSkillRun,
    idx: usize,
    step_spec: &MetaSkillStep,
    result: Result<serde_json::Value, String>,
    _mirror_outputs: Option<&[String]>,
) {
    match result {
        Ok(value) => {
            run.steps[idx].status = MetaSkillStepStatus::Ok;
            run.steps[idx].ended_at = Some(SystemTime::now());
            run.steps[idx].output.value = Some(value.clone());
            for name in &step_spec.outputs {
                run.state.insert(name.clone(), value.clone());
            }
            if let Some(prompt) = clarify_prompt_from(&value) {
                run.status = RunStatus::AwaitingUser;
                run.clarify_prompt = Some(prompt);
            }
        }
        Err(err) => {
            run.steps[idx].status = MetaSkillStepStatus::Failed;
            run.steps[idx].ended_at = Some(SystemTime::now());
            run.steps[idx].error = Some(err.clone());
            run.error = Some(err);
            run.status = RunStatus::Failed;
            run.ended_at = Some(SystemTime::now());
        }
    }
}

fn uses_dag(spec: &MetaSkillSpec) -> bool {
    spec.steps.iter().any(|s| !s.depends_on.is_empty())
}

fn uses_advanced_scheduler(spec: &MetaSkillSpec) -> bool {
    uses_dag(spec)
        || spec.max_parallelism.is_some()
        || spec.steps.iter().any(|s| {
            !s.route.is_empty()
                || s.on_failure.as_deref().is_some_and(|x| !x.is_empty())
                || matches!(
                    s.when.as_deref().map(str::trim),
                    Some(w) if !w.is_empty() && w != "always" && w != "never"
                )
        })
}

fn step_id_map(spec: &MetaSkillSpec) -> HashMap<String, usize> {
    spec.steps
        .iter()
        .enumerate()
        .map(|(i, s)| (s.resolved_id(i), i))
        .collect()
}

fn deps_satisfied(
    run: &MetaSkillRun,
    step: &MetaSkillStep,
    id_map: &HashMap<String, usize>,
) -> bool {
    for dep in &step.depends_on {
        let Some(&dep_idx) = id_map.get(dep) else {
            return false;
        };
        match run.steps[dep_idx].status {
            MetaSkillStepStatus::Ok
            | MetaSkillStepStatus::Skipped
            | MetaSkillStepStatus::Substituted => {}
            _ => return false,
        }
    }
    true
}

fn collect_ready_indices(run: &MetaSkillRun) -> Vec<usize> {
    let spec = &run.spec_snapshot;
    let id_map = step_id_map(spec);
    let sub_only = substitute_only_ids(spec);
    let mut ready = Vec::new();
    for (i, step_spec) in spec.steps.iter().enumerate() {
        if !matches!(run.steps[i].status, MetaSkillStepStatus::Pending) {
            continue;
        }
        let sid = step_spec.resolved_id(i);
        if sub_only.contains(&sid) {
            continue;
        }
        if !eval_when(step_spec.when.as_deref(), &run.inputs, &run.state) {
            continue;
        }
        if deps_satisfied(run, step_spec, &id_map) {
            ready.push(i);
        }
    }
    ready
}

fn next_executable_step_index(run: &MetaSkillRun) -> Option<usize> {
    let spec = &run.spec_snapshot;
    if !uses_dag(spec) && !uses_advanced_scheduler(spec) {
        return run.next_step_index();
    }
    collect_ready_indices(run).into_iter().next()
}

fn all_steps_terminal(run: &MetaSkillRun) -> bool {
    run.steps.iter().all(|s| {
        matches!(
            s.status,
            MetaSkillStepStatus::Ok
                | MetaSkillStepStatus::Failed
                | MetaSkillStepStatus::Skipped
                | MetaSkillStepStatus::Substituted
        )
    })
}

fn all_steps_terminal_or_unused_sub(run: &MetaSkillRun) -> bool {
    let sub_only = substitute_only_ids(&run.spec_snapshot);
    run.steps.iter().enumerate().all(|(i, s)| {
        matches!(
            s.status,
            MetaSkillStepStatus::Ok
                | MetaSkillStepStatus::Failed
                | MetaSkillStepStatus::Skipped
                | MetaSkillStepStatus::Substituted
        ) || (matches!(s.status, MetaSkillStepStatus::Pending)
            && sub_only.contains(&run.spec_snapshot.steps[i].resolved_id(i)))
    })
}

fn clarify_prompt_from(value: &serde_json::Value) -> Option<String> {
    value
        .get("clarify")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn detect_dependency_cycle(spec: &MetaSkillSpec) -> Result<(), String> {
    let id_map = step_id_map(spec);
    let mut state = vec![0u8; spec.steps.len()];

    fn dfs(
        idx: usize,
        spec: &MetaSkillSpec,
        id_map: &HashMap<String, usize>,
        state: &mut [u8],
    ) -> Result<(), String> {
        if state[idx] == 1 {
            return Err(spec.steps[idx].resolved_id(idx));
        }
        if state[idx] == 2 {
            return Ok(());
        }
        state[idx] = 1;
        for dep in &spec.steps[idx].depends_on {
            let Some(&dep_idx) = id_map.get(dep) else {
                return Err(format!("unknown dependency `{dep}`"));
            };
            dfs(dep_idx, spec, id_map, state)?;
        }
        state[idx] = 2;
        Ok(())
    }

    for i in 0..spec.steps.len() {
        if state[i] == 0 {
            if let Err(msg) = dfs(i, spec, &id_map, &mut state) {
                return Err(format!("cycle involving `{msg}`"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta_skill::model::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    fn step(
        label: &str,
        skill: &str,
        id: Option<&str>,
        depends_on: Vec<&str>,
        outputs: Vec<&str>,
        requires: Vec<&str>,
    ) -> MetaSkillStep {
        MetaSkillStep {
            label: label.into(),
            skill_name: skill.into(),
            inputs: serde_json::Map::new(),
            outputs: outputs.into_iter().map(str::to_string).collect(),
            requires: requires.into_iter().map(str::to_string).collect(),
            timeout_secs: None,
            id: id.map(str::to_string),
            depends_on: depends_on.into_iter().map(str::to_string).collect(),
            when: None,
            route: vec![],
            on_failure: None,
        }
    }

    fn spec_two_steps() -> MetaSkillSpec {
        MetaSkillSpec {
            name: "demo".into(),
            version: "1".into(),
            description: "two-step demo".into(),
            steps: vec![
                step("produce A", "echo", None, vec![], vec!["a"], vec![]),
                step("consume A", "verify", None, vec![], vec![], vec!["a"]),
            ],
            checkpoints: vec![],
            digest: "deadbeef".into(),
            triggers: vec![],
            max_parallelism: None,
        }
    }

    fn spec_dag_two_steps() -> MetaSkillSpec {
        MetaSkillSpec {
            name: "dag-demo".into(),
            version: "1".into(),
            description: String::new(),
            steps: vec![
                step("first", "echo", Some("step-a"), vec![], vec!["x"], vec![]),
                step(
                    "second",
                    "verify",
                    Some("step-b"),
                    vec!["step-a"],
                    vec![],
                    vec!["x"],
                ),
            ],
            checkpoints: vec![],
            digest: "deadbeef".into(),
            triggers: vec![],
            max_parallelism: None,
        }
    }

    fn store() -> (tempfile::TempDir, MetaSkillStore) {
        let tmp = tempfile::tempdir().unwrap();
        let store = MetaSkillStore::with_base_dir(tmp.path());
        (tmp, store)
    }

    #[test]
    fn start_persists_pending_run() {
        let (_tmp, store) = store();
        let runner = MetaSkillRunner::new(store.clone());
        let id = runner
            .start(spec_two_steps(), serde_json::Map::new())
            .unwrap();
        let run = store.load(id).unwrap();
        assert_eq!(run.steps.len(), 2);
        assert!(run
            .steps
            .iter()
            .all(|s| s.status == MetaSkillStepStatus::Pending));
    }

    #[test]
    fn run_step_invokes_skill_and_records_output() {
        let (_tmp, store) = store();
        let mock = MockSkillExecutor::new();
        mock.on(
            "echo",
            &serde_json::Map::new(),
            Ok(serde_json::json!("hello")),
        );
        let runner = MetaSkillRunner::with_executor(store.clone(), Arc::new(mock));
        let id = runner
            .start(spec_two_steps(), serde_json::Map::new())
            .unwrap();

        let outcome = runner.run_step(id).unwrap();
        assert_eq!(outcome, StepOutcome::Ok);
        let run = store.load(id).unwrap();
        assert_eq!(run.state.get("a").unwrap(), &serde_json::json!("hello"));
        assert_eq!(run.steps[0].status, MetaSkillStepStatus::Ok);
    }

    #[test]
    fn second_step_fails_when_required_output_missing() {
        let (_tmp, store) = store();
        let mock = MockSkillExecutor::new();
        mock.on("echo", &serde_json::Map::new(), Ok(serde_json::json!("x")));
        let runner = MetaSkillRunner::with_executor(store.clone(), Arc::new(mock));
        let mut spec = spec_two_steps();
        spec.steps[0].outputs.clear();
        spec.steps[1].requires = vec!["missing".into()];
        let id = runner.start(spec, serde_json::Map::new()).unwrap();
        let outcome = runner.run_step(id).unwrap();
        assert_eq!(outcome, StepOutcome::Ok);
        let err = runner.run_step(id).unwrap_err();
        assert!(matches!(err, MetaSkillError::MissingRequiredOutput(..)));
    }

    #[test]
    fn run_to_completion_emits_terminal_outcome() {
        let (_tmp, store) = store();
        let mock = MockSkillExecutor::new();
        mock.on("echo", &serde_json::Map::new(), Ok(serde_json::json!("x")));
        mock.on(
            "verify",
            &serde_json::Map::new(),
            Ok(serde_json::json!("ok")),
        );
        let runner = MetaSkillRunner::with_executor(store.clone(), Arc::new(mock));
        let id = runner
            .start(spec_two_steps(), serde_json::Map::new())
            .unwrap();
        let outcome = runner.run_to_completion(id).unwrap();
        assert_eq!(outcome, StepOutcome::Completed);
        let run = store.load(id).unwrap();
        assert_eq!(run.status, RunStatus::Completed);
        assert!(run.final_text.is_some());
    }

    #[test]
    fn skill_failure_marks_run_failed() {
        let (_tmp, store) = store();
        let mock = MockSkillExecutor::new();
        mock.on("echo", &serde_json::Map::new(), Err("kaboom".into()));
        let runner = MetaSkillRunner::with_executor(store.clone(), Arc::new(mock));
        let id = runner
            .start(spec_two_steps(), serde_json::Map::new())
            .unwrap();
        let outcome = runner.run_step(id).unwrap();
        assert!(matches!(outcome, StepOutcome::Failed(_)));
        let run = store.load(id).unwrap();
        assert_eq!(run.status, RunStatus::Failed);
        assert!(run.error.is_some());
    }

    #[test]
    fn pause_then_resume_picks_up_at_next_step() {
        let (_tmp, store) = store();
        let mock = MockSkillExecutor::new();
        mock.on("echo", &serde_json::Map::new(), Ok(serde_json::json!("y")));
        let runner = MetaSkillRunner::with_executor(store.clone(), Arc::new(mock));
        let id = runner
            .start(spec_two_steps(), serde_json::Map::new())
            .unwrap();
        runner.pause(id).unwrap();
        assert!(matches!(store.load(id).unwrap().status, RunStatus::Paused));
        runner.resume(id).unwrap();
        let outcome = runner.run_step(id).unwrap();
        assert_eq!(outcome, StepOutcome::Ok);
    }

    #[test]
    fn cancel_is_terminal() {
        let (_tmp, store) = store();
        let runner = MetaSkillRunner::new(store.clone());
        let id = runner
            .start(spec_two_steps(), serde_json::Map::new())
            .unwrap();
        runner.cancel(id).unwrap();
        assert!(matches!(
            store.load(id).unwrap().status,
            RunStatus::Cancelled
        ));
        assert!(matches!(
            runner.cancel(id),
            Err(MetaSkillError::Terminal(..))
        ));
    }

    #[test]
    fn dag_run_respects_dep_order() {
        let (_tmp, store) = store();
        let mock = MockSkillExecutor::new();
        mock.on("echo", &serde_json::Map::new(), Ok(serde_json::json!("x")));
        mock.on(
            "verify",
            &serde_json::Map::new(),
            Ok(serde_json::json!("ok")),
        );
        let runner = MetaSkillRunner::with_executor(store.clone(), Arc::new(mock));
        let id = runner
            .start(spec_dag_two_steps(), serde_json::Map::new())
            .unwrap();
        let outcome = runner.run_to_completion(id).unwrap();
        assert_eq!(outcome, StepOutcome::Completed);
        let run = store.load(id).unwrap();
        assert_eq!(run.steps[0].status, MetaSkillStepStatus::Ok);
        assert_eq!(run.steps[1].status, MetaSkillStepStatus::Ok);
    }

    #[test]
    fn cycle_detection_fails_at_start() {
        let (_tmp, store) = store();
        let runner = MetaSkillRunner::new(store);
        let mut spec = spec_dag_two_steps();
        spec.steps[0].depends_on = vec!["step-b".into()];
        let err = runner.start(spec, serde_json::Map::new()).unwrap_err();
        assert!(matches!(err, MetaSkillError::DependencyCycle(..)));
    }

    #[test]
    fn pause_for_clarify_sets_awaiting_user() {
        let (_tmp, store) = store();
        let runner = MetaSkillRunner::new(store.clone());
        let id = runner
            .start(spec_two_steps(), serde_json::Map::new())
            .unwrap();
        runner
            .pause_for_clarify(id, "Which branch?".into())
            .unwrap();
        let run = store.load(id).unwrap();
        assert_eq!(run.status, RunStatus::AwaitingUser);
        assert_eq!(run.clarify_prompt.as_deref(), Some("Which branch?"));
    }

    #[test]
    fn clarify_output_pauses_run_and_resume_continues() {
        let (_tmp, store) = store();
        let mock = MockSkillExecutor::new();
        mock.on(
            "echo",
            &serde_json::Map::new(),
            Ok(serde_json::json!({"clarify": "Which env?"})),
        );
        mock.on(
            "verify",
            &serde_json::Map::new(),
            Ok(serde_json::json!("ok")),
        );
        let runner = MetaSkillRunner::with_executor(store.clone(), Arc::new(mock));
        let id = runner
            .start_triggered(spec_two_steps(), serde_json::Map::new(), "auto_trigger")
            .unwrap();
        let outcome = runner.run_to_completion(id).unwrap();
        assert_eq!(outcome, StepOutcome::Paused);
        let run = store.load(id).unwrap();
        assert_eq!(run.status, RunStatus::AwaitingUser);
        assert_eq!(run.clarify_prompt.as_deref(), Some("Which env?"));
        assert_eq!(run.steps[0].status, MetaSkillStepStatus::Ok);
        assert_eq!(run.steps[1].status, MetaSkillStepStatus::Pending);

        runner
            .resume_from_clarify_with(id, Some("prod"))
            .unwrap();
        let run = store.load(id).unwrap();
        assert_eq!(
            run.state.get("user_clarification"),
            Some(&serde_json::json!("prod"))
        );
        let outcome = runner.run_to_completion(id).unwrap();
        assert_eq!(outcome, StepOutcome::Completed);
        let run = store.load(id).unwrap();
        assert_eq!(run.status, RunStatus::Completed);
        assert!(run.clarify_prompt.is_none());
    }

    #[test]
    fn when_false_skips_but_satisfies_deps() {
        let (_tmp, store) = store();
        let mock = MockSkillExecutor::new();
        mock.on(
            "verify",
            &serde_json::Map::new(),
            Ok(serde_json::json!("ok")),
        );
        let runner = MetaSkillRunner::with_executor(store.clone(), Arc::new(mock));
        let mut spec = spec_dag_two_steps();
        spec.steps[0].when = Some("never".into());
        let id = runner.start(spec, serde_json::Map::new()).unwrap();
        let outcome = runner.run_to_completion(id).unwrap();
        assert_eq!(outcome, StepOutcome::Completed);
        let run = store.load(id).unwrap();
        assert_eq!(run.steps[0].status, MetaSkillStepStatus::Skipped);
        assert_eq!(run.steps[1].status, MetaSkillStepStatus::Ok);
    }

    #[test]
    fn route_overrides_skill_name() {
        let (_tmp, store) = store();
        let mock = MockSkillExecutor::new();
        mock.on(
            "routed",
            &serde_json::Map::new(),
            Ok(serde_json::json!("via-route")),
        );
        let runner = MetaSkillRunner::with_executor(store.clone(), Arc::new(mock));
        let mut spec = MetaSkillSpec {
            name: "route-demo".into(),
            version: "1".into(),
            description: String::new(),
            steps: vec![step("r", "default", Some("r1"), vec![], vec!["o"], vec![])],
            checkpoints: vec![],
            digest: "x".into(),
            triggers: vec![],
            max_parallelism: None,
        };
        spec.steps[0].route = vec![RouteCase {
            when: "true".into(),
            to: "routed".into(),
        }];
        // route alone triggers advanced scheduler
        let id = runner.start(spec, serde_json::Map::new()).unwrap();
        let outcome = runner.run_to_completion(id).unwrap();
        assert_eq!(outcome, StepOutcome::Completed);
        let run = store.load(id).unwrap();
        assert_eq!(run.steps[0].skill_name, "routed");
        assert_eq!(run.state.get("o"), Some(&serde_json::json!("via-route")));
    }

    #[test]
    fn on_failure_substitute_mirrors_outputs() {
        let (_tmp, store) = store();
        let mock = MockSkillExecutor::new();
        mock.on("primary", &serde_json::Map::new(), Err("boom".into()));
        mock.on(
            "backup",
            &serde_json::Map::new(),
            Ok(serde_json::json!("recovered")),
        );
        mock.on(
            "downstream",
            &serde_json::Map::new(),
            Ok(serde_json::json!("done")),
        );
        let runner = MetaSkillRunner::with_executor(store.clone(), Arc::new(mock));
        let mut spec = MetaSkillSpec {
            name: "failover".into(),
            version: "1".into(),
            description: String::new(),
            steps: vec![
                step(
                    "primary",
                    "primary",
                    Some("p"),
                    vec![],
                    vec!["out"],
                    vec![],
                ),
                step("backup", "backup", Some("b"), vec![], vec![], vec![]),
                step(
                    "next",
                    "downstream",
                    Some("n"),
                    vec!["p"],
                    vec![],
                    vec!["out"],
                ),
            ],
            checkpoints: vec![],
            digest: "x".into(),
            triggers: vec![],
            max_parallelism: None,
        };
        spec.steps[0].on_failure = Some("b".into());
        let id = runner.start(spec, serde_json::Map::new()).unwrap();
        let outcome = runner.run_to_completion(id).unwrap();
        assert_eq!(outcome, StepOutcome::Completed);
        let run = store.load(id).unwrap();
        assert_eq!(run.steps[0].status, MetaSkillStepStatus::Substituted);
        assert_eq!(run.steps[1].status, MetaSkillStepStatus::Ok);
        assert_eq!(run.state.get("out"), Some(&serde_json::json!("recovered")));
        assert_eq!(run.steps[2].status, MetaSkillStepStatus::Ok);
    }

    #[test]
    fn parallel_ready_steps_overlap() {
        let (_tmp, store) = store();
        let inflight = Arc::new(AtomicUsize::new(0));
        let max_inflight = Arc::new(AtomicUsize::new(0));
        #[derive(Clone)]
        struct SlowMock {
            inflight: Arc<AtomicUsize>,
            max_inflight: Arc<AtomicUsize>,
        }
        impl SkillExecutor for SlowMock {
            fn execute(
                &self,
                _skill_name: &str,
                _inputs: &serde_json::Map<String, serde_json::Value>,
                _timeout_secs: u32,
            ) -> Result<serde_json::Value, String> {
                let cur = self.inflight.fetch_add(1, Ordering::SeqCst) + 1;
                self.max_inflight.fetch_max(cur, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(80));
                self.inflight.fetch_sub(1, Ordering::SeqCst);
                Ok(serde_json::json!("ok"))
            }
        }
        let runner = MetaSkillRunner::with_executor(
            store.clone(),
            Arc::new(SlowMock {
                inflight: Arc::clone(&inflight),
                max_inflight: Arc::clone(&max_inflight),
            }),
        );
        let spec = MetaSkillSpec {
            name: "par".into(),
            version: "1".into(),
            description: String::new(),
            steps: vec![
                step("a", "a", Some("a"), vec![], vec![], vec![]),
                step("b", "b", Some("b"), vec![], vec![], vec![]),
            ],
            checkpoints: vec![],
            digest: "x".into(),
            triggers: vec![],
            max_parallelism: Some(4),
        };
        let id = runner.start(spec, serde_json::Map::new()).unwrap();
        let outcome = runner.run_to_completion(id).unwrap();
        assert_eq!(outcome, StepOutcome::Completed);
        assert!(
            max_inflight.load(Ordering::SeqCst) >= 2,
            "expected overlapping execution, max_inflight={}",
            max_inflight.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn invalid_on_failure_rejected_at_start() {
        let (_tmp, store) = store();
        let runner = MetaSkillRunner::new(store);
        let mut spec = MetaSkillSpec {
            name: "bad".into(),
            version: "1".into(),
            description: String::new(),
            steps: vec![step("a", "a", Some("a"), vec![], vec![], vec![])],
            checkpoints: vec![],
            digest: "x".into(),
            triggers: vec![],
            max_parallelism: None,
        };
        spec.steps[0].on_failure = Some("a".into());
        let err = runner.start(spec, serde_json::Map::new()).unwrap_err();
        assert!(matches!(err, MetaSkillError::InvalidSpec(_)));
    }
}
