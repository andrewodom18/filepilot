use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use chrono::{DateTime, Utc};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use walkdir::{DirEntry, WalkDir};

use crate::{FilePilotError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOptions {
    pub recursive: bool,
    pub include_hidden: bool,
    pub follow_symlinks: bool,
    pub excludes: Vec<String>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            recursive: true,
            include_hidden: false,
            follow_symlinks: false,
            excludes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecord {
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub size_bytes: u64,
    pub extension: Option<String>,
    pub modified_at: Option<DateTime<Utc>>,
    pub is_symlink: bool,
    pub is_hidden: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanWarning {
    pub path: Option<PathBuf>,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub root: PathBuf,
    pub files: Vec<FileRecord>,
    pub warnings: Vec<ScanWarning>,
}

pub fn scan(root: impl AsRef<Path>, options: &ScanOptions) -> Result<ScanResult> {
    let root = absolute_path(root.as_ref())?;
    let metadata = fs::symlink_metadata(&root)?;
    let excludes = build_excludes(&options.excludes)?;
    let mut warnings = Vec::new();
    let mut files = Vec::new();

    if metadata.is_file() {
        if should_skip_path(&root, &root, options, &excludes, false) {
            return Ok(ScanResult {
                root,
                files,
                warnings,
            });
        }
        files.push(file_record(&root, &root, false)?);
    } else if metadata.is_dir() {
        let max_depth = if options.recursive { usize::MAX } else { 1 };
        let walker = WalkDir::new(&root)
            .follow_links(options.follow_symlinks)
            .max_depth(max_depth)
            .into_iter();

        for entry in walker {
            match entry {
                Ok(entry) => {
                    if entry.path() == root || entry.file_type().is_dir() {
                        continue;
                    }

                    let is_symlink = entry.file_type().is_symlink();
                    if should_skip_path(entry.path(), &root, options, &excludes, is_symlink) {
                        continue;
                    }

                    if !entry.file_type().is_file() && !is_symlink {
                        continue;
                    }

                    match file_record(entry.path(), &root, is_symlink) {
                        Ok(record) => files.push(record),
                        Err(error) => warnings.push(ScanWarning {
                            path: Some(entry.path().to_path_buf()),
                            kind: "metadata".to_string(),
                            message: error.to_string(),
                        }),
                    }
                }
                Err(error) => warnings.push(ScanWarning {
                    path: error.path().map(Path::to_path_buf),
                    kind: "walk".to_string(),
                    message: error.to_string(),
                }),
            }
        }
    } else {
        return Err(FilePilotError::InvalidInput(format!(
            "scan root is neither a file nor directory: {}",
            root.display()
        )));
    }

    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    Ok(ScanResult {
        root,
        files,
        warnings,
    })
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    fs::canonicalize(&path).map_err(|_| FilePilotError::InvalidPath(path))
}

fn build_excludes(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern).map_err(|error| {
            FilePilotError::InvalidInput(format!("invalid exclude glob `{pattern}`: {error}"))
        })?);
    }
    builder
        .build()
        .map_err(|error| FilePilotError::InvalidInput(error.to_string()))
}

fn should_skip_path(
    path: &Path,
    root: &Path,
    options: &ScanOptions,
    excludes: &GlobSet,
    is_symlink: bool,
) -> bool {
    if is_symlink && !options.follow_symlinks {
        return true;
    }

    let relative = path.strip_prefix(root).unwrap_or(path);
    if excludes.is_match(relative) || excludes.is_match(path) {
        return true;
    }

    if options.include_hidden {
        return false;
    }

    relative.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        name.starts_with('.') || is_system_directory(&name)
    })
}

fn is_system_directory(name: &str) -> bool {
    matches!(
        name,
        "$Recycle.Bin"
            | "System Volume Information"
            | "node_modules"
            | "target"
            | ".git"
            | "__pycache__"
    )
}

fn file_record(path: &Path, root: &Path, is_symlink: bool) -> Result<FileRecord> {
    let metadata = fs::symlink_metadata(path)?;
    let relative_path = path.strip_prefix(root).unwrap_or(path).to_path_buf();
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase());
    let modified_at = metadata.modified().ok().map(system_time_to_utc);

    Ok(FileRecord {
        path: path.to_path_buf(),
        relative_path,
        size_bytes: metadata.len(),
        extension,
        modified_at,
        is_symlink,
        is_hidden: file_name.starts_with('.'),
    })
}

fn system_time_to_utc(time: SystemTime) -> DateTime<Utc> {
    DateTime::<Utc>::from(time)
}

#[allow(dead_code)]
fn _entry_is_hidden(entry: &DirEntry) -> bool {
    entry.file_name().to_string_lossy().starts_with('.')
}
