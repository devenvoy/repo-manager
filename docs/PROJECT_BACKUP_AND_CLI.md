# Project Local data.json Backup, Reconciliation & CLI Engine

## Overview

In Phase 5, Release Desk implemented a resilient, decentralized project backup snapshot engine and a complete modular CLI (`release-desk-cli`). 

Whenever a project is created with a clone directory, Release Desk automatically initializes and maintains a local `<project_root>/.release-desk/data.json` backup snapshot inside the project folder. This provides two major guarantees:
1. **Mismatch Detection**: Detects discrepancies between the centralized SQLite database and the project folder's on-disk state (missing repositories, diverged head commit revisions, or tracking changes).
2. **Disaster Recovery**: If the SQLite database is ever lost, corrupted, or migrated, the complete project state can be seamlessly restored directly from the project's local `data.json` backup.

---

## Architectural Components

### 1. Pure Domain (`release-desk-core`)
- **`ProjectIdentity`**: Holds stable `id: ProjectId`, `name: ProjectName`, and `clone_location: Option<PathBuf>`.
- **`MismatchReport` & `RevisionMismatch`**: Pure value types summarizing synchronization state:
  - `matched_repositories: Vec<String>`
  - `missing_in_database: Vec<String>`
  - `missing_on_disk: Vec<String>`
  - `revision_mismatches: Vec<RevisionMismatch>`
  - `is_synchronized(&self) -> bool`
- **`ProjectBackupPort`**: Abstract port definition for reading, writing, and mismatch checking without tying domain to filesystem specifics.

### 2. Application Core (`release-desk-application`)
- **`CreateProjectUseCase`**: Accepts `clone_location`. When present, initializes and verifies the `<clone_location>/.release-desk/data.json` backup snapshot.
- **`ReconcileProjectSnapshotUseCase`**:
  - `check_mismatch(project_id)`: Compares SQLite records against the local backup snapshot.
  - `restore_database_from_backup(project_id)`: Imports all repositories recorded in `data.json` into the database.
  - `sync_backup_from_database(project_id)`: Exports active database state to the local `data.json` backup snapshot.

### 3. Infrastructure Adapter (`release-desk-store`)
- **`ProjectSnapshotManager`**:
  - Implements `ProjectBackupPort`.
  - Atomically writes `data.json` via temporary files (`tempfile::NamedTempFile`) to guarantee zero partial-write corruption.
  - Enforces the ArtCraft security boundary: strictly rejects any attempt to place or write backups inside `/Volumes/Devansh2/codes/ArtCraft/`.
- **SQLite Schema Migration v2**:
  - Adds `clone_location TEXT` to the `projects` table.

### 4. Modular CLI (`release-desk-cli`)
Decoupled into single-responsibility modules:
- `src/args.rs`: Clap command line argument parser definitions.
- `src/context.rs`: Dependency injection wiring and runtime environment.
- `src/output.rs`: Structured machine-readable JSON (`--json`) and human-formatted output.
- `src/commands/`:
  - `init.rs`: First-run library initialization with read-only snapshot linking.
  - `project.rs`: `create`, `list`, and `reconcile` (with `--restore-db` and `--sync-backup`).
  - `auth.rs`: Multi-host login (GitHub, GitLab, Bitbucket) supporting `--token` and `--browser` with automatic webview modal fallback.
  - `discovery.rs`: Discovers repositories for connected accounts.
  - `track.rs`: Toggles tracking preferences (`source`, `installers`, `pr`).
  - `sync.rs`: Safe clone and fast-forward sync (with dirty working tree protection and `--dry-run`).
  - `installers.rs`: Inspects and downloads installer packages.
  - `pr.rs`: Lists pull requests with filtering.
  - `backup.rs`: Exports compressed `.tar.gz` archive.

---

## Verification & Safety Evidence

All automated verification gates pass with zero warnings:

- **Formatting Check**: `cargo fmt --check` (100% compliant).
- **Linter Quality**: `cargo clippy --all-targets -- -D warnings` (0 warnings).
- **Unit and Integration Test Suite**:
  - `release-desk-application`: 11 unit tests + 1 full lifecycle tracer test.
  - `release-desk-core`: 25 pure domain tests.
  - `release-desk-git`: 7 git adapter tests.
  - `release-desk-store`: 20 adapter tests + 6 ArtCraft safety boundary tests + 1 real SQLite integration test + 4 multi-host auth tests.
  - `release-desk-cli`: 2 end-to-end binary integration tests (`cli_e2e_test.rs`, `commands.rs`).
  - `release-desk-tui`: 1 keyboard navigation test.
  - `repo-manager`: 1 UI settings validation test.
  - **Total**: 79 tests passing.
- **ArtCraft Boundary Protection**: Real SHA-256 hash assertions confirm `/Volumes/Devansh2/codes/ArtCraft/.release-desk/data.json` was never written to or modified.
