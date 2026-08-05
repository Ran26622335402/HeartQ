//! Meta-skill persistence — one JSON file per run under
//! `${HEARTQ_HOME}/meta_runs/<run_id>.json`.
//!
//! Borrowed from opensquilla's `meta_skill_runs`/`meta_skill_run_steps`
//! SQLite tables. HeartQ uses JSON files instead of SQLite because:
//!
//! 1. We already use the JSON-file pattern for memory backend
//!    (`MemoryStorage` — see `crates/codegen/xai-grok-memory/src/storage.rs`).
//! 2. Run count is low (one per session) so a relational schema buys
//!    nothing; flat files with atomic-write (temp + rename) are enough.
//! 3. The runner reads + writes the whole file per checkpoint, which is
//!    already what the runner needs to do for the in-memory copy.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;
use uuid::Uuid;

use super::model::MetaSkillRun;
use super::model::MetaSkillSpec;

/// All the ways loading or saving a run can fail.
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("meta-skill store I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("meta-skill store JSON error at {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("meta-skill run not found: {0}")]
    NotFound(Uuid),

    #[error("meta-skill spec not found: {0}")]
    SpecNotFound(String),

    #[error("meta-skill home directory could not be determined")]
    NoHome,
}

/// Default base directory for meta-skill spec files.
pub fn default_meta_skills_dir() -> PathBuf {
    let home = std::env::var("HEARTQ_HOME")
        .or_else(|_| std::env::var("GROK_HOME"))
        .unwrap_or_else(|_| {
            let h = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            format!("{h}/.grok")
        });
    PathBuf::from(home).join("meta_skills")
}

/// Default base directory for meta-skill run files. Overridable via
/// `MetaSkillStore::new` so tests can use a tempdir.
pub fn default_meta_runs_dir() -> PathBuf {
    let home = std::env::var("HEARTQ_HOME")
        .or_else(|_| std::env::var("GROK_HOME"))
        .unwrap_or_else(|_| {
            let h = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            format!("{h}/.grok")
        });
    PathBuf::from(home).join("meta_runs")
}

/// File-backed store for `MetaSkillRun`s.
///
/// Concurrency: the runner holds the run in memory and `save`s after each
/// step transition. Cross-process safety is best-effort: two processes
/// writing the same `<run_id>.json` would race; we serialize via temp +
/// rename to keep each individual file write atomic on POSIX.
#[derive(Debug, Clone)]
pub struct MetaSkillStore {
    base_dir: PathBuf,
}

impl MetaSkillStore {
    /// Default store at `${HEARTQ_HOME}/meta_runs/`. Creates the directory
    /// on first use (not at construction — cheap to defer).
    pub fn new() -> Self {
        Self {
            base_dir: default_meta_runs_dir(),
        }
    }

