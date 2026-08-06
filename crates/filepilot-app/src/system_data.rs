use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    process::Command,
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
    pub is_directory: bool,
    pub cleanup_allowed: bool,
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
    pub scope_path: Option<PathBuf>,
    pub volume: Option<VolumeInfo>,
    pub items: Vec<StorageItem>,
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeInfo {
    pub used_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub free_bytes: Option<u64>,
    pub apfs_snapshot_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub destination: String,
}

struct DirectoryReport {
    items: Vec<StorageItem>,
    total_bytes: u64,
}

pub fn analyze_system_data(context: &OperationContext, deep: bool) -> Result<StorageReport> {
    let mut report = empty_report();

    if std::env::consts::OS != "macos" {
        report.notes.insert(
            0,
            "System Data analysis is currently available on macOS only. The desktop app remains available on Windows and Linux for the other file-control workflows.".to_string(),
        );
        return Ok(report);
    }

    let home = home_directory()?;

    for (root, label) in system_data_roots(&home) {
        scan_report_root(&mut report, root, label, deep, context)?;
    }
    add_volume_information(&mut report, context);
    sort_storage_items(&mut report.items);
    Ok(report)
}

pub fn analyze_system_data_location(
    context: &OperationContext,
    requested_path: &Path,
    deep: bool,
) -> Result<StorageReport> {
    let mut report = empty_report();
    if std::env::consts::OS != "macos" {
        report.notes.insert(
            0,
            "System Data analysis is currently available on macOS only. The desktop app remains available on Windows and Linux for the other file-control workflows.".to_string(),
        );
        return Ok(report);
    }

    let home = home_directory()?;
    let root = validate_system_data_path(requested_path, &home)?;
    report.scope_path = Some(root.clone());
    report.notes.insert(
        0,
        format!(
            "Detailed view for {}. Use Analyze all locations to return to the top-level report.",
            root.display()
        ),
    );
    let scanned = scan_directory(
        &root,
        &format!("Selected location / {}", display_name(&root)),
        deep,
        context,
        &mut report.warnings,
    )?;
    report.total_bytes = scanned.total_bytes;
    report.items = scanned.items;
    add_volume_information(&mut report, context);
    sort_storage_items(&mut report.items);
    Ok(report)
}

pub fn cleanup_system_data_path(
    context: &OperationContext,
    requested_path: &Path,
    expected_size: u64,
) -> Result<CleanupResult> {
    if std::env::consts::OS != "macos" {
        return Err(filepilot_core::FilePilotError::InvalidInput(
            "System Data cleanup is currently available on macOS only".to_string(),
        ));
    }
    let home = home_directory()?;
    let candidate = validate_cleanup_path(requested_path, &home)?;
    context.cancellation.check()?;
    let metadata = fs::symlink_metadata(&candidate)?;
    let actual_size = if metadata.is_dir() {
        measure_directory(&candidate, context)?
    } else {
        metadata.len()
    };
    if actual_size != expected_size {
        return Err(filepilot_core::FilePilotError::Conflict(format!(
            "cleanup target changed since the report: {}",
            candidate.display()
        )));
    }
    context.reporter.report(ProgressEvent {
        phase: "moving selected cache to Trash".to_string(),
        completed: 0,
        total: Some(1),
        current_path: Some(candidate.clone()),
        message: Some("The selected item will remain recoverable in the system Trash.".to_string()),
    });
    trash::delete(&candidate).map_err(|error| {
        filepilot_core::FilePilotError::InvalidInput(format!(
            "could not move {} to Trash: {error}",
            candidate.display()
        ))
    })?;
    Ok(CleanupResult {
        path: candidate,
        size_bytes: actual_size,
        destination: "System Trash".to_string(),
    })
}

fn empty_report() -> StorageReport {
    StorageReport {
        platform: std::env::consts::OS.to_string(),
        generated_at: Utc::now(),
        total_bytes: 0,
        scope_path: None,
        volume: None,
        items: Vec::new(),
        warnings: Vec::new(),
        notes: vec![
            "This is a FilePilot estimate of large System Data contributors, not an exact copy of the number shown in macOS Storage settings.".to_string(),
            "macOS also counts APFS purgeable space and system-managed structures that cannot be safely attributed to ordinary files.".to_string(),
            "APFS volume usage and snapshot counts are read through fixed, read-only macOS tools when available; per-snapshot reclaimable size is not exposed reliably.".to_string(),
            "FilePilot only offers Trash-based cleanup for selected direct children of the current user's cache and log folders. Treat every recommendation as review guidance, not proof that a path is safe to remove.".to_string(),
        ],
    }
}

