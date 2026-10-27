# Specification Feature Inventory

> Extracted from `desktop-app-agent-prompt.md`. Each feature has a stable ID
> (`SPEC-NNN`), a category, a brief description, and a classification as **New**
> (no equivalent in the legacy tool) or **Replace** (supersedes legacy behavior).

---

## Projects

| ID | Category | Description | Type |
|----|----------|-------------|------|
| SPEC-001 | Projects | Create, rename, duplicate, archive, restore, and delete projects | New |
| SPEC-002 | Projects | Deletion prompt shows disk size that would be freed and asks whether to remove local clones/installers | New |
| SPEC-003 | Projects | Each project targets one GitHub account (org or personal user), validated on entry | New |
| SPEC-004 | Projects | Detect whether account is organization or user; show avatar, display name, repo counts | New |
| SPEC-005 | Projects | Per-project settings: pre-release inclusion, show archived repos, concurrency limits, installer retention count, file-type filters, auto-refresh/auto-sync schedule | New |
| SPEC-006 | Projects | Managed library root with automatic per-project folder layout (account → repo → source/current/archived) | Replace |
| SPEC-007 | Projects | Library root relocation with safe move/re-link, progress, and integrity verification | New |

## Repositories

| ID | Category | Description | Type |
|----|----------|-------------|------|
| SPEC-008 | Repositories | Discover all repos of a project's account | Replace |
| SPEC-009 | Repositories | Track/ignore individual repos with add, remove, bulk select, search, and filters (all, has installers, has source clone, archived, private, language) | New |
| SPEC-010 | Repositories | Independent per-repo tracking switches: source code, installers, pull requests | New |
| SPEC-011 | Repositories | Removing a tracked repo offers to keep or delete its local data | New |
| SPEC-012 | Repositories | Newly discovered repos appear in an "inbox" for review instead of being silently added | New |
| SPEC-013 | Repositories | Option to auto-track new repositories | New |
| SPEC-014 | Repositories | Remember rename/transfer events so history and local files are preserved | New |

## Source Code Sync (Git)

| ID | Category | Description | Type |
|----|----------|-------------|------|
| SPEC-015 | Git | Clone missing tracked repositories | Replace |
| SPEC-016 | Git | Fast-forward existing repos only when on default branch with clean working tree | Replace |
| SPEC-017 | Git | Never overwrite local work | Replace |
| SPEC-018 | Git | Report per-repo state: not cloned, current, behind, ahead, diverged, dirty, detached, error (with local/remote revision IDs) | Replace |
| SPEC-019 | Git | Native Git library or system Git behind a single abstraction | New |
| SPEC-020 | Git | Secure authentication: no tokens in process arguments or logs | New |
| SPEC-021 | Git | Concurrent clones/fetches up to configurable limit with cancellation, retry with backoff, per-repo progress | Replace |
| SPEC-022 | Git | Native actions: open in file manager, open in terminal, copy clone address, update all | Replace |

## Releases & Installers

| ID | Category | Description | Type |
|----|----------|-------------|------|
| SPEC-023 | Releases | Fetch releases using batched GitHub API (GraphQL) and conditional requests; minimize API calls | Replace |
| SPEC-024 | Releases | Cache results in DB with timestamps and configurable freshness window | New |
| SPEC-025 | Releases | Release selection rules: skip drafts, honor pre-release setting, newest by publication date | Replace |
| SPEC-026 | Releases | Group installer files per release with architecture labels derived from file names (Apple Silicon / Intel / universal) | Replace |
| SPEC-027 | Releases | Per-repo installer status: no installer, new, update available, missing locally, latest, downloading, failed | Replace |
| SPEC-028 | Releases | Aggregate status counts across a project | New |
| SPEC-029 | Releases | Resumable, verified downloads: stream to temp, check size, compute checksum, atomic move into place | Replace |
| SPEC-030 | Releases | Parallel downloads with pause, resume, cancel, bandwidth limit, and retry | New |
| SPEC-031 | Releases | Superseded installers archived per retention setting; prune beyond retention count | Replace |
| SPEC-032 | Releases | Installer manifest stored in database (not loose files) | New |
| SPEC-033 | Releases | "Download all updates" for a project, selection, or all projects | Replace |
| SPEC-034 | Releases | Drag-and-drop or file-picker import of externally obtained installers, matched to repo/version | New |
| SPEC-035 | Releases | Show rendered release notes (sanitized markdown), links open in system browser | New |
| SPEC-036 | Releases | Release history per repo with ability to download older versions on demand | New |

## Pull Requests

| ID | Category | Description | Type |
|----|----------|-------------|------|
| SPEC-037 | PRs | List PRs per tracked repo with state filters (all, open, merged, closed, draft), labels, author, branches, dates | Replace |
| SPEC-038 | PRs | Refreshed on same schedule and cached in database | New |
| SPEC-039 | PRs | Link out to web page for each PR | Replace |

