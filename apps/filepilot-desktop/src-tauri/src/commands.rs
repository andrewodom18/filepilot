use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

use filepilot_app::{
    analyze_system_data, analyze_system_data_location, cleanup_system_data_path, AppError,
    AppSettings, SettingsStore, TaskId, TaskKind, TaskOutput, TaskRegistry, TaskSnapshot,
};
use filepilot_core::{
    apply_operation_with_context, build_organize_plan_with_context, build_rename_plan_with_context,
    clean_images_with_context, duplicate_files_with_context, large_files_with_context,
    list_operations as core_list_operations, scan_with_context, OperationContext, OperationPlan,
    OrganizeBy, RenameOptions, ScanOptions,
};
use serde::Deserialize;
use tauri::{AppHandle, Emitter, State};

pub struct AppState {
    pub tasks: TaskRegistry,
    pub settings: Mutex<SettingsStore>,
}

impl AppState {
    pub fn new() -> Result<Self, AppError> {
        Ok(Self {
            tasks: TaskRegistry::default(),
            settings: Mutex::new(SettingsStore::new()?),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRequest {
    pub path: PathBuf,
    #[serde(default)]
    pub options: ScanOptions,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeFilesRequest {
    pub path: PathBuf,
    #[serde(default)]
    pub options: ScanOptions,
    pub top: usize,
    pub min_size: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicatesRequest {
    pub path: PathBuf,
    #[serde(default)]
    pub options: ScanOptions,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemDataRequest {
    #[serde(default = "default_true")]
    pub deep: bool,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupSystemDataRequest {
    pub path: PathBuf,
    pub expected_size: u64,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreviewRequest {
    pub path: PathBuf,
    #[serde(default)]
    pub scan_options: ScanOptions,
    pub options: RenameOptions,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizePreviewRequest {
    pub path: PathBuf,
    #[serde(default)]
    pub scan_options: ScanOptions,
    pub organize_by: OrganizeBy,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataPreviewRequest {
    pub path: PathBuf,
    pub output_directory: Option<PathBuf>,
    #[serde(default)]
    pub scan_options: ScanOptions,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOperationRequest {
    pub plan: OperationPlan,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyMetadataRequest {
    pub path: PathBuf,
    pub output_directory: Option<PathBuf>,
    #[serde(default)]
    pub scan_options: ScanOptions,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoRequest {
    pub operation_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRequest {
    pub task_id: TaskId,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSettingsRequest {
    pub settings: AppSettings,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RememberPathRequest {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Json,
    Csv,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReportRequest {
    pub output: TaskOutput,
    pub path: PathBuf,
    pub format: ExportFormat,
}

fn notifier(app: AppHandle) -> Arc<dyn Fn(TaskSnapshot) + Send + Sync> {
    Arc::new(move |snapshot| {
        let _ = app.emit("task-updated", snapshot);
    })
}

fn start_task<F>(
    app: AppHandle,
    state: &AppState,
    kind: TaskKind,
    work: F,
) -> Result<TaskId, String>
where
    F: FnOnce(OperationContext) -> filepilot_core::Result<TaskOutput> + Send + 'static,
{
    let handle = state.tasks.start(kind, notifier(app));
    let task_id = handle.task_id.clone();
    let worker_task_id = task_id.clone();
    let registry = state.tasks.clone();
    let cancellation = handle.cancellation.clone();
    let reporter = Arc::new(handle.reporter);

    thread::spawn(move || {
        let context = OperationContext {
            cancellation,
            reporter,
        };
        match work(context) {
            Ok(output) => registry.complete(&worker_task_id, output),
            Err(error) => registry.fail(
                &worker_task_id,
                error.to_string(),
                matches!(error, filepilot_core::FilePilotError::Cancelled),
            ),
        }
    });

    Ok(task_id)
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    state
        .settings
        .lock()
        .map_err(|_| "settings lock is unavailable".to_string())?
        .load()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_settings(
    request: SaveSettingsRequest,
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    state
        .settings
        .lock()
        .map_err(|_| "settings lock is unavailable".to_string())?
        .save(&request.settings)
        .map_err(|error| error.to_string())?;
    Ok(request.settings)
}

#[tauri::command]
pub fn remember_path(
    request: RememberPathRequest,
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    state
        .settings
        .lock()
        .map_err(|_| "settings lock is unavailable".to_string())?
        .remember(request.path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn start_scan(
    request: ScanRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TaskId, String> {
    start_task(app, &state, TaskKind::Scan, move |context| {
        Ok(TaskOutput::Scan(scan_with_context(
            request.path,
            &request.options,
            &context,
        )?))
    })
}

#[tauri::command]
pub fn start_large_files(
    request: LargeFilesRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TaskId, String> {
    start_task(app, &state, TaskKind::LargeFiles, move |context| {
        let scan = scan_with_context(&request.path, &request.options, &context)?;
        let entries = large_files_with_context(&scan, request.top, request.min_size, &context);
        context.cancellation.check()?;
        Ok(TaskOutput::LargeFiles {
            entries,
            warnings: scan.warnings,
        })
    })
}

#[tauri::command]
pub fn start_duplicates(
    request: DuplicatesRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TaskId, String> {
    start_task(app, &state, TaskKind::Duplicates, move |context| {
        let scan = scan_with_context(&request.path, &request.options, &context)?;
        let groups = duplicate_files_with_context(&scan, &context)?;
        Ok(TaskOutput::Duplicates {
            groups,
            warnings: scan.warnings,
        })
    })
}

#[tauri::command]
pub fn start_system_data(
    request: SystemDataRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TaskId, String> {
    start_task(app, &state, TaskKind::SystemData, move |context| {
        let report = match request.path {
            Some(path) => analyze_system_data_location(&context, &path, request.deep)?,
            None => analyze_system_data(&context, request.deep)?,
        };
        Ok(TaskOutput::SystemData(report))
    })
}

#[tauri::command]
pub fn cleanup_system_data(
    request: CleanupSystemDataRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TaskId, String> {
    start_task(app, &state, TaskKind::CleanupSystemData, move |context| {
        Ok(TaskOutput::Cleanup(cleanup_system_data_path(
            &context,
            &request.path,
            request.expected_size,
        )?))
    })
}

#[tauri::command]
pub fn preview_rename(
    request: RenamePreviewRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TaskId, String> {
    start_task(app, &state, TaskKind::RenamePreview, move |context| {
        Ok(TaskOutput::OperationPlan(build_rename_plan_with_context(
            request.path,
            &request.scan_options,
            &request.options,
            &context,
        )?))
    })
}

#[tauri::command]
pub fn preview_organize(
    request: OrganizePreviewRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TaskId, String> {
    start_task(app, &state, TaskKind::OrganizePreview, move |context| {
        Ok(TaskOutput::OperationPlan(build_organize_plan_with_context(
            request.path,
            &request.scan_options,
            request.organize_by,
            &context,
        )?))
    })
}

#[tauri::command]
pub fn preview_metadata(
    request: MetadataPreviewRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TaskId, String> {
    start_task(app, &state, TaskKind::MetadataPreview, move |context| {
        Ok(TaskOutput::Metadata(clean_images_with_context(
            request.path,
            request.output_directory.as_deref(),
            &request.scan_options,
            true,
            &context,
        )?))
    })
}

#[tauri::command]
pub fn apply_operation(
    request: ApplyOperationRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TaskId, String> {
    start_task(app, &state, TaskKind::ApplyOperation, move |context| {
        Ok(TaskOutput::Operation(apply_operation_with_context(
            &request.plan,
            request.dry_run,
            &context,
        )?))
    })
}

#[tauri::command]
pub fn apply_metadata(
    request: ApplyMetadataRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TaskId, String> {
    start_task(app, &state, TaskKind::ApplyMetadata, move |context| {
        Ok(TaskOutput::Metadata(clean_images_with_context(
            request.path,
            request.output_directory.as_deref(),
            &request.scan_options,
            false,
            &context,
        )?))
    })
}

#[tauri::command]
pub fn undo(
    request: UndoRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TaskId, String> {
    start_task(app, &state, TaskKind::Undo, move |context| {
        Ok(TaskOutput::Operation(
            filepilot_core::undo_operation_with_context(request.operation_id.as_deref(), &context)?,
        ))
    })
}

#[tauri::command]
pub fn get_task(
    request: TaskRequest,
    state: State<'_, AppState>,
) -> Result<Option<TaskSnapshot>, String> {
    Ok(state.tasks.snapshot(&request.task_id))
}

#[tauri::command]
pub fn cancel_task(request: TaskRequest, state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.tasks.cancel(&request.task_id))
}

#[tauri::command]
pub fn list_operations() -> Result<Vec<filepilot_core::OperationRecord>, String> {
    core_list_operations().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn export_report(request: ExportReportRequest) -> Result<(), String> {
    let bytes = match request.format {
        ExportFormat::Json => {
            serde_json::to_vec_pretty(&request.output).map_err(|error| error.to_string())?
        }
        ExportFormat::Csv => csv_bytes(&request.output)?,
    };
    fs::write(request.path, bytes).map_err(|error| error.to_string())
}

fn csv_bytes(output: &TaskOutput) -> Result<Vec<u8>, String> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    match output {
        TaskOutput::Scan(scan) => {
            writer
                .write_record([
                    "path",
                    "relative_path",
                    "size_bytes",
                    "extension",
                    "modified_at",
                    "is_symlink",
                    "is_hidden",
                ])
                .map_err(|error| error.to_string())?;
            for file in &scan.files {
                writer
                    .write_record([
                        file.path.to_string_lossy().as_ref(),
                        file.relative_path.to_string_lossy().as_ref(),
                        &file.size_bytes.to_string(),
                        file.extension.as_deref().unwrap_or_default(),
                        file.modified_at
                            .map(|date| date.to_rfc3339())
                            .unwrap_or_default()
                            .as_str(),
                        &file.is_symlink.to_string(),
                        &file.is_hidden.to_string(),
                    ])
                    .map_err(|error| error.to_string())?;
            }
        }
        TaskOutput::LargeFiles { entries, .. } => {
            writer
                .write_record(["path", "size_bytes"])
                .map_err(|error| error.to_string())?;
            for entry in entries {
                writer
                    .write_record([
                        entry.path.to_string_lossy().as_ref(),
                        &entry.size_bytes.to_string(),
                    ])
                    .map_err(|error| error.to_string())?;
            }
        }
        TaskOutput::Duplicates { groups, .. } => {
            writer
                .write_record([
                    "hash",
                    "size_bytes",
                    "recommended_primary",
                    "reclaimable_bytes",
                    "paths",
                ])
                .map_err(|error| error.to_string())?;
            for group in groups {
                let paths = group
                    .paths
                    .iter()
                    .map(|path| path.to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join("|");
                writer
                    .write_record([
                        group.hash.as_str(),
                        &group.size_bytes.to_string(),
                        group.recommended_primary.to_string_lossy().as_ref(),
                        &group.reclaimable_bytes.to_string(),
                        paths.as_str(),
                    ])
                    .map_err(|error| error.to_string())?;
            }
        }
        TaskOutput::SystemData(report) => {
            writer
                .write_record([
                    "label",
                    "path",
                    "size_bytes",
                    "size_known",
                    "assessment",
                    "reason",
                    "recommendation",
                ])
                .map_err(|error| error.to_string())?;
            for item in &report.items {
                write_storage_csv_row(&mut writer, item)?;
            }
        }
        _ => {
            return Err(
                "CSV export is supported for scan, large-file, duplicate, and System Data reports"
                    .into(),
            )
        }
    }
    writer.into_inner().map_err(|error| error.to_string())
}

fn write_storage_csv_row(
    writer: &mut csv::Writer<Vec<u8>>,
    item: &filepilot_app::StorageItem,
) -> Result<(), String> {
    writer
        .write_record([
            item.label.as_str(),
            item.path.to_string_lossy().as_ref(),
            &item.size_bytes.to_string(),
            &item.size_known.to_string(),
            &serde_json::to_string(&item.assessment).map_err(|error| error.to_string())?,
            item.reason.as_str(),
            item.recommendation.as_str(),
        ])
        .map_err(|error| error.to_string())?;
    for child in &item.children {
        write_storage_csv_row(writer, child)?;
    }
    Ok(())
}
