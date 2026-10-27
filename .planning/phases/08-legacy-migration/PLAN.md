# Phase 8: Legacy Migration & Automated Adoption Preview Engine

## Goal
Implement a verified legacy migration engine that discovers, inspects, and previews legacy setups (such as ArtCraft-style setups with `data.json`, `storytold-sync.sh`, legacy scripts, and old installer directories). Allow users to safely adopt legacy projects into Release Desk managed storage and SQLite database with complete `<clone_location>/.release-desk/data.json` local backup snapshot guarantees, while strictly enforcing that legacy source folders remain 100% untouched and bit-for-bit immutable without explicit user consent.

## Plans
1. [`08-01-PLAN.md`](08-01-PLAN.md) — Pure domain models (`crates/release-desk-core`): `LegacySetupKind`, `LegacySetupAnalysis`, `MigrationAction`, `MigrationPlan`, and `MigrationResult`.
2. [`08-02-PLAN.md`](08-02-PLAN.md) — Application & Storage layer (`crates/release-desk-store` & `crates/release-desk-application`): `LegacyScanner`, `AnalyzeLegacySetupUseCase`, `ExecuteLegacyMigrationUseCase`, and atomic database adoption with ArtCraft read-only safety guard.
3. [`08-03-PLAN.md`](08-03-PLAN.md) — CLI commands (`release-desk migrate analyze/preview/execute`), user confirmation gate, and automated integration test suite proving source folder immutability.
