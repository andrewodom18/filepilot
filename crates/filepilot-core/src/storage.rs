use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{FilePilotError, OperationRecord, Result};

pub fn data_directory() -> Result<PathBuf> {
    directories::ProjectDirs::from("com", "FilePilot", "FilePilot")
        .map(|directories| directories.data_dir().to_path_buf())
        .ok_or_else(|| {
            FilePilotError::InvalidInput(
                "could not determine the platform data directory".to_string(),
            )
        })
}

pub fn operations_directory() -> Result<PathBuf> {
    let directory = data_directory()?.join("operations");
    fs::create_dir_all(&directory)?;
    Ok(directory)
}

pub fn operation_path(id: &str) -> Result<PathBuf> {
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains('.') {
        return Err(FilePilotError::InvalidInput(
            "invalid operation id".to_string(),
        ));
    }
    Ok(operations_directory()?.join(format!("{id}.json")))
}

pub fn save_operation(record: &OperationRecord) -> Result<()> {
    let path = operation_path(&record.id)?;
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(record)?;
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)?;
    Ok(())
}

pub fn load_operation(id: &str) -> Result<OperationRecord> {
    let path = operation_path(id)?;
    let bytes = fs::read(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            FilePilotError::OperationNotFound(id.to_string())
        } else {
            FilePilotError::Io(error)
        }
    })?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn latest_operation_id() -> Result<Option<String>> {
    let directory = operations_directory()?;
    let mut candidates = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let modified = entry.metadata()?.modified().ok();
        candidates.push((modified, path));
    }

    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
    Ok(candidates
        .first()
        .and_then(|(_, path)| path.file_stem())
        .and_then(|stem| stem.to_str())
        .map(str::to_string))
}

pub fn list_operations() -> Result<Vec<OperationRecord>> {
    let directory = operations_directory()?;
    let mut records = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let bytes = fs::read(path)?;
        if let Ok(record) = serde_json::from_slice::<OperationRecord>(&bytes) {
            records.push(record);
        }
    }
    records.sort_by_key(|record| std::cmp::Reverse(record.completed_at));
    Ok(records)
}

#[allow(dead_code)]
fn _operation_file(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()) == Some("json")
}
