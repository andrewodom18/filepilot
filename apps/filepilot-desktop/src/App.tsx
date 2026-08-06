import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api } from "./api";
import { StatusPill } from "./StatusPill";
import type {
  AppSettings,
  CleanMetadataResult,
  DuplicateGroup,
  ModuleId,
  OperationPlan,
  OperationRecord,
  ScanOptions,
  ScanResult,
  StorageItem,
  TaskKind,
  TaskOutput,
  TaskSnapshot,
  Theme,
} from "./types";
import { DEFAULT_SCAN_OPTIONS as defaultScanOptions } from "./types";
import "./styles.css";

const modules: { id: ModuleId; label: string; icon: string }[] = [
  { id: "overview", label: "Overview", icon: "⌂" },
  { id: "scan", label: "Scan files", icon: "⌕" },
  { id: "large-files", label: "Large files", icon: "▤" },
  { id: "duplicates", label: "Duplicates", icon: "◈" },
  { id: "system-data", label: "System Data", icon: "◒" },
  { id: "rename", label: "Rename", icon: "✎" },
  { id: "organize", label: "Organize", icon: "↗" },
  { id: "metadata", label: "Clean metadata", icon: "◇" },
  { id: "activity", label: "Activity", icon: "◷" },
  { id: "settings", label: "Settings", icon: "⚙" },
];

type Outputs = Partial<Record<ModuleId, TaskOutput>>;

function moduleForTask(kind: TaskKind): ModuleId {
  if (kind === "scan") return "scan";
  if (kind === "large-files") return "large-files";
  if (kind === "duplicates") return "duplicates";
  if (kind === "system-data") return "system-data";
  if (kind === "rename-preview" || kind === "apply-operation") return "rename";
  if (kind === "organize-preview") return "organize";
  if (kind === "metadata-preview" || kind === "apply-metadata") return "metadata";
  return "activity";
}