## Rate Limits, Errors & Offline

| ID | Category | Description | Type |
|----|----------|-------------|------|
| SPEC-040 | Diagnostics | Track and display remaining API quota and reset time per credential | Replace |
| SPEC-041 | Diagnostics | Automatic back-off when throttled with clear explanation | Replace |
| SPEC-042 | Diagnostics | Translate every failure (network, permission, missing-repo, expired-token, rate-limit) into human-readable message with suggested remedy; never show raw responses | New |
| SPEC-043 | Diagnostics | Full offline usability for cached/downloaded data; stale data marked with age | New |

## Authentication & Secrets

| ID | Category | Description | Type |
|----|----------|-------------|------|
| SPEC-044 | Auth | Sign in with personal access token | Replace |
| SPEC-045 | Auth | Sign in with device-flow browser sign-in | New |
| SPEC-046 | Auth | Detect existing credential from GitHub CLI (`gh`) if present | Replace |
| SPEC-047 | Auth | Store secrets only in OS secure credential store (Keychain / credential manager) | New |
| SPEC-048 | Auth | Never write secrets to database, config, logs, exports, or backups | New |
| SPEC-049 | Auth | Support multiple credentials; assign one per project | New |
| SPEC-050 | Auth | Show scopes, expiry, and warn before expiry | New |

## Scheduling & Background Operation

| ID | Category | Description | Type |
|----|----------|-------------|------|
| SPEC-051 | Schedules | Built-in scheduler per project (every N min/hours, daily, every few days, weekly, monthly, at chosen time) replaces cron editing | Replace |
| SPEC-052 | Schedules | "Run missed jobs on next launch" option | New |
| SPEC-053 | Schedules | Jobs stored in DB with last run, next run, result, and duration | New |
| SPEC-054 | Schedules | Desktop app minimizes to system tray / menu bar and runs scheduled work in background | New |
| SPEC-055 | Schedules | Notifications when new releases appear or a sync fails | New |
| SPEC-056 | Schedules | CLI can run a single scheduled pass or run as a long-lived background service | New |
| SPEC-057 | Schedules | CLI can install/remove an OS-level launch entry (launch agent, systemd unit, scheduled task) | Replace |
| SPEC-058 | Schedules | Cross-process lock so GUI, CLI, TUI, and background runs never overlap on same project | New |
| SPEC-059 | Schedules | Stale lock detection and recovery from crashed processes | New |

## Backup, Export & Import

| ID | Category | Description | Type |
|----|----------|-------------|------|
| SPEC-060 | Backups | Full backup: single portable archive (DB, settings, tracking lists, schedules, history) | New |
| SPEC-061 | Backups | Optional inclusion of source clones and installers with estimated size shown; option for latest-only | New |
| SPEC-062 | Backups | Secrets never included in backups | New |
| SPEC-063 | Backups | Project export: small shareable file with account, tracked repos, settings, schedules | New |
| SPEC-064 | Backups | Project import: validate, preview, detect conflicts, merge/replace/copy options | New |
| SPEC-065 | Backups | Restore from archive with integrity verification, dry-run preview, automatic safety snapshot before overwriting | New |
| SPEC-066 | Backups | Scheduled automatic backups with retention rules and configurable destination (local, external, network) | New |
| SPEC-067 | Backups | Pluggable cloud storage abstraction with local-folder implementation and defined interface for additional providers | New |
| SPEC-068 | Backups | Data export of tracking tables and release history to tabular/structured formats | New |
| SPEC-069 | Backups | Backups and restores are streamable, resumable, cancellable; never load huge files fully into memory | New |

## Logs, Activity & Diagnostics

| ID | Category | Description | Type |
|----|----------|-------------|------|
| SPEC-070 | Diagnostics | Persistent searchable activity log per project and global, with levels, timestamps, repo refs, durations | New |
| SPEC-071 | Diagnostics | "Export diagnostics" bundle that redacts secrets and personal paths | New |
| SPEC-072 | Diagnostics | Health check: verify Git availability, credential validity, disk space, library permissions, DB integrity, schedule status | New |
| SPEC-073 | Diagnostics | One-click repair suggestions from health check | New |

## CLI

