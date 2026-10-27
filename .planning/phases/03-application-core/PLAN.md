# Phase 3: Application Core

## Goal
Implement typed application use cases, event stream, and job coordinator in `crates/release-desk-application`. All use cases depend strictly on `release-desk-core` domain types and port traits, remaining 100% decoupled from concrete infrastructure adapters.

## Execution Order
1. [`03-01-PLAN.md`](03-01-PLAN.md) — Application crate setup, event stream, and job coordinator with cancellation and priority.
2. [`03-02-PLAN.md`](03-02-PLAN.md) — Typed use cases in separate modules (Project, Discovery, Git Sync, Installers, PRs, Schedules).
3. [`03-03-PLAN.md`](03-03-PLAN.md) — In-memory port fakes and end-to-end tracer integration test suite.
