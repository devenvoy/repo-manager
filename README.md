# Release Desk

An early native desktop app for inspecting a `storytold-sync.sh` snapshot. It
uses Rust and GPUI Kit. The current window shows repositories, their latest
eligible installer release, and a **Settings** panel. Settings persist locally;
no credentials are stored by this version.

## Run

```sh
cargo run
```

Open **Settings**, enter the absolute path to an existing
`.release-desk/data.json`, choose whether to include pre-releases and archived
repositories, and select an installer extension (default `dmg`). Save settings
to load the snapshot. **Reload snapshot** rereads the file after the shell sync
updates it.

This version reads the snapshot only. It does not yet sync Git repositories,
download installers, or contact GitHub. See [reference analysis](docs/REFERENCE_ANALYSIS.md)
and [architecture plan](docs/ARCHITECTURE.md) for the behavior map and next
milestones.

## Verify

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Project development guidance is in [AGENTS.md](AGENTS.md).
