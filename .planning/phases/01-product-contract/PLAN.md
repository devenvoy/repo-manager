# Phase 1 execution order

Execute these plans in order. Each has requirement IDs, artifacts, actions, and
verification evidence. Mark completion in `STATE.md` only after all three pass.

1. [`01-01-PLAN.md`](01-01-PLAN.md) — reference behavior and parity matrix
2. [`01-02-PLAN.md`](01-02-PLAN.md) — architecture, schema, read-only boundary
3. [`01-03-PLAN.md`](01-03-PLAN.md) — UX contract, tests, coverage review

Current status: **complete**. All three plans (01-01, 01-02, 01-03) have been executed and verified. Output artifacts are present in `docs/`: `FEATURE_PARITY.md`, `ARCHITECTURE.md`, `SCHEMA.md`, `BOUNDARIES.md`, `UX-SPEC.md`, `TEST-STRATEGY.md`. Reference file hashes confirmed unchanged. CI checks (`cargo fmt`, `cargo clippy`, `cargo test`) all pass.
