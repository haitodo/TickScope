//! MT5 auto-deployment errors.

use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DeployError {
    #[error("Failed to read file '{path}': {message}")]
    FileRead { path: PathBuf, message: String },

    #[error("Failed to write file '{path}': {message}")]
    FileWrite { path: PathBuf, message: String },

    #[error("Failed to create directory '{path}': {message}")]
    CreateDir { path: PathBuf, message: String },

    #[error("Archive operation failed: {0}")]
    Archive(String),

    #[error("MetaEditor compilation failed: {0}")]
    CompilationFailed(String),

    #[error("MetaEditor executable not found")]
    MetaEditorNotFound,

    #[error("Deployment failed: {0}")]
    Other(String),
}

impl From<DeployError> for String {
    fn from(e: DeployError) -> Self {
        e.to_string()
    }
}
