use std::{
    error::Error,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use clap::{Args, Parser, Subcommand, ValueEnum};
use filepilot_core::{
    apply_operation, build_organize_plan, build_rename_plan, clean_images, duplicate_files,
    large_files, scan, undo_operation, CleanMetadataResult, DuplicateGroup, LargeFileEntry,
    OperationPlan, OrganizeBy, RenameOptions, ScanOptions, ScanResult,
};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "filepilot",
    version,
    about = "Safe, local-first file control for macOS, Windows, and Linux"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List files and scan warnings.
    Scan(ScanArgs),
    /// Report the largest files in a directory.
    LargeFiles(LargeFilesArgs),
    /// Report groups of identical files.
    Duplicates(DuplicatesArgs),
    /// Preview and apply a batch rename.
    Rename(RenameArgs),
    /// Preview and apply a rule-based organization.
    Organize(OrganizeArgs),
    /// Write cleaned copies of supported images without metadata.
    CleanMetadata(CleanMetadataArgs),
    /// Undo the latest operation or a specific operation ID.
    Undo(UndoArgs),
}

#[derive(Debug, Args, Clone)]
struct ScanArgs {
    path: PathBuf,
    #[command(flatten)]
    scan: ScanFlags,
    #[command(flatten)]
    output: OutputArgs,
}

#[derive(Debug, Args, Clone)]
struct LargeFilesArgs {
    path: PathBuf,
    #[arg(long, default_value_t = 20)]
    top: usize,
    #[arg(long, default_value_t = 0)]
    min_size: u64,
    #[command(flatten)]
    scan: ScanFlags,
    #[command(flatten)]
    output: OutputArgs,
}

#[derive(Debug, Args, Clone)]
struct DuplicatesArgs {
    path: PathBuf,
    #[command(flatten)]
    scan: ScanFlags,
    #[command(flatten)]
    output: OutputArgs,
}

#[derive(Debug, Args, Clone)]
struct RenameArgs {
    path: PathBuf,
    #[arg(long, conflicts_with_all = ["regex", "replace"])]
    pattern: Option<String>,
    #[arg(long, conflicts_with = "pattern")]
    regex: Option<String>,
    #[arg(long, requires = "regex", conflicts_with = "pattern")]
    replace: Option<String>,
    #[arg(long, default_value_t = 0)]
    width: usize,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    yes: bool,
    #[command(flatten)]
    scan: ScanFlags,
}

#[derive(Debug, Args, Clone)]
struct OrganizeArgs {
    path: PathBuf,
    #[arg(long, value_enum)]
    by: OrganizeByArg,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    yes: bool,
    #[command(flatten)]
    scan: ScanFlags,
}

#[derive(Debug, Args, Clone)]
struct CleanMetadataArgs {
    path: PathBuf,
    #[arg(long, value_enum, default_value_t = RemoveMode::All)]
    remove: RemoveMode,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    yes: bool,
    #[command(flatten)]
    scan: ScanFlags,
}

#[derive(Debug, Args, Clone)]
struct UndoArgs {
    operation_id: Option<String>,
}

#[derive(Debug, Args, Clone, Default)]
struct ScanFlags {
    /// Do not recurse into subdirectories.
    #[arg(long)]
    no_recursive: bool,
    /// Include hidden files and known system/generated directories.
    #[arg(long)]
    include_hidden: bool,
    /// Follow symbolic links.
    #[arg(long)]
    follow_symlinks: bool,
    /// Exclude paths matching a glob. May be repeated.
    #[arg(long, action = clap::ArgAction::Append)]
    exclude: Vec<String>,
}

