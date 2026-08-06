# FilePilot

FilePilot is a privacy-first, cross-platform file-control suite for macOS, Windows, and Linux. It combines safe file renaming, folder organization, duplicate detection, large-file reporting, and image metadata cleaning in one product.

## Product vision

Make potentially destructive file operations understandable, previewable, and reversible. FilePilot should be useful from the command line first, with an optional graphical interface built on the same core.

## Core modules

- Bulk file renaming using templates, numbering, dates, and regular expressions.
- Folder organization by extension, filename, date, or custom rules.
- Duplicate detection using size filtering and cryptographic hashes.
- Large-file reporting with sorting, filtering, and CSV/JSON export.
- Image metadata cleaning for GPS, camera, date, software, thumbnails, or all EXIF data.
- File search and filtering.
- Operation history and undo support.
- Dry-run previews before changes are applied.
- Optional backups before destructive operations.

## Example commands

```text
filepilot rename ~/Downloads --pattern "IMG_{date}_{number}"
filepilot organize ~/Downloads --by extension
filepilot duplicates ~/Pictures
filepilot large-files ~/ --top 50
filepilot clean-metadata ~/Pictures --remove-location
filepilot undo
```

## MVP

1. Scan a selected directory.
2. Display a preview of all proposed changes.
3. Require explicit confirmation before modifying files.
4. Support dry runs.
5. Store an operation log for undo.
6. Ignore hidden files, system folders, symlinks, and excluded paths by default.
7. Export scan results to JSON or CSV.
8. Implement the five core modules.

## Safety requirements

- Never overwrite by default.
- Detect filename collisions before applying changes.
- Preserve Unicode filenames.
- Handle case-sensitive and case-insensitive filesystems safely.
- Provide clear warnings for permissions, locked files, and symlinks.
- Move deletions to the system trash/recycle bin when possible.
- Default the metadata cleaner to creating a cleaned copy rather than changing the original.
- Keep a human-readable audit log for every operation.

## Technical direction

Rust is a strong candidate for a portable native core and single binaries across the three operating systems. The architecture should separate the core library from the CLI and any future desktop interface.

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

The duplicate finder should first group by file size, then use partial hashes for large files, and finally use full hashes before reporting a duplicate. The metadata cleaner should preserve image content and remove only explicitly selected metadata fields.

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