    /// Custom base directory (tests).
    pub fn with_base_dir(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    /// Where this store writes runs.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Ensure the runs directory exists. Idempotent.
    pub fn ensure_dir(&self) -> Result<(), StoreError> {
        fs::create_dir_all(&self.base_dir).map_err(|e| StoreError::Io {
            path: self.base_dir.clone(),
            source: e,
        })
    }

    /// Load a meta-skill spec by name from `${HEARTQ_HOME}/meta_skills/<name>/spec.json`.
    pub fn load_spec(name: &str) -> Result<MetaSkillSpec, StoreError> {
        let path = default_meta_skills_dir().join(name).join("spec.json");
        let bytes = fs::read(&path).map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                StoreError::SpecNotFound(name.to_string())
            } else {
                StoreError::Io {
                    path: path.clone(),
                    source: e,
                }
            }
        })?;
        serde_json::from_slice(&bytes).map_err(|e| StoreError::Json {
            path,
            source: e,
        })
    }

    /// Atomically write `spec` to `${HEARTQ_HOME}/meta_skills/<name>/spec.json`.
    ///
    /// Refuses to overwrite an existing spec (returns Io AlreadyExists).
    pub fn save_spec(spec: &MetaSkillSpec) -> Result<PathBuf, StoreError> {
        let dir = default_meta_skills_dir().join(&spec.name);
        fs::create_dir_all(&dir).map_err(|e| StoreError::Io {
            path: dir.clone(),
            source: e,
        })?;
        let canonical = dir.join("spec.json");
        if canonical.exists() {
            return Err(StoreError::Io {
                path: canonical,
                source: io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("meta-skill `{}` already exists", spec.name),
                ),
            });
        }
        let tmp = dir.join(format!(".spec.{}.json.tmp", Uuid::new_v4()));
        let json = serde_json::to_vec_pretty(spec).map_err(|e| StoreError::Json {
            path: canonical.clone(),
            source: e,
        })?;
        fs::write(&tmp, &json).map_err(|e| StoreError::Io {
            path: tmp.clone(),
            source: e,
        })?;
        fs::rename(&tmp, &canonical).map_err(|e| StoreError::Io {
            path: canonical.clone(),
            source: e,
        })?;
        Ok(canonical)
    }

    /// Whether a meta-skill spec directory already exists.
    pub fn spec_exists(name: &str) -> bool {
        default_meta_skills_dir()
            .join(name)
            .join("spec.json")
            .is_file()
    }

    /// List all meta-skill specs on disk (best-effort; skips unreadable entries).
    pub fn list_specs() -> Result<Vec<MetaSkillSpec>, StoreError> {
        let dir = default_meta_skills_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut specs = Vec::new();
        for entry in fs::read_dir(&dir).map_err(|e| StoreError::Io {
            path: dir.clone(),
            source: e,
        })? {
            let entry = entry.map_err(|e| StoreError::Io {
                path: dir.clone(),
                source: e,
            })?;
            let spec_path = entry.path().join("spec.json");
            if !spec_path.is_file() {
                continue;
            }
            let bytes = fs::read(&spec_path).map_err(|e| StoreError::Io {
                path: spec_path.clone(),
                source: e,
            })?;
            if let Ok(spec) = serde_json::from_slice::<MetaSkillSpec>(&bytes) {
                specs.push(spec);
            }
        }
        specs.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(specs)
    }

    /// Save a run atomically: write to `.<run_id>.json.tmp` then rename
    /// over the canonical path. Crash-safe on POSIX; on Windows the
    /// rename-over-existing is atomic since Rust 1.5+.
    pub fn save(&self, run: &MetaSkillRun) -> Result<(), StoreError> {
        self.ensure_dir()?;
        let canonical = self.path_for(run.id);
        let tmp = self.tmp_path_for(run.id);
        let json = serde_json::to_vec_pretty(run).map_err(|e| StoreError::Json {
            path: canonical.clone(),
            source: e,
        })?;
        fs::write(&tmp, &json).map_err(|e| StoreError::Io {
            path: tmp.clone(),
            source: e,
        })?;
        fs::rename(&tmp, &canonical).map_err(|e| StoreError::Io {
            path: canonical.clone(),
            source: e,
        })
    }

    /// Load a run by id. Returns `NotFound` if the file is missing.
    pub fn load(&self, id: Uuid) -> Result<MetaSkillRun, StoreError> {
        let path = self.path_for(id);
        let bytes = fs::read(&path).map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                StoreError::NotFound(id)
            } else {
                StoreError::Io {
                    path: path.clone(),
                    source: e,
                }
            }
        })?;
        serde_json::from_slice(&bytes).map_err(|e| StoreError::Json {
            path,
            source: e,
        })
    }

    /// Delete a run by id. Missing → `NotFound` (no silent ignore; callers
    /// who want silent behavior can match on the variant).
    pub fn delete(&self, id: Uuid) -> Result<(), StoreError> {
        let path = self.path_for(id);
        fs::remove_file(&path).map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                StoreError::NotFound(id)
            } else {
                StoreError::Io {
                    path: path.clone(),
                    source: e,
                }
            }
        })
    }

    /// List every run on disk, sorted by `started_at` (newest first).
    /// Tombstones (unparseable files) are logged at warn-level and skipped.
    pub fn list(&self) -> Result<Vec<MetaSkillRun>, StoreError> {
        self.ensure_dir()?;
        let mut runs = Vec::new();
        for entry in fs::read_dir(&self.base_dir).map_err(|e| StoreError::Io {
            path: self.base_dir.clone(),
            source: e,
        })? {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(error = %e, "meta_skill: skipping unreadable dir entry");
                    continue;
                }
            };
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let bytes = match fs::read(&path) {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(path = ?path, error = %e, "meta_skill: skipping unreadable run");
                    continue;
                }
            };
            match serde_json::from_slice::<MetaSkillRun>(&bytes) {
                Ok(r) => runs.push(r),
                Err(e) => {
                    tracing::warn!(path = ?path, error = %e, "meta_skill: skipping corrupt run");
                    continue;
                }
            }
        }
        runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        Ok(runs)
    }

    /// Compute the canonical path for a run id.
    fn path_for(&self, id: Uuid) -> PathBuf {
        self.base_dir.join(format!("{id}.json"))
    }

    /// Compute the temp path used during atomic write.
    fn tmp_path_for(&self, id: Uuid) -> PathBuf {
        self.base_dir.join(format!(".{id}.json.tmp"))
    }
}

impl Default for MetaSkillStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta_skill::model::*;
    use std::collections::HashMap;
    use std::time::SystemTime;

    fn make_run() -> MetaSkillRun {
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
            steps: vec![],
            state: HashMap::new(),
            status: RunStatus::Running,
            inputs: serde_json::Map::new(),
            final_text: None,
            error: None,
            clarify_prompt: None,
        }
    }

    #[test]
    fn save_load_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MetaSkillStore::with_base_dir(tmp.path());
        let run = make_run();
        let id = run.id;
        store.save(&run).unwrap();
        let loaded = store.load(id).unwrap();
        assert_eq!(loaded.id, id);
        assert_eq!(loaded.meta_skill_name, "test");
    }

    #[test]
    fn load_missing_returns_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MetaSkillStore::with_base_dir(tmp.path());
        let result = store.load(Uuid::new_v4());
        assert!(matches!(result, Err(StoreError::NotFound(_))));
    }

    #[test]
    fn list_returns_all_runs_newest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MetaSkillStore::with_base_dir(tmp.path());
        let older = make_run();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let newer = make_run();
        store.save(&older).unwrap();
        store.save(&newer).unwrap();
        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, newer.id);
        assert_eq!(listed[1].id, older.id);
    }

    #[test]
    fn delete_removes_run() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MetaSkillStore::with_base_dir(tmp.path());
        let run = make_run();
        let id = run.id;
        store.save(&run).unwrap();
        store.delete(id).unwrap();
        assert!(matches!(store.load(id), Err(StoreError::NotFound(_))));
    }
}

