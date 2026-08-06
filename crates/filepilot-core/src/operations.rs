use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::storage;
use crate::{scan, FilePilotError, FileRecord, Result, ScanOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationKind {
    Rename,
    Organize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrganizeBy {
    Extension,
    Date,
    Name,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RenameOptions {
    pub pattern: Option<String>,
    pub regex: Option<String>,
    pub replace: Option<String>,
    pub width: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationAction {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationPlan {
    pub id: String,
    pub kind: OperationKind,
    pub created_at: DateTime<Utc>,
    pub root: PathBuf,
    pub actions: Vec<OperationAction>,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationRecord {
    pub id: String,
    pub kind: OperationKind,
    pub created_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub root: PathBuf,
    pub actions: Vec<OperationAction>,
    pub undone_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResult {
    pub operation_id: String,
    pub kind: OperationKind,
    pub dry_run: bool,
    pub actions: Vec<OperationAction>,
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn build_rename_plan(
    root: impl AsRef<Path>,
    scan_options: &ScanOptions,
    options: &RenameOptions,
) -> Result<OperationPlan> {
    validate_rename_options(options)?;
    let scan_result = scan(root.as_ref(), scan_options)?;
    let mut actions = Vec::new();
    let mut skipped = scan_result
        .warnings
        .iter()
        .map(|warning| warning.message.clone())
        .collect::<Vec<_>>();

    for (index, file) in scan_result.files.iter().enumerate() {
        let destination_name = render_destination_name(file, index + 1, options)?;
        ensure_safe_filename(&destination_name)?;
        let destination = file
            .path
            .parent()
            .ok_or_else(|| FilePilotError::InvalidPath(file.path.clone()))?
            .join(destination_name);

        if destination == file.path {
            skipped.push(format!("already named correctly: {}", file.path.display()));
            continue;
        }

        actions.push(OperationAction {
            source: file.path.clone(),
            destination,
            size_bytes: file.size_bytes,
        });
    }

    validate_actions(&actions)?;
    Ok(new_plan(
        OperationKind::Rename,
        scan_result.root,
        actions,
        skipped,
    ))
}

pub fn build_organize_plan(
    root: impl AsRef<Path>,
    scan_options: &ScanOptions,
    organize_by: OrganizeBy,
) -> Result<OperationPlan> {
    let scan_result = scan(root.as_ref(), scan_options)?;
    if !scan_result.root.is_dir() {
        return Err(FilePilotError::InvalidInput(
            "organize requires a directory root".to_string(),
        ));
    }

    let mut actions = Vec::new();
    let mut skipped = scan_result
        .warnings
        .iter()
        .map(|warning| warning.message.clone())
        .collect::<Vec<_>>();

    for file in &scan_result.files {
        let bucket = organization_bucket(file, organize_by);
        let destination = scan_result.root.join(bucket).join(
            file.path
                .file_name()
                .ok_or_else(|| FilePilotError::InvalidPath(file.path.clone()))?,
        );

        if destination == file.path {
            skipped.push(format!("already organized: {}", file.path.display()));
            continue;
        }

        actions.push(OperationAction {
            source: file.path.clone(),
            destination,
            size_bytes: file.size_bytes,
        });
    }

    validate_actions(&actions)?;
    Ok(new_plan(
        OperationKind::Organize,
        scan_result.root,
        actions,
        skipped,
    ))
}

pub fn apply_operation(plan: &OperationPlan, dry_run: bool) -> Result<OperationResult> {
    validate_actions(&plan.actions)?;

    if dry_run {
        return Ok(OperationResult {
            operation_id: plan.id.clone(),
            kind: plan.kind,
            dry_run: true,
            actions: plan.actions.clone(),
            skipped: plan.skipped.clone(),
            warnings: Vec::new(),
        });
    }

    execute_actions(&plan.actions, &plan.root)?;
    let record = OperationRecord {
        id: plan.id.clone(),
        kind: plan.kind,
        created_at: plan.created_at,
        completed_at: Utc::now(),
        root: plan.root.clone(),
        actions: plan.actions.clone(),
        undone_at: None,
    };

    let mut warnings = Vec::new();
    if let Err(error) = storage::save_operation(&record) {
        warnings.push(format!(
            "operation completed but could not be logged: {error}"
        ));
    }

    Ok(OperationResult {
        operation_id: plan.id.clone(),
        kind: plan.kind,
        dry_run: false,
        actions: plan.actions.clone(),
        skipped: plan.skipped.clone(),
        warnings,
    })
}

pub fn undo_operation(operation_id: Option<&str>) -> Result<OperationResult> {
    let id = match operation_id {
        Some(id) => id.to_string(),
        None => storage::latest_operation_id()?.ok_or_else(|| {
            FilePilotError::OperationNotFound("no completed operations found".to_string())
        })?,
    };
    let mut record = storage::load_operation(&id)?;

    if record.undone_at.is_some() {
        return Err(FilePilotError::UndoRefused(format!(
            "operation {id} has already been undone"
        )));
    }

    for action in &record.actions {
        if !action.destination.is_file() {
            return Err(FilePilotError::UndoRefused(format!(
                "destination is missing: {}",
                action.destination.display()
            )));
        }
        if action.source.exists() {
            return Err(FilePilotError::UndoRefused(format!(
                "original path is occupied: {}",
                action.source.display()
            )));
        }
        let current_size = fs::metadata(&action.destination)?.len();
        if current_size != action.size_bytes {
            return Err(FilePilotError::UndoRefused(format!(
                "destination changed: {}",
                action.destination.display()
            )));
        }
    }

    let reverse_actions: Vec<_> = record
        .actions
        .iter()
        .map(|action| OperationAction {
            source: action.destination.clone(),
            destination: action.source.clone(),
            size_bytes: action.size_bytes,
        })
        .collect();
    execute_actions(&reverse_actions, &record.root)?;

    record.undone_at = Some(Utc::now());
    let mut warnings = Vec::new();
    if let Err(error) = storage::save_operation(&record) {
        warnings.push(format!(
            "operation was undone but log update failed: {error}"
        ));
    }

    Ok(OperationResult {
        operation_id: record.id,
        kind: record.kind,
        dry_run: false,
        actions: reverse_actions,
        skipped: Vec::new(),
        warnings,
    })
}

fn new_plan(
    kind: OperationKind,
    root: PathBuf,
    actions: Vec<OperationAction>,
    skipped: Vec<String>,
) -> OperationPlan {
    OperationPlan {
        id: Uuid::new_v4().to_string(),
        kind,
        created_at: Utc::now(),
        root,
        actions,
        skipped,
    }
}

fn validate_rename_options(options: &RenameOptions) -> Result<()> {
    if options.pattern.is_some() == options.regex.is_some() {
        return Err(FilePilotError::InvalidInput(
            "provide exactly one of --pattern or --regex".to_string(),
        ));
    }
    if options.regex.is_some() && options.replace.is_none() {
        return Err(FilePilotError::InvalidInput(
            "--regex requires --replace".to_string(),
        ));
    }
    if options.pattern.is_some() && options.replace.is_some() {
        return Err(FilePilotError::InvalidInput(
            "--replace can only be used with --regex".to_string(),
        ));
    }
    if let Some(pattern) = &options.pattern {
        if pattern.is_empty() {
            return Err(FilePilotError::InvalidInput(
                "rename pattern cannot be empty".to_string(),
            ));
        }
    }
    if let Some(regex) = &options.regex {
        Regex::new(regex)?;
    }
    Ok(())
}

fn render_destination_name(
    file: &FileRecord,
    number: usize,
    options: &RenameOptions,
) -> Result<String> {
    if let Some(pattern) = &options.pattern {
        let name = file.path.file_name().unwrap_or_default().to_string_lossy();
        let stem = file.path.file_stem().unwrap_or_default().to_string_lossy();
        let extension = file
            .path
            .extension()
            .map(|extension| format!(".{}", extension.to_string_lossy()))
            .unwrap_or_default();
        let date = file
            .modified_at
            .map(|date| date.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let formatted_number = if options.width == 0 {
            number.to_string()
        } else {
            format!("{number:0width$}", width = options.width)
        };

        Ok(pattern
            .replace("{name}", &name)
            .replace("{stem}", &stem)
            .replace("{ext}", &extension)
            .replace("{date}", &date)
            .replace("{number}", &formatted_number))
    } else if let (Some(expression), Some(replacement)) = (&options.regex, &options.replace) {
        let regex = Regex::new(expression)?;
        let name = file.path.file_name().unwrap_or_default().to_string_lossy();
        Ok(regex.replace(&name, replacement.as_str()).into_owned())
    } else {
        Err(FilePilotError::InvalidInput(
            "rename pattern is missing".to_string(),
        ))
    }
}

fn organization_bucket(file: &FileRecord, organize_by: OrganizeBy) -> PathBuf {
    match organize_by {
        OrganizeBy::Extension => PathBuf::from(
            file.extension
                .as_deref()
                .filter(|extension| !extension.is_empty())
                .unwrap_or("_no-extension"),
        ),
        OrganizeBy::Date => {
            let date = file
                .modified_at
                .map(|date| date.format("%Y/%Y-%m-%d").to_string())
                .unwrap_or_else(|| "unknown-date".to_string());
            PathBuf::from(date)
        }
        OrganizeBy::Name => {
            let first = file
                .path
                .file_name()
                .and_then(|name| name.to_string_lossy().chars().next())
                .map(|character| character.to_ascii_uppercase())
                .filter(|character| character.is_ascii_alphabetic())
                .map(|character| character.to_string())
                .unwrap_or_else(|| "_other".to_string());
            PathBuf::from(first)
        }
    }
}

fn ensure_safe_filename(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains('\0') {
        return Err(FilePilotError::InvalidInput(
            "generated filename is invalid".to_string(),
        ));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(FilePilotError::InvalidInput(format!(
            "generated filename contains a path separator: {name}"
        )));
    }
    Ok(())
}

fn validate_actions(actions: &[OperationAction]) -> Result<()> {
    let source_keys: HashSet<_> = actions
        .iter()
        .map(|action| path_key(&action.source))
        .collect();
    let mut destinations = HashSet::new();

    for action in actions {
        if !action.source.is_file() {
            return Err(FilePilotError::InvalidPath(action.source.clone()));
        }
        let destination_key = path_key(&action.destination);
        if !destinations.insert(destination_key.clone()) {
            return Err(FilePilotError::Conflict(format!(
                "multiple files target {}",
                action.destination.display()
            )));
        }
        if action.destination.exists() && !source_keys.contains(&destination_key) {
            return Err(FilePilotError::Conflict(format!(
                "destination already exists: {}",
                action.destination.display()
            )));
        }
    }
    Ok(())
}

fn execute_actions(actions: &[OperationAction], root: &Path) -> Result<()> {
    if actions.is_empty() {
        return Ok(());
    }

    let staging_root = if root.is_dir() {
        root.to_path_buf()
    } else {
        root.parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    };
    let staging = staging_root.join(format!(".filepilot-staging-{}", Uuid::new_v4()));
    fs::create_dir(&staging)?;

    let mut staged = Vec::with_capacity(actions.len());
    for (index, action) in actions.iter().enumerate() {
        let temporary = staging.join(index.to_string());
        if let Err(error) = fs::rename(&action.source, &temporary) {
            rollback_staged(&staged);
            let _ = fs::remove_dir(&staging);
            return Err(error.into());
        }
        staged.push((temporary, action.source.clone(), action.destination.clone()));
    }

    for (temporary, source, destination) in &staged {
        if let Some(parent) = destination.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                rollback_after_final_failure(&staged, temporary, source);
                let _ = fs::remove_dir_all(&staging);
                return Err(error.into());
            }
        }
        if let Err(error) = fs::rename(temporary, destination) {
            rollback_after_final_failure(&staged, temporary, source);
            let _ = fs::remove_dir_all(&staging);
            return Err(error.into());
        }
    }

    fs::remove_dir(&staging)?;
    Ok(())
}

fn rollback_staged(staged: &[(PathBuf, PathBuf, PathBuf)]) {
    for (temporary, source, _) in staged.iter().rev() {
        let _ = fs::rename(temporary, source);
    }
}

fn rollback_after_final_failure(
    staged: &[(PathBuf, PathBuf, PathBuf)],
    failed_temporary: &Path,
    failed_source: &Path,
) {
    for (temporary, source, destination) in staged.iter().rev() {
        if temporary == failed_temporary {
            let _ = fs::rename(temporary, failed_source);
        } else if temporary.exists() {
            let _ = fs::rename(temporary, source);
        } else if destination.exists() {
            let _ = fs::rename(destination, source);
        }
    }
}

fn path_key(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    if cfg!(target_os = "windows") || cfg!(target_os = "macos") {
        value.to_lowercase()
    } else {
        value
    }
}
