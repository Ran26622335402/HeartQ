//! `@`-reference parsing and expansion for agent context injection.
//!
//! Parses inline references such as `@file:src/main.rs`, `@folder:docs/`,
//! `@diff`, `@staged`, `@git:3`, and `@url:https://…` from user/agent
//! text, expands them into readable context blocks, and enforces path
//! security constraints before touching the filesystem.

mod context_refs;
mod expander;
mod preprocess;
mod security;

pub use context_refs::{ContextReference, ParsedReference, parse_context_references};
pub use expander::{ExpandBudget, ExpandError, expand_file, expand_folder};
pub use preprocess::{PreprocessBudget, preprocess_context_references};
pub use security::{
    BLOCKED_PATTERNS, SecurityError, check_blocked_path, is_path_allowed, resolve_under_root,
};
