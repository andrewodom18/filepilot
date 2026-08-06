# Security Policy

## Supported versions

The latest release on the `main` branch is supported for security fixes. Older releases may not receive fixes.

## Reporting a vulnerability

Please do not disclose a suspected vulnerability in a public issue. Use GitHub's private vulnerability reporting for this repository when available. If it is unavailable, contact the repository owner privately through GitHub with:

- A description of the issue
- Affected version and platform
- Reproduction steps or a proof of concept
- Any suggested mitigation

FilePilot is a local desktop app and CLI. It does not provide a network service, upload user files, or require credentials. Security reports should still cover path traversal, unintended file modification, unsafe overwrite behavior, symlink handling, metadata leakage, Tauri capability expansion, frontend-to-Rust command validation, and release-artifact integrity.
