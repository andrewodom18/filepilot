use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use chrono::{DateTime, Utc};
use filepilot_core::{OperationContext, ProgressEvent, Result};
use serde::{Deserialize, Serialize};

use crate::trash_support::move_to_trash;
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StorageCategory {
    AppCaches,
    AppLogs,
    TemporaryFiles,
    DeveloperArtifacts,
    DeviceBackups,
    AppSupportData,
    SandboxedAppData,
    AppSettings,
    MailData,
    MessagesData,
    CloudFiles,
    PersonalMedia,
    VirtualMemory,
    SystemDatabases,
    SystemManaged,
    OtherSystemWorkingData,
    OtherData,
}

impl StorageCategory {
    fn assessment(self) -> StorageAssessment {
        match self {
            Self::AppCaches | Self::AppLogs | Self::TemporaryFiles => {
                StorageAssessment::LikelySafeToReview
            }
            Self::DeveloperArtifacts
            | Self::DeviceBackups
            | Self::AppSupportData
            | Self::SandboxedAppData
            | Self::AppSettings => StorageAssessment::ReviewBeforeRemoving,
            Self::MailData | Self::MessagesData | Self::CloudFiles | Self::PersonalMedia => {
                StorageAssessment::UserData
            }
            Self::VirtualMemory | Self::SystemDatabases | Self::SystemManaged => {
                StorageAssessment::SystemManaged
            }
            Self::OtherSystemWorkingData | Self::OtherData => StorageAssessment::Unknown,
        }
    }
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
    pub category: StorageCategory,
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

struct SystemDataRoot {
    path: PathBuf,
    label: &'static str,
    excluded_direct_children: &'static [&'static str],
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

