//! `LearningGraphBuilder` — wires skill discovery + memory storage into
//! the pure [`LearningGraph::build`] function.
//!
//! Translated from Hermes Agent's `agent/learning_graph.py`.
//! Deterministic (no LLM); produces a stable JSON-serializable graph.

use std::path::{Path, PathBuf};

use crate::learning::graph::{LearningGraph, MemoryCard, SkillCardInputs, SkillNode};

/// Where the builder should look for skills / memory. Defaults are
/// environment-driven; tests inject their own values via
/// `with_skills_dir` / `with_memory_paths`.
#[derive(Debug, Clone)]
pub struct LearningGraphInputs {
    pub skills_dir: PathBuf,
    /// Paths to memory markdown files to read as `MemoryCard`s.
    pub memory_paths: Vec<PathBuf>,
}

impl Default for LearningGraphInputs {
    fn default() -> Self {
        let home = std::env::var("HEARTQ_HOME")
            .or_else(|_| std::env::var("HERMES_HOME"))
            .unwrap_or_else(|_| {
                let h = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                format!("{h}/.heartq")
            });
        Self {
            skills_dir: PathBuf::from(home).join("skills"),
            memory_paths: vec![],
        }
    }
}

pub struct LearningGraphBuilder {
    inputs: LearningGraphInputs,
}

impl LearningGraphBuilder {
    pub fn new(inputs: LearningGraphInputs) -> Self {
        Self { inputs }
    }

    pub fn with_skills_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.inputs.skills_dir = dir.into();
        self
    }

    pub fn with_memory_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.inputs.memory_paths = paths;
        self
    }

    /// Build the graph. Reads skills from `skills_dir` (only the
    /// frontmatter, not the body — keeps the graph small) and memory
    /// cards from each path in `memory_paths`.
    ///
    /// Returns an empty graph if `skills_dir` doesn't exist; missing
    /// memory files are skipped silently.
    pub fn build(&self) -> std::io::Result<LearningGraph> {
        let skills = collect_skills(&self.inputs.skills_dir);
        let cards = collect_memory_cards(&self.inputs.memory_paths);
        Ok(LearningGraph::build(SkillCardInputs {
            skills,
            cards,
        }))
    }
}

/// Walk `dir` recursively for `SKILL.md` files; parse only the
/// frontmatter to build `SkillNode`s. Each top-level subdirectory is
/// treated as a `category`; the leaf directory containing `SKILL.md`
/// is the skill.
fn collect_skills(dir: &Path) -> Vec<SkillNode> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // The top-level name is the category. Recurse one level to find
        // any leaf directory containing SKILL.md.
        let category = entry.file_name().to_string_lossy().to_string();
        if let Some(skill_md) = find_skill_md(&path) {
            let leaf_dir = skill_md.parent().unwrap_or(&path);
            if let Some(node) = build_skill_node(&category, leaf_dir, &skill_md) {
                out.push(node);
            }
        }
    }
    out
}

/// Recursively search `dir` for a `SKILL.md`. Returns the path to the
/// file (not the directory) on success. Limits recursion to one level
/// for simplicity — Hermes' real layout is two levels
/// (`<root>/<category>/<skill>/SKILL.md`) and we mirror that.
fn find_skill_md(dir: &Path) -> Option<PathBuf> {
    let direct = dir.join("SKILL.md");
    if direct.is_file() {
        return Some(direct);
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if let Some(found) = find_skill_md(&p) {
                return Some(found);
            }
        }
    }
    None
}

fn build_skill_node(category: &str, leaf_dir: &Path, skill_md: &Path) -> Option<SkillNode> {
    let timestamp = std::fs::metadata(skill_md)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    let content = std::fs::read_to_string(skill_md).ok()?;
    let front = parse_frontmatter_only(&content);
    let name: String = front
        .as_ref()
        .and_then(|v| {
            let n = v.get("name")?;
            n.as_str().map(str::to_string)
        })
        .unwrap_or_else(|| {
            leaf_dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| category.to_string())
        });
    let related = extract_related_skills(front.as_ref());
    let state = if leaf_dir.join(".archived").exists() {
        "archived".into()
    } else {
        "active".into()
    };
    let pinned = leaf_dir.join(".pinned").exists();
    Some(SkillNode {
        name,
        category: category.to_string(),
        source: "base".into(),
        timestamp,
        use_count: 0,
        state,
        created_by: None,
        pinned,
        related,
    })
}

