//! Auto-proposal + accept/reject closed loop for meta-skills.
//!
//! Dream co-occurrence sketches land under `${HEARTQ_HOME}/proposals/`.
//! `/proposals accept <id>` promotes them to `${HEARTQ_HOME}/meta_skills/<name>/spec.json`.

use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::meta_skill::model::{MetaSkillSpec, MetaSkillStep};
use crate::meta_skill::store::{MetaSkillStore, StoreError};

/// Origin of an auto-generated proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalSource {
    Dream,
    DecisionLog,
}

/// Lifecycle status of a proposal on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    #[default]
    Pending,
    Accepted,
    Rejected,
}

/// A meta-skill proposal stored as JSON under `${HEARTQ_HOME}/proposals/`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub title: String,
    pub steps_sketch: Vec<String>,
    pub source: ProposalSource,
    /// Executable steps used on accept (preferred over sketch parsing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<MetaSkillStep>>,
    /// Target meta-skill name after accept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub status: ProposalStatus,
}

/// Errors from the proposal closed loop.
#[derive(Debug, thiserror::Error)]
pub enum ProposalError {
    #[error("proposal I/O: {0}")]
    Io(#[from] io::Error),
    #[error("proposal not found: {0}")]
    NotFound(String),
    #[error("proposal `{0}` is not pending (status={1:?})")]
    NotPending(String, ProposalStatus),
    #[error("meta-skill `{0}` already exists")]
    SpecExists(String),
    #[error("cannot build steps from proposal `{0}`")]
    NoSteps(String),
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// If at least two distinct skill names co-occur, emit a proposal sketch
/// plus executable linear steps.
pub fn propose_from_cooccurrence(skill_names: &[String]) -> Option<Proposal> {
    let mut unique: Vec<String> = skill_names
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    unique.sort();
    unique.dedup();
    if unique.len() < 2 {
        return None;
    }

    let title = format!("Combine {}", unique.join(" + "));
    let steps_sketch: Vec<String> = unique
        .iter()
        .enumerate()
        .map(|(i, name)| format!("Step {}: invoke `{name}`", i + 1))
        .collect();
    let steps = Some(linear_steps_from_skills(&unique));
    let name = Some(slugify_name(&title));

    Some(Proposal {
        id: Uuid::new_v4().to_string(),
        title,
        steps_sketch,
        source: ProposalSource::Dream,
        steps,
        name,
        triggers: unique.clone(),
        status: ProposalStatus::Pending,
    })
}

fn linear_steps_from_skills(skills: &[String]) -> Vec<MetaSkillStep> {
    let mut steps = Vec::new();
    let mut prev_id: Option<String> = None;
    for (i, skill) in skills.iter().enumerate() {
        let id = format!("step-{i:02}");
        let depends_on = prev_id.clone().into_iter().collect();
        steps.push(MetaSkillStep {
            label: format!("invoke {skill}"),
            skill_name: skill.clone(),
            inputs: serde_json::Map::new(),
            outputs: vec![],
            requires: vec![],
            timeout_secs: None,
            id: Some(id.clone()),
            depends_on,
            when: None,
            route: vec![],
            on_failure: None,
        });
        prev_id = Some(id);
    }
    steps
}

/// Parse ``invoke `name` `` sketches into linear steps (legacy proposals).
fn steps_from_sketch(sketch: &[String]) -> Option<Vec<MetaSkillStep>> {
    let mut skills = Vec::new();
    for line in sketch {
        if let Some(start) = line.find('`') {
            if let Some(end) = line[start + 1..].find('`') {
                let name = line[start + 1..start + 1 + end].trim();
                if !name.is_empty() {
                    skills.push(name.to_string());
                }
            }
        }
    }
    if skills.is_empty() {
        None
    } else {
        Some(linear_steps_from_skills(&skills))
    }
}

fn slugify_name(s: &str) -> String {
    let slug: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = slug.trim_matches('-').to_string();
    if trimmed.is_empty() {
        format!("meta-{}", &Uuid::new_v4().to_string()[..8])
    } else {
        trimmed.chars().take(64).collect()
    }
}

fn digest_for_spec(spec: &MetaSkillSpec) -> String {
    let bytes = serde_json::to_vec(spec).unwrap_or_default();
    blake3::hash(&bytes).to_hex().to_string()
}

/// Default proposals directory: `${HEARTQ_HOME}/proposals/`.
pub fn default_proposals_dir() -> PathBuf {
    let home = std::env::var("HEARTQ_HOME")
        .or_else(|_| std::env::var("GROK_HOME"))
        .unwrap_or_else(|_| {
            let h = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            format!("{h}/.grok")
        });
    PathBuf::from(home).join("proposals")
}

fn proposal_path(id: &str) -> PathBuf {
    default_proposals_dir().join(format!("{id}.json"))
}

/// Persist a proposal as `<id>.json` under the proposals directory.
pub fn save_proposal(proposal: &Proposal) -> io::Result<PathBuf> {
    let dir = default_proposals_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", proposal.id));
    let json = serde_json::to_string_pretty(proposal).map_err(io::Error::other)?;
    fs::write(&path, json)?;
    Ok(path)
}

/// Load all proposals from disk (best-effort; skips corrupt files).
pub fn list_proposals() -> io::Result<Vec<Proposal>> {
    let dir = default_proposals_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(p) = serde_json::from_slice::<Proposal>(&bytes) {
                out.push(p);
            }
        }
    }
    out.sort_by(|a, b| a.title.cmp(&b.title));
    Ok(out)
}

