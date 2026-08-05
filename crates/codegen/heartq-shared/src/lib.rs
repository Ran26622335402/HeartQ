//! Shared utilities used by both `heartq-shell` and its downstream clients
//! (e.g. `heartq-pager-render`). This crate sits upstream of `heartq-shell`
//! so it must never depend on it.

pub mod clipboard;
pub mod placeholder_images;
pub mod session;
pub mod stderr;
pub mod ui_config;