/// Very small frontmatter-only parser. We deliberately avoid
/// constructing the full [`SkillFrontmatter`] from `heartq-tools`
/// here to keep this crate free of that dependency; we just need the
/// `name` and `metadata.hermes.related_skills` fields.
fn parse_frontmatter_only(content: &str) -> Option<serde_yaml::Value> {
    let after_open = content.strip_prefix("---\n")?;
    let close_idx = after_open.find("\n---")?;
    serde_yaml::from_str(&after_open[..close_idx]).ok()
}

fn extract_related_skills(fm: Option<&serde_yaml::Value>) -> Vec<String> {
    let mut out = Vec::new();
    let Some(fm) = fm else { return out };
    let Some(meta) = fm.get("metadata") else { return out };
    let Some(hermes) = meta.get("hermes") else { return out };
    let Some(arr) = hermes.get("related_skills").and_then(|v| v.as_sequence()) else {
        return out;
    };
    for v in arr {
        if let Some(s) = v.as_str() {
            out.push(s.to_string());
        }
    }
    out
}

/// Read each `path` as a Markdown file and split on `## ` headings.
/// Each heading becomes a `MemoryCard` whose `title` is the heading
/// text and whose `body` is everything until the next heading.
fn collect_memory_cards(paths: &[PathBuf]) -> Vec<MemoryCard> {
    let mut out = Vec::new();
    for path in paths {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let source = "memory".to_string();
        let timestamp = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        let mut sections = content.split("\n## ").enumerate();
        // First section is the file preamble (before the first heading);
        // only emit it as a card if it has actual content.
        while let Some((i, raw)) = sections.next() {
            let (title, body) = if i == 0 {
                if let Some(idx) = raw.find('\n') {
                    let t = raw[..idx].trim();
                    if t.is_empty() {
                        continue;
                    }
                    (t.to_string(), raw[idx + 1..].trim().to_string())
                } else if raw.trim().is_empty() {
                    continue;
                } else {
                    (raw.trim().to_string(), String::new())
                }
            } else {
                if let Some(idx) = raw.find('\n') {
                    (raw[..idx].trim().to_string(), raw[idx + 1..].trim().to_string())
                } else {
                    (raw.trim().to_string(), String::new())
                }
            };
            if title.is_empty() && body.is_empty() {
                continue;
            }
            out.push(MemoryCard {
                source: source.clone(),
                timestamp,
                title,
                body,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn builder_with_empty_inputs_produces_empty_graph() {
        let tmp = tempfile::tempdir().unwrap();
        let b = LearningGraphBuilder::new(LearningGraphInputs {
            skills_dir: tmp.path().to_path_buf(),
            memory_paths: vec![],
        });
        let g = b.build().unwrap();
        assert_eq!(g.stats.skill_count, 0);
        assert_eq!(g.stats.memory_card_count, 0);
    }

    #[test]
    fn builder_reads_skills_and_memory() {
        let tmp = tempfile::tempdir().unwrap();
        // One skill at `<root>/<category>/<skill>/SKILL.md`. The walker
        // treats the top-level entries of `skills_dir` as categories.
        let category = tmp.path().join("cat1");
        let skill_dir = category.join("foo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: foo\ndescription: x\nmetadata:\n  hermes:\n    related_skills: [bar]\n---\nbody\n",
        )
        .unwrap();
        // Memory: one card.
        let mem_path = tmp.path().join("MEMORY.md");
        fs::write(&mem_path, "# preamble\n\nintro text\n\n## First note\n\nbody of first\n").unwrap();

        let b = LearningGraphBuilder::new(LearningGraphInputs {
            skills_dir: tmp.path().to_path_buf(),
            memory_paths: vec![mem_path],
        });
        let g = b.build().unwrap();
        assert_eq!(g.stats.skill_count, 1, "expected one skill");
        assert!(g.stats.memory_card_count >= 1);
        // The skill's related_skills edge points to `bar`, which doesn't
        // exist locally, so the edge is dropped.
    }
}