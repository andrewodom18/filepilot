use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};

use filepilot_core::{
    file_hash_with_context, FilePilotError, OperationContext, ProgressEvent, Result,
};
use serde::{Deserialize, Serialize};

use crate::trash_support::move_to_trash;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateCleanupCandidate {
    pub path: PathBuf,
    pub expected_size_bytes: u64,
    pub expected_hash: String,
    pub group_paths: Vec<PathBuf>,
    pub recommended_primary: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateCleanupItem {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub destination: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateCleanupFailure {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateCleanupResult {
    pub moved: Vec<DuplicateCleanupItem>,
    pub failed: Vec<DuplicateCleanupFailure>,
    pub reclaimed_bytes: u64,
}

pub fn cleanup_duplicate_files(
    context: &OperationContext,
    candidates: &[DuplicateCleanupCandidate],
) -> Result<DuplicateCleanupResult> {
    if candidates.is_empty() {
        return Err(FilePilotError::InvalidInput(
            "select at least one duplicate copy to move to Trash".to_string(),
        ));
    }

    let mut seen = HashSet::new();
    let mut groups: HashMap<String, (HashSet<PathBuf>, HashSet<PathBuf>)> = HashMap::new();
    let mut prepared = Vec::with_capacity(candidates.len());

    for candidate in candidates {
        context.cancellation.check()?;

        if !seen.insert(candidate.path.clone()) {
            return Err(FilePilotError::Conflict(format!(
                "duplicate path selected more than once: {}",
                candidate.path.display()
            )));
        }
        let group_paths: HashSet<_> = candidate.group_paths.iter().cloned().collect();
        if group_paths.len() < 2
            || !group_paths.contains(&candidate.recommended_primary)
            || !group_paths.contains(&candidate.path)
        {
            return Err(FilePilotError::Conflict(format!(
                "duplicate group information is invalid for {}",
                candidate.path.display()
            )));
        }
        let group = groups
            .entry(candidate.expected_hash.clone())
            .or_insert_with(|| (group_paths.clone(), HashSet::new()));
        if group.0 != group_paths {
            return Err(FilePilotError::Conflict(format!(
                "duplicate group information changed for {}",
                candidate.path.display()
            )));
        }
        group.1.insert(candidate.path.clone());

        let metadata = fs::symlink_metadata(&candidate.path).map_err(|error| {
            FilePilotError::Conflict(format!(
                "could not revalidate {} before moving it to Trash: {error}",
                candidate.path.display()
            ))
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(FilePilotError::Conflict(format!(
                "duplicate cleanup only accepts regular files: {}",
                candidate.path.display()
            )));
        }
        if metadata.len() != candidate.expected_size_bytes {
            return Err(FilePilotError::Conflict(format!(
                "file changed since the duplicate report: {}",
                candidate.path.display()
            )));
        }

        let current_hash = file_hash_with_context(&candidate.path, context)?;
        if current_hash != candidate.expected_hash {
            return Err(FilePilotError::Conflict(format!(
                "file contents changed since the duplicate report: {}",
                candidate.path.display()
            )));
        }

        prepared.push((candidate.path.clone(), candidate.expected_size_bytes));
    }

    if let Some((_, (group_paths, _selected_paths))) = groups
        .iter()
        .find(|(_, (group_paths, selected_paths))| selected_paths.len() >= group_paths.len())
    {
        return Err(FilePilotError::Conflict(format!(
            "at least one file must remain in every duplicate group ({} files were selected)",
            group_paths.len()
        )));
    }

    let total = prepared.len() as u64;
    let mut moved = Vec::new();
    let mut failed = Vec::new();

    for (index, (path, size_bytes)) in prepared.into_iter().enumerate() {
        context.cancellation.check()?;
        context.report(ProgressEvent {
            phase: "moving selected duplicates to Trash".to_string(),
            completed: index as u64,
            total: Some(total),
            current_path: Some(path.clone()),
            message: Some(
                "Cancellation is checked before each file; files already moved remain recoverable in Trash."
                    .to_string(),
            ),
        });

        match move_to_trash(&path) {
            Ok(()) => moved.push(DuplicateCleanupItem {
                path,
                size_bytes,
                destination: "System Trash".to_string(),
            }),
            Err(error) => failed.push(DuplicateCleanupFailure {
                path,
                message: format!("could not move file to Trash: {error}"),
            }),
        }
    }

    context.report(ProgressEvent {
        phase: "duplicate cleanup complete".to_string(),
        completed: total,
        total: Some(total),
        current_path: None,
        message: None,
    });

    Ok(DuplicateCleanupResult {
        reclaimed_bytes: moved.iter().map(|item| item.size_bytes).sum(),
        moved,
        failed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use filepilot_core::file_hash;
    use tempfile::tempdir;

    #[test]
    fn stale_candidate_aborts_the_batch_before_any_file_moves() {
        let directory = tempdir().unwrap();
        let primary = directory.path().join("primary.txt");
        let extra = directory.path().join("extra.txt");
        let stale = directory.path().join("stale.txt");
        fs::write(&primary, b"same").unwrap();
        fs::write(&extra, b"same").unwrap();
        fs::write(&stale, b"changed").unwrap();

        let result = cleanup_duplicate_files(
            &OperationContext::default(),
            &[
                DuplicateCleanupCandidate {
                    path: extra.clone(),
                    expected_size_bytes: 4,
                    expected_hash: file_hash(&extra).unwrap(),
                    group_paths: vec![primary.clone(), extra.clone()],
                    recommended_primary: primary.clone(),
                },
                DuplicateCleanupCandidate {
                    path: stale.clone(),
                    expected_size_bytes: 4,
                    expected_hash: "not-the-current-hash".to_string(),
                    group_paths: vec![primary.clone(), stale.clone()],
                    recommended_primary: primary.clone(),
                },
            ],
        );

        assert!(matches!(result, Err(FilePilotError::Conflict(_))));
        assert!(extra.exists());
        assert!(stale.exists());
    }

    #[test]
    fn cleanup_cannot_remove_every_file_in_a_group() {
        let directory = tempdir().unwrap();
        let primary = directory.path().join("primary.txt");
        let extra = directory.path().join("extra.txt");
        fs::write(&primary, b"same").unwrap();
        fs::write(&extra, b"same").unwrap();

        let result = cleanup_duplicate_files(
            &OperationContext::default(),
            &[
                DuplicateCleanupCandidate {
                    path: primary.clone(),
                    expected_size_bytes: 4,
                    expected_hash: file_hash(&primary).unwrap(),
                    group_paths: vec![primary.clone(), extra.clone()],
                    recommended_primary: primary.clone(),
                },
                DuplicateCleanupCandidate {
                    path: extra.clone(),
                    expected_size_bytes: 4,
                    expected_hash: file_hash(&extra).unwrap(),
                    group_paths: vec![primary.clone(), extra.clone()],
                    recommended_primary: primary.clone(),
                },
            ],
        );

        assert!(matches!(result, Err(FilePilotError::Conflict(_))));
        assert!(primary.exists());
        assert!(extra.exists());
    }
}
