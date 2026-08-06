use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    scan_with_context, FilePilotError, OperationContext, ProgressEvent, Result, ScanOptions,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanMetadataResult {
    pub cleaned: Vec<CleanedImage>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanedImage {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub format: String,
}

pub fn clean_images(
    root: impl AsRef<Path>,
    output_directory: Option<&Path>,
    scan_options: &ScanOptions,
    dry_run: bool,
) -> Result<CleanMetadataResult> {
    clean_images_with_context(
        root,
        output_directory,
        scan_options,
        dry_run,
        &OperationContext::default(),
    )
}

pub fn clean_images_with_context(
    root: impl AsRef<Path>,
    output_directory: Option<&Path>,
    scan_options: &ScanOptions,
    dry_run: bool,
    context: &OperationContext,
) -> Result<CleanMetadataResult> {
    let scan_result = scan_with_context(root.as_ref(), scan_options, context)?;
    let output_root = match output_directory {
        Some(directory) => absolute_output_path(directory)?,
        None => default_output_directory(&scan_result.root),
    };

    if output_root == scan_result.root {
        return Err(FilePilotError::InvalidInput(
            "metadata output directory must differ from the source".to_string(),
        ));
    }

    let mut candidates = Vec::new();
    let mut warnings = scan_result
        .warnings
        .iter()
        .map(|warning| warning.message.clone())
        .collect::<Vec<_>>();

    for (index, file) in scan_result.files.into_iter().enumerate() {
        context.cancellation.check()?;
        context.report(ProgressEvent {
            phase: "preparing metadata cleaning".to_string(),
            completed: index as u64 + 1,
            total: None,
            current_path: Some(file.path.clone()),
            message: None,
        });
        let Some(format) = image_format(file.extension.as_deref()) else {
            warnings.push(format!("unsupported image format: {}", file.path.display()));
            continue;
        };

        let relative = if scan_result.root.is_dir() {
            file.relative_path.clone()
        } else {
            PathBuf::from(file.path.file_name().unwrap_or_default())
        };
        let destination = output_root.join(relative);
        candidates.push((file.path, destination, format));
    }

    let mut destinations = HashSet::new();
    for (_, destination, _) in &candidates {
        if !destinations.insert(destination.clone()) {
            return Err(FilePilotError::Conflict(format!(
                "multiple images target {}",
                destination.display()
            )));
        }
        if destination.exists() {
            return Err(FilePilotError::Conflict(format!(
                "metadata output already exists: {}",
                destination.display()
            )));
        }
    }

    context.cancellation.check()?;
    let mut prepared = Vec::new();
    for (source, destination, format) in candidates {
        let bytes = if dry_run {
            None
        } else {
            let bytes = fs::read(&source)?;
            Some(strip_metadata(&bytes, format)?)
        };
        prepared.push((source, destination, format, bytes));
    }

    let mut cleaned = Vec::new();
    for (source, destination, format, bytes) in prepared {
        if let Some(bytes) = bytes {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&destination, bytes)?;
        }
        cleaned.push(CleanedImage {
            source,
            destination,
            format: format_name(format).to_string(),
        });
    }

    Ok(CleanMetadataResult { cleaned, warnings })
}

#[derive(Debug, Clone, Copy)]
enum SupportedFormat {
    Jpeg,
    Png,
    Webp,
}

fn image_format(extension: Option<&str>) -> Option<SupportedFormat> {
    match extension {
        Some("jpg") | Some("jpeg") => Some(SupportedFormat::Jpeg),
        Some("png") => Some(SupportedFormat::Png),
        Some("webp") => Some(SupportedFormat::Webp),
        _ => None,
    }
}

fn format_name(format: SupportedFormat) -> &'static str {
    match format {
        SupportedFormat::Jpeg => "jpeg",
        SupportedFormat::Png => "png",
        SupportedFormat::Webp => "webp",
    }
}

fn strip_metadata(bytes: &[u8], format: SupportedFormat) -> Result<Vec<u8>> {
    match format {
        SupportedFormat::Jpeg => strip_jpeg_metadata(bytes),
        SupportedFormat::Png => strip_png_metadata(bytes),
        SupportedFormat::Webp => strip_webp_metadata(bytes),
    }
}

fn strip_jpeg_metadata(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() < 2 || bytes[..2] != [0xff, 0xd8] {
        return Err(invalid_image("invalid JPEG signature"));
    }

    let mut output = bytes[..2].to_vec();
    let mut cursor = 2;
    while cursor < bytes.len() {
        if bytes[cursor] != 0xff {
            return Err(invalid_image("invalid JPEG marker"));
        }
        while cursor < bytes.len() && bytes[cursor] == 0xff {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            return Err(invalid_image("truncated JPEG marker"));
        }

        let marker = bytes[cursor];
        cursor += 1;
        if marker == 0xda {
            output.extend_from_slice(&[0xff, marker]);
            output.extend_from_slice(&bytes[cursor..]);
            break;
        }
        if marker == 0xd9 {
            output.extend_from_slice(&[0xff, marker]);
            break;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            output.extend_from_slice(&[0xff, marker]);
            continue;
        }
        if cursor + 2 > bytes.len() {
            return Err(invalid_image("truncated JPEG segment length"));
        }
        let length = u16::from_be_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
        if length < 2 || cursor + length > bytes.len() {
            return Err(invalid_image("invalid JPEG segment length"));
        }
        let segment_start = cursor - 2;
        let segment_end = cursor + length;
        let is_metadata = (0xe0..=0xef).contains(&marker) || marker == 0xfe;
        if !is_metadata {
            output.extend_from_slice(&[0xff, marker]);
            output.extend_from_slice(&bytes[segment_start..segment_end]);
        }
        cursor = segment_end;
    }

    Ok(output)
}

