//! Replace `@`-references inline with expanded context blocks.

use std::path::Path;

use git2::{DiffOptions, Repository};

use super::context_refs::{ContextReference, parse_context_references};
use super::expander::{ExpandBudget, ExpandError, expand_file, expand_folder};
use super::security::resolve_under_root;

/// Token/byte budget for a single preprocess pass.
#[derive(Debug, Clone, Copy)]
pub struct PreprocessBudget {
    pub max_bytes: usize,
    pub max_tokens: u64,
}

impl Default for PreprocessBudget {
    fn default() -> Self {
        Self {
            max_bytes: 512 * 1024,
            max_tokens: 32_000,
        }
    }
}

/// Parse `@`-references in `text`, expand each in place, and return the
/// rewritten text plus the list of references encountered.
pub fn preprocess_context_references(
    text: &str,
    cwd: &Path,
    budget: PreprocessBudget,
) -> (String, Vec<ContextReference>) {
    let allowed_root = cwd;
    let parsed = parse_context_references(text);
    if parsed.is_empty() {
        return (text.to_string(), Vec::new());
    }

    let mut refs = Vec::with_capacity(parsed.len());
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    let mut expand_budget = ExpandBudget::new(budget.max_bytes, budget.max_tokens);

    for item in parsed {
        out.push_str(&text[cursor..item.start]);
        cursor = item.end;

        let replacement = match &item.reference {
            ContextReference::File { path } => {
                expand_file(
                    &path.to_string_lossy(),
                    cwd,
                    allowed_root,
                    &mut expand_budget,
                )
                .unwrap_or_else(|e| format!("<!-- @file expansion failed: {e} -->"))
            }
            ContextReference::Folder { path } => {
                expand_folder(
                    &path.to_string_lossy(),
                    cwd,
                    allowed_root,
                    &mut expand_budget,
                )
                .unwrap_or_else(|e| format!("<!-- @folder expansion failed: {e} -->"))
            }
            ContextReference::Diff => expand_git_diff(cwd, false)
                .unwrap_or_else(|e| format!("<!-- @diff failed: {e} -->")),
            ContextReference::Staged => expand_git_diff(cwd, true)
                .unwrap_or_else(|e| format!("<!-- @staged failed: {e} -->")),
            ContextReference::Git { n } => expand_git_log(cwd, *n)
                .unwrap_or_else(|e| format!("<!-- @git:{n} failed: {e} -->")),
            ContextReference::Url { url } => {
                if resolve_under_root(url, cwd, allowed_root).is_ok() {
                    format!("<!-- @url blocked (local path): {url} -->")
                } else {
                    format!("<!-- @url (not fetched in v1): {url} -->")
                }
            }
        };

        refs.push(item.reference);
        out.push_str(&replacement);
        out.push('\n');
    }

    out.push_str(&text[cursor..]);
    (out, refs)
}

fn expand_git_diff(cwd: &Path, staged: bool) -> Result<String, ExpandError> {
    let repo = Repository::discover(cwd).map_err(|e| ExpandError::Io(std::io::Error::other(e)))?;
    let mut opts = DiffOptions::new();
    let diff = if staged {
        let head = repo
            .head()
            .map_err(|e| ExpandError::Io(std::io::Error::other(e)))?
            .peel_to_tree()
            .map_err(|e| ExpandError::Io(std::io::Error::other(e)))?;
        repo.diff_tree_to_index(Some(&head), None, Some(&mut opts))
    } else {
        repo.diff_index_to_workdir(None, Some(&mut opts))
    }
    .map_err(|e| ExpandError::Io(std::io::Error::other(e)))?;

    let mut buf = Vec::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        buf.extend_from_slice(line.content());
        true
    })
    .map_err(|e| ExpandError::Io(std::io::Error::other(e)))?;
    let label = if staged { "@staged" } else { "@diff" };
    Ok(format!(
        "<!-- {label} -->\n```diff\n{}\n```",
        String::from_utf8_lossy(&buf)
    ))
}

fn expand_git_log(cwd: &Path, n: u32) -> Result<String, ExpandError> {
    let repo = Repository::discover(cwd).map_err(|e| ExpandError::Io(std::io::Error::other(e)))?;
    let mut revwalk = repo
        .revwalk()
        .map_err(|e| ExpandError::Io(std::io::Error::other(e)))?;
    revwalk
        .push_head()
        .map_err(|e| ExpandError::Io(std::io::Error::other(e)))?;

    let mut lines = Vec::new();
    for (idx, oid) in revwalk.take(n as usize).enumerate() {
        let oid = oid.map_err(|e| ExpandError::Io(std::io::Error::other(e)))?;
        let commit = repo
            .find_commit(oid)
            .map_err(|e| ExpandError::Io(std::io::Error::other(e)))?;
        let summary = commit.summary().unwrap_or("(no message)");
        lines.push(format!(
            "{} {} {}",
            idx + 1,
            oid,
            summary
        ));
    }
    Ok(format!(
        "<!-- @git:{n} -->\n```\n{}\n```",
        lines.join("\n")
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn preprocess_expands_file_reference() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("note.txt"), "payload").unwrap();
        let (out, refs) = preprocess_context_references(
            "before @file:note.txt after",
            tmp.path(),
            PreprocessBudget::default(),
        );
        assert_eq!(refs.len(), 1);
        assert!(out.contains("payload"));
        assert!(out.contains("after"));
    }
}
