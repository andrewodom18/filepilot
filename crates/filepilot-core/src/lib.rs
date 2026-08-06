//! Reusable, terminal-independent functionality for FilePilot.

mod metadata;
mod operations;
mod reports;
mod scan;
mod storage;

pub use metadata::{clean_images, CleanMetadataResult, CleanedImage};
pub use operations::{
    apply_operation, build_organize_plan, build_rename_plan, undo_operation, OperationAction,
    OperationKind, OperationPlan, OperationRecord, OperationResult, OrganizeBy, RenameOptions,
};
pub use reports::{duplicate_files, large_files, DuplicateGroup, LargeFileEntry};
pub use scan::{scan, FileRecord, ScanOptions, ScanResult, ScanWarning};

use std::{io, path::PathBuf};

use thiserror::Error;

pub type Result<T> = std::result::Result<T, FilePilotError>;

#[derive(Debug, Error)]
pub enum FilePilotError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("invalid path: {0}")]
    InvalidPath(PathBuf),

    #[error("invalid regular expression: {0}")]
    InvalidRegex(#[from] regex::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("operation conflict: {0}")]
    Conflict(String),

    #[error("operation not found: {0}")]
    OperationNotFound(String),

    #[error("operation cannot be undone: {0}")]
    UndoRefused(String),
}