| ID | Category | Description | Type |
|----|----------|-------------|------|
| SPEC-074 | CLI | Subcommand structure covering every use case (project, account, repo, sync, installers, PRs, schedules, backup, restore, import, export, health, config, credentials, library, diagnostics) | New |
| SPEC-075 | CLI | Project selector flag on every command | New |
| SPEC-076 | CLI | Non-interactive operation with explicit flags; interactive prompts when run in terminal and flags missing | New |
| SPEC-077 | CLI | Dry-run for destructive/large operations; confirmation prompts with force override | New |
| SPEC-078 | CLI | Clear exit codes documented per command | New |
| SPEC-079 | CLI | Human-readable tables with progress bars and machine-readable structured output | New |
| SPEC-080 | CLI | Respect color preferences and terminal width; quiet and verbose levels | New |
| SPEC-081 | CLI | Shell completion generation for common shells and generated man pages | New |
| SPEC-082 | CLI | Safe for cron/CI: no prompts when non-interactive, deterministic output, proper locking | Replace |
| SPEC-083 | CLI | Legacy migration command: import old script's directory structure, data snapshot, installer folders/manifests into a project without re-downloading | Replace |

## TUI

| ID | Category | Description | Type |
|----|----------|-------------|------|
| SPEC-084 | TUI | Full-screen interactive interface mirroring GUI functional areas | New |
| SPEC-085 | TUI | Vim-style and arrow-key navigation | New |
| SPEC-086 | TUI | Help overlay listing every key binding | New |
| SPEC-087 | TUI | Command palette | New |
| SPEC-088 | TUI | Mouse support where terminal allows | New |
| SPEC-089 | TUI | Resize handling and live progress via shared event stream | New |
| SPEC-090 | TUI | Works over remote shells and low-color terminals; never corrupts terminal on exit or panic | New |

## GUI

| ID | Category | Description | Type |
|----|----------|-------------|------|
| SPEC-091 | GUI | GPUI framework with GPUI component library | New |
| SPEC-092 | GUI | Splash screen with honest startup progress (resolve dirs, open/migrate DB, verify library, load credentials, check connectivity, restore window state) | New |
| SPEC-093 | GUI | Splash handles failure states (corrupt DB, locked DB, missing library root, unavailable credential store) with recovery choices | New |
| SPEC-094 | GUI | Project hub: list of projects with summary counts, last sync, pending updates, health; create/import/restore entry points; persistent project switcher | New |
| SPEC-095 | GUI | Project creation wizard: choose credential → enter/validate account → preview repos → select tracking → choose folders/schedule → confirm | New |
| SPEC-096 | GUI | Project workspace with sidebar and tabbed areas: Overview, Repositories, Installers, Pull Requests, Schedules, Backups, Activity Log, Project Settings | New |
| SPEC-097 | GUI | Repository master list with filters, search, multi-select, bulk actions, detail pane (source status, releases, PRs, local files) | New |
| SPEC-098 | GUI | Installer shelf: all repos with status, per-architecture files, download, update all, drag-and-drop import, open folder | New |
| SPEC-099 | GUI | Global settings: library root, credentials, appearance, defaults, notifications, tray, network/proxy, bandwidth, logging, app updates, diagnostics | New |
| SPEC-100 | GUI | Backup and restore center | New |
| SPEC-101 | GUI | Import and export dialogs | New |
| SPEC-102 | GUI | Job manager panel: running, queued, finished jobs with cancel and retry | New |
| SPEC-103 | GUI | Command palette for keyboard-driven use | New |
| SPEC-104 | GUI | Native app menus and keyboard shortcuts for every major action | New |
| SPEC-105 | GUI | Full keyboard navigation, accessible labels, focus order | New |
| SPEC-106 | GUI | Window state persistence | New |
| SPEC-107 | GUI | Multi-window support (open second project side by side) | New |
| SPEC-108 | GUI | Drag and drop of files and folders; native file dialogs | New |
| SPEC-109 | GUI | Never block UI thread; every operation reports progress, supports cancellation, shows errors inline | New |
| SPEC-110 | GUI | Virtualized lists smooth with thousands of rows | New |
| SPEC-111 | GUI | High-density display support and varied window sizes | New |
| SPEC-112 | GUI | System tray / menu bar presence | New |
| SPEC-113 | GUI | Native notifications | New |
| SPEC-114 | GUI | "Reveal in file manager" action | New |
| SPEC-115 | GUI | Deep links to open a project | New |
| SPEC-116 | GUI | Single-instance behavior forwarding activation to running instance | New |
| SPEC-117 | GUI | Signed installable bundles (macOS first, Linux and Windows planned) | New |
| SPEC-118 | GUI | Automatic update checking for the app itself | New |
| SPEC-119 | GUI | Light and dark theme support via component library theme system | New |

## Settings

| ID | Category | Description | Type |
|----|----------|-------------|------|
| SPEC-120 | Settings | Configuration precedence: defaults → user config file → environment variables → CLI flags | New |
| SPEC-121 | Settings | Per-project settings (see SPEC-005) | New |
| SPEC-122 | Settings | Global settings: library root, appearance, default behaviors, notifications, tray, network/proxy, bandwidth, logging | New |

