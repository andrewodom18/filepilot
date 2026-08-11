use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use filepilot_core::{
    CancellationToken, CleanMetadataResult, DuplicateGroup, LargeFileEntry, OperationPlan,
    OperationRecord, OperationResult, ProgressEvent, ProgressReporter, ScanResult, ScanWarning,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

mod duplicates;
mod system_data;
mod trash_support;

pub use duplicates::{
    cleanup_duplicate_files, DuplicateCleanupCandidate, DuplicateCleanupFailure,
    DuplicateCleanupItem, DuplicateCleanupResult,
};

pub use system_data::{
    analyze_system_data, analyze_system_data_location, cleanup_system_data_path, CleanupResult,
    StorageAssessment, StorageCategory, StorageItem, StorageReport, VolumeInfo,
};

pub type TaskId = String;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("platform application directories are unavailable")]
    PlatformDirectories,

    #[error("settings I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("settings serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub recent_paths: Vec<PathBuf>,
    #[serde(default)]
    pub full_disk_access_setup_complete: bool,
}

pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new() -> Result<Self, AppError> {
        let directories = directories::ProjectDirs::from("com", "FilePilot", "FilePilot")
            .ok_or(AppError::PlatformDirectories)?;
        Ok(Self {
            path: directories.config_dir().join("settings.json"),
        })
    }

    pub fn load(&self) -> Result<AppSettings, AppError> {
        match fs::read(&self.path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(AppSettings::default())
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn save(&self, settings: &AppSettings) -> Result<(), AppError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(settings)?)?;
        fs::rename(temporary, &self.path)?;
        Ok(())
    }

    pub fn remember(&self, path: PathBuf) -> Result<AppSettings, AppError> {
        let mut settings = self.load()?;
        settings.recent_paths.retain(|recent| recent != &path);
        settings.recent_paths.insert(0, path);
        settings.recent_paths.truncate(8);
        self.save(&settings)?;
        Ok(settings)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskKind {
    Scan,
    LargeFiles,
    Duplicates,
    CleanupDuplicates,
    SystemData,
    CleanupSystemData,
    RenamePreview,
    OrganizePreview,
    MetadataPreview,
    ApplyOperation,
    ApplyMetadata,
    Undo,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProgress {
    pub phase: String,
    pub completed: u64,
    pub total: Option<u64>,
    pub current_path: Option<PathBuf>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum TaskOutput {
    Scan(ScanResult),
    LargeFiles {
        entries: Vec<LargeFileEntry>,
        warnings: Vec<ScanWarning>,
    },
    Duplicates {
        groups: Vec<DuplicateGroup>,
        warnings: Vec<ScanWarning>,
    },
    DuplicateCleanup(DuplicateCleanupResult),
    SystemData(StorageReport),
    Cleanup(CleanupResult),
    OperationPlan(OperationPlan),
    Metadata(CleanMetadataResult),
    Operation(OperationResult),
    History(Vec<OperationRecord>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSnapshot {
    pub task_id: TaskId,
    pub kind: TaskKind,
    pub status: TaskStatus,
    pub progress: Option<TaskProgress>,
    pub error: Option<String>,
    pub output: Option<TaskOutput>,
}

struct TaskEntry {
    snapshot: TaskSnapshot,
    cancellation: CancellationToken,
    notifier: Arc<dyn Fn(TaskSnapshot) + Send + Sync>,
}

#[derive(Clone, Default)]
pub struct TaskRegistry {
    tasks: Arc<Mutex<HashMap<TaskId, TaskEntry>>>,
}

pub struct TaskHandle {
    pub task_id: TaskId,
    pub cancellation: CancellationToken,
    pub reporter: TaskReporter,
}

#[derive(Clone)]
pub struct TaskReporter {
    registry: TaskRegistry,
    task_id: TaskId,
}

impl TaskRegistry {
    pub fn start(
        &self,
        kind: TaskKind,
        notifier: Arc<dyn Fn(TaskSnapshot) + Send + Sync>,
    ) -> TaskHandle {
        let task_id = Uuid::new_v4().to_string();
        let cancellation = CancellationToken::default();
        let snapshot = TaskSnapshot {
            task_id: task_id.clone(),
            kind,
            status: TaskStatus::Running,
            progress: None,
            error: None,
            output: None,
        };
        let entry = TaskEntry {
            snapshot: snapshot.clone(),
            cancellation: cancellation.clone(),
            notifier: notifier.clone(),
        };
        self.tasks
            .lock()
            .expect("task registry poisoned")
            .insert(task_id.clone(), entry);
        notifier(snapshot);

        TaskHandle {
            task_id: task_id.clone(),
            cancellation,
            reporter: TaskReporter {
                registry: self.clone(),
                task_id,
            },
        }
    }

    pub fn snapshot(&self, task_id: &str) -> Option<TaskSnapshot> {
        self.tasks
            .lock()
            .expect("task registry poisoned")
            .get(task_id)
            .map(|entry| entry.snapshot.clone())
    }

    pub fn cancel(&self, task_id: &str) -> bool {
        let tasks = self.tasks.lock().expect("task registry poisoned");
        let entry = tasks.get(task_id);
        if let Some(entry) =
            entry.filter(|entry| matches!(entry.snapshot.status, TaskStatus::Running))
        {
            entry.cancellation.cancel();
            true
        } else {
            false
        }
    }

    pub fn complete(&self, task_id: &str, output: TaskOutput) {
        self.update(task_id, |snapshot| {
            snapshot.status = TaskStatus::Completed;
            snapshot.output = Some(output);
        });
    }

    pub fn fail(&self, task_id: &str, error: String, cancelled: bool) {
        self.update(task_id, |snapshot| {
            snapshot.status = if cancelled {
                TaskStatus::Cancelled
            } else {
                TaskStatus::Failed
            };
            snapshot.error = Some(error);
        });
    }

    fn report(&self, task_id: &str, event: ProgressEvent) {
        self.update(task_id, |snapshot| {
            snapshot.progress = Some(TaskProgress {
                phase: event.phase,
                completed: event.completed,
                total: event.total,
                current_path: event.current_path,
                message: event.message,
            });
        });
    }

    fn update(&self, task_id: &str, update: impl FnOnce(&mut TaskSnapshot)) {
        let notification = {
            let mut tasks = self.tasks.lock().expect("task registry poisoned");
            let Some(entry) = tasks.get_mut(task_id) else {
                return;
            };
            update(&mut entry.snapshot);
            (entry.notifier.clone(), entry.snapshot.clone())
        };
        (notification.0)(notification.1);
    }
}

impl ProgressReporter for TaskReporter {
    fn report(&self, event: ProgressEvent) {
        self.registry.report(&self.task_id, event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn task_registry_reports_progress_and_cancellation() {
        let registry = TaskRegistry::default();
        let notifications = Arc::new(AtomicUsize::new(0));
        let callback_notifications = notifications.clone();
        let handle = registry.start(
            TaskKind::Scan,
            Arc::new(move |_snapshot| {
                callback_notifications.fetch_add(1, Ordering::Relaxed);
            }),
        );

        handle.reporter.report(ProgressEvent {
            phase: "scanning".into(),
            completed: 2,
            total: Some(4),
            current_path: Some(PathBuf::from("example.txt")),
            message: None,
        });
        assert_eq!(
            registry
                .snapshot(&handle.task_id)
                .unwrap()
                .progress
                .unwrap()
                .completed,
            2
        );
        assert!(registry.cancel(&handle.task_id));
        assert!(handle.cancellation.is_cancelled());
        assert!(notifications.load(Ordering::Relaxed) >= 2);
    }
}
