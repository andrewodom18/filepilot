use std::fs;

use filepilot_core::{
    apply_operation, build_organize_plan, build_rename_plan, clean_images, duplicate_files,
    large_files, scan, scan_with_context, CancellationToken, OperationContext, OrganizeBy,
    RenameOptions, ScanOptions,
};
use image::{DynamicImage, ImageFormat, Rgb};
use tempfile::tempdir;

fn write_file(path: &std::path::Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

#[test]
fn scanner_is_recursive_deterministic_and_conservative() {
    let directory = tempdir().unwrap();
    write_file(&directory.path().join("nested/b.txt"), b"b");
    write_file(&directory.path().join("a.txt"), b"a");
    write_file(&directory.path().join(".hidden.txt"), b"hidden");
    write_file(
        &directory.path().join("nested/.hidden-nested.txt"),
        b"hidden",
    );

    let result = scan(directory.path(), &ScanOptions::default()).unwrap();
    let paths: Vec<_> = result
        .files
        .iter()
        .map(|file| file.relative_path.to_string_lossy().to_string())
        .collect();

    assert_eq!(paths, vec!["a.txt", "nested/b.txt"]);
    assert!(result.warnings.is_empty());
}

#[test]
fn scanner_supports_non_recursive_mode_and_exclusions() {
    let directory = tempdir().unwrap();
    write_file(&directory.path().join("root.txt"), b"root");
    write_file(&directory.path().join("nested/file.txt"), b"nested");
    write_file(&directory.path().join("keep.txt"), b"keep");

    let options = ScanOptions {
        recursive: false,
        excludes: vec!["keep.txt".to_string()],
        ..ScanOptions::default()
    };
    let result = scan(directory.path(), &options).unwrap();

    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].relative_path.to_string_lossy(), "root.txt");
}

#[test]
fn large_files_are_sorted_and_limited() {
    let directory = tempdir().unwrap();
    write_file(&directory.path().join("small.txt"), b"1");
    write_file(&directory.path().join("large.txt"), b"12345");
    write_file(&directory.path().join("medium.txt"), b"123");

    let result = scan(directory.path(), &ScanOptions::default()).unwrap();
    let entries = large_files(&result, 2, 0);

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].path.file_name().unwrap(), "large.txt");
    assert_eq!(entries[1].path.file_name().unwrap(), "medium.txt");
}

#[test]
fn duplicate_report_groups_equal_contents_without_mutating_files() {
    let directory = tempdir().unwrap();
    write_file(&directory.path().join("one.bin"), b"same contents");
    write_file(&directory.path().join("two.bin"), b"same contents");
    write_file(&directory.path().join("different.bin"), b"other contents");

    let result = scan(directory.path(), &ScanOptions::default()).unwrap();
    let groups = duplicate_files(&result).unwrap();

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].paths.len(), 2);
    assert_eq!(
        fs::read(directory.path().join("one.bin")).unwrap(),
        b"same contents"
    );
}

#[test]
fn rename_plan_supports_templates_and_zero_padding() {
    let directory = tempdir().unwrap();
    write_file(&directory.path().join("b.txt"), b"b");
    write_file(&directory.path().join("a.txt"), b"a");

    let plan = build_rename_plan(
        directory.path(),
        &ScanOptions::default(),
        &RenameOptions {
            pattern: Some("renamed_{number}{ext}".to_string()),
            width: 2,
            ..RenameOptions::default()
        },
    )
    .unwrap();

    assert_eq!(plan.actions.len(), 2);
    assert!(plan.actions[0].destination.ends_with("renamed_01.txt"));
    assert!(plan.actions[1].destination.ends_with("renamed_02.txt"));
}

#[test]
fn rename_conflicts_abort_before_changes() {
    let directory = tempdir().unwrap();
    write_file(&directory.path().join("a.txt"), b"a");
    write_file(&directory.path().join("existing.txt"), b"existing");

    let result = build_rename_plan(
        directory.path(),
        &ScanOptions::default(),
        &RenameOptions {
            pattern: Some("existing.txt".to_string()),
            ..RenameOptions::default()
        },
    );

    assert!(result.is_err());
    assert!(directory.path().join("a.txt").exists());
    assert!(directory.path().join("existing.txt").exists());
}

#[test]
fn dry_run_never_changes_files() {
    let directory = tempdir().unwrap();
    write_file(&directory.path().join("a.txt"), b"a");

    let plan = build_rename_plan(
        directory.path(),
        &ScanOptions::default(),
        &RenameOptions {
            pattern: Some("b.txt".to_string()),
            ..RenameOptions::default()
        },
    )
    .unwrap();
    let result = apply_operation(&plan, true).unwrap();

    assert!(result.dry_run);
    assert!(directory.path().join("a.txt").exists());
    assert!(!directory.path().join("b.txt").exists());
}

#[test]
fn organize_plan_uses_extension_buckets() {
    let directory = tempdir().unwrap();
    write_file(&directory.path().join("photo.JPG"), b"photo");
    write_file(&directory.path().join("notes.txt"), b"notes");

    let plan = build_organize_plan(
        directory.path(),
        &ScanOptions::default(),
        OrganizeBy::Extension,
    )
    .unwrap();

    assert!(plan
        .actions
        .iter()
        .any(|action| action.destination.ends_with("jpg/photo.JPG")));
    assert!(plan
        .actions
        .iter()
        .any(|action| action.destination.ends_with("txt/notes.txt")));
}

#[test]
fn metadata_cleaner_creates_output_without_touching_source() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("image.png");
    let output = directory.path().join("cleaned");
    let image = DynamicImage::ImageRgb8(image::ImageBuffer::from_pixel(2, 2, Rgb([1, 2, 3])));
    image.save_with_format(&source, ImageFormat::Png).unwrap();

    let preview = clean_images(
        directory.path(),
        Some(&output),
        &ScanOptions::default(),
        true,
    )
    .unwrap();
    assert_eq!(preview.cleaned.len(), 1);
    assert!(!output.join("image.png").exists());

    let result = clean_images(
        directory.path(),
        Some(&output),
        &ScanOptions::default(),
        false,
    )
    .unwrap();
    assert_eq!(result.cleaned.len(), 1);
    assert!(output.join("image.png").exists());
    assert!(source.exists());
}

#[test]
fn cancelled_scan_stops_before_touching_files() {
    let directory = tempdir().unwrap();
    write_file(&directory.path().join("file.txt"), b"contents");
    let context = OperationContext::default();
    let token: CancellationToken = context.cancellation.clone();
    token.cancel();

    let result = scan_with_context(directory.path(), &ScanOptions::default(), &context);

    assert!(matches!(
        result,
        Err(filepilot_core::FilePilotError::Cancelled)
    ));
    assert!(directory.path().join("file.txt").exists());
}
