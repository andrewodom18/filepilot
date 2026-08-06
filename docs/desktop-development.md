# Desktop development

FilePilot Desktop is a Tauri 2 application with a React, TypeScript, and Vite frontend. It is intentionally local-only: the frontend invokes a small Rust command surface, while all filesystem access remains in Rust.

## Architecture

```text
React UI
  └── Tauri commands and task events
        └── filepilot-app task registry and settings
              └── filepilot-core scanner and safe operations
```

The core crate does not depend on Tauri, a terminal, or frontend behavior. Long-running commands return a task ID immediately. The desktop worker emits `task-updated` events containing progress, warnings, completion, cancellation, and failure states.

## System Data analysis

The System Data workflow is a macOS-only, read-only analyzer. It measures direct contributors under the current user's `~/Library`, `/Library`, and `/private/var` locations, shows the largest child locations for broad categories, and labels each result as likely safe to review, review before removing, personal data, system managed, or unknown.

The labels are guidance, not proof that a path can be deleted. FilePilot does not remove anything from this workflow. APFS purgeable space, Time Machine local snapshots, and other macOS-managed storage are called out as limits because their reclaimable size cannot be safely established from an ordinary file scan. On Windows and Linux the workflow explains that this macOS-specific analysis is unavailable.

## Local run

```bash
cd apps/filepilot-desktop
npm install
npm run tauri:dev
```

The frontend-only build is useful for checking TypeScript and CSS:

```bash
npm test
npm run build
```

## Release artifacts

The release workflow builds the existing CLI and the Tauri app for macOS arm64/x64, Windows x64, and Linux x64. Desktop bundles are DMG, NSIS, and AppImage. The workflow also publishes portable CLI archives and SHA-256 checksums.

Signing hooks are intentionally kept in the workflow boundary. No signing keys or credentials belong in this public repository. Until repository secrets are configured, release notes must disclose when an artifact is unsigned.

## Capability boundary

The default Tauri capability grants only the core window permissions and the native dialog plugin. The UI does not use a frontend filesystem plugin, shell plugin, HTTP client, remote script, account, or telemetry service. The System Data analyzer uses a fixed macOS location allowlist and never executes a user-supplied command. All paths supplied by the UI are treated as untrusted input and validated again by Rust.
