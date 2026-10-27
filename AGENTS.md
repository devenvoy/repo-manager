# Rust development instructions

These instructions apply to this project only. The Rust skills are installed in
`.agents/skills/` and should be used when developing or reviewing this crate.

## Skill workflow

- For every Rust task, read `.agents/skills/rust-router/SKILL.md` first and follow
  its routing guidance. Load the relevant language, design, and domain skills it
  identifies before changing code.
- For code style, naming, formatting, and reviews, also read
  `.agents/skills/coding-guidelines/SKILL.md`.
- Use the specific installed skill for the problem at hand, such as
  `m01-ownership`, `m06-error-handling`, `m09-domain`, `m13-domain-error`,
  `m15-anti-pattern`, or `unsafe-checker`. Read `domain-cli` if this project
  becomes a command-line tool.
- Treat skill examples as guidance. Follow the project's actual requirements and
  current Rust toolchain when a skill's example is outdated or inapplicable.

## Code and architecture

- Keep domain rules independent from input/output, CLI, persistence, and other
  infrastructure. Put orchestration at the boundary between them.
- Prefer small modules with one clear responsibility and explicit interfaces.
  Add layers or traits when they clarify a real boundary, not in anticipation of
  hypothetical implementations.
- Model domain concepts with meaningful types and enforce invariants at their
  construction boundaries. Use borrowing and ownership deliberately; avoid
  cloning merely to silence compiler errors.
- Return useful errors with context. Reserve panics and `unwrap()` for proven
  invariants or tests. Keep recoverable failures in `Result` values.
- Follow Rust naming conventions, use `rustfmt`, and document public behavior
  and non-obvious tradeoffs. Favor readable, idiomatic code over cleverness.
- Keep dependencies minimal. Before adding a crate, check whether the standard
  library or an existing dependency covers the need.

## Verification

- Add focused tests for behavior and edge cases when implementing features or
  fixing bugs.
- Before finishing a code change, run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  and `cargo test`; report any command that could not run or did not pass.
