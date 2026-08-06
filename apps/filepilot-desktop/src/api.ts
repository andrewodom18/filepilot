import { invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  OperationPlan,
  ScanOptions,
  TaskId,
  TaskOutput,
  TaskSnapshot,
} from "./types";

export const api = {
  getSettings: () => invoke<AppSettings>("get_settings"),
  saveSettings: (settings: AppSettings) =>
    invoke<AppSettings>("save_settings", { request: { settings } }),
  rememberPath: (path: string) =>
    invoke<AppSettings>("remember_path", { request: { path } }),
  startScan: (path: string, options: ScanOptions) =>
    invoke<TaskId>("start_scan", { request: { path, options } }),
  startLargeFiles: (path: string, options: ScanOptions, top: number, minSize: number) =>
    invoke<TaskId>("start_large_files", {
      request: { path, options, top, minSize },
    }),
  startDuplicates: (path: string, options: ScanOptions) =>
    invoke<TaskId>("start_duplicates", { request: { path, options } }),
  startSystemData: (deep = true, path?: string) =>
    invoke<TaskId>("start_system_data", { request: { deep, path } }),
  cleanupSystemData: (path: string, expectedSize: number) =>
    invoke<TaskId>("cleanup_system_data", { request: { path, expectedSize } }),
  previewRename: (path: string, scanOptions: ScanOptions, options: object) =>
    invoke<TaskId>("preview_rename", {
      request: { path, scanOptions, options },
    }),
  previewOrganize: (path: string, scanOptions: ScanOptions, organizeBy: string) =>
    invoke<TaskId>("preview_organize", {
      request: { path, scanOptions, organizeBy },
    }),
  previewMetadata: (path: string, outputDirectory: string | undefined, scanOptions: ScanOptions) =>
    invoke<TaskId>("preview_metadata", {
      request: { path, outputDirectory, scanOptions },
    }),
  applyOperation: (plan: OperationPlan, dryRun = false) =>
    invoke<TaskId>("apply_operation", { request: { plan, dryRun } }),
  applyMetadata: (path: string, outputDirectory: string | undefined, scanOptions: ScanOptions) =>
    invoke<TaskId>("apply_metadata", {
      request: { path, outputDirectory, scanOptions },
    }),
  undo: (operationId?: string) =>
    invoke<TaskId>("undo", { request: { operationId } }),
  getTask: (taskId: string) => invoke<TaskSnapshot | null>("get_task", { request: { taskId } }),
  cancelTask: (taskId: string) => invoke<boolean>("cancel_task", { request: { taskId } }),
  listOperations: () => invoke<import("./types").OperationRecord[]>("list_operations"),
  exportReport: (output: TaskOutput, path: string, format: "json" | "csv") =>
    invoke<void>("export_report", { request: { output, path, format } }),
};
