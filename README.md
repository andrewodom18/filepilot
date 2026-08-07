# FilePilot

[![CI](https://github.com/andrewodom18/filepilot/actions/workflows/ci.yml/badge.svg)](https://github.com/andrewodom18/filepilot/actions/workflows/ci.yml)
[![Latest Release](https://img.shields.io/github/v/release/andrewodom18/filepilot?sort=semver)](https://github.com/andrewodom18/filepilot/releases)

FilePilot is a privacy-first desktop app and CLI for safe file control on macOS, Windows, and Linux. Scan storage, find duplicates, preview renames, organize folders, and clean image metadata without uploading files or requiring an account.

## FilePilot 2.0.2

The v2 desktop app is a local-only Tauri application with a React interface. It shares the same Rust core as the CLI and includes:

- Read-only recursive scans with warnings, sortable table columns, and a display-only minimum-size filter.
- The CLI large-files report remains available for scripting and JSON/CSV export.
- macOS System Data analysis that identifies large user-library, shared-library, and system-working-data contributors, with constrained Trash cleanup for selected user cache and log entries.
- Duplicate detection using size grouping, partial hashes, and full hashes, with explicit desktop selection to move unwanted copies to the recoverable system Trash.
- Template and regex batch renaming.
- Organization by extension, modified date, or filename.
- JPEG, PNG, and WebP metadata cleaning to new copies.
- Dry runs, previews, collision checks, staging, audit logs, and undo.
- Background progress, cancellation for analysis tasks, and a unified Activity view.

FilePilot does not include authentication, a server, a database, telemetry, cloud uploads, or an auto-updater. Settings and recent folders are stored locally in platform-standard application data directories.

## Download

Download the latest platform installer from [GitHub Releases](https://github.com/andrewodom18/filepilot/releases):

- macOS arm64: `.dmg`
- macOS x64: `.dmg`
- Windows x64: NSIS installer `.exe`
- Linux x64: `.AppImage`

Each release also includes CLI archives and `SHA256SUMS.txt`. macOS and Windows artifacts may show platform security warnings when signing credentials are not configured; verify the checksum and review the release notes before opening an unsigned artifact.

## CLI installation

The CLI remains a first-class interface for scripts and automation. Download its platform archive from the release page or build it locally:

```bash
cargo install --path crates/filepilot-cli
```

The executable is named `filepilot`.

## CLI quick start

```bash
filepilot scan ~/Downloads
filepilot large-files ~/ --top 20
filepilot duplicates ~/Pictures
filepilot rename ./photos --pattern "IMG_{date}_{number}{ext}" --dry-run
filepilot organize ./Downloads --by extension --dry-run
filepilot clean-metadata ./photos --remove all --dry-run
```

Read-only reports support `--format human`, `--format json`, and `--format csv`, plus `--output <path>`. Use `--yes` only after reviewing a rename, organization, or metadata-cleaning preview. Metadata cleaning writes new copies and never overwrites the source.

## Desktop development

Requirements:

- Rust stable
- Node.js 20 or newer
- Tauri 2 platform prerequisites

Start the desktop app from the repository root:

```bash
cd apps/filepilot-desktop
npm install
npm run tauri:dev
```

Run the frontend checks:

```bash
npm test
npm run build
```

Build a local desktop bundle with `npm run tauri:build`. The Tauri shell is under `apps/filepilot-desktop/src-tauri`; reusable filesystem behavior remains in `crates/filepilot-core`, and desktop task/settings services live in `crates/filepilot-app`.

## Safety contract

- Never overwrite by default.
- Preview and preflight every mutating batch before it starts.
- Ask for explicit confirmation immediately before every desktop mutation, including undo and Trash cleanup.
- Abort before mutation when a collision, invalid path, or stale source is detected.
- Stage rename and organization batches so swaps are safe.
- Do not follow symlinks by default.
- Keep originals intact during metadata cleaning.
- Report permission failures as warnings where possible.
- Duplicate cleanup is never automatic: the desktop app requires explicit file selection and confirmation, revalidates size and full hash, keeps at least one file in every group, and moves selected files only to the recoverable system Trash. The CLI duplicate command remains report-only.
- Disable cancellation once a mutation batch begins.
- Keep an operation log and refuse unsafe undo when a destination changed.
- Keep System Data analysis read-only by default; optional cleanup only moves explicitly selected direct children of the current user's cache or log folders to the system Trash and never touches backups, system files, snapshots, or personal data.

## Privacy and security

FilePilot is designed for local-only use. The desktop UI does not request arbitrary frontend filesystem access, shell execution, network access, or remote content. Rust validates all paths and operations before touching files. See [SECURITY.md](SECURITY.md) for reporting guidance.

## Validation

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd apps/filepilot-desktop
npm test
npm run build
npm run tauri:build -- --no-bundle
```

GitHub Actions runs Rust and frontend checks on Linux, Windows, and both macOS runner targets. Tagged releases build desktop installers, CLI archives, and checksums for the supported architectures.

## Repository layout

```text
filepilot/
├── apps/filepilot-desktop/       # Tauri 2 shell and React UI
├── crates/filepilot-app/         # background tasks, settings, UI DTOs
├── crates/filepilot-core/        # terminal-independent filesystem engine
└── crates/filepilot-cli/         # first-class command-line interface
```

## Roadmap

- Watch folders and scheduled workflows.
- Safe, user-confirmed cleanup actions for clearly understood cache and temporary-data categories.
- Saved cleanup profiles.
- File tagging and file-manager integrations.
- Signed release artifacts and an opt-in updater after the desktop release is stable.
