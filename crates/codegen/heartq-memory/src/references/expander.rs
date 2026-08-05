//! Expand `@file:` / `@folder:` references into inline context text.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::utils::estimate_tokens;

use super::security::{SecurityError, resolve_under_root};

/// Remaining byte/token budget while expanding references.
#[derive(Debug, Clone, Copy)]
pub struct ExpandBudget {
    pub max_bytes: usize,
    pub max_tokens: u64,
}

impl ExpandBudget {
    pub fn new(max_bytes: usize, max_tokens: u64) -> Self {
        Self {
            max_bytes,
            max_tokens,
        }
    }

    fn consume(&mut self, chunk: &str) -> bool {
        if chunk.len() > self.max_bytes {
            return false;
        }
        let tokens = estimate_tokens(chunk);
        if tokens > self.max_tokens {
            return false;
        }
        self.max_bytes -= chunk.len();
        self.max_tokens -= tokens;
        true
    }
}

#[derive(Debug, Error)]
pub enum ExpandError {
    #[error("security: {0}")]
    Security(#[from] SecurityError),
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("budget exhausted")]
    BudgetExhausted,
    #[error("not a file: {0}")]
    NotAFile(PathBuf),
    #[error("not a directory: {0}")]
    NotADirectory(PathBuf),
}

const MAX_SINGLE_FILE_BYTES: usize = 256 * 1024;
const MAX_FOLDER_FILES: usize = 64;

/// Read a single file and wrap it in a fenced context block.
pub fn expand_file(
    raw_path: &str,
    cwd: &Path,
    allowed_root: &Path,
    budget: &mut ExpandBudget,
) -> Result<String, ExpandError> {
    let path = resolve_under_root(raw_path, cwd, allowed_root)?;
    if !path.is_file() {
        return Err(ExpandError::NotAFile(path));
    }
    let meta = fs::metadata(&path)?;
    if meta.len() as usize > MAX_SINGLE_FILE_BYTES {
        return Err(ExpandError::BudgetExhausted);
    }
    let body = fs::read_to_string(&path)?;
    let display = path.strip_prefix(allowed_root).unwrap_or(&path);
    let block = format!(
        "<!-- @file:{} -->\n```\n{}\n```",
        display.display(),
        body
    );
    if !budget.consume(&block) {
        return Err(ExpandError::BudgetExhausted);
    }
    Ok(block)
}

/// Walk a folder (non-recursive listing, one level of nested files) and
/// concatenate readable text files up to the budget.
pub fn expand_folder(
    raw_path: &str,
    cwd: &Path,
    allowed_root: &Path,
    budget: &mut ExpandBudget,
) -> Result<String, ExpandError> {
    let path = resolve_under_root(raw_path, cwd, allowed_root)?;
    if !path.is_dir() {
        return Err(ExpandError::NotADirectory(path));
    }

    let mut out = format!("<!-- @folder:{} -->\n", path.display());
    let mut files_seen = 0usize;

    for entry in fs::read_dir(&path)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let entry_path = entry.path();
        if file_type.is_dir() {
            for nested in fs::read_dir(&entry_path)? {
                let nested = nested?;
                if nested.file_type()?.is_file() {
                    append_file_chunk(&mut out, &nested.path(), allowed_root, budget, &mut files_seen)?;
                }
            }
        } else if file_type.is_file() {
            append_file_chunk(&mut out, &entry_path, allowed_root, budget, &mut files_seen)?;
        }
        if files_seen >= MAX_FOLDER_FILES || budget.max_bytes == 0 {
            break;
        }
    }

    if files_seen == 0 {
        out.push_str("(empty folder)\n");
    }
    Ok(out)
}

fn append_file_chunk(
    out: &mut String,
    path: &Path,
    allowed_root: &Path,
    budget: &mut ExpandBudget,
    files_seen: &mut usize,
) -> Result<(), ExpandError> {
    if *files_seen >= MAX_FOLDER_FILES {
        return Ok(());
    }
    let meta = fs::metadata(path)?;
    if !meta.is_file() || meta.len() as usize > MAX_SINGLE_FILE_BYTES {
        return Ok(());
    }
    let body = fs::read_to_string(path).unwrap_or_default();
    let display = path.strip_prefix(allowed_root).unwrap_or(path);
    let chunk = format!("### {}\n```\n{}\n```\n", display.display(), body);
    if budget.consume(&chunk) {
        out.push_str(&chunk);
        *files_seen += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn expand_file_respects_budget() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("a.txt"), "hello").unwrap();
        let mut budget = ExpandBudget::new(10_000, 10_000);
        let out = expand_file("a.txt", root, root, &mut budget).unwrap();
        assert!(out.contains("hello"));
    }
}
