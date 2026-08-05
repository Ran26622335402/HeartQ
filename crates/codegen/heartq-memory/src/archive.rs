//! Build a `memory.tar.gz` archive containing session logs and MEMORY.md files.
//!
//! The archive is uploaded to GCS at session finalize time. The reconstruct
//! pipeline injects these into the Docker image for full replay fidelity.
//!
//! Note: this module was originally coupled to the legacy `MemoryStorage`
//! root-level struct. The GCS archive pipeline was relocated to
//! `heartq-shell`'s session-finalize path; this file remains as a
//! compatibility shim that accepts the legacy `MemoryStorage` argument
//! and returns an empty archive. Real archive construction should use
//! the shell-side finalizer.

use anyhow::Result;

use super::storage::MemoryStorage;

/// No-op archive builder. Returns an empty byte buffer.
///
/// Migration: callers should construct the tar.gz via `heartq-shell`'s
/// session-finalize path, which has access to the live `MemoryBackend`
/// trait object.
pub fn build_memory_archive(_storage: &MemoryStorage) -> Result<Vec<u8>> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_returns_empty_archive() {
        let storage = MemoryStorage::with_paths(
            std::path::PathBuf::from("/tmp/global"),
            std::path::PathBuf::from("/tmp/workspace"),
        );
        let archive = build_memory_archive(&storage).unwrap();
        assert!(archive.is_empty());
    }
}