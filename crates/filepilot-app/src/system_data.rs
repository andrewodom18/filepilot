use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use filepilot_core::{OperationContext, ProgressEvent, Result};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StorageAssessment {
    LikelySafeToReview,
    ReviewBeforeRemoving,
    UserData,
    SystemManaged,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageItem {
    pub label: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub size_known: bool,
    pub assessment: StorageAssessment,
    pub reason: String,
    pub recommendation: String,
    pub children: Vec<StorageItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageReport {
    pub platform: String,
    pub generated_at: DateTime<Utc>,
    pub total_bytes: u64,
    pub items: Vec<StorageItem>,
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
}

struct DirectoryReport {
    items: Vec<StorageItem>,
    total_bytes: u64,
}

pub fn analyze_system_data(context: &OperationContext, deep: bool) -> Result<StorageReport> {
    let mut report = StorageReport {
        platform: std::env::consts::OS.to_string(),
        generated_at: Utc::now(),
        total_bytes: 0,
        items: Vec::new(),
        warnings: Vec::new(),
        notes: vec![
            "This is a FilePilot estimate of large System Data contributors, not an exact copy of the number shown in macOS Storage settings.".to_string(),
            "macOS also counts APFS purgeable space and system-managed structures that cannot be safely attributed to ordinary files.".to_string(),
            "Time Machine local snapshots are managed by macOS and are not sized or modified by this analyzer. Review them with supported macOS storage tools if they are suspected contributors.".to_string(),
            "FilePilot does not delete anything from this report. Treat every recommendation as review guidance, not proof that a path is safe to remove.".to_string(),
        ],
    };

    if std::env::consts::OS != "macos" {
        report.notes.insert(
            0,
            "System Data analysis is currently available on macOS only. The desktop app remains available on Windows and Linux for the other file-control workflows.".to_string(),
        );
        return Ok(report);
    }

    let home = directories::BaseDirs::new()
        .map(|directories| directories.home_dir().to_path_buf())
        .ok_or_else(|| {
            filepilot_core::FilePilotError::InvalidInput(
                "could not determine the current user's home directory".to_string(),
            )
        })?;

    let roots = [
        (home.join("Library"), "User Library"),
        (PathBuf::from("/Library"), "Shared Library"),
        (PathBuf::from("/private/var"), "System working data"),
    ];

    for (root, label) in roots {
        context.cancellation.check()?;
        if !root.exists() {
            report
                .warnings
                .push(format!("Skipped missing location: {}", root.display()));
            continue;
        }

        let scanned = scan_directory(&root, label, deep, context, &mut report.warnings)?;
        report.total_bytes = report.total_bytes.saturating_add(scanned.total_bytes);
        report.items.extend(scanned.items);
    }

    report.items.sort_by(|left, right| {
        right
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(report)
}

fn scan_directory(
    root: &Path,
    root_label: &str,
    deep: bool,
    context: &OperationContext,
    warnings: &mut Vec<String>,
) -> Result<DirectoryReport> {
    let mut direct_sizes: BTreeMap<PathBuf, u64> = BTreeMap::new();
    let mut nested_sizes: HashMap<PathBuf, BTreeMap<PathBuf, u64>> = HashMap::new();
    let mut completed = 0_u64;

    let entries = fs::read_dir(root).map_err(|error| {
        warnings.push(format!("Could not read {}: {}", root.display(), error));
        error
    });
    let Ok(entries) = entries else {
        return Ok(DirectoryReport {
            items: Vec::new(),
            total_bytes: 0,
        });
    };

    for entry in entries {
        match entry {
            Ok(entry) => {
                let path = entry.path();
                if fs::symlink_metadata(&path)
                    .map(|metadata| metadata.file_type().is_symlink())
                    .unwrap_or(false)
                {
                    continue;
                }
                direct_sizes.entry(path).or_insert(0);
            }
            Err(error) => warnings.push(format!(
                "Could not read an entry in {}: {}",
                root.display(),
                error
            )),
        }
    }

    for entry in WalkDir::new(root).follow_links(false).into_iter() {
        context.cancellation.check()?;
        completed += 1;
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warnings.push(format!("Could not inspect {}: {}", root.display(), error));
                continue;
            }
        };
        if entry.depth() == 0 || entry.file_type().is_symlink() {
            continue;
        }

        let path = entry.path().to_path_buf();
        let relative = match path.strip_prefix(root) {
            Ok(relative) => relative,
            Err(_) => continue,
        };
        let Some(first) = relative.components().next() else {
            continue;
        };
        let direct = root.join(first.as_os_str());
        let size = if entry.file_type().is_file() {
            match entry.metadata() {
                Ok(metadata) => metadata.len(),
                Err(error) => {
                    warnings.push(format!("Could not size {}: {}", path.display(), error));
                    0
                }
            }
        } else {
            0
        };
        *direct_sizes.entry(direct.clone()).or_insert(0) = direct_sizes
            .get(&direct)
            .copied()
            .unwrap_or_default()
            .saturating_add(size);

        if deep && should_expand(&direct) {
            if let Some(second) = relative.components().nth(1) {
                let nested = direct.join(second.as_os_str());
                let children = nested_sizes.entry(direct.clone()).or_default();
                *children.entry(nested).or_insert(0) = children
                    .get(&nested)
                    .copied()
                    .unwrap_or_default()
                    .saturating_add(size);
            }
        }

        context.reporter.report(ProgressEvent {
            phase: "analyzing system data".to_string(),
            completed,
            total: None,
            current_path: Some(path),
            message: Some("Reading local storage usage".to_string()),
        });
    }

    let total_bytes = direct_sizes.values().copied().sum();
    let mut items = direct_sizes
        .into_iter()
        .map(|(path, size_bytes)| {
            let children = nested_sizes
                .remove(&path)
                .unwrap_or_default()
                .into_iter()
                .map(|(child_path, child_size)| storage_item(child_path, child_size, Vec::new()))
                .collect::<Vec<_>>();
            storage_item_with_label(
                format!("{root_label} / {}", display_name(&path)),
                path,
                size_bytes,
                children,
            )
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| left.path.cmp(&right.path))
    });
    items.truncate(100);
    Ok(DirectoryReport { items, total_bytes })
}

fn storage_item(path: PathBuf, size_bytes: u64, children: Vec<StorageItem>) -> StorageItem {
    storage_item_with_label(display_name(&path), path, size_bytes, children)
}

fn storage_item_with_label(
    label: String,
    path: PathBuf,
    size_bytes: u64,
    children: Vec<StorageItem>,
) -> StorageItem {
    let assessment = classify_path(&path);
    let (reason, recommendation) = guidance(assessment);
    StorageItem {
        label,
        path,
        size_bytes,
        size_known: true,
        assessment,
        reason: reason.to_string(),
        recommendation: recommendation.to_string(),
        children,
    }
}

fn classify_path(path: &Path) -> StorageAssessment {
    let normalized = path.to_string_lossy().to_lowercase();
    if normalized.contains("/private/var/vm")
        || normalized.starts_with("/system")
        || normalized.starts_with("/usr")
        || normalized.starts_with("/bin")
        || normalized.starts_with("/sbin")
        || normalized.contains("/launchdaemons")
        || normalized.contains("/launchagents")
        || normalized.contains("/preferences")
        || normalized.contains("/private/var/db")
        || normalized.contains("/private/var/root")
    {
        return StorageAssessment::SystemManaged;
    }
    if normalized.contains("/library/caches")
        || normalized.contains("/library/logs")
        || normalized.contains("/private/var/folders")
        || normalized.contains("/private/var/tmp")
        || normalized.contains("/private/var/log")
    {
        return StorageAssessment::LikelySafeToReview;
    }
    if normalized.contains("/developer")
        || normalized.contains("/mobilesync/backup")
        || normalized.contains("/coresimulator")
        || normalized.contains("/containers")
        || normalized.contains("/group containers")
        || normalized.contains("/application support")
        || normalized.contains("/backups")
    {
        return StorageAssessment::ReviewBeforeRemoving;
    }
    if normalized.contains("/messages")
        || normalized.contains("/mail")
        || normalized.contains("/photos")
        || normalized.contains("/music")
        || normalized.contains("/cloudstorage")
    {
        return StorageAssessment::UserData;
    }
    StorageAssessment::Unknown
}

fn guidance(assessment: StorageAssessment) -> (&'static str, &'static str) {
    match assessment {
        StorageAssessment::LikelySafeToReview => (
            "Cache, log, or temporary data. Some of it may be recreated by macOS or an app, but FilePilot cannot prove that every entry is disposable.",
            "Review the path and the owning app first. Remove only content you recognize, using the app or macOS cleanup flow when available.",
        ),
        StorageAssessment::ReviewBeforeRemoving => (
            "This may contain app state, developer artifacts, backups, containers, or device data. It can be large and still be needed.",
            "Review the child folders and keep backups or project data you may need. Prefer the owning app's cleanup controls.",
        ),
        StorageAssessment::UserData => (
            "This appears to contain personal content such as messages, mail, photos, music, or cloud files.",
            "Do not delete it as cleanup. Manage it from the owning app and confirm that important data is backed up.",
        ),
        StorageAssessment::SystemManaged => (
            "macOS-managed or protected content. Its reported size may change automatically and manual removal can make the system unstable.",
            "Leave it to macOS. Use supported system settings, restart/shutdown cycles, or the owning system tool instead of deleting files directly.",
        ),
        StorageAssessment::Unknown => (
            "FilePilot cannot prove whether this path is needed from its name alone.",
            "Treat it as review-only. Identify the owning app or service before making any change.",
        ),
    }
}

fn should_expand(path: &Path) -> bool {
    let normalized = path.to_string_lossy().to_lowercase();
    normalized.ends_with("/application support")
        || normalized.ends_with("/containers")
        || normalized.ends_with("/group containers")
        || normalized.ends_with("/developer")
        || normalized.ends_with("/caches")
        || normalized.ends_with("/folders")
        || normalized.ends_with("/backups")
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_cache_and_system_paths_conservatively() {
        assert_eq!(
            classify_path(Path::new("/Users/test/Library/Caches/Browser")),
            StorageAssessment::LikelySafeToReview
        );
        assert_eq!(
            classify_path(Path::new("/private/var/vm/swapfile0")),
            StorageAssessment::SystemManaged
        );
        assert_eq!(
            classify_path(Path::new("/Users/test/Library/Developer/Xcode")),
            StorageAssessment::ReviewBeforeRemoving
        );
        assert_eq!(
            classify_path(Path::new("/Users/test/Library/Messages")),
            StorageAssessment::UserData
        );
    }
}
