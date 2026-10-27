# Release Desk

## Product

Release Desk is a native manager for GitHub organizations and users. A person
creates multiple projects, chooses which repositories to track, and manages
source clones, releases, installers, pull requests, schedules, backups, and
diagnostics from a desktop GUI, a scriptable CLI, and an interactive TUI. All
three clients use the same domain rules, application use cases, and storage.

The two legacy references are:

- `/Volumes/Devansh2/codes/ArtCraft/release-desk.html`
- `/Volumes/Devansh2/codes/ArtCraft/storytold-sync.sh`

The user-provided product specification is
`/Users/devanshpc/Downloads/desktop-app-agent-prompt.md`. The references are
evidence of existing behavior. The prompt states desired new behavior. The
current `repo-manager` GPUI snapshot viewer is a prototype, not the target
product or a completed milestone.

## Locked user decisions — 2026-10-27

1. Follow the specification's build order: shared core, then CLI, then TUI,
   then the complete GPUI desktop GUI.
2. The GUI gets a new visual identity with system, light, and dark appearance.
   It must be polished and include purposeful animation. Respect the OS reduced
   motion setting.
3. On first setup, create the first project using the existing ArtCraft
   `.release-desk/data.json` as a **read-only linked data source**. Leave the
  ArtCraft folder and its clones/installers in place; do not move, rewrite,
  delete, or sync into that folder. The read-only link is implemented in the
  infrastructure and first-run CLI phases, not deferred to optional migration.
  New managed files belong in the separately selected library.
4. Ask for the managed library location during first-run setup.

## Success definition

The app is ready only when the legacy feature-parity checklist passes, the new
specification's project, sync, installer, PR, scheduler, backup, and diagnostics
flows work through the shared core, CLI and TUI are usable, the desktop GUI
passes visual and interaction review in a running window, and the release build
is packaged and verified for macOS. Linux and Windows remain planned release
targets as stated in the specification.

## Constraints

- No business decisions in GUI, CLI, or TUI views.
- Never place credentials in process arguments, logs, database, config, or
  backups. Use the OS credential store for stored secrets.
- Preserve local work: only fast-forward a clean checkout on its default
  branch. Never overwrite dirty, ahead, detached, or diverged clones.
- Offline access must keep cached data and local files usable with age labels.
- Multi-step file operations must be recoverable. User-visible actions show
  progress, errors, cancellation, and recovery paths.
- The initial ArtCraft link is read-only unless the user gives a later explicit
  instruction changing that decision. The implementation must canonicalize
  paths, reject symlink escapes and traversal, and make all write ports reject
  ArtCraft paths rather than trusting UI convention alone.

## Source of truth

`REQUIREMENTS.md` records the product contract, `ROADMAP.md` maps it to phases,
and each phase's `PLAN.md` defines executable tasks and verification. Decisions
are recorded in `DECISIONS.md` before implementation. The reference analysis
in `docs/REFERENCE_ANALYSIS.md` must be expanded to a full parity matrix in
Phase 1.
