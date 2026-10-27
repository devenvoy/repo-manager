# Phase 7: Desktop GUI (`repo-manager`)

## Goal
Transform the root GPUI application (`repo-manager`) into a production-grade, modular native desktop app conforming to `docs/UX-SPEC.md`. Completely eliminate monolithic code by splitting the GUI into clean, single-responsibility components and views: Overview, Master-Detail Repositories, Installer Shelf, Pull Requests, Local Backup & Reconcile Center, and Settings. Wire the GUI directly to the shared application layer, domain rules, and SQLite storage adapters.

## Plans
1. [`07-01-PLAN.md`](07-01-PLAN.md) — Modular GUI architecture, native theme tokens (`theme.rs`), application state (`state.rs`), and shell navigation components (`header.rs`, `sidebar.rs`).
2. [`07-02-PLAN.md`](07-02-PLAN.md) — Workspace views: Overview dashboard, Master-Detail Repositories, Installer Shelf, Pull Requests, Local Backup Mismatch & Disaster Recovery panel, and Settings.
3. [`07-03-PLAN.md`](07-03-PLAN.md) — GUI integration testing, workspace verification (`cargo fmt`, `cargo clippy`, `cargo test`), and ArtCraft safety guarantee.