/// Load one proposal by id.
pub fn show_proposal(id: &str) -> Result<Proposal, ProposalError> {
    let path = proposal_path(id);
    let bytes = fs::read(&path).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            ProposalError::NotFound(id.to_string())
        } else {
            ProposalError::Io(e)
        }
    })?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Promote a pending proposal to a meta-skill spec, then delete the proposal file.
pub fn accept_proposal(id: &str) -> Result<MetaSkillSpec, ProposalError> {
    let mut proposal = show_proposal(id)?;
    if !matches!(proposal.status, ProposalStatus::Pending) {
        return Err(ProposalError::NotPending(id.to_string(), proposal.status));
    }

    let name = proposal
        .name
        .clone()
        .unwrap_or_else(|| slugify_name(&proposal.title));
    if MetaSkillStore::spec_exists(&name) {
        return Err(ProposalError::SpecExists(name));
    }

    let steps = proposal
        .steps
        .clone()
        .or_else(|| steps_from_sketch(&proposal.steps_sketch))
        .ok_or_else(|| ProposalError::NoSteps(id.to_string()))?;

    let mut spec = MetaSkillSpec {
        name: name.clone(),
        version: "1".into(),
        description: proposal.title.clone(),
        steps,
        checkpoints: vec![],
        digest: String::new(),
        triggers: proposal.triggers.clone(),
        max_parallelism: None,
    };
    spec.digest = digest_for_spec(&spec);

    MetaSkillStore::save_spec(&spec)?;

    // Remove proposal file (OpenSquilla moves; we delete after successful promote).
    let _ = fs::remove_file(proposal_path(id));
    proposal.status = ProposalStatus::Accepted;
    Ok(spec)
}

/// Reject (delete) a pending proposal.
pub fn reject_proposal(id: &str) -> Result<(), ProposalError> {
    let proposal = show_proposal(id)?;
    if !matches!(proposal.status, ProposalStatus::Pending) {
        return Err(ProposalError::NotPending(id.to_string(), proposal.status));
    }
    fs::remove_file(proposal_path(id)).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            ProposalError::NotFound(id.to_string())
        } else {
            ProposalError::Io(e)
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn cooccurrence_requires_two_skills() {
        assert!(propose_from_cooccurrence(&["a".into()]).is_none());
        let p = propose_from_cooccurrence(&["a".into(), "b".into(), "a".into()]).unwrap();
        assert_eq!(p.steps_sketch.len(), 2);
        assert!(p.steps.as_ref().unwrap().len() >= 2);
        assert!(p.name.is_some());
        assert_eq!(p.status, ProposalStatus::Pending);
    }

    #[test]
    fn save_and_list_round_trip() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("HEARTQ_HOME", tmp.path());
        }
        let proposal = propose_from_cooccurrence(&["lint".into(), "test".into()]).unwrap();
        let saved = save_proposal(&proposal).unwrap();
        assert!(saved.exists());
        let listed = list_proposals().unwrap();
        assert!(
            listed.iter().any(|p| p.id == proposal.id),
            "saved proposal missing from list: {:?}",
            listed.iter().map(|p| p.id.clone()).collect::<Vec<_>>()
        );
        let _ = std::fs::remove_file(&saved);
        unsafe {
            std::env::remove_var("HEARTQ_HOME");
        }
    }

    #[test]
    fn accept_promotes_to_meta_skill_and_reject_removes() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("HEARTQ_HOME", tmp.path());
        }
        let proposal = propose_from_cooccurrence(&["lint".into(), "test".into()]).unwrap();
        let id = proposal.id.clone();
        save_proposal(&proposal).unwrap();

        let shown = show_proposal(&id).unwrap();
        assert_eq!(shown.title, proposal.title);

        let spec = accept_proposal(&id).unwrap();
        assert_eq!(spec.steps.len(), 2);
        assert!(MetaSkillStore::spec_exists(&spec.name));
        assert!(matches!(
            show_proposal(&id),
            Err(ProposalError::NotFound(_))
        ));

        let p2 = propose_from_cooccurrence(&["review".into(), "commit".into()]).unwrap();
        let id2 = p2.id.clone();
        save_proposal(&p2).unwrap();
        reject_proposal(&id2).unwrap();
        assert!(list_proposals().unwrap().iter().all(|p| p.id != id2));

        unsafe {
            std::env::remove_var("HEARTQ_HOME");
        }
    }

    #[test]
    fn accept_legacy_sketch_only_proposal() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("HEARTQ_HOME", tmp.path());
        }
        let proposal = Proposal {
            id: Uuid::new_v4().to_string(),
            title: "Legacy Combine".into(),
            steps_sketch: vec![
                "Step 1: invoke `alpha`".into(),
                "Step 2: invoke `beta`".into(),
            ],
            source: ProposalSource::Dream,
            steps: None,
            name: Some("legacy-combine".into()),
            triggers: vec![],
            status: ProposalStatus::Pending,
        };
        save_proposal(&proposal).unwrap();
        let spec = accept_proposal(&proposal.id).unwrap();
        assert_eq!(spec.name, "legacy-combine");
        assert_eq!(spec.steps.len(), 2);
        assert_eq!(spec.steps[0].skill_name, "alpha");
        unsafe {
            std::env::remove_var("HEARTQ_HOME");
        }
    }
}
