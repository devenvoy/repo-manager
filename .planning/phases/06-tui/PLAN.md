# Phase 6: Terminal User Interface (TUI)

## Goal
Implement a responsive, full-featured interactive terminal user interface (`crates/release-desk-tui`) built on top of the shared application core (`release-desk-application`), pure domain (`release-desk-core`), and storage adapters (`release-desk-store`). Provide live project switching, repository browsing and search filtering, tracking toggles, release installer shelves, pull request inspection, database/disk backup mismatch reconciliation, multi-host auth status, and safe Git sync over the shared event stream.

## Plans
1. [`06-01-PLAN.md`](06-01-PLAN.md) — TUI state architecture, terminal RAII lifecycle guard, event loop, and live application event stream integration.
2. [`06-02-PLAN.md`](06-02-PLAN.md) — Modular TUI views (`Projects`, `Repositories`, `Installers`, `PullRequests`, `Reconcile`, `Auth`, `Help`) with keyboard navigation and interactive actions.
3. [`06-03-PLAN.md`](06-03-PLAN.md) — TUI headless rendering test suite (`ratatui::backend::TestBackend`), keyboard navigation verification, and full workspace quality gates.