    for root in system_data_roots(&home) {
        scan_report_root(&mut report, &root, deep, context)?;
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
        &[],
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
    move_to_trash(&candidate).map_err(|error| {
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
            "FilePilot measures named, non-overlapping on-disk locations. The total is the storage it could inspect in those locations, not macOS Storage Settings' System Data total.".to_string(),
            "macOS Storage Settings can include APFS snapshots, purgeable space, system-managed data, and protected locations that do not have a reliable per-folder size.".to_string(),
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

fn system_data_roots(home: &Path) -> Vec<SystemDataRoot> {
    const USER_LIBRARY_KNOWN: &[&str] = &[
        "Application Support",
        "Caches",
        "CloudStorage",
        "Containers",
        "Developer",
        "Group Containers",
        "Logs",
        "Mail",
        "Messages",
        "MobileSync",
    ];
    const SHARED_LIBRARY_KNOWN: &[&str] = &["Application Support", "Caches", "Developer", "Logs"];
    const SYSTEM_WORKING_DATA_KNOWN: &[&str] = &["db", "folders", "log", "tmp", "vm"];

    vec![
        SystemDataRoot {
            path: home.join("Library/Caches"),
            label: "App caches",
            excluded_direct_children: &[],
        },
        SystemDataRoot {
            path: home.join("Library/Logs"),
            label: "App logs",
            excluded_direct_children: &[],
        },
        SystemDataRoot {
            path: home.join("Library/Application Support"),
            label: "App support data",
            excluded_direct_children: &[],
        },
        SystemDataRoot {
            path: home.join("Library/Containers"),
            label: "Sandboxed app data",
            excluded_direct_children: &[],
        },
        SystemDataRoot {
            path: home.join("Library/Group Containers"),
            label: "Shared sandboxed app data",
            excluded_direct_children: &[],
        },
        SystemDataRoot {
            path: home.join("Library/Developer"),
            label: "Developer artifacts",
            excluded_direct_children: &[],
        },
        SystemDataRoot {
            path: home.join("Library/MobileSync/Backup"),
            label: "iPhone and iPad backups",
            excluded_direct_children: &[],
        },
        SystemDataRoot {
            path: home.join("Library/Mail"),
            label: "Mail data",
            excluded_direct_children: &[],
        },
        SystemDataRoot {
            path: home.join("Library/Messages"),
            label: "Messages data",
            excluded_direct_children: &[],
        },
        SystemDataRoot {
            path: home.join("Library/CloudStorage"),
            label: "Cloud provider files",
            excluded_direct_children: &[],
        },
        SystemDataRoot {
            path: home.join("Library"),
            label: "Other user library data",
            excluded_direct_children: USER_LIBRARY_KNOWN,
        },
        SystemDataRoot {
            path: PathBuf::from("/Library/Caches"),
            label: "Shared app caches",
            excluded_direct_children: &[],
        },
        SystemDataRoot {
            path: PathBuf::from("/Library/Logs"),
            label: "Shared app logs",
            excluded_direct_children: &[],
        },
        SystemDataRoot {
            path: PathBuf::from("/Library/Application Support"),
            label: "Shared app support data",
            excluded_direct_children: &[],
        },
        SystemDataRoot {
            path: PathBuf::from("/Library/Developer"),
            label: "Shared developer artifacts",
            excluded_direct_children: &[],
        },
        SystemDataRoot {
            path: PathBuf::from("/Library"),
            label: "Other shared library data",
            excluded_direct_children: SHARED_LIBRARY_KNOWN,
        },
        SystemDataRoot {
            path: PathBuf::from("/private/var/folders"),
            label: "macOS temporary app data",
            excluded_direct_children: &[],
        },
        SystemDataRoot {
            path: PathBuf::from("/private/var/tmp"),
            label: "System temporary files",
            excluded_direct_children: &[],
        },
        SystemDataRoot {
            path: PathBuf::from("/private/var/log"),
            label: "System logs",
            excluded_direct_children: &[],
        },
        SystemDataRoot {
            path: PathBuf::from("/private/var/vm"),
            label: "Virtual memory and swap",
            excluded_direct_children: &[],
        },
        SystemDataRoot {
            path: PathBuf::from("/private/var/db"),
            label: "System databases",
            excluded_direct_children: &[],
        },
        SystemDataRoot {
            path: PathBuf::from("/private/var"),
            label: "Other system working data",
            excluded_direct_children: SYSTEM_WORKING_DATA_KNOWN,
        },
    ]
}

fn scan_report_root(
    report: &mut StorageReport,
    root: &SystemDataRoot,
    deep: bool,
    context: &OperationContext,
) -> Result<()> {
    context.cancellation.check()?;
    if !root.path.exists() {
        report
            .warnings
            .push(format!("Skipped missing location: {}", root.path.display()));
        return Ok(());
    }
    let scanned = scan_directory(
        &root.path,
        root.label,
        deep,
        context,
        &mut report.warnings,
        root.excluded_direct_children,
    )?;
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
    for root in system_data_roots(home) {
        let Ok(root) = fs::canonicalize(root.path) else {
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
    excluded_direct_children: &[&str],
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
                if is_excluded_direct_child(&path, root, excluded_direct_children) {
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

    let mut walker = WalkDir::new(root).follow_links(false).into_iter();
    while let Some(entry_result) = walker.next() {
        context.cancellation.check()?;
        completed += 1;
        let entry = match entry_result {
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
        if is_excluded_direct_child(&direct, root, excluded_direct_children) {
            if entry.depth() == 1 && entry.file_type().is_dir() {
                // Every skipped category is scanned separately. Do not descend into it here,
                // otherwise the top-level report would double-count storage.
                walker.skip_current_dir();
                continue;
            }
            continue;
        }
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

        if deep {
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

fn is_excluded_direct_child(path: &Path, root: &Path, excluded_direct_children: &[&str]) -> bool {
    path.parent() == Some(root)
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| excluded_direct_children.contains(&name))
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
    let category = classify_path(&path);
    let assessment = category.assessment();
    let (reason, recommendation) = guidance(category);
    let cleanup_allowed = cleanup_allowed_for_path(&path);
    StorageItem {
        label,
        path,
        size_bytes,
        size_known: true,
        is_directory,
        cleanup_allowed,
        category,
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

fn classify_path(path: &Path) -> StorageCategory {
    let normalized = path.to_string_lossy().to_lowercase();
    if normalized.contains("/private/var/vm") {
        return StorageCategory::VirtualMemory;
    }
    if normalized.contains("/private/var/db") {
        return StorageCategory::SystemDatabases;
    }
    if normalized.contains("/library/caches") {
        return StorageCategory::AppCaches;
    }
    if normalized.contains("/library/logs") || normalized.contains("/private/var/log") {
        return StorageCategory::AppLogs;
    }
    if normalized.contains("/private/var/folders") || normalized.contains("/private/var/tmp") {
        return StorageCategory::TemporaryFiles;
    }
    if normalized.contains("/developer") || normalized.contains("/coresimulator") {
        return StorageCategory::DeveloperArtifacts;
    }
    if normalized.contains("/mobilesync/backup") || normalized.contains("/backups") {
        return StorageCategory::DeviceBackups;
    }
    if normalized.contains("/group containers") || normalized.contains("/containers") {
        return StorageCategory::SandboxedAppData;
    }
    if normalized.contains("/application support") {
        return StorageCategory::AppSupportData;
    }
    if normalized.contains("/cloudstorage") {
        return StorageCategory::CloudFiles;
    }
    if normalized.contains("/messages") {
        return StorageCategory::MessagesData;
    }
    if normalized.contains("/mail") {
        return StorageCategory::MailData;
    }
    if normalized.contains("/photos") || normalized.contains("/music") {
        return StorageCategory::PersonalMedia;
    }
    if normalized.contains("/preferences") || normalized.contains("/saved application state") {
        return StorageCategory::AppSettings;
    }
    if normalized.starts_with("/system")
        || normalized.starts_with("/usr")
        || normalized.starts_with("/bin")
        || normalized.starts_with("/sbin")
        || normalized.contains("/launchdaemons")
        || normalized.contains("/launchagents")
        || normalized.contains("/private/var/root")
    {
        return StorageCategory::SystemManaged;
    }
    if normalized.starts_with("/private/var") {
        return StorageCategory::OtherSystemWorkingData;
    }
    StorageCategory::OtherData
}

fn guidance(category: StorageCategory) -> (&'static str, &'static str) {
    match category {
        StorageCategory::AppCaches => (
            "Recreatable cache files created by an app. They can be large, but clearing a cache may sign you out or make the app rebuild data.",
            "Inspect the largest app folder. Use the app's own clear-cache control first; FilePilot can move only direct user-cache folders to Trash.",
        ),
        StorageCategory::AppLogs => (
            "Diagnostic logs written by an app or macOS. Old logs are often reviewable, but active logs can help diagnose an issue.",
            "Review the owner and date. Remove only logs you no longer need; FilePilot can move direct user-log folders to Trash.",
        ),
        StorageCategory::TemporaryFiles => (
            "Temporary working files. macOS or an app may recreate them, and some disappear after a restart or after the owning app closes.",
            "Restart the owning app or Mac before removing anything. Do not delete files you cannot identify.",
        ),
        StorageCategory::DeveloperArtifacts => (
            "Developer tools, simulators, build products, or downloaded toolchains. These can be safely large when you actively use them.",
            "Use Xcode, Simulator, Docker, or the owning developer tool's cleanup controls so you keep active projects and required runtimes.",
        ),
        StorageCategory::DeviceBackups => (
            "Local iPhone or iPad backups. Removing one permanently removes that device backup.",
            "Review backup dates and devices in Finder or Apple Devices before deleting an obsolete backup.",
        ),
        StorageCategory::AppSupportData => (
            "Application data such as downloads, indexes, offline content, and local databases. It may be required for an app to work normally.",
            "Inspect the owning app and use its storage controls. Keep data you need for offline access, projects, or recovery.",
        ),
        StorageCategory::SandboxedAppData => (
            "Data stored inside an app sandbox or shared app container. It can include documents, databases, and offline files.",
            "Treat this as app data, not cache. Use the owning app to remove downloads or reset its data only when you understand the impact.",
        ),
        StorageCategory::AppSettings => (
            "Preferences and saved app state. These files are usually small but can reset apps if removed.",
            "Leave this in place unless you are deliberately troubleshooting a specific app and have its reset instructions.",
        ),
        StorageCategory::MailData => (
            "Local mail messages, attachments, and indexes.",
            "Manage attachments and mailboxes in Mail or your mail provider. Confirm sync and backups before removing anything.",
        ),
        StorageCategory::MessagesData => (
            "Messages history and attachments, which may include personal photos and videos.",
            "Manage attachments in Messages and confirm iCloud sync before deleting content.",
        ),
        StorageCategory::CloudFiles => (
            "Files managed by a cloud-storage provider. Local size can reflect downloaded copies, not the account's total size.",
            "Use the provider's app to make folders online-only or remove synced content; do not manually delete unknown container data.",
        ),
        StorageCategory::PersonalMedia => (
            "Personal media or libraries. This is not safe cache cleanup.",
            "Manage it in the owning app and ensure it is backed up before removing files.",
        ),
        StorageCategory::VirtualMemory => (
            "macOS swap and virtual-memory files. Their size changes automatically with memory pressure.",
            "Do not remove these files. Restarting the Mac is the safe way to release swap space when appropriate.",
        ),
        StorageCategory::SystemDatabases => (
            "Databases managed by macOS services. Manual deletion can break indexing, accounts, or system features.",
            "Leave this to macOS or use the supported tool for the specific service.",
        ),
        StorageCategory::SystemManaged => (
            "Protected or macOS-managed content. Its reported size can change automatically and manual removal can make the system unstable.",
            "Leave it to macOS. Use supported system settings or the owning system tool instead of deleting files directly.",
        ),
        StorageCategory::OtherSystemWorkingData => (
            "System working data that FilePilot cannot safely assign to a specific app or service.",
            "Inspect only to identify an owner. Do not delete it manually; use the owning app or macOS maintenance flow.",
        ),
        StorageCategory::OtherData => (
            "FilePilot cannot identify the owner from this path alone.",
            "Treat this as review-only. Inspect the largest child folders and identify the app or service before changing anything.",
        ),
    }
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
            StorageCategory::AppCaches
        );
        assert_eq!(
            classify_path(Path::new("/private/var/vm/swapfile0")),
            StorageCategory::VirtualMemory
        );
        assert_eq!(
            classify_path(Path::new("/Users/test/Library/Developer/Xcode")),
            StorageCategory::DeveloperArtifacts
        );
        assert_eq!(
            classify_path(Path::new("/Users/test/Library/Messages")),
            StorageCategory::MessagesData
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
            &[],
        )
        .expect("scan location");

        assert!(warnings.is_empty());
        assert_eq!(report.total_bytes, 10);
        assert_eq!(report.items.len(), 1);
        assert_eq!(report.items[0].children[0].size_bytes, 8);
        assert_eq!(report.items[0].children[1].size_bytes, 2);
    }

    #[test]
    fn system_data_roots_are_named_and_non_overlapping() {
        let home = Path::new("/Users/test");
        let roots = system_data_roots(home);
        let labels = roots.iter().map(|root| root.label).collect::<Vec<_>>();

        assert!(labels.contains(&"App caches"));
        assert!(labels.contains(&"iPhone and iPad backups"));
        assert!(labels.contains(&"Virtual memory and swap"));
        assert!(roots.iter().any(|root| {
            root.label == "Other user library data"
                && root.excluded_direct_children.contains(&"Caches")
                && root
                    .excluded_direct_children
                    .contains(&"Application Support")
        }));
    }

    #[test]
    fn excluded_direct_children_are_not_counted_in_the_parent_root() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let root = temporary.path().join("Library");
        fs::create_dir_all(root.join("Caches/Browser")).expect("create cache folder");
        fs::create_dir_all(root.join("Preferences")).expect("create preferences folder");
        fs::write(root.join("Caches/Browser/data"), [1_u8; 5]).expect("write cache file");
        fs::write(root.join("Preferences/data"), [2_u8; 3]).expect("write preferences file");
        let mut warnings = Vec::new();

        let report = scan_directory(
            &root,
            "Other user library data",
            true,
            &OperationContext::default(),
            &mut warnings,
            &["Caches"],
        )
        .expect("scan filtered location");

        assert!(warnings.is_empty());
        assert_eq!(report.total_bytes, 3);
        assert_eq!(report.items.len(), 1);
        assert_eq!(report.items[0].path, root.join("Preferences"));
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
