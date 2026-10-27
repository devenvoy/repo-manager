# File, credential, and process boundaries

This document is the implementation contract for the user's locked ArtCraft
decision (D-03) and managed-library choice (D-04). It applies to GUI, CLI,
TUI, services, jobs, migration, backup, and restore.

## ArtCraft is a read-only linked source

The first project links
`/Volumes/Devansh2/codes/ArtCraft/.release-desk/data.json`. The link is a
record of the source file's path and identity, not a copy, mount, or adoption of
the surrounding folder. `LinkedSnapshotSource` can open, read, parse, and watch
that file. It cannot create, write, rename, delete, clone, fetch, download, or
rotate files there. If the volume is absent, show last cached metadata with a
stale marker and a reconnect action. Refreshing from GitHub writes only to the
app's database and managed library.

ArtCraft's existing clones and installers may be observed for migration
preview, but are never a sync or installer destination. A later request to
manage them requires a new explicit decision and a reversible migration plan.

## Managed library capability

First-run setup asks for a library directory. The application creates its own
named child directory there and stores a root identity in configuration. All
new project clones, downloads, staging files, archives, and exports written by
background work are resolved through `ManagedLibrary`, never from arbitrary
GitHub names or user-supplied strings. Project/repository IDs form on-disk
components; display names are metadata, not paths.

Before accepting a root, canonicalize it and the ArtCraft root, reject roots
equal to or nested inside ArtCraft, and reject a root that contains ArtCraft.
For every write, resolve relative components under the known root, reject
absolute paths, `.`/`..`, separators, NUL, and symlink traversal. For paths
that do not exist yet, validate each existing ancestor and use handle-relative
no-follow operations for creation so a symlink cannot be swapped between a
check and a write. Do not rely only on a string prefix or a one-time
`canonicalize` call. Keep destructive operations scoped to stored managed IDs
and offer a dry-run/preview at the application layer.

| Candidate | Decision |
| --- | --- |
| Direct path in ArtCraft | Reject write |
| Symlink in library pointing into ArtCraft | Reject write |
| Library root inside ArtCraft | Reject during setup |
| ArtCraft path through `..` or mixed separators | Reject before resolution |
| Missing child under an approved managed root | Create with handle-relative, no-follow operation |
| Removed/offline external library root | Stop writes, preserve queued job state, prompt to reconnect |

Adapter tests must attempt create, overwrite, rename, and delete through every
write port using direct ArtCraft paths, symlink aliases, and traversal. Each
must fail without changing any ArtCraft file. Tests must also cover a missing
library volume and interruption during staged installation.

## Credentials and process execution

Persist only credential references in SQLite. PAT and device-flow tokens live
in the OS credential store. Never put tokens in CLI arguments, Git `-c`
arguments, logs, diagnostics, crash reports, backups, or exports. A Git
adapter uses a credential helper or an authenticated transport with redacted
diagnostics. URL and asset names are treated as untrusted input. External Git
processes receive validated arguments without a shell and have controlled
working directories and cancellation. A process-wide/project lock uses an
owner identity and recovery rule so stale locks do not block forever.

## Backup and restore

Archives contain relative managed paths and IDs, never absolute ArtCraft paths
as restore destinations. A linked-source reference can be preserved as an
optional unresolved reference on another machine. Restoring asks for a library
root, previews the target, verifies archive entries against traversal and
symlinks, and takes a safety snapshot before replacing managed data. It never
writes into ArtCraft by following a restored path.
