//! Security checks for `@`-reference path expansion.

use std::path::{Component, Path, PathBuf};

use thiserror::Error;

/// Path fragments that must never be expanded, even when nested inside
/// otherwise innocuous-looking paths.
pub const BLOCKED_PATTERNS: &[&str] = &[
    "..",
    "/etc/passwd",
    "/etc/shadow",
    "/etc/sudoers",
    "/proc/",
    "/sys/",
    "/dev/",
    "~/.ssh",
    ".ssh/id_",
    ".env",
    ".git/config",
    "id_rsa",
    "id_ed25519",
];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SecurityError {
    #[error("blocked path pattern `{0}`")]
    BlockedPattern(&'static str),
    #[error("path escapes allowed root")]
    EscapesRoot,
    #[error("absolute path outside allowed root")]
    OutsideRoot,
}

/// Return the first blocked pattern matched in `path`, if any.
pub fn check_blocked_path(path: &str) -> Option<&'static str> {
    let normalized = path.replace('\\', "/").to_lowercase();
    BLOCKED_PATTERNS
        .iter()
        .find(|pat| normalized.contains(**pat))
        .copied()
}

/// Resolve `raw` relative to `cwd`, then verify the result stays inside
/// `allowed_root` and does not match [`BLOCKED_PATTERNS`].
pub fn resolve_under_root(raw: &str, cwd: &Path, allowed_root: &Path) -> Result<PathBuf, SecurityError> {
    if let Some(pat) = check_blocked_path(raw) {
        return Err(SecurityError::BlockedPattern(pat));
    }

    let candidate = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        cwd.join(raw)
    };

    let canonical = dunce::canonicalize(&candidate).unwrap_or(candidate);
    if !is_path_allowed(&canonical, allowed_root) {
        return Err(SecurityError::EscapesRoot);
    }
    Ok(canonical)
}

/// Return `true` when `path` is strictly inside or equal to `allowed_root`.
pub fn is_path_allowed(path: &Path, allowed_root: &Path) -> bool {
    if path
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return false;
    }

    let root = dunce::canonicalize(allowed_root).unwrap_or_else(|_| allowed_root.to_path_buf());
    let resolved = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    if resolved == root {
        return true;
    }
    resolved.starts_with(&root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn blocks_traversal_patterns() {
        assert_eq!(check_blocked_path("../etc/passwd"), Some(".."));
        assert_eq!(check_blocked_path("/etc/passwd"), Some("/etc/passwd"));
        assert!(check_blocked_path("src/main.rs").is_none());
    }

    #[test]
    fn resolve_stays_under_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("workspace");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();

        let resolved = resolve_under_root("src/main.rs", &root, &root).unwrap();
        assert!(resolved.ends_with("main.rs"));

        let err = resolve_under_root("../outside", &root.join("src"), &root);
        assert!(matches!(err, Err(SecurityError::BlockedPattern(".."))));
    }
}
