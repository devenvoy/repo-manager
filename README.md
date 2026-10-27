# repo-manager

A new Rust binary project. It currently contains the default starter program; repository management features have not been implemented yet.

## Requirements

- Rust toolchain with support for the 2024 edition

## Run

```sh
cargo run
```

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Project development guidance is in [AGENTS.md](AGENTS.md). The `actionbook/rust-skills` collection is installed for Codex in [`.agents/skills/`](.agents/skills/) and recorded in [`skills-lock.json`](skills-lock.json). These skills are local to this project.
