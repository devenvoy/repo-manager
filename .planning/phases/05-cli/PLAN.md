# Phase 5: CLI & Project Snapshot Backup Engine

## Goal
Implement a robust, scriptable CLI (`crates/release-desk-cli`) built entirely on top of the shared application core and infrastructure adapters. Provide first-run setup, multi-host authentication, repository discovery, safe Git fast-forward sync, installer shelf management, portable backups, and an automated project folder `.release-desk/data.json` backup snapshot and mismatch reconciliation engine.

## Plans
1. [`05-01-PLAN.md`](05-01-PLAN.md) — Project clone location, automated local `.release-desk/data.json` backup snapshot, and mismatch reconciliation engine.
2. [`05-02-PLAN.md`](05-02-PLAN.md) — Comprehensive CLI commands (`init`, `project`, `auth`, `discovery`, `sync`, `installers`, `pr`, `backup`, `daemon`) with scriptable `--json` output.
3. [`05-03-PLAN.md`](05-03-PLAN.md) — End-to-end CLI integration test suite verifying project creation, local `data.json` backup generation, database-disk mismatch detection, and ArtCraft write rejection.
