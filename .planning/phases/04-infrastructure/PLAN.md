# Phase 4: Infrastructure & Storage Adapters

## Goal
Implement production-grade infrastructure adapters for persistence, Git, file downloads, OS credentials, daemon scheduling, and GitHub API, strictly bounded by the ArtCraft read-only safety policy and SQLite schema contract.

## Plans
1. [`04-01-PLAN.md`](04-01-PLAN.md) — Managed Library capability, ArtCraft safety boundary guard, SQLite migrations, and `ProjectStore` / `TrackingStore` / `JobStore` implementations.
2. [`04-02-PLAN.md`](04-02-PLAN.md) — `GitPort` adapter, streaming `DownloadPort` with SHA-256 verification, and `ArchivePort` backup packager.
3. [`04-03-PLAN.md`](04-03-PLAN.md) — OS Keychain `SecretStorePort`, OS daemon `SchedulerPort`, and GitHub GraphQL/REST `GitHubGateway`.
4. [`04-04-PLAN.md`](04-04-PLAN.md) — End-to-end infrastructure integration test suite and strict ArtCraft write rejection test suite.