## Architecture & Quality (Cross-Cutting)

| ID | Category | Description | Type |
|----|----------|-------------|------|
| SPEC-123 | Architecture | Multi-crate workspace with strict one-way dependencies | New |
| SPEC-124 | Architecture | Domain layer: pure Rust, no I/O, defines entities, value objects, domain services, and port traits | New |
| SPEC-125 | Architecture | Application layer: use cases, job system (queue, priorities, concurrency, progress, cancellation, retry, idempotency), event stream | New |
| SPEC-126 | Architecture | Data/infrastructure layer: SQLite with WAL, migrations, GitHub GraphQL+REST client, Git integration, filesystem service, credential store, archive service, notification service, OS scheduler integration | New |
| SPEC-127 | Architecture | Three presentation layers (GUI, TUI, CLI) as separate crates/binaries calling shared core | New |
| SPEC-128 | Architecture | Composition root builds dependency graph once; support combined executable and separate binaries | New |
| SPEC-129 | Architecture | Async runtime with structured concurrency throughout | New |
| SPEC-130 | Architecture | Typed domain errors and rich contextual errors at edges | New |
| SPEC-131 | Architecture | Structured tracing with rotating log files | New |
| SPEC-132 | Architecture | Dependency injection by constructor; no global mutable state | New |
| SPEC-133 | Architecture | Time and randomness behind abstractions for deterministic tests | New |
| SPEC-134 | Architecture | Feature flags for optional capabilities (e.g., cloud backup providers) | New |

## Data & Storage

| ID | Category | Description | Type |
|----|----------|-------------|------|
| SPEC-135 | Data | Normalized relational schema: projects, accounts, credentials (refs only), repos, tracking prefs, releases, assets, local files, download jobs, source sync state, PRs, schedules, job runs, activity events, backup records, settings, schema version | New |
| SPEC-136 | Data | Created/updated timestamps, soft-delete/archive flags, unique constraints, indexes for all list/filter queries | New |
| SPEC-137 | Data | Timestamps in single canonical form | New |
| SPEC-138 | Data | Integrity checks, vacuum, optimize maintenance tasks | New |
| SPEC-139 | Data | Automatic pre-migration backups | New |
| SPEC-140 | Data | Portable database: library paths stored relative to library root | New |

## Quality & Testing

| ID | Category | Description | Type |
|----|----------|-------------|------|
| SPEC-141 | Quality | Unit tests for domain layer | New |
| SPEC-142 | Quality | Use-case tests with in-memory fakes of every port | New |
| SPEC-143 | Quality | Integration tests against real temp DBs and filesystems | New |
| SPEC-144 | Quality | GitHub client tests against recorded/mocked responses | New |
| SPEC-145 | Quality | CLI tests running binary and checking output/exit codes | New |
| SPEC-146 | Quality | GUI tests for view models and critical flows | New |
| SPEC-147 | Quality | Migration tests upgrading from every past schema version | New |
| SPEC-148 | Quality | Failure-injection tests: interrupted downloads, full disks, killed processes, corrupted archives | New |
| SPEC-149 | Quality | Security: least privilege, no secrets in logs/exports, validated paths (no traversal), verified downloads, sanitized markdown, safe handling of untrusted names | New |
| SPEC-150 | Quality | Performance: handle thousands of repos, minimize API calls, stream large files, fast startup, near-zero idle resource use | New |
| SPEC-151 | Quality | Reliability: atomic or resumable multi-step file ops; crashes never leave inconsistent state; idempotent background jobs | New |
| SPEC-152 | Quality | Observability: structured logs, job history, surfaced errors with correlation IDs | New |
| SPEC-153 | Quality | Documentation: architecture overview, layer responsibilities, how-to guides, user guide, command reference, changelog | New |
| SPEC-154 | Quality | Tooling: formatting/lint checks, dependency audit, CI pipelines, reproducible builds | New |

---

**Total features inventoried: 154**

| Category | Count | New | Replace |
|---|---|---|---|
| Projects | 7 | 6 | 1 |
| Repositories | 7 | 6 | 1 |
| Git | 8 | 2 | 6 |
| Releases | 14 | 7 | 7 |
| PRs | 3 | 1 | 2 |
| Diagnostics | 6 | 4 | 2 |
| Auth | 7 | 5 | 2 |
| Schedules | 9 | 7 | 2 |
| Backups | 10 | 10 | 0 |
| CLI | 10 | 8 | 2 |
| TUI | 7 | 7 | 0 |
| GUI | 29 | 29 | 0 |
| Settings | 3 | 3 | 0 |
| Architecture | 12 | 12 | 0 |
| Data | 6 | 6 | 0 |
| Quality | 14 | 14 | 0 |
| **Total** | **154** | **127** | **27** |