export default function App() {
  const [activeModule, setActiveModule] = useState<ModuleId>("overview");
  const [selectedPath, setSelectedPath] = useState("");
  const [scanOptions, setScanOptions] = useState<ScanOptions>(defaultScanOptions);
  const [settings, setSettings] = useState<AppSettings>({ theme: "system", recentPaths: [] });
  const [tasks, setTasks] = useState<Record<string, TaskSnapshot>>({});
  const [outputs, setOutputs] = useState<Outputs>({});
  const [renamePlan, setRenamePlan] = useState<OperationPlan | undefined>();
  const [organizePlan, setOrganizePlan] = useState<OperationPlan | undefined>();
  const [metadataPreview, setMetadataPreview] = useState<CleanMetadataResult | undefined>();
  const [activeTaskId, setActiveTaskId] = useState<string>();
  const activeTaskRef = useRef<string | undefined>(undefined);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  useEffect(() => {
    api.getSettings().then((loaded) => {
      setSettings(loaded);
      if (loaded.recentPaths[0]) setSelectedPath(loaded.recentPaths[0]);
    }).catch((reason) => setError(String(reason)));

    let unlisten: (() => void) | undefined;
    listen<TaskSnapshot>("task-updated", ({ payload }) => {
      setTasks((previous) => ({ ...previous, [payload.taskId]: payload }));
      if (payload.output) {
        setOutputs((previous) => ({ ...previous, [moduleForTask(payload.kind)]: payload.output }));
        if (payload.output.type === "operationPlan") {
          if (payload.output.data.kind === "Rename") setRenamePlan(payload.output.data);
          if (payload.output.data.kind === "Organize") setOrganizePlan(payload.output.data);
        }
        if (payload.output.type === "metadata" && payload.kind === "metadata-preview") {
          setMetadataPreview(payload.output.data);
        }
      }
      if (payload.taskId === activeTaskRef.current && payload.status !== "running") {
        if (payload.status === "completed") setNotice("Task completed");
        if (payload.status === "cancelled") setNotice("Task cancelled safely");
        if (payload.status === "failed") setError(payload.error ?? "Task failed");
      }
    }).then((cleanup) => { unlisten = cleanup; }).catch((reason) => setError(String(reason)));

    return () => unlisten?.();
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = settings.theme;
  }, [settings.theme]);

  useEffect(() => {
    const window = getCurrentWebviewWindow();
    let unlisten: (() => void) | undefined;
    window.onDragDropEvent((event) => {
      if (event.payload.type === "drop" && event.payload.paths[0]) {
        selectPath(event.payload.paths[0]);
      }
    }).then((cleanup) => { unlisten = cleanup; }).catch(() => undefined);
    return () => unlisten?.();
  }, []);

  const activeTask = activeTaskId ? tasks[activeTaskId] : undefined;
  const runningTasks = useMemo(
    () => Object.values(tasks).filter((task) => task.status === "running"),
    [tasks],
  );

  function selectPath(path: string) {
    setSelectedPath(path);
    setError("");
    if (path) api.rememberPath(path).then(setSettings).catch(() => undefined);
  }

  async function pickFolder() {
    const selected = await open({ directory: true, multiple: false, title: "Choose a folder" });
    if (typeof selected === "string") selectPath(selected);
  }

  async function run(start: () => Promise<string>) {
    setError("");
    setNotice("");
    try {
      const taskId = await start();
      activeTaskRef.current = taskId;
      setActiveTaskId(taskId);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function cancelActive(taskId = activeTaskId) {
    if (!taskId) return;
    await api.cancelTask(taskId).catch((reason) => setError(String(reason)));
  }

  async function exportOutput(output: TaskOutput, format: "json" | "csv") {
    const path = await save({
      title: `Export ${format.toUpperCase()} report`,
      defaultPath: `filepilot-report.${format}`,
      filters: [{ name: format.toUpperCase(), extensions: [format] }],
    });
    if (!path) return;
    await api.exportReport(output, path, format).then(() => setNotice(`Saved ${path}`)).catch((reason) => setError(String(reason)));
  }

  function updateScanOptions(update: Partial<ScanOptions>) {
    setScanOptions((previous) => ({ ...previous, ...update }));
  }

  function renderModule() {
    const common = { selectedPath, pickFolder, selectPath, scanOptions, updateScanOptions, run, outputs, exportOutput };
    switch (activeModule) {
      case "scan": return <ScanPage {...common} />;
      case "large-files": return <LargeFilesPage {...common} />;
      case "duplicates": return <DuplicatesPage {...common} />;
      case "system-data": return <SystemDataPage {...common} />;
      case "rename": return <RenamePage {...common} plan={renamePlan} setPlan={setRenamePlan} />;
      case "organize": return <OrganizePage {...common} plan={organizePlan} setPlan={setOrganizePlan} />;
      case "metadata": return <MetadataPage {...common} preview={metadataPreview} />;
      case "activity": return <ActivityPage tasks={tasks} runningTasks={runningTasks} run={run} onCancel={cancelActive} />;
      case "settings": return <SettingsPage settings={settings} setSettings={setSettings} setNotice={setNotice} setError={setError} />;
      default: return <OverviewPage {...common} setActiveModule={setActiveModule} lastScan={outputs.scan?.type === "scan" ? outputs.scan.data : undefined} />;
    }
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand"><span className="brand-mark">✦</span><span>FilePilot</span><small>2.0.0</small></div>
        <nav aria-label="Main navigation">
          {modules.map((item) => (
            <button key={item.id} className={`nav-item ${activeModule === item.id ? "active" : ""}`} onClick={() => setActiveModule(item.id)}>
              <span className="nav-icon" aria-hidden="true">{item.icon}</span><span>{item.label}</span>
            </button>
          ))}
        </nav>
        <div className="sidebar-footer"><span className="status-dot" /> Local only<br /><span className="muted">No account · No telemetry</span></div>
      </aside>
      <main className="main-content">
        <header className="topbar">
          <div><p className="eyebrow">FILE CONTROL CENTER</p><h1>{modules.find((item) => item.id === activeModule)?.label}</h1></div>
          <div className="topbar-actions"><span className="folder-pill" title={selectedPath || "No folder selected"}>◉ {selectedPath || "Select a folder to begin"}</span><button className="button secondary" onClick={pickFolder}>Choose folder</button></div>
        </header>
        {error && <div className="alert error" role="alert"><strong>Something needs attention</strong><span>{error}</span><button onClick={() => setError("")} aria-label="Dismiss error">×</button></div>}
        {notice && <div className="alert success" role="status"><span>{notice}</span><button onClick={() => setNotice("")} aria-label="Dismiss notice">×</button></div>}
        {activeTask?.status === "running" && <TaskBanner task={activeTask} onCancel={cancelActive} />}
        <div className="content-wrap">{renderModule()}</div>
      </main>
    </div>
  );
}

interface CommonProps {
  selectedPath: string;
  pickFolder: () => Promise<void>;
  selectPath: (path: string) => void;
  scanOptions: ScanOptions;
  updateScanOptions: (update: Partial<ScanOptions>) => void;
  run: (start: () => Promise<string>) => Promise<void>;
  outputs: Outputs;
  exportOutput: (output: TaskOutput, format: "json" | "csv") => Promise<void>;
}

function PageIntro({ title, description, children }: { title: string; description: string; children?: React.ReactNode }) {
  return <div className="page-intro"><div><p className="eyebrow">WORKFLOW</p><h2>{title}</h2><p>{description}</p></div>{children}</div>;
}

function FolderBar({ selectedPath, pickFolder, selectPath }: Pick<CommonProps, "selectedPath" | "pickFolder" | "selectPath">) {
  return <section className={`folder-bar ${selectedPath ? "has-path" : ""}`}><div className="folder-icon">⌂</div><div className="folder-copy"><strong>{selectedPath || "Drop a folder here"}</strong><span>{selectedPath ? "This folder is the current FilePilot workspace" : "Choose a folder or drag one anywhere in this window"}</span></div><button className="button primary" onClick={pickFolder}>{selectedPath ? "Change folder" : "Choose folder"}</button>{selectedPath && <button className="icon-button" onClick={() => selectPath("")} aria-label="Clear selected folder">×</button>}</section>;
}

function ScanControls({ options, update }: { options: ScanOptions; update: (update: Partial<ScanOptions>) => void }) {
  const [excludeText, setExcludeText] = useState(options.excludes.join(", "));
  return <details className="advanced-controls"><summary>Scan options</summary><div className="control-grid"><label className="check"><input type="checkbox" checked={options.recursive} onChange={(event) => update({ recursive: event.target.checked })} /> Include subfolders</label><label className="check"><input type="checkbox" checked={options.include_hidden} onChange={(event) => update({ include_hidden: event.target.checked })} /> Include hidden/system paths</label><label className="check"><input type="checkbox" checked={options.follow_symlinks} onChange={(event) => update({ follow_symlinks: event.target.checked })} /> Follow symlinks</label><label>Exclude globs<input value={excludeText} placeholder="node_modules, *.tmp" onChange={(event) => { setExcludeText(event.target.value); update({ excludes: event.target.value.split(",").map((item) => item.trim()).filter(Boolean) }); }} /></label></div></details>;
}

function TaskBanner({ task, onCancel }: { task: TaskSnapshot; onCancel: () => Promise<void> }) {
  const progress = task.progress;
  const percent = progress?.total ? Math.min(100, Math.round((progress.completed / progress.total) * 100)) : undefined;
  return <div className="task-banner"><div className="spinner" /><div className="task-copy"><strong>{progress?.phase || "Working…"}</strong><span>{progress?.currentPath || "FilePilot is working in the background"}</span></div>{percent !== undefined && <span className="progress-percent">{percent}%</span>}<button className="button ghost" onClick={() => void onCancel()}>Cancel</button></div>;
}

function OverviewPage({ selectedPath, pickFolder, selectPath, scanOptions, updateScanOptions, run, outputs, exportOutput, setActiveModule, lastScan }: CommonProps & { setActiveModule: (module: ModuleId) => void; lastScan?: ScanResult }) {
  const scan = () => { if (selectedPath) void run(() => api.startScan(selectedPath, scanOptions)); };
  return <><PageIntro title="A calmer way to control your files" description="Preview every change, keep your originals safe, and understand what is taking up space."><span className="privacy-badge">● Private by default</span></PageIntro><FolderBar selectedPath={selectedPath} pickFolder={pickFolder} selectPath={selectPath} /><ScanControls options={scanOptions} update={updateScanOptions} />
    <section className="hero-card"><div><span className="card-kicker">READY WHEN YOU ARE</span><h3>{selectedPath ? "Start with a scan" : "Choose a folder to get started"}</h3><p>FilePilot reads locally, never uploads your files, and never changes anything during a scan.</p><button className="button primary" onClick={scan} disabled={!selectedPath}>Scan folder <span>→</span></button></div><div className="hero-orbit"><span>SCAN</span><span>PREVIEW</span><span>UNDO</span></div></section>
    {lastScan && <section className="stats-grid"><Stat label="Files found" value={lastScan.files.length.toLocaleString()} detail="in the last scan" /><Stat label="Total size" value={formatBytes(lastScan.files.reduce((sum, file) => sum + file.size_bytes, 0))} detail="across scanned files" /><Stat label="Warnings" value={lastScan.warnings.length.toString()} detail="review in scan report" /></section>}
    <section className="module-grid">{modules.filter((item) => !["overview", "settings", "activity"].includes(item.id)).map((item) => <button key={item.id} className="module-card" onClick={() => setActiveModule(item.id)}><span className="module-card-icon">{item.icon}</span><strong>{item.label}</strong><span>Open workflow <b>→</b></span></button>)}</section>
    {outputs.scan && <ExportActions output={outputs.scan} exportOutput={exportOutput} />}
  </>;
}

function Stat({ label, value, detail }: { label: string; value: string; detail: string }) { return <div className="stat-card"><span>{label}</span><strong>{value}</strong><small>{detail}</small></div>; }

function ScanPage(props: CommonProps) {
  const output = props.outputs.scan?.type === "scan" ? props.outputs.scan.data : undefined;
  return <><PageIntro title="Scan files" description="Build a deterministic, read-only inventory of a folder and its contents."><button className="button primary" onClick={() => props.selectedPath && void props.run(() => api.startScan(props.selectedPath, props.scanOptions))} disabled={!props.selectedPath}>Run scan</button></PageIntro><FolderBar selectedPath={props.selectedPath} pickFolder={props.pickFolder} selectPath={props.selectPath} /><ScanControls options={props.scanOptions} update={props.updateScanOptions} />{output ? <><ReportHeader count={`${output.files.length} files · ${output.warnings.length} warnings`} output={props.outputs.scan} exportOutput={props.exportOutput} /><div className="table-card"><table><thead><tr><th>Relative path</th><th>Size</th><th>Modified</th><th>Flags</th></tr></thead><tbody>{output.files.map((file) => <tr key={file.path}><td className="path-cell">{file.relative_path}</td><td>{formatBytes(file.size_bytes)}</td><td>{file.modified_at ? new Date(file.modified_at).toLocaleString() : "—"}</td><td>{file.is_symlink ? "Symlink" : file.is_hidden ? "Hidden" : "—"}</td></tr>)}</tbody></table>{output.warnings.length > 0 && <WarningList warnings={output.warnings.map((warning) => warning.message)} />}</div></> : <EmptyState title="No scan yet" description="Choose a folder and run a scan to see every file and any access warnings." />}</>;
}

function LargeFilesPage(props: CommonProps) {
  const output = props.outputs["large-files"]?.type === "largeFiles" ? props.outputs["large-files"].data : undefined;
  const [top, setTop] = useState(50);
  const [minSize, setMinSize] = useState(0);
  const runReport = () => props.selectedPath && void props.run(() => api.startLargeFiles(props.selectedPath, props.scanOptions, top, minSize));
  return <><PageIntro title="Large files" description="Find the files that are consuming the most storage, sorted with stable results."><button className="button primary" onClick={runReport} disabled={!props.selectedPath}>Find large files</button></PageIntro><FolderBar selectedPath={props.selectedPath} pickFolder={props.pickFolder} selectPath={props.selectPath} /><ScanControls options={props.scanOptions} update={props.updateScanOptions} /><div className="inline-form"><label>Show top<input type="number" min="1" value={top} onChange={(event) => setTop(Number(event.target.value))} /></label><label>Minimum size<input type="number" min="0" value={minSize} onChange={(event) => setMinSize(Number(event.target.value))} /></label><span className="field-hint">bytes</span></div>{output ? <><ReportHeader count={`${output.entries.length} files`} output={props.outputs["large-files"]} exportOutput={props.exportOutput} /><div className="table-card"><table><thead><tr><th>#</th><th>Path</th><th>Size</th></tr></thead><tbody>{output.entries.map((entry, index) => <tr key={entry.path}><td>{index + 1}</td><td className="path-cell">{entry.path}</td><td className="size-cell">{formatBytes(entry.size_bytes)}</td></tr>)}</tbody></table>{output.warnings.length > 0 && <WarningList warnings={output.warnings.map((warning) => warning.message)} />}</div></> : <EmptyState title="No report yet" description="Run a large-file report to surface the biggest storage wins." />}</>;
}

function DuplicatesPage(props: CommonProps) {
  const output = props.outputs.duplicates?.type === "duplicates" ? props.outputs.duplicates.data : undefined;
  return <><PageIntro title="Duplicate finder" description="Compare file sizes, partial hashes, and full hashes to identify identical files without deleting anything."><button className="button primary" onClick={() => props.selectedPath && void props.run(() => api.startDuplicates(props.selectedPath, props.scanOptions))} disabled={!props.selectedPath}>Find duplicates</button></PageIntro><FolderBar selectedPath={props.selectedPath} pickFolder={props.pickFolder} selectPath={props.selectPath} /><ScanControls options={props.scanOptions} update={props.updateScanOptions} />{output ? <><ReportHeader count={`${output.groups.length} duplicate groups`} output={props.outputs.duplicates} exportOutput={props.exportOutput} />{output.groups.length ? <div className="duplicate-list">{output.groups.map((group) => <DuplicateCard key={group.hash} group={group} />)}</div> : <EmptyState title="No duplicates found" description="These files appear unique within the selected folder." />}</> : <EmptyState title="No report yet" description="Run the duplicate finder to compare matching file candidates." />}</>;
}

function SystemDataPage(props: CommonProps) {
  const output = props.outputs["system-data"]?.type === "systemData"
    ? props.outputs["system-data"].data
    : undefined;
  const analyze = () => void props.run(() => api.startSystemData(true));
  const reviewableBytes = output?.items
    .filter((item) => item.assessment === "likely-safe-to-review")
    .reduce((total, item) => total + item.sizeBytes, 0) ?? 0;
  const reviewBytes = output?.items
    .filter((item) => item.assessment === "review-before-removing")
    .reduce((total, item) => total + item.sizeBytes, 0) ?? 0;
  return <><PageIntro title="System Data" description="Find large local contributors to macOS System Data and understand what deserves review before you touch it."><button className="button primary" onClick={analyze}>Analyze System Data</button></PageIntro><section className="system-data-notice"><strong>Read-only by design</strong><span>FilePilot will inspect local storage, show likely categories, and explain the tradeoffs. It will never delete system files, caches, backups, or personal data from this report.</span></section>{output ? <><ReportHeader count={`${output.items.length} locations · ${formatBytes(output.totalBytes)} scanned`} output={props.outputs["system-data"]} exportOutput={props.exportOutput} /><section className="stats-grid"><Stat label="Likely reviewable" value={formatBytes(reviewableBytes)} detail="caches, logs, and temporary data" /><Stat label="Review carefully" value={formatBytes(reviewBytes)} detail="app state, backups, or developer data" /><Stat label="Platform" value={output.platform === "macos" ? "macOS" : output.platform} detail="System Data estimate" /></section><div className="storage-list">{output.items.map((item) => <StorageItemCard key={`${item.path}-${item.label}`} item={item} />)}</div>{output.notes.length > 0 && <div className="info-list"><strong>What this means</strong>{output.notes.map((note) => <span key={note}>• {note}</span>)}</div>}{output.warnings.length > 0 && <WarningList warnings={output.warnings} />}</> : <EmptyState title="No System Data report yet" description="Run the analyzer to inspect macOS user-library, shared-library, and system-working-data locations without changing them." />}</>;
}

function StorageItemCard({ item, nested = false }: { item: StorageItem; nested?: boolean }) {
  return <article className={`storage-item ${nested ? "nested" : ""}`}><div className="storage-item-heading"><div><strong>{item.label}</strong><span className="storage-path">{item.path}</span></div><div className="storage-size">{item.sizeKnown ? formatBytes(item.sizeBytes) : "Size not reported"}</div></div><div className="storage-meta"><span className={`assessment-pill ${item.assessment}`}>{assessmentLabel(item.assessment)}</span><span>{item.reason}</span></div><p className="storage-recommendation"><strong>Next step:</strong> {item.recommendation}</p>{item.children.length > 0 && <details className="storage-children"><summary>Show largest child locations</summary>{item.children.map((child) => <StorageItemCard key={child.path} item={child} nested />)}</details>}</article>;
}

function DuplicateCard({ group }: { group: DuplicateGroup }) { return <article className="duplicate-card"><div className="duplicate-heading"><div><span className="hash-label">BLAKE3</span><code>{group.hash.slice(0, 18)}…</code></div><strong>{formatBytes(group.reclaimable_bytes)} reclaimable</strong></div><p>{group.paths.length} identical files · {formatBytes(group.size_bytes)} each · keep <span className="primary-path">{group.recommended_primary}</span></p><ul>{group.paths.map((path) => <li key={path}>{path}</li>)}</ul></article>; }

function RenamePage(props: CommonProps & { plan?: OperationPlan; setPlan: (plan: OperationPlan | undefined) => void }) {
  const [mode, setMode] = useState<"pattern" | "regex">("pattern");
  const [pattern, setPattern] = useState("{stem}_{number}{ext}");
  const [expression, setExpression] = useState("IMG_(\\d+)");
  const [replace, setReplace] = useState("photo_$1");
  const [width, setWidth] = useState(3);
  const [dryRun, setDryRun] = useState(false);
  const preview = () => props.selectedPath && void props.run(() => api.previewRename(props.selectedPath, props.scanOptions, { pattern: mode === "pattern" ? pattern : undefined, regex: mode === "regex" ? expression : undefined, replace: mode === "regex" ? replace : undefined, width }));
  const apply = () => props.plan && void props.run(() => api.applyOperation(props.plan!, dryRun));
  return <><PageIntro title="Batch rename" description="Preview deterministic names using templates or regular expressions before changing anything."><button className="button primary" onClick={preview} disabled={!props.selectedPath}>Preview names</button></PageIntro><FolderBar selectedPath={props.selectedPath} pickFolder={props.pickFolder} selectPath={props.selectPath} /><div className="form-card"><div className="segmented"><button className={mode === "pattern" ? "selected" : ""} onClick={() => setMode("pattern")}>Template</button><button className={mode === "regex" ? "selected" : ""} onClick={() => setMode("regex")}>Regular expression</button></div>{mode === "pattern" ? <><label>Pattern<input value={pattern} onChange={(event) => setPattern(event.target.value)} /><small>{"Use {name}, {stem}, {ext}, {date}, and {number}."}</small></label><label>Number width<input type="number" min="0" max="12" value={width} onChange={(event) => setWidth(Number(event.target.value))} /></label></> : <div className="two-col"><label>Find<input value={expression} onChange={(event) => setExpression(event.target.value)} /></label><label>Replace<input value={replace} onChange={(event) => setReplace(event.target.value)} /></label></div>}<label className="check"><input type="checkbox" checked={dryRun} onChange={(event) => setDryRun(event.target.checked)} /> Dry run — never modify files</label></div>{props.plan && <OperationPreview plan={props.plan} onApply={apply} dryRun={dryRun} />}</>;
}

function OrganizePage(props: CommonProps & { plan?: OperationPlan; setPlan: (plan: OperationPlan | undefined) => void }) {
  const [organizeBy, setOrganizeBy] = useState("extension");
  const [dryRun, setDryRun] = useState(false);
  const preview = () => props.selectedPath && void props.run(() => api.previewOrganize(props.selectedPath, props.scanOptions, organizeBy));
  const apply = () => props.plan && void props.run(() => api.applyOperation(props.plan!, dryRun));
  return <><PageIntro title="Organize files" description="Move files into predictable buckets by extension, modified date, or first letter."><button className="button primary" onClick={preview} disabled={!props.selectedPath}>Preview organization</button></PageIntro><FolderBar selectedPath={props.selectedPath} pickFolder={props.pickFolder} selectPath={props.selectPath} /><div className="form-card"><label>Organize by<select value={organizeBy} onChange={(event) => setOrganizeBy(event.target.value)}><option value="extension">Extension</option><option value="date">Modified date</option><option value="name">Name</option></select></label><label className="check"><input type="checkbox" checked={dryRun} onChange={(event) => setDryRun(event.target.checked)} /> Dry run — never move files</label></div>{props.plan && <OperationPreview plan={props.plan} onApply={apply} dryRun={dryRun} />}</>;
}

function MetadataPage(props: CommonProps & { preview?: CleanMetadataResult }) {
  const [outputDirectory, setOutputDirectory] = useState("");
  const preview = () => props.selectedPath && void props.run(() => api.previewMetadata(props.selectedPath, outputDirectory || undefined, props.scanOptions));
  const apply = () => props.selectedPath && void props.run(() => api.applyMetadata(props.selectedPath, outputDirectory || undefined, props.scanOptions));
  return <><PageIntro title="Clean image metadata" description="Write cleaned JPEG, PNG, and WebP copies while preserving each original source file."><button className="button primary" onClick={preview} disabled={!props.selectedPath}>Preview cleaning</button></PageIntro><FolderBar selectedPath={props.selectedPath} pickFolder={props.pickFolder} selectPath={props.selectPath} /><div className="form-card"><label>Output directory <span className="optional">optional</span><div className="input-with-button"><input value={outputDirectory} placeholder="Default: beside the source as -cleaned" onChange={(event) => setOutputDirectory(event.target.value)} /><button className="button secondary" onClick={async () => { const selected = await open({ directory: true, multiple: false, title: "Choose output directory" }); if (typeof selected === "string") setOutputDirectory(selected); }}>Browse</button></div></label><p className="form-note">Supported formats: JPEG, PNG, WebP. Unsupported files are skipped with a warning.</p></div>{props.preview && <MetadataPreview result={props.preview} onApply={apply} />}</>;
}

function OperationPreview({ plan, onApply, dryRun }: { plan: OperationPlan; onApply: () => void; dryRun: boolean }) { return <section className="preview-card"><div className="preview-heading"><div><span className="card-kicker">PREVIEW READY</span><h3>{plan.actions.length} proposed changes</h3></div><button className="button primary" onClick={onApply}>{dryRun ? "Run dry run" : "Apply safely"} <span>→</span></button></div><p className="safety-note">No overwrite conflicts detected. FilePilot will stage the batch before applying it.</p><div className="table-card compact"><table><thead><tr><th>Current</th><th>Destination</th><th>Size</th></tr></thead><tbody>{plan.actions.slice(0, 100).map((action) => <tr key={action.source}><td className="path-cell">{action.source}</td><td className="path-cell destination">{action.destination}</td><td>{formatBytes(action.size_bytes)}</td></tr>)}</tbody></table>{plan.actions.length > 100 && <p className="table-footnote">Showing the first 100 of {plan.actions.length} actions.</p>}</div>{plan.skipped.length > 0 && <WarningList warnings={plan.skipped} />}</section>; }

function MetadataPreview({ result, onApply }: { result: CleanMetadataResult; onApply: () => void }) { return <section className="preview-card"><div className="preview-heading"><div><span className="card-kicker">PREVIEW READY</span><h3>{result.cleaned.length} cleaned copies</h3></div><button className="button primary" onClick={onApply}>Write cleaned copies <span>→</span></button></div><p className="safety-note">Original files will remain untouched. Existing output files cause the batch to stop.</p><div className="table-card compact"><table><thead><tr><th>Source</th><th>Cleaned copy</th><th>Format</th></tr></thead><tbody>{result.cleaned.slice(0, 100).map((image) => <tr key={image.source}><td className="path-cell">{image.source}</td><td className="path-cell destination">{image.destination}</td><td>{image.format.toUpperCase()}</td></tr>)}</tbody></table></div>{result.warnings.length > 0 && <WarningList warnings={result.warnings} />}</section>; }

function ActivityPage({ tasks, runningTasks, run, onCancel }: { tasks: Record<string, TaskSnapshot>; runningTasks: TaskSnapshot[]; run: (start: () => Promise<string>) => Promise<void>; onCancel: (taskId?: string) => Promise<void> }) {
  const [history, setHistory] = useState<OperationRecord[]>([]);
  const refresh = () => api.listOperations().then(setHistory).catch(() => undefined);
  useEffect(() => { refresh(); }, []);
  const taskList = Object.values(tasks).sort((left, right) => left.taskId < right.taskId ? 1 : -1);
  return <><PageIntro title="Activity" description="See active work, completed reports, and the operations that can be undone."><button className="button secondary" onClick={refresh}>Refresh history</button></PageIntro><div className="activity-grid"><section className="panel"><div className="panel-heading"><div><span className="card-kicker">LIVE TASKS</span><h3>{runningTasks.length ? `${runningTasks.length} in progress` : "No active tasks"}</h3></div></div>{taskList.length ? taskList.map((task) => <div className="task-row" key={task.taskId}><span className={`task-state ${task.status}`} /> <div><strong>{labelForTask(task.kind)}</strong><small>{task.progress?.phase || task.status}{task.progress?.currentPath ? ` · ${task.progress.currentPath}` : ""}</small></div><StatusPill status={task.status} />{task.status === "running" && <button className="button ghost" onClick={() => void onCancel(task.taskId)}>Cancel</button>}</div>) : <EmptyState title="Nothing running" description="Long-running reports and previews will appear here." />}</section><section className="panel"><div className="panel-heading"><div><span className="card-kicker">OPERATION HISTORY</span><h3>Recent changes</h3></div></div>{history.length ? history.map((record) => <div className="history-row" key={record.id}><div><strong>{record.kind} · {record.actions.length} files</strong><small>{new Date(record.completed_at).toLocaleString()} · {record.root}</small></div><span className={record.undone_at ? "undone" : "ready-undo"}>{record.undone_at ? "Undone" : "Logged"}</span>{!record.undone_at && <button className="button ghost" onClick={() => void run(() => api.undo(record.id))}>Undo</button>}</div>) : <EmptyState title="No operations yet" description="Applied rename and organize batches will be recorded here." />}</section></div></>;
}

function SettingsPage({ settings, setSettings, setNotice, setError }: { settings: AppSettings; setSettings: (settings: AppSettings) => void; setNotice: (notice: string) => void; setError: (error: string) => void }) { return <><PageIntro title="Settings" description="Keep FilePilot comfortable and predictable across launches." /><section className="form-card settings-card"><label>Appearance<select value={settings.theme} onChange={(event) => setSettings({ ...settings, theme: event.target.value as Theme })}><option value="system">Follow system</option><option value="light">Light</option><option value="dark">Dark</option></select></label><div><span className="field-label">Recent folders</span><p className="form-note">Stored locally so you can return to your workspaces faster. FilePilot keeps the eight most recent paths.</p>{settings.recentPaths.length ? <ul className="recent-list">{settings.recentPaths.map((path) => <li key={path}>{path}</li>)}</ul> : <p className="muted">No recent folders yet.</p>}</div><button className="button primary" onClick={() => api.saveSettings(settings).then(() => setNotice("Settings saved locally")).catch((reason) => setError(String(reason)))}>Save settings</button></section></>; }

function ReportHeader({ count, output, exportOutput }: { count: string; output?: TaskOutput; exportOutput: (output: TaskOutput, format: "json" | "csv") => Promise<void> }) { return <div className="report-header"><span>{count}</span>{output && <ExportActions output={output} exportOutput={exportOutput} />}</div>; }
function ExportActions({ output, exportOutput }: { output: TaskOutput; exportOutput: (output: TaskOutput, format: "json" | "csv") => Promise<void> }) { const csvSupported = ["scan", "largeFiles", "duplicates", "systemData"].includes(output.type); return <div className="export-actions"><button className="button ghost" onClick={() => void exportOutput(output, "json")}>Export JSON</button>{csvSupported && <button className="button ghost" onClick={() => void exportOutput(output, "csv")}>Export CSV</button>}</div>; }
function WarningList({ warnings }: { warnings: string[] }) { return <div className="warning-list"><strong>Warnings</strong>{warnings.slice(0, 12).map((warning, index) => <span key={`${warning}-${index}`}>! {warning}</span>)}{warnings.length > 12 && <span>…and {warnings.length - 12} more</span>}</div>; }
function EmptyState({ title, description }: { title: string; description: string }) { return <section className="empty-state"><span className="empty-icon">◇</span><h3>{title}</h3><p>{description}</p></section>; }
function labelForTask(kind: TaskKind) { return ({ scan: "File scan", "large-files": "Large-file report", duplicates: "Duplicate scan", "system-data": "System Data analysis", "rename-preview": "Rename preview", "organize-preview": "Organization preview", "metadata-preview": "Metadata preview", "apply-operation": "File operation", "apply-metadata": "Metadata cleaning", undo: "Undo operation" } as Record<TaskKind, string>)[kind]; }
function assessmentLabel(assessment: StorageItem["assessment"]) { return ({ "likely-safe-to-review": "Likely safe to review", "review-before-removing": "Review before removing", "user-data": "Personal data", "system-managed": "System managed", unknown: "Unknown" })[assessment]; }
function formatBytes(bytes: number) { if (bytes === 0) return "0 B"; const units = ["B", "KB", "MB", "GB", "TB"]; const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1); return `${(bytes / 1024 ** index).toFixed(index ? 1 : 0)} ${units[index]}`; }
