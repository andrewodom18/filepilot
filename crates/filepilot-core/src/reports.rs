use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{FilePilotError, FileRecord, Result, ScanResult};

const PARTIAL_HASH_CHUNK: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LargeFileEntry {
    pub path: PathBuf,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateGroup {
    pub hash: String,
    pub size_bytes: u64,
    pub paths: Vec<PathBuf>,
    pub recommended_primary: PathBuf,
    pub reclaimable_bytes: u64,
}

pub fn large_files(scan: &ScanResult, top: usize, min_size: u64) -> Vec<LargeFileEntry> {
    let mut entries: Vec<_> = scan
        .files
        .iter()
        .filter(|file| file.size_bytes >= min_size)
        .map(|file| LargeFileEntry {
            path: file.path.clone(),
            size_bytes: file.size_bytes,
        })
        .collect();

    entries.sort_by(|left, right| {
        right
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| left.path.cmp(&right.path))
    });

    if top == 0 {
        entries
    } else {
        entries.into_iter().take(top).collect()
    }
}

pub fn duplicate_files(scan: &ScanResult) -> Result<Vec<DuplicateGroup>> {
    let mut by_size: BTreeMap<u64, Vec<&FileRecord>> = BTreeMap::new();
    for file in &scan.files {
        by_size.entry(file.size_bytes).or_default().push(file);
    }

    let mut groups = Vec::new();
    for (size, candidates) in by_size {
        if candidates.len() < 2 {
            continue;
        }

        let mut by_partial_hash: HashMap<String, Vec<&FileRecord>> = HashMap::new();
        for candidate in candidates {
            let hash = hash_file(&candidate.path, true)?;
            by_partial_hash.entry(hash).or_default().push(candidate);
        }

        for partial_candidates in by_partial_hash.into_values() {
            if partial_candidates.len() < 2 {
                continue;
            }

            let mut by_full_hash: HashMap<String, Vec<PathBuf>> = HashMap::new();
            for candidate in partial_candidates {
                let hash = hash_file(&candidate.path, false)?;
                by_full_hash
                    .entry(hash)
                    .or_default()
                    .push(candidate.path.clone());
            }

            for (hash, mut paths) in by_full_hash {
                if paths.len() < 2 {
                    continue;
                }
                paths.sort();
                let recommended_primary = paths[0].clone();
                let reclaimable_bytes = size.saturating_mul(paths.len().saturating_sub(1) as u64);
                groups.push(DuplicateGroup {
                    hash,
                    size_bytes: size,
                    paths,
                    recommended_primary,
                    reclaimable_bytes,
                });
            }
        }
    }

    groups.sort_by(|left, right| {
        right
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| left.recommended_primary.cmp(&right.recommended_primary))
    });
    Ok(groups)
}

fn hash_file(path: &Path, partial: bool) -> Result<String> {
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    let mut hasher = blake3::Hasher::new();

    if !partial || metadata.len() <= (PARTIAL_HASH_CHUNK * 2) as u64 {
        let mut buffer = [0_u8; 1024 * 64];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
    } else {
        let mut first = vec![0_u8; PARTIAL_HASH_CHUNK];
        file.read_exact(&mut first)?;
        hasher.update(&first);

        file.seek(SeekFrom::End(-(PARTIAL_HASH_CHUNK as i64)))?;
        let mut last = vec![0_u8; PARTIAL_HASH_CHUNK];
        file.read_exact(&mut last)?;
        hasher.update(&last);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

#[allow(dead_code)]
fn _ensure_regular_file(path: &Path) -> Result<()> {
    if !path.is_file() {
        return Err(FilePilotError::InvalidPath(path.to_path_buf()));
    }
    Ok(())
}
