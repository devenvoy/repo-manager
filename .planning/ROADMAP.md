# Release Desk roadmap

**Delivery order:** shared core → CLI → TUI → GPUI desktop → migration → ship.
Each phase gets a detailed `PLAN.md` and a verification report before the next
phase begins. The current snapshot-viewer prototype is not counted as a phase
completion.

| Phase | Goal | Requirements | Exit evidence |
| --- | --- | --- | --- |
| 1. Product contract and architecture | Complete reference parity map, domain boundaries, crate graph, schema, migration safety, UX states and verification strategy. | PAR-01, ARC-01 | Reviewed contract and trace matrix; no unresolved destructive decision. |
| 2. Pure domain | Implement validated project/repository/release/installer/PR/job/schedule/backup types and deterministic rules. | ARC-01, PROJ-01, GIT-01, REL-01, INST-01 | Focused edge-case unit tests; no I/O/UI dependency in domain. |
| 3. Application core | Use cases, ports, event stream, job queue, cancellation/retry/idempotency, offline and rate-limit policy. | ARC-01, REPO-01, GIT-02, REL-01, OFF-01, JOB-01 | Fake-backed use-case tests and one end-to-end tracer. |
| 4. Infrastructure | SQLite/migrations, GitHub, Git, managed files, OS credentials, scheduler, archives, notifications and the read-only ArtCraft snapshot adapter. | PROJ-02, REPO-01, GIT-01, GIT-02, REL-01, INST-01, PR-01, AUTH-01, OFF-01, JOB-01, SCHED-01, DATA-01, BACK-01, MIG-01, OPS-01 | Real temporary DB/filesystem integration tests and failure recovery tests. ArtCraft write attempts are rejected. |
| 5. CLI | Complete commands over shared core, first-run setup with library picker and automatic first-project ArtCraft JSON link, scriptable output, help/completions. | PROJ-01, PROJ-02, REPO-01, GIT-01, REL-01, INST-01, PR-01, AUTH-01, OFF-01, SCHED-01, BACK-01, CLI-01, OPS-01, MIG-01 | End-to-end CLI tests exercise creation, initial read-only link, discovery, sync, installers, backup/restore and errors. |
| 6. TUI | Full interactive client over the same use cases and event stream. | TUI-01, PROJ-01, REPO-01, INST-01, PR-01, JOB-01, BACK-01 | Keyboard/mouse/resize/error/offline tests and terminal restoration. |
| 7. Desktop GUI | Full GPUI product: splash, project hub/wizard, workspace, installer shelf, PRs, jobs, schedules, backups, Settings, diagnostics and system integration. | GUI-01, GUI-02, PROJ-01, PROJ-02, REPO-01, GIT-01, REL-01, INST-01, PR-01, AUTH-01, OFF-01, JOB-01, SCHED-01, BACK-01, OPS-01 | Visual/interaction review in a real window for system/light/dark and reduced motion; workflow tests; no placeholders. |
| 8. Legacy migration | Detect and preview other old setups; offer optional adoption into managed storage only after a new explicit user decision. The first ArtCraft link is already working from Phase 5. | MIG-01, PAR-01 | Integrity reports prove no ArtCraft write; migration tests cover interrupted/duplicate data. |
| 9. Release | Feature-parity closure, security/performance/reliability gates, docs, packaging, signing/notarization and update path. | PAR-01, SHIP-01 | Signed and notarized macOS installable build, clean-room test, Linux/Windows build plan. Release remains gated until signing credentials are supplied. |

## Dependencies

1 precedes all code work. 2 precedes 3; 3 precedes 4; 4 precedes the CLI.
The TUI and GUI consume the stable application contracts proven through the
CLI. Packaging follows feature parity and production verification. A narrow
end-to-end tracer is required inside phases 3 and 4, but it must use the same
architecture planned for the final product.

## Risk and decision gates

- **One-way:** writing, moving, or deleting files under ArtCraft is forbidden
  by D-03. Phase 4 must implement an explicit read-only adapter and a write
  boundary that rejects canonical paths under ArtCraft, including symlinked
  paths. A future migration must obtain a separate explicit decision.
- **Costly:** library relocation needs preview, progress, rollback/recovery,
  and integrity verification. Library location is chosen by the user at first
  run (D-04).
- **Costly:** signing/notarization and cloud backup provider selection are
  deferred until their concrete release/backup plans are reviewable. Signing
  credentials are required for the Phase 9 release gate, not optional scope.
