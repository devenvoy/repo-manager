# SQLite schema contract

This is a logical schema for Phase 4 implementation. Column names and indexes
are fixed here before writing migrations; migration SQL may add implementation
columns while preserving these invariants. All timestamps are UTC RFC 3339
strings. Primary keys are stable application UUIDs unless a GitHub numeric ID
is explicitly named. Foreign keys are enabled on every connection.

## Identity and configuration

| Table | Required columns and constraints | Read indexes |
| --- | --- | --- |
| `schema_migrations` | `version` PK, `applied_at`, `checksum` | PK |
| `app_settings` | `key` PK, typed JSON `value`, `updated_at`; never secrets | PK |
| `library_roots` | `id` PK, machine-local canonical path, identity marker, `active`, `created_at`; one active root | active |
| `credential_refs` | `id` PK, provider, OS-store key, display label, scopes, expiry, created/updated; no token value | provider, expiry |
| `accounts` | `id` PK, `github_id` UNIQUE, login, account kind (`organization`/`user`), display name, avatar URL, updated | login, kind |
| `projects` | `id` PK, name, `account_id` FK, `credential_ref_id` nullable FK, root ID FK, archived/deleted flags, created/updated, last active; unique active name | archived, account, updated |
| `project_settings` | `project_id` PK/FK; pre-release and archived switches, source/installer/PR defaults, concurrency, retention count, extension filter JSON, refresh policy, updated | PK |
| `linked_sources` | `id` PK, `project_id` FK, source kind `legacy_snapshot`, machine-local source path, last file identity/hash/read time/error; no write destination | project, source kind |

`linked_sources` is specifically for ArtCraft's read-only `data.json` link. A
backup may include its metadata but must mark the path unresolved when restored
on another machine. The source path is never joined to the managed library.

## Repository and GitHub data

| Table | Required columns and constraints | Read indexes |
| --- | --- | --- |
| `repositories` | `id` PK, `github_id` UNIQUE nullable during legacy import, `account_id` FK, name/full name, clone and web URLs, visibility, archived flag, language, default branch/head, pushed/updated, soft-deleted | account+name UNIQUE while live, archived, language, pushed |
| `repository_aliases` | `repository_id` FK, old account/name, first/last seen; composite PK | old account/name |
| `project_repositories` | `project_id` + `repository_id` PK, discovery state (`inbox`/`tracked`/`ignored`), independent source/installer/PR booleans, local-data-retention choice, created/updated | project+state, each tracking flag |
| `releases` | `id` PK, `repository_id` FK, `github_id` UNIQUE nullable for legacy import, tag/name/body, draft/pre-release, created/published, URL, fetched_at; unique repo+tag while live | repo+published, repo+created, repo+draft |
| `assets` | `id` PK, `release_id` FK, GitHub asset ID nullable, name, size, URL, content type, architecture label, checksum if supplied; unique release+name+size | release, name |
| `pull_requests` | `id` PK, `github_id` UNIQUE, `repository_id` FK, number, state, draft, merged_at, title, author, head/base refs, URL, created/updated/closed, fetched_at; unique repo+number | repo+state+updated, repo+author |
| `pull_request_labels` | `pull_request_id` FK, label name/color, composite PK | label name |
| `api_cache` | account/credential/scope/request key PK, ETag or last-modified, fetched/expires, response reference, rate-limit reset/remaining; no credential value | expires, account |

GitHub IDs are the continuity key across rename and transfer. A legacy
snapshot may initially lack IDs; reconciliation uses account/name plus URL,
then attaches the stable GitHub ID before acting on live repositories.

## Local state and jobs

| Table | Required columns and constraints | Read indexes |
| --- | --- | --- |
| `source_states` | project+repo PK/FK, managed relative clone path nullable, branch, local/remote revision, status, dirty flag, checked_at, error | project+status, checked_at |
| `installer_files` | `id` PK, project/repo/release/asset FKs, managed relative path, role (`current`/`archive`/`staging`), provenance (`download`/`manual`), architecture, size, SHA-256, verification time, created; unique managed path | project+repo+role, release, SHA-256 |
| `jobs` | `id` PK, project/repo nullable FKs, kind, priority, state, idempotency key UNIQUE, progress bytes/items, cancel requested, retry count/next retry, created/started/finished, error code/context | state+priority, project+state, next retry |
| `job_runs` | `id` PK, job FK, attempt, started/finished, result, duration, redacted error; unique job+attempt | job, result+finished |
| `schedules` | `id` PK, project FK, job kind, validated recurrence JSON, timezone, enabled, missed-run policy, last/next run, created/updated | enabled+next run, project |
| `activity_events` | `id` PK, project/repo/job nullable FKs, level, kind, message, safe structured details JSON, duration, correlation ID, timestamp | project+timestamp, level+timestamp, repo+timestamp |
| `backup_records` | `id` PK, project nullable FK, relative/local destination reference, mode, included content, size/checksum, status, started/finished, verified_at, retention group | project+finished, status |

`installer_files` stores the manifest that the HTML kept as loose
`.state/*.manifest` text. Managed paths are relative to the selected library
root and can be re-resolved after relocation. The old HTML manifest is read by
the migration adapter only. Source-state paths are null for linked ArtCraft
clones because the app must not treat them as managed destinations.

## Transaction and migration rules

- SQLite WAL, busy timeout, and short transactions permit GUI/CLI/TUI readers
  while a single write operation commits. Foreign keys and uniqueness are
  checked in every process. Jobs and multi-file operations use explicit
  pending/staging states; database commit occurs only after a verified atomic
  file install, or recovery reconciles staged files on startup.
- Every migration is forward-only, numbered, checksummed, and applied in a
  transaction. Take and verify a pre-migration database backup first. If a
  migration fails, retain the previous database and show a recovery path.
- Repository rename updates identity/aliases, not project tracking history or
  managed directory IDs. Soft-delete rows where restoration matters.
- Library relocation copies or moves managed content to a staging destination,
  verifies hashes and row/path counts, switches the active root transactionally,
  then retains the old root until the user confirms cleanup. Linked ArtCraft
  paths are excluded from relocation.
- Backups use a SQLite consistent snapshot plus a manifest of relative files
  and checksums. Restore validates archive paths and sizes before mutation and
  creates a safety snapshot of current managed state.

## Query and volume checks

The implementation must exercise list/filter queries for at least 1,000 repos,
cached releases/assets, PR state filters, installer updates, running jobs, and
activity history. `EXPLAIN QUERY PLAN` should confirm indexes are used for the
filter/sort combinations exposed by CLI, TUI, and GUI. Migrations and restore
tests cover missing external volumes and a read-only linked ArtCraft source.
