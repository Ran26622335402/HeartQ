use thiserror::Error;
use std::io;

/// Error type for memory operations
#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("Failed to read memory from storage: {0}")]
    ReadError(String),

    #[error("Failed to write memory to storage: {0}")]
    WriteError(String),

    #[error("Storage path does not exist: {0}")]
    PathNotFound(String),
    
    #[error("Failed to create storage directory: {0}")]
    DirectoryCreationFailed(String),
    
    #[error("Search failed: {0}")]
    SearchError(String),
    
    #[error("Index error: {0}")]
    IndexError(String),
    
    #[error("Backend error: {0}")]
    Backend(String),
    
    #[error("Backend not available")]
    BackendNotAvailable,
    
    #[error("Configuration error: {0}")]
    Config(String),
    
    #[error("Invalid configuration value for {field}: {message}")]
    InvalidConfig { field: String, message: String },
    
    #[error("Operation timed out after {0} seconds")]
    Timeout(u64),
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("Memory error: {0}")]
    Other(String),
}

impl MemoryError {
    pub fn is_transient(&self) -> bool {
        matches!(self, MemoryError::Timeout(_) | MemoryError::WriteError(_) | MemoryError::ReadError(_))
    }
    
    pub fn user_message(&self) -> String {
        match self {
            MemoryError::PathNotFound(p) => format!("Memory file not found: {}", p),
            MemoryError::BackendNotAvailable => "Memory system not configured".into(),
            MemoryError::Timeout(s) => format!("Memory operation timed out after {}s", s),
            MemoryError::PermissionDenied(p) => format!("Permission denied: {}", p),
            _ => self.to_string(),
        }
    }
}

impl From<io::Error> for MemoryError {
    fn from(err: io::Error) -> Self {
        // Without additional context we conservatively classify as a read error.
        MemoryError::ReadError(err.to_string())
    }
}
