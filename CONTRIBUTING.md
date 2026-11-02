# Contributing to Release Desk

Thanks for taking the time to contribute.

## Before you start

- Search existing issues and pull requests to avoid duplicate work.
- Open an issue for substantial changes so the approach can be discussed first.
- Follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Development setup

Release Desk requires Rust 1.85 or newer.

```sh
git clone https://github.com/devenvoy/repo-manager.git
cd repo-manager
cargo run
```

Development guidance, including module boundaries and domain design rules, is in
[AGENTS.md](AGENTS.md). Read it before changing code.

## Making changes

1. Fork the repository and create a topic branch from `main`.
2. Keep commits focused and write clear commit messages.
3. Add or update tests for behavior changes and edge cases.
4. Keep the domain independent from I/O, CLI, persistence, and other
   infrastructure.
5. Do not add dependencies without checking whether the standard library or an
   existing dependency covers the need.

## Verify

Run all three checks before opening a pull request:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs the same checks. Fix warnings rather than suppressing them.

## Pull requests

- Describe what changed and why, and link the related issue.
- Keep the diff scoped to the change; avoid unrelated refactors.
- Confirm the verification commands pass locally.
- Respond to review feedback and keep the branch up to date.

## Reporting bugs

Include the platform, the Release Desk version or commit, steps to reproduce,
expected behavior, actual behavior, and any relevant logs. Remove credentials
and personal data from logs before posting.

## Security issues

Do not open a public issue for a security problem. Report it privately through
GitHub's security advisory feature for this repository.
