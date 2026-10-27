# Decisions

| ID | Decision | Source | Reversal cost |
| --- | --- | --- | --- |
| D-01 | Build CLI, then TUI, then GUI on shared core | User reply, 2026-10-27 | Costly schedule change |
| D-02 | New native visual identity with system/light/dark modes and purposeful motion | User reply, 2026-10-27 | Moderate design rework |
| D-03 | First project links ArtCraft `data.json` read-only; ArtCraft folder and files remain untouched | User reply, 2026-10-27 | One-way if violated; enforce as a capability boundary |
| D-04 | First-run setup asks where to create managed library | User reply, 2026-10-27 | Costly storage migration |
| D-05 | Desktop uses Rust GPUI and its component library | Product specification | Costly framework rewrite |
| D-06 | All three front ends share domain and application rules | Product specification | Costly data/behavior migration |

## Decisions deferred to the relevant phase

These are not blockers for Phase 1. Ask before a costly choice is implemented,
showing concrete options and consequences:

- Which Apple Developer signing/notarization credentials will be used at
  packaging time. The requirement to ship a signed macOS build is already
  locked; release cannot pass without those credentials.
- Which cloud backup provider, if any, should be the first implementation.
  Local-folder backup is required regardless.
- Whether an existing ArtCraft clone should ever become managed by the new
  library. The present decision forbids modifying it; later adoption requires
  a separate, explicit migration choice.

Routine choices already specified in the product prompt (SQLite, GPUI,
first-party GitHub authentication options, macOS first, safe sync rules) should
be implemented without repeatedly asking for approval.
