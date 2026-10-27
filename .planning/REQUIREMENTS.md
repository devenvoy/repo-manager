# Release Desk requirements

The user-provided `desktop-app-agent-prompt.md` is the detailed product
specification. These IDs make delivery and verification traceable. All entries
are required unless explicitly marked as a later platform release.

| ID | Requirement and acceptance condition | Phase |
| --- | --- | --- |
| PAR-01 | Map every legacy HTML/script feature, status, rule, and edge case to a new use case or an explicit correction; no silent omissions. | 1, 9 |
| ARC-01 | One-way multi-crate dependencies: pure domain → application contracts → infrastructure/composition → presentations; automated boundary check. | 1–4 |
| PROJ-01 | Multiple projects for GitHub organizations or users; create, validate, rename, duplicate, archive, restore, delete, switch, import/export. | 2–5, 7 |
| PROJ-02 | First-run library picker and safe relocation. The ArtCraft-linked first project reads its JSON without writing anywhere in ArtCraft. | 4, 5, 7–8 |
| REPO-01 | Discover all repositories with pagination; tracking and ignore choices, independent source/installer/PR switches, new-repository inbox, bulk/filter/search actions, rename/transfer continuity. | 3–5, 7 |
| GIT-01 | Clone and fetch tracked source with clean/default-branch fast-forward only; distinguish not cloned, current, behind, ahead, diverged, dirty, detached, error and show revisions. | 2–5, 7 |
| GIT-02 | Concurrent, cancellable, retryable sync with progress and cross-process lock; no token in command arguments or logs. | 3–5 |
| REL-01 | Batched/cached/paginated release and asset retrieval, rate-limit awareness, draft exclusion, pre-release choice, newest published eligible installer, architecture labels. | 2–5, 7 |
| INST-01 | Resumable verified downloads into managed storage, staging and atomic install, per-architecture files, retention/archive, manual import, update-all, failure recovery. | 2–5, 7 |
| PR-01 | Cached pull requests with all/open/merged/closed/draft filters, author, labels, branches, dates and links. | 4–7 |
| AUTH-01 | PAT, GitHub device flow, and existing `gh` credentials; OS secret store, multiple credentials, scope/expiry status. | 4–5, 7 |
| OFF-01 | Cached and downloaded content remains usable offline, with data age and clear error/retry guidance; API quota and reset shown. | 3–7 |
| JOB-01 | Shared job queue, priority/concurrency, progress events, cancellation, retry/backoff, idempotency and persistent history. | 3–7 |
| SCHED-01 | Per-project schedules and missed-run behavior, background service integration, system notifications, cross-process coordination. | 4–7 |
| DATA-01 | Versioned SQLite schema/migrations, integrity and recovery, portable relative paths, backup before migration. | 4 |
| BACK-01 | Verified portable backups and restores, project import/export, automatic local backup/retention, report exports; no secrets. | 4–7 |
| CLI-01 | Complete scriptable commands for every use case with selectors, JSON, dry-run, exit codes, completion/man pages, non-interactive safety. | 5 |
| TUI-01 | Full-screen project/repository/installer/PR/job/backup/settings flows, keyboard and mouse navigation, SSH/low-color support, safe terminal restore. | 6 |
| GUI-01 | GPUI splash, hub, creation wizard, full workspace, job manager, backup/restore, diagnostics, global and project Settings; native dialogs and accessibility. | 7 |
| GUI-02 | New coherent visual system with system/light/dark modes, responsive layouts, real progress and state animation, reduced-motion support, no blocking UI work. | 7 |
| MIG-01 | Read-only link to ArtCraft data as first project; optional verified migration tools never modify ArtCraft without a new explicit decision. | 4, 8 |
| OPS-01 | Health checks, searchable structured activity logs, redacted diagnostics, disk/credential/database/schedule status. | 4–7 |
| SHIP-01 | Focused unit/integration/CLI/TUI/GUI/failure tests, format/lint/security gates, macOS signed packaging and documentation; Linux/Windows build targets planned. | Every phase, 9 |

## Global acceptance rules

Every phase must state what is actually runnable, attach test evidence, update
the parity matrix, and mark missing work plainly. Screens that only display
placeholder controls do not satisfy a feature. The GUI milestone requires a
running-window visual and interaction review for first run, empty, loading,
success, error, offline, and large-list states.
