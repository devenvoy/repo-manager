# Release Desk

Release Desk is a Rust desktop app for managing projects, repositories, and their releases.

## Run

```sh
cargo run
```

Create projects and select repositories in the Project Hub. The **App Store** reads a local SQLite catalog at `<library_dir>/store/catalog.sqlite3`; opening it does not fetch from the network. Refreshing a project's Releases page updates that project's store records, and the Store's **Refresh** button checks every project online. Existing `catalog.json` data is migrated automatically. Failed provider requests keep the last known listing. Install uses a matching file already downloaded in the project's release shelf before contacting a provider.

## Store metadata

Place `release-desk-store.json` at the root of a cloned repository to supply its store page:

```json
{
  "name": "Example App",
  "description": "A short explanation of what the app does.",
  "category": "Productivity",
  "icon": "assets/icon.png",
  "images": ["assets/screenshot-1.png", "assets/screenshot-2.png"],
  "bundle_name": "Example App.app",
  "show_in_store": true
}
```

Artwork paths are relative to the repository and must resolve to files within it. When `images` is omitted, the store looks for screenshots in conventional `assets/app-store`, `assets/screenshots`, `screenshots`, and `docs/screenshots` locations. Set `show_in_store` to `false` for asset, corpus, or support repositories. The store otherwise lists repositories with a verified desktop installer for the running platform. Release notes come from the connected provider's release description. On macOS, DMG assets can be installed or updated into the configured Applications directory and installed apps can be opened. Other package formats are downloaded to their project's release shelf for manual installation.

See [provider release flows](docs/provider-releases.md) for each provider's artifact source, authentication, and the optional manifest format for hosts without release attachments.

## Verify

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Project development guidance is in [AGENTS.md](AGENTS.md).
