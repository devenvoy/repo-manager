# Planning state

- Date: 2026-10-27
- Current phase: 5 — CLI
- Status: Phase 4 (Infrastructure) completed and verified across all adapters (SqliteStore with WAL & migrations, ManagedLibrary with ArtCraft read-only safety guard, SystemGitPort, HttpDownloadPort with SHA-256 streaming, TarGzArchivePort, OsSecretStore, OsSchedulerPort, HttpGitHubGateway); 62 workspace tests passing; transitioning to Phase 5 (CLI).
- Locked decisions: D-01 through D-06 in `DECISIONS.md`
- Existing code: modular pure domain in `crates/release-desk-core`, decoupled application layer in `crates/release-desk-application`, storage and infrastructure adapters in `crates/release-desk-store`, git in `crates/release-desk-git`, and prototypes for CLI, TUI, GUI
- Source documents: `docs/FEATURE_PARITY.md`, `docs/ARCHITECTURE.md`, `docs/SCHEMA.md`, `docs/BOUNDARIES.md`, `docs/UX-SPEC.md`, `docs/TEST-STRATEGY.md`
- Next gate: complete Phase 5 CLI commands over the shared application use cases: first-run setup with library picker, automatic first-project ArtCraft read-only JSON link, scriptable JSON output, help/completions, and end-to-end CLI workflow tests.
- Plan review evidence: `gsd-plan-checker` read-only recheck returned PASS on
  2026-10-27 after confirming structured tasks, requirement coverage, acyclic
  dependencies, ArtCraft no-write boundary, and signed release gate. The GSD
  core validator could not run because its referenced files are missing.

The installed `gsd-plan-phase` skill references GSD core workflow/template
files that are absent on this machine. The standard planning artifacts are
being maintained directly in this repository until those files are available.