fn home_directory() -> Result<PathBuf> {
    directories::BaseDirs::new()
        .map(|directories| directories.home_dir().to_path_buf())
        .ok_or_else(|| {
            filepilot_core::FilePilotError::InvalidInput(
                "could not determine the current user's home directory".to_string(),
            )
        })
}

fn system_data_roots(home: &Path) -> [(PathBuf, &'static str); 3] {
    [
        (home.join("Library"), "User Library"),
        (PathBuf::from("/Library"), "Shared Library"),
        (PathBuf::from("/private/var"), "System working data"),
    ]
}

fn scan_report_root(
    report: &mut StorageReport,
    root: PathBuf,
    label: &str,
    deep: bool,
    context: &OperationContext,
) -> Result<()> {
    context.cancellation.check()?;
    if !root.exists() {
        report
            .warnings
            .push(format!("Skipped missing location: {}", root.display()));
        return Ok(());
    }
    let scanned = scan_directory(&root, label, deep, context, &mut report.warnings)?;
    report.total_bytes = report.total_bytes.saturating_add(scanned.total_bytes);
    report.items.extend(scanned.items);
    Ok(())
}

fn validate_system_data_path(requested_path: &Path, home: &Path) -> Result<PathBuf> {
    let candidate = fs::canonicalize(requested_path)?;
    if !candidate.is_dir() {
        return Err(filepilot_core::FilePilotError::InvalidInput(
            "System Data drill-down requires a directory".to_string(),
        ));
    }
    for (root, _) in system_data_roots(home) {
        let Ok(root) = fs::canonicalize(root) else {
            continue;
        };
        if candidate.starts_with(root) {
            return Ok(candidate);
        }
    }
    Err(filepilot_core::FilePilotError::InvalidInput(
        "System Data drill-down is limited to FilePilot's fixed macOS storage locations"
            .to_string(),
    ))
}

fn validate_cleanup_path(requested_path: &Path, home: &Path) -> Result<PathBuf> {
    let metadata = fs::symlink_metadata(requested_path)?;
    if metadata.file_type().is_symlink() {
        return Err(filepilot_core::FilePilotError::InvalidInput(
            "cleanup refuses symbolic links".to_string(),
        ));
    }
    let candidate = fs::canonicalize(requested_path)?;
    let allowed_roots = [home.join("Library/Caches"), home.join("Library/Logs")];
    for root in allowed_roots {
        let Ok(root) = fs::canonicalize(root) else {
            continue;
        };
        if candidate.parent() == Some(root.as_path()) {
            return Ok(candidate);
        }
    }
    Err(filepilot_core::FilePilotError::InvalidInput(
        "only direct children of the current user's Library/Caches and Library/Logs may be moved to Trash".to_string(),
    ))
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
            let mut children = nested_sizes
                .remove(&path)
                .unwrap_or_default()
                .into_iter()
                .map(|(child_path, child_size)| storage_item(child_path, child_size, Vec::new()))
                .collect::<Vec<_>>();
            sort_storage_items(&mut children);
            children.truncate(40);
            let is_directory = fs::symlink_metadata(&path)
                .map(|metadata| metadata.is_dir())
                .unwrap_or(false);
            storage_item_with_label(
                format!("{root_label} / {}", display_name(&path)),
                path,
                size_bytes,
                is_directory,
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
    let is_directory = fs::symlink_metadata(&path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false);
    storage_item_with_label(
        display_name(&path),
        path,
        size_bytes,
        is_directory,
        children,
    )
}

fn storage_item_with_label(
    label: String,
    path: PathBuf,
    size_bytes: u64,
    is_directory: bool,
    children: Vec<StorageItem>,
) -> StorageItem {
    let assessment = classify_path(&path);
    let (reason, recommendation) = guidance(assessment);
    let cleanup_allowed = cleanup_allowed_for_path(&path);
    StorageItem {
        label,
        path,
        size_bytes,
        size_known: true,
        is_directory,
        cleanup_allowed,
        assessment,
        reason: reason.to_string(),
        recommendation: recommendation.to_string(),
        children,
    }
}

fn sort_storage_items(items: &mut [StorageItem]) {
    items.sort_by(|left, right| {
        right
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| left.path.cmp(&right.path))
    });
}

fn cleanup_allowed_for_path(path: &Path) -> bool {
    if std::env::consts::OS != "macos" {
        return false;
    }
    let Some(home) =
        directories::BaseDirs::new().map(|directories| directories.home_dir().to_path_buf())
    else {
        return false;
    };
    cleanup_allowed_at_path(path, &home)
}

fn cleanup_allowed_at_path(path: &Path, home: &Path) -> bool {
    [home.join("Library/Caches"), home.join("Library/Logs")]
        .iter()
        .any(|root| path.parent() == Some(root.as_path()))
}

fn measure_directory(path: &Path, context: &OperationContext) -> Result<u64> {
    let mut total = 0_u64;
    for entry in WalkDir::new(path).follow_links(false).into_iter() {
        context.cancellation.check()?;
        let entry = entry.map_err(|error| {
            filepilot_core::FilePilotError::InvalidInput(format!(
                "could not revalidate cleanup target {}: {error}",
                path.display()
            ))
        })?;
        if entry.file_type().is_symlink() || !entry.file_type().is_file() {
            continue;
        }
        let metadata = entry.metadata().map_err(|error| {
            filepilot_core::FilePilotError::InvalidInput(format!(
                "could not revalidate cleanup target {}: {error}",
                path.display()
            ))
        })?;
        total = total.saturating_add(metadata.len());
    }
    Ok(total)
}

fn add_volume_information(report: &mut StorageReport, context: &OperationContext) {
    if std::env::consts::OS != "macos" {
        return;
    }
    if context.cancellation.is_cancelled() {
        return;
    }

    let diskutil = "/usr/sbin/diskutil";
    let info = match Command::new(diskutil).args(["info", "/"]).output() {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
        Ok(_) => {
            report
                .notes
                .push("macOS volume statistics were unavailable from diskutil.".to_string());
            String::new()
        }
        Err(error) => {
            report
                .notes
                .push(format!("macOS volume statistics were unavailable: {error}"));
            String::new()
        }
    };

    let snapshots = match Command::new(diskutil)
        .args(["apfs", "listSnapshots", "/"])
        .output()
    {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
        Ok(_) | Err(_) => String::new(),
    };
    let snapshot_count = snapshots
        .lines()
        .filter(|line| line.trim_start().starts_with("Snapshot UUID:"))
        .count();
    if snapshot_count > 0 {
        report.notes.push(format!(
            "macOS reports {snapshot_count} APFS snapshot(s). Their individual reclaimable sizes are not exposed reliably by this read-only check."
        ));
    }

    report.volume = Some(VolumeInfo {
        used_bytes: parse_diskutil_bytes(&info, "Volume Used Space"),
        total_bytes: parse_diskutil_bytes(&info, "Container Total Space"),
        free_bytes: parse_diskutil_bytes(&info, "Container Free Space"),
        apfs_snapshot_count: snapshot_count,
    });
}

fn parse_diskutil_bytes(output: &str, label: &str) -> Option<u64> {
    output.lines().find_map(|line| {
        if !line.contains(label) {
            return None;
        }
        let bytes_section = line.split_once('(')?.1;
        let digits = bytes_section
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        digits.parse().ok()
    })
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
    use std::fs;

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

    #[test]
    fn scans_a_location_and_sorts_nested_contributors() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let root = temporary.path().join("Library");
        fs::create_dir_all(root.join("Application Support/Small")).expect("create folders");
        fs::create_dir_all(root.join("Application Support/Large")).expect("create folders");
        fs::write(root.join("Application Support/Small/file"), [1_u8; 2]).expect("write file");
        fs::write(root.join("Application Support/Large/file"), [2_u8; 8]).expect("write file");
        let mut warnings = Vec::new();

        let report = scan_directory(
            &root,
            "Test Library",
            true,
            &OperationContext::default(),
            &mut warnings,
        )
        .expect("scan location");

        assert!(warnings.is_empty());
        assert_eq!(report.total_bytes, 10);
        assert_eq!(report.items.len(), 1);
        assert_eq!(report.items[0].children[0].size_bytes, 8);
        assert_eq!(report.items[0].children[1].size_bytes, 2);
    }

    #[test]
    fn parses_diskutil_byte_fields_without_trusting_display_units() {
        let output = "   Container Total Space: 494.4 GB (494384795648 Bytes)\n   Container Free Space: 71.8 GB (71833886720 Bytes)";
        assert_eq!(
            parse_diskutil_bytes(output, "Container Total Space"),
            Some(494384795648)
        );
        assert_eq!(
            parse_diskutil_bytes(output, "Container Free Space"),
            Some(71833886720)
        );
    }

    #[test]
    fn cleanup_allowlist_excludes_broader_library_data() {
        let home = Path::new("/Users/test");
        assert!(cleanup_allowed_at_path(
            Path::new("/Users/test/Library/Caches/Example.app"),
            home
        ));
        assert!(cleanup_allowed_at_path(
            Path::new("/Users/test/Library/Logs/Example.app"),
            home
        ));
        assert!(!cleanup_allowed_at_path(
            Path::new("/Users/test/Library/Application Support/Example.app"),
            home
        ));
    }
}
