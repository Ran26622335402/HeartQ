//! LLM-assisted curator consolidation hook.
//!
//! v1 exposes a prompt builder and an async wrapper the host can plug
//! into its existing summarization core. The deterministic transitions
//! in [`super::transitions`] still run separately.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::transitions::SkillUsage;

/// One skill eligible for LLM-driven umbrella consolidation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsolidationCandidate {
    pub name: String,
    pub category: String,
    pub description: String,
    pub use_count: u64,
    pub status: String,
    pub related: Vec<String>,
}

/// Build the consolidation prompt for the host LLM to execute.
pub fn plan_llm_curator_prompt(candidates: &[ConsolidationCandidate]) -> String {
    let mut lines = vec![
        "You are the HeartQ skill curator.".into(),
        "Review the candidate skills below and propose umbrella consolidations.".into(),
        "Return markdown with sections: Summary, Merge proposals, Archive suggestions, Notes.".into(),
        "For Merge proposals, output one bullet per merge using: `- merge: <from1>,<from2> into <target>`.".into(),
        "For Archive suggestions, output one bullet per skill using: `- archive: <skill>`.".into(),
        String::new(),
        "## Candidates".into(),
    ];

    for c in candidates {
        lines.push(format!("### {} ({})", c.name, c.category));
        lines.push(format!("- status: {}", c.status));
        lines.push(format!("- use_count: {}", c.use_count));
        lines.push(format!("- description: {}", c.description));
        if !c.related.is_empty() {
            lines.push(format!("- related: {}", c.related.join(", ")));
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

/// Run the LLM consolidation pass using a caller-provided summarizer.
pub async fn run_llm_curator_review<F, Fut>(
    candidates: &[ConsolidationCandidate],
    summarize: F,
) -> Result<String, String>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    if candidates.is_empty() {
        return Ok("No consolidation candidates.".into());
    }
    let prompt = plan_llm_curator_prompt(candidates);
    summarize(prompt).await
}

/// Walk `skills_root` and collect inputs for [`run_llm_curator_review`].
pub fn collect_consolidation_candidates(skills_root: &Path) -> Vec<ConsolidationCandidate> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(skills_root) else {
        return out;
    };

    for entry in entries.flatten() {
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }

        // Layout A (扁平): `skills_root/<skill>/SKILL.md`
        let direct_skill_md = entry_path.join("SKILL.md");
        if direct_skill_md.exists() {
            let name = entry
                .file_name()
                .to_str()
                .unwrap_or("unknown")
                .to_string();
            let description = read_description(&direct_skill_md);
            let usage = read_usage_sidecar(&entry_path.join(".usage.json"));
            let status = if entry_path.join(".stale").exists() {
                "stale"
            } else {
                "active"
            }
            .to_string();
            out.push(ConsolidationCandidate {
                name,
                category: "uncategorized".into(),
                description,
                use_count: usage.use_count,
                status,
                related: Vec::new(),
            });
            continue;
        }

        // Layout B (分层): `skills_root/<category>/<skill>/SKILL.md`
        let category_name = entry
            .file_name()
            .to_str()
            .unwrap_or("uncategorized")
            .to_string();

        let Ok(skills) = std::fs::read_dir(&entry_path) else {
            continue;
        };
        for skill in skills.flatten() {
            let skill_path = skill.path();
            if !skill_path.is_dir() {
                continue;
            }
            let skill_md = skill_path.join("SKILL.md");
            if !skill_md.exists() {
                continue;
            }
            let name = skill
                .file_name()
                .to_str()
                .unwrap_or("unknown")
                .to_string();
            let description = read_description(&skill_md);
            let usage = read_usage_sidecar(&skill_path.join(".usage.json"));
            let status = if skill_path.join(".stale").exists() {
                "stale"
            } else {
                "active"
            }
            .to_string();
            out.push(ConsolidationCandidate {
                name,
                category: category_name.clone(),
                description,
                use_count: usage.use_count,
                status,
                related: Vec::new(),
            });
        }
    }

    out
}

fn read_description(skill_md: &Path) -> String {
    let Ok(body) = std::fs::read_to_string(skill_md) else {
        return String::new();
    };
    parse_frontmatter_field(&body, "description").unwrap_or_default()
}

fn parse_frontmatter_field(body: &str, field: &str) -> Option<String> {
    if !body.starts_with("---") {
        return None;
    }
    let after = body.strip_prefix("---")?;
    let yaml = after.split("\n---").next()?;
    for line in yaml.lines() {
        let prefix = format!("{field}:");
        if let Some(rest) = line.strip_prefix(&prefix) {
            return Some(rest.trim().to_string());
        }
    }
    None
}

fn read_usage_sidecar(path: &Path) -> SkillUsage {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_lists_candidates() {
        let prompt = plan_llm_curator_prompt(&[ConsolidationCandidate {
            name: "demo".into(),
            category: "general".into(),
            description: "demo skill".into(),
            use_count: 2,
            status: "active".into(),
            related: vec!["other".into()],
        }]);
        assert!(prompt.contains("demo"));
        assert!(prompt.contains("Merge proposals"));
    }

    #[tokio::test]
    async fn run_llm_curator_review_delegates_to_summarizer() {
        let candidates = vec![ConsolidationCandidate {
            name: "x".into(),
            category: "c".into(),
            description: "d".into(),
            use_count: 0,
            status: "active".into(),
            related: vec![],
        }];
        let out = run_llm_curator_review(&candidates, |prompt| async move {
            Ok(format!("OK:{}", prompt.len()))
        })
        .await
        .unwrap();
        assert!(out.starts_with("OK:"));
    }
}
