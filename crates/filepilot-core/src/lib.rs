//! Reusable, terminal-independent functionality for FilePilot.

mod metadata;
mod operations;
mod progress;
mod reports;
mod scan;
mod storage;

pub use metadata::{clean_images, clean_images_with_context, CleanMetadataResult, CleanedImage};
pub use operations::{
    apply_operation, apply_operation_with_context, build_organize_plan,
    build_organize_plan_with_context, build_rename_plan, build_rename_plan_with_context,
    list_operations, undo_operation, undo_operation_with_context, OperationAction, OperationKind,
    OperationPlan, OperationRecord, OperationResult, OrganizeBy, RenameOptions,
};
pub use progress::{CancellationToken, OperationContext, ProgressEvent, ProgressReporter};
pub use reports::{
    duplicate_files, duplicate_files_with_context, file_hash, file_hash_with_context, large_files,
    large_files_with_context, DuplicateGroup, LargeFileEntry,
};
pub use scan::{scan, scan_with_context, FileRecord, ScanOptions, ScanResult, ScanWarning};

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

    #[error("operation was cancelled")]
    Cancelled,
}
