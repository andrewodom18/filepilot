# FilePilot

[![CI](https://github.com/andrewodom18/filepilot/actions/workflows/ci.yml/badge.svg)](https://github.com/andrewodom18/filepilot/actions/workflows/ci.yml)
[![Latest Release](https://img.shields.io/github/v/release/andrewodom18/filepilot?sort=semver)](https://github.com/andrewodom18/filepilot/releases)

FilePilot is a privacy-first, cross-platform file-control suite for macOS, Windows, and Linux. It combines safe file renaming, folder organization, duplicate detection, large-file reporting, and image metadata cleaning in one product.

## v0.1.0

FilePilot v0.1.0 is a local-only command-line application. It does not upload files, collect telemetry, require an account, or delete duplicate files. Rename and organize operations show a complete preview, abort on conflicts, and require confirmation before changing files.

## Install

Download the archive for your platform from the [latest GitHub Release](https://github.com/andrewodom18/filepilot/releases). The first release targets:

- macOS arm64
- macOS x64
- Windows x64
- Linux x64

For development builds, install from the repository with Rust:

```bash
cargo install --path crates/filepilot-cli
```

The installed executable is named `filepilot`.

## Product vision

Make potentially destructive file operations understandable, previewable, and reversible. FilePilot should be useful from the command line first, with an optional graphical interface built on the same core.

## Quick start

Preview a directory before making changes:

```bash
filepilot scan ~/Downloads
filepilot large-files ~/ --top 20
filepilot duplicates ~/Pictures
filepilot rename ./photos --pattern "IMG_{date}_{number}{ext}" --dry-run
filepilot organize ./Downloads --by extension --dry-run
filepilot clean-metadata ./photos --remove all --dry-run
```

Read-only reports support `--format human`, `--format json`, and `--format csv`, plus `--output <path>`. Use `--yes` only when running a reviewed rename, organize, or metadata-cleaning operation non-interactively. Metadata cleaning writes new JPEG, PNG, or WebP files and never overwrites the source.

Run the same commands from a checkout with `cargo run -p filepilot-cli -- ...`.

Run validation with:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Included in v0.1.0

- Recursive directory scanning with hidden-file, symlink, and exclusion controls.
- Bulk renaming using templates, numbering, dates, and regular expressions.
- Folder organization by extension, modified date, or first filename character.
- Duplicate detection using size filtering, partial hashes, and full hashes.
- Large-file reporting with stable sorting and CSV/JSON export.
- JPEG, PNG, and WebP metadata-container removal without recompression.
- Dry-run previews and preflight conflict detection.
- Operation logs and undo for rename and organize operations.

## Example commands

```text
filepilot scan ~/Downloads
filepilot rename ~/Downloads --pattern "IMG_{date}_{number}{ext}"
filepilot organize ~/Downloads --by extension
filepilot duplicates ~/Pictures
filepilot large-files ~/ --top 50
filepilot clean-metadata ~/Pictures --remove all
filepilot undo
```

## v1 safety contract

- Never overwrite by default.
- Detect filename collisions before applying changes.
- Preserve Unicode filenames.
- Handle case-sensitive and case-insensitive filesystems safely.
- Provide clear warnings for permissions, locked files, and symlinks.
- Duplicate reporting never deletes or moves files.
- Metadata cleaning creates cleaned copies rather than changing originals.
- Keep a human-readable audit log for every operation.

## Technical direction

Rust provides the portable native core and single binaries across the three operating systems. The architecture separates the reusable core library from the CLI and any future desktop interface.

```text
filepilot/
  core/
  cli/
  desktop/
  rules/
  tests/
  docs/
  packaging/
```

The duplicate finder first groups by file size, then uses partial hashes for large files, and finally uses full hashes before reporting a duplicate. The metadata cleaner removes supported metadata containers while preserving the original image data instead of decoding and recompressing it.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

GitHub Actions runs the test suite on Linux, Windows, and both macOS architectures. Tagged releases publish platform archives through GitHub Releases.

## Future features

- Watch folders automatically.
- Add a graphical drag-and-drop interface.
- Schedule recurring cleanup jobs.
- Support duplicate deletion to the system trash.
- Add file tagging.
- Support cloud-storage folders.
- Add saved cleanup profiles.
- Add Finder, Explorer, and Linux file-manager integrations.

## Planned build order

1. Cross-platform path and file-operation library.
2. Read-only directory scanner.
3. Large-file reporter.
4. Duplicate finder.
5. Rename preview and execution.
6. Rule-based folder organizer.
7. Metadata cleaner.
8. Undo/audit system.
9. Packaging and release automation.
10. Optional desktop interface.
