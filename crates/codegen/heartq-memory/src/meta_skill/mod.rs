//! Meta-skill framework — P0.1 of the opensquilla → HeartQ import plan.
//!
//! A **meta-skill** packages a multi-step workflow as an inspectable, resumable,
//! replayable run. Each run goes through these phases:
//!
//! ```text
//!   Draft  →  Run  →  Step i (skill call)  →  Step i+1  →  ...  →  Done
//!                  ↓ pause/resume        ↓ fail/skip
//!                  Paused               Failed step (recorded)
//! ```
//!
//! Borrowed from `opensquilla docs/features/meta-skills.md` and the
//! V010/V011/V012/V013/V014 migrations. HeartQ-specific differences:
//!
//! - Persisted as JSON files under `${HEARTQ_HOME}/meta_runs/<run_id>.json`
//!   (matches our memory backend layout; opensquilla uses SQLite).
//! - Steps invoke ordinary HeartQ skills (`skill_manage`) — there is no
//!   separate "child skill" concept.
//! - Checkpoints are pause-points that auto-flush when `CompressionState`
//!   triggers (we already have the anti-thrash gate from Phase B).

pub mod executor;
pub mod model;
pub mod runner;
pub mod store;
pub mod templating;
pub mod trigger;

pub use executor::SkillManagerExecutor;
pub use model::{
    validate_dag_spec, MetaSkill, MetaSkillRun, MetaSkillSpec, MetaSkillStep,
    MetaSkillStepStatus, RouteCase, RunStatus, StepOutput, DEFAULT_MAX_PARALLELISM,
};
pub use runner::{MetaSkillError, MetaSkillRunner, RunOutcome, SkillExecutor, StepOutcome};
pub use store::{MetaSkillStore, StoreError, default_meta_skills_dir};
pub use trigger::{TriggerMatch, match_triggers};