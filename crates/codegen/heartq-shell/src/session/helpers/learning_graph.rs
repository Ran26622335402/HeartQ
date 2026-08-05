//! Session-end learning graph snapshot.

use std::path::{Path, PathBuf};

use heartq_memory::learning::{LearningGraphBuilder, LearningGraphInputs};

/// Build a learning graph from skills + memory files and write
/// `~/.heartq/memory/learning_graph.json` (or `$HEARTQ_HOME/memory/...`).
pub fn write_learning_graph(
    skills_dir: &Path,
    memory_paths: Vec<PathBuf>,
) -> Result<PathBuf, String> {
    let graph = LearningGraphBuilder::new(LearningGraphInputs {
        skills_dir: skills_dir.to_path_buf(),
        memory_paths,
    })
    .build()
    .map_err(|e| format!("learning graph build failed: {e}"))?;

    let home = std::env::var("HEARTQ_HOME")
        .or_else(|_| std::env::var("HERMES_HOME"))
        .or_else(|_| std::env::var("GROK_HOME"))
        .unwrap_or_else(|_| {
            let h = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            format!("{h}/.heartq")
        });
    let out = PathBuf::from(home).join("memory").join("learning_graph.json");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(&graph)
        .map_err(|e| format!("serialize learning graph: {e}"))?;
    std::fs::write(&out, json).map_err(|e| format!("write {}: {e}", out.display()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_graph_json() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join("skills");
        std::fs::create_dir_all(&skills).unwrap();
        let skill_dir = skills.join("demo");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: d\n---\n\nbody\n",
        )
        .unwrap();
        let mem = tmp.path().join("MEMORY.md");
        std::fs::write(&mem, "# mem\n").unwrap();

        unsafe {
            std::env::set_var("HEARTQ_HOME", tmp.path());
        }
        let path = write_learning_graph(&skills, vec![mem]).unwrap();
        assert!(path.exists());
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("nodes") || raw.contains("stats"));
    }
}