fn strip_png_metadata(bytes: &[u8]) -> Result<Vec<u8>> {
    const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < SIGNATURE.len() || &bytes[..SIGNATURE.len()] != SIGNATURE {
        return Err(invalid_image("invalid PNG signature"));
    }

    let mut output = bytes[..SIGNATURE.len()].to_vec();
    let mut cursor = SIGNATURE.len();
    while cursor < bytes.len() {
        if cursor + 12 > bytes.len() {
            return Err(invalid_image("truncated PNG chunk"));
        }
        let length = u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
        let chunk_end = cursor
            .checked_add(12)
            .and_then(|end| end.checked_add(length))
            .ok_or_else(|| invalid_image("PNG chunk length overflow"))?;
        if chunk_end > bytes.len() {
            return Err(invalid_image("invalid PNG chunk length"));
        }
        let chunk_type = &bytes[cursor + 4..cursor + 8];
        let is_metadata = matches!(
            chunk_type,
            b"eXIf" | b"tEXt" | b"zTXt" | b"iTXt" | b"tIME" | b"pHYs" | b"iCCP"
        );
        if !is_metadata {
            output.extend_from_slice(&bytes[cursor..chunk_end]);
        }
        cursor = chunk_end;
        if chunk_type == b"IEND" {
            break;
        }
    }

    Ok(output)
}

fn strip_webp_metadata(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return Err(invalid_image("invalid WebP signature"));
    }

    let mut output = bytes[..12].to_vec();
    let mut cursor = 12;
    while cursor < bytes.len() {
        if cursor + 8 > bytes.len() {
            return Err(invalid_image("truncated WebP chunk"));
        }
        let chunk_type = &bytes[cursor..cursor + 4];
        let length = u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        let data_end = cursor
            .checked_add(8)
            .and_then(|end| end.checked_add(length))
            .ok_or_else(|| invalid_image("WebP chunk length overflow"))?;
        let chunk_end = data_end + (length % 2);
        if chunk_end > bytes.len() {
            return Err(invalid_image("invalid WebP chunk length"));
        }
        let is_metadata = matches!(chunk_type, b"EXIF" | b"XMP " | b"ICCP");
        if !is_metadata {
            output.extend_from_slice(&bytes[cursor..chunk_end]);
        }
        cursor = chunk_end;
    }

    let riff_size = (output.len() - 8) as u32;
    output[4..8].copy_from_slice(&riff_size.to_le_bytes());
    Ok(output)
}

fn invalid_image(message: &str) -> FilePilotError {
    FilePilotError::InvalidInput(message.to_string())
}

fn absolute_output_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn default_output_directory(root: &Path) -> PathBuf {
    if root.is_dir() {
        let name = root
            .file_name()
            .map(|name| format!("{}-cleaned", name.to_string_lossy()))
            .unwrap_or_else(|| "filepilot-cleaned".to_string());
        root.parent().unwrap_or_else(|| Path::new(".")).join(name)
    } else {
        let stem = root
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(|| "image".to_string());
        root.parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!("{stem}-cleaned"))
    }
}

#[cfg(test)]
mod tests {
    use super::{strip_jpeg_metadata, strip_png_metadata, strip_webp_metadata};

    #[test]
    fn jpeg_metadata_segments_are_removed_without_touching_image_data() {
        let input = vec![
            0xff, 0xd8, // SOI
            0xff, 0xe1, 0x00, 0x04, 0x01, 0x02, // EXIF
            0xff, 0xfe, 0x00, 0x04, 0x03, 0x04, // comment
            0xff, 0xda, 0x00, 0x02, 0x11, 0x22, // image data
        ];
        let output = strip_jpeg_metadata(&input).unwrap();
        assert_eq!(output, vec![0xff, 0xd8, 0xff, 0xda, 0x00, 0x02, 0x11, 0x22]);
    }

    #[test]
    fn png_text_chunks_are_removed() {
        let input = [
            b"\x89PNG\r\n\x1a\n".as_slice(),
            &[0, 0, 0, 3],
            b"tEXt",
            b"abc",
            &[0, 0, 0, 0],
            &[0, 0, 0, 0],
            b"IEND",
            &[0, 0, 0, 0],
        ]
        .concat();
        let output = strip_png_metadata(&input).unwrap();
        assert!(!output.windows(4).any(|window| window == b"tEXt"));
        assert!(output.windows(4).any(|window| window == b"IEND"));
    }

    #[test]
    fn webp_metadata_chunks_are_removed_and_riff_size_is_updated() {
        let input = [
            b"RIFF".as_slice(),
            &[0, 0, 0, 0],
            b"WEBP",
            b"EXIF",
            &[4, 0, 0, 0],
            &[1, 2, 3, 4],
            b"VP8 ",
            &[2, 0, 0, 0],
            &[5, 6],
        ]
        .concat();
        let output = strip_webp_metadata(&input).unwrap();
        assert!(!output.windows(4).any(|window| window == b"EXIF"));
        assert!(output.windows(4).any(|window| window == b"VP8 "));
        assert_eq!(
            u32::from_le_bytes(output[4..8].try_into().unwrap()),
            (output.len() - 8) as u32
        );
    }
}