#[derive(Debug, Args, Clone)]
struct OutputArgs {
    /// Output format for read-only reports.
    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    format: OutputFormat,
    /// Write report output to a file instead of stdout.
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
    Csv,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OrganizeByArg {
    Extension,
    Date,
    Name,
}

impl From<OrganizeByArg> for OrganizeBy {
    fn from(value: OrganizeByArg) -> Self {
        match value {
            OrganizeByArg::Extension => Self::Extension,
            OrganizeByArg::Date => Self::Date,
            OrganizeByArg::Name => Self::Name,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RemoveMode {
    All,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Scan(args) => run_scan(args)?,
        Command::LargeFiles(args) => run_large_files(args)?,
        Command::Duplicates(args) => run_duplicates(args)?,
        Command::Rename(args) => run_rename(args)?,
        Command::Organize(args) => run_organize(args)?,
        Command::CleanMetadata(args) => run_clean_metadata(args)?,
        Command::Undo(args) => run_undo(args)?,
    }
    Ok(())
}

fn run_scan(args: ScanArgs) -> Result<(), Box<dyn Error>> {
    let result = scan(&args.path, &scan_options(&args.scan))?;
    match args.output.format {
        OutputFormat::Human => print_scan_human(&result),
        OutputFormat::Json => emit_json(&result, args.output.output.as_deref())?,
        OutputFormat::Csv => emit_scan_csv(&result, args.output.output.as_deref())?,
    }
    Ok(())
}

fn run_large_files(args: LargeFilesArgs) -> Result<(), Box<dyn Error>> {
    let result = scan(&args.path, &scan_options(&args.scan))?;
    let entries = large_files(&result, args.top, args.min_size);
    match args.output.format {
        OutputFormat::Human => print_large_files_human(&entries, &result.warnings),
        OutputFormat::Json => emit_json(&entries, args.output.output.as_deref())?,
        OutputFormat::Csv => emit_large_files_csv(&entries, args.output.output.as_deref())?,
    }
    Ok(())
}

fn run_duplicates(args: DuplicatesArgs) -> Result<(), Box<dyn Error>> {
    let result = scan(&args.path, &scan_options(&args.scan))?;
    let groups = duplicate_files(&result)?;
    match args.output.format {
        OutputFormat::Human => print_duplicates_human(&groups, &result.warnings),
        OutputFormat::Json => emit_json(&groups, args.output.output.as_deref())?,
        OutputFormat::Csv => emit_duplicates_csv(&groups, args.output.output.as_deref())?,
    }
    Ok(())
}

fn run_rename(args: RenameArgs) -> Result<(), Box<dyn Error>> {
    let plan = build_rename_plan(
        &args.path,
        &scan_options(&args.scan),
        &RenameOptions {
            pattern: args.pattern,
            regex: args.regex,
            replace: args.replace,
            width: args.width,
        },
    )?;
    apply_confirmed_plan(plan, args.dry_run, args.yes)
}

fn run_organize(args: OrganizeArgs) -> Result<(), Box<dyn Error>> {
    let plan = build_organize_plan(&args.path, &scan_options(&args.scan), args.by.into())?;
    apply_confirmed_plan(plan, args.dry_run, args.yes)
}

fn run_clean_metadata(args: CleanMetadataArgs) -> Result<(), Box<dyn Error>> {
    if !matches!(args.remove, RemoveMode::All) {
        return Err("only --remove all is supported in v1".into());
    }

    let output = args.output.as_deref();
    let preview = clean_images(&args.path, output, &scan_options(&args.scan), true)?;
    print_metadata_preview(&preview, args.dry_run);
    if preview.cleaned.is_empty() {
        return Ok(());
    }
    if args.dry_run {
        return Ok(());
    }
    if !args.yes && !confirm("Create the cleaned image copies? [y/N] ")? {
        println!("Cancelled.");
        return Ok(());
    }

    let result = clean_images(&args.path, output, &scan_options(&args.scan), false)?;
    println!("Created {} cleaned image(s).", result.cleaned.len());
    for image in result.cleaned {
        println!(
            "  {} -> {}",
            image.source.display(),
            image.destination.display()
        );
    }
    print_warnings(&result.warnings);
    Ok(())
}

fn run_undo(args: UndoArgs) -> Result<(), Box<dyn Error>> {
    let result = undo_operation(args.operation_id.as_deref())?;
    println!("Undid operation {}.", result.operation_id);
    for action in result.actions {
        println!(
            "  {} -> {}",
            action.source.display(),
            action.destination.display()
        );
    }
    print_warnings(&result.warnings);
    Ok(())
}

fn apply_confirmed_plan(
    plan: OperationPlan,
    dry_run: bool,
    yes: bool,
) -> Result<(), Box<dyn Error>> {
    print_plan(&plan);
    if plan.actions.is_empty() {
        println!("No changes required.");
        print_warnings(&plan.skipped);
        return Ok(());
    }
    if dry_run {
        println!("Dry run complete; no files were changed.");
        return Ok(());
    }
    if !yes && !confirm("Apply these changes? [y/N] ")? {
        println!("Cancelled.");
        return Ok(());
    }

    let result = apply_operation(&plan, false)?;
    println!(
        "Applied {} action(s). Operation ID: {}",
        result.actions.len(),
        result.operation_id
    );
    print_warnings(&result.warnings);
    Ok(())
}

fn scan_options(flags: &ScanFlags) -> ScanOptions {
    ScanOptions {
        recursive: !flags.no_recursive,
        include_hidden: flags.include_hidden,
        follow_symlinks: flags.follow_symlinks,
        excludes: flags.exclude.clone(),
    }
}

fn print_scan_human(result: &ScanResult) {
    println!("Root: {}", result.root.display());
    println!("Files: {}", result.files.len());
    for file in &result.files {
        println!(
            "  {:>12}  {}",
            format_bytes(file.size_bytes),
            file.relative_path.display()
        );
    }
    print_warnings(
        &result
            .warnings
            .iter()
            .map(|warning| warning.message.clone())
            .collect::<Vec<_>>(),
    );
}

fn print_large_files_human(entries: &[LargeFileEntry], warnings: &[filepilot_core::ScanWarning]) {
    println!("Largest files: {}", entries.len());
    for (index, entry) in entries.iter().enumerate() {
        println!(
            "  {:>4}. {:>12}  {}",
            index + 1,
            format_bytes(entry.size_bytes),
            entry.path.display()
        );
    }
    print_scan_warnings(warnings);
}

fn print_duplicates_human(groups: &[DuplicateGroup], warnings: &[filepilot_core::ScanWarning]) {
    if groups.is_empty() {
        println!("No duplicate groups found.");
    } else {
        println!("Duplicate groups: {}", groups.len());
        for (index, group) in groups.iter().enumerate() {
            println!(
                "\nGroup {}: {} each, reclaimable {}",
                index + 1,
                format_bytes(group.size_bytes),
                format_bytes(group.reclaimable_bytes)
            );
            println!("  hash: {}", group.hash);
            println!(
                "  recommended primary: {}",
                group.recommended_primary.display()
            );
            for path in &group.paths {
                println!("  - {}", path.display());
            }
        }
    }
    print_scan_warnings(warnings);
}

fn print_plan(plan: &OperationPlan) {
    println!("Operation {}: {} action(s)", plan.id, plan.actions.len());
    for action in &plan.actions {
        println!(
            "  {} -> {}",
            action.source.display(),
            action.destination.display()
        );
    }
    print_warnings(&plan.skipped);
}

fn print_metadata_preview(result: &CleanMetadataResult, dry_run: bool) {
    if dry_run {
        println!(
            "Metadata-cleaning preview: {} image(s)",
            result.cleaned.len()
        );
    }
    for image in &result.cleaned {
        println!(
            "  {} -> {}",
            image.source.display(),
            image.destination.display()
        );
    }
    print_warnings(&result.warnings);
}

fn print_scan_warnings(warnings: &[filepilot_core::ScanWarning]) {
    print_warnings(
        &warnings
            .iter()
            .map(|warning| warning.message.clone())
            .collect::<Vec<_>>(),
    );
}

fn print_warnings(warnings: &[String]) {
    for warning in warnings {
        eprintln!("warning: {warning}");
    }
}

fn confirm(prompt: &str) -> io::Result<bool> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(
        input.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn emit_json<T: Serialize>(value: &T, output: Option<&Path>) -> Result<(), Box<dyn Error>> {
    emit_text(&serde_json::to_string_pretty(value)?, output)
}

fn emit_scan_csv(result: &ScanResult, output: Option<&Path>) -> Result<(), Box<dyn Error>> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    for file in &result.files {
        writer.serialize(file)?;
    }
    let bytes = writer.into_inner()?;
    emit_bytes(bytes, output)
}

fn emit_large_files_csv(
    entries: &[LargeFileEntry],
    output: Option<&Path>,
) -> Result<(), Box<dyn Error>> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    for entry in entries {
        writer.serialize(entry)?;
    }
    let bytes = writer.into_inner()?;
    emit_bytes(bytes, output)
}

#[derive(Serialize)]
struct DuplicateCsvRow {
    hash: String,
    size_bytes: u64,
    path: PathBuf,
    recommended_primary: PathBuf,
    reclaimable_bytes: u64,
}

fn emit_duplicates_csv(
    groups: &[DuplicateGroup],
    output: Option<&Path>,
) -> Result<(), Box<dyn Error>> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    for group in groups {
        for path in &group.paths {
            writer.serialize(DuplicateCsvRow {
                hash: group.hash.clone(),
                size_bytes: group.size_bytes,
                path: path.clone(),
                recommended_primary: group.recommended_primary.clone(),
                reclaimable_bytes: group.reclaimable_bytes,
            })?;
        }
    }
    let bytes = writer.into_inner()?;
    emit_bytes(bytes, output)
}

fn emit_text(text: &str, output: Option<&Path>) -> Result<(), Box<dyn Error>> {
    emit_bytes(text.as_bytes().to_vec(), output)
}

fn emit_bytes(bytes: Vec<u8>, output: Option<&Path>) -> Result<(), Box<dyn Error>> {
    if let Some(path) = output {
        fs::write(path, bytes)?;
    } else {
        io::stdout().write_all(&bytes)?;
        if !bytes.ends_with(b"\n") {
            println!();
        }
    }
    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[allow(dead_code)]
fn _read_all(mut reader: impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    Ok(bytes)
}
