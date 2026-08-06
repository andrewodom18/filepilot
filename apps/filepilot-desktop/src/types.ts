export type Theme = "system" | "light" | "dark";
export type TaskId = string;

export interface AppSettings {
  theme: Theme;
  recentPaths: string[];
}

export interface ScanOptions {
  recursive: boolean;
  include_hidden: boolean;
  follow_symlinks: boolean;
  excludes: string[];
}

export interface FileRecord {
  path: string;
  relative_path: string;
  size_bytes: number;
  extension?: string;
  modified_at?: string;
  is_symlink: boolean;
  is_hidden: boolean;
}

export interface ScanWarning {
  path?: string;
  kind: string;
  message: string;
}

export interface ScanResult {
  root: string;
  files: FileRecord[];
  warnings: ScanWarning[];
}

export interface LargeFileEntry {
  path: string;
  size_bytes: number;
}

export interface DuplicateGroup {
  hash: string;
  size_bytes: number;
  paths: string[];
  recommended_primary: string;
  reclaimable_bytes: number;
}

export interface OperationAction {
  source: string;
  destination: string;
  size_bytes: number;
}

export interface OperationPlan {
  id: string;
  kind: "Rename" | "Organize";
  created_at: string;
  root: string;
  actions: OperationAction[];
  skipped: string[];
}

export interface OperationResult {
  operation_id: string;
  kind: "Rename" | "Organize";
  dry_run: boolean;
  actions: OperationAction[];
  skipped: string[];
  warnings: string[];
}

export interface CleanedImage {
  source: string;
  destination: string;
  format: string;
}

export interface CleanMetadataResult {
  cleaned: CleanedImage[];
  warnings: string[];
}

export type StorageAssessment =
  | "likely-safe-to-review"
  | "review-before-removing"
  | "user-data"
  | "system-managed"
  | "unknown";

export interface StorageItem {
  label: string;
  path: string;
  sizeBytes: number;
  sizeKnown: boolean;
  isDirectory: boolean;
  cleanupAllowed: boolean;
  assessment: StorageAssessment;
  reason: string;
  recommendation: string;
  children: StorageItem[];
}

export interface StorageReport {
  platform: string;
  generatedAt: string;
  totalBytes: number;
  scopePath?: string;
  volume?: VolumeInfo;
  items: StorageItem[];
  warnings: string[];
  notes: string[];
}

export interface VolumeInfo {
  usedBytes?: number;
  totalBytes?: number;
  freeBytes?: number;
  apfsSnapshotCount: number;
}

export interface CleanupResult {
  path: string;
  sizeBytes: number;
  destination: string;
}

export interface OperationRecord {
  id: string;
  kind: "Rename" | "Organize";
  created_at: string;
  completed_at: string;
  root: string;
  actions: OperationAction[];
  undone_at?: string;
}

export interface TaskProgress {
  phase: string;
  completed: number;
  total?: number;
  currentPath?: string;
  message?: string;
}

export type TaskKind =
  | "scan"
  | "large-files"
  | "duplicates"
  | "system-data"
  | "cleanup-system-data"
  | "rename-preview"
  | "organize-preview"
  | "metadata-preview"
  | "apply-operation"
  | "apply-metadata"
  | "undo";

export type TaskStatus = "running" | "completed" | "failed" | "cancelled";

export type TaskOutput =
  | { type: "scan"; data: ScanResult }
  | { type: "largeFiles"; data: { entries: LargeFileEntry[]; warnings: ScanWarning[] } }
  | { type: "duplicates"; data: { groups: DuplicateGroup[]; warnings: ScanWarning[] } }
  | { type: "systemData"; data: StorageReport }
  | { type: "cleanup"; data: CleanupResult }
  | { type: "operationPlan"; data: OperationPlan }
  | { type: "metadata"; data: CleanMetadataResult }
  | { type: "operation"; data: OperationResult }
  | { type: "history"; data: OperationRecord[] };

export interface TaskSnapshot {
  taskId: string;
  kind: TaskKind;
  status: TaskStatus;
  progress?: TaskProgress;
  error?: string;
  output?: TaskOutput;
}

export const DEFAULT_SCAN_OPTIONS: ScanOptions = {
  recursive: true,
  include_hidden: false,
  follow_symlinks: false,
  excludes: [],
};

export type ModuleId =
  | "overview"
  | "scan"
  | "large-files"
  | "duplicates"
  | "system-data"
  | "rename"
  | "organize"
  | "metadata"
  | "activity"
  | "settings";
