# Provider release flows

Release Desk's store is a view of managed repositories and their release files. It is not a separate online marketplace. The project Releases page and Store refresh use the same `WorkspaceRelease` model. A successful project refresh writes that project's listings to `store/catalog.sqlite3`. Opening Store reads SQLite and checks installed apps on disk; it does not contact a provider. The first explicit Store refresh or a project Releases refresh contacts providers. A download made on the Releases page is placed in the project release shelf, and Store checks that shelf before downloading again.

## Provider map

| Provider | Release discovery | Private metadata | Private installer download |
| --- | --- | --- | --- |
| GitHub | Releases API | Project token | Asset ID is looked up by name and size; token goes to `api.github.com` only. |
| GitLab | Releases API and direct asset links | Project token | Direct link is resolved on `gitlab.com`; token is stripped on external redirects. |
| Bitbucket Cloud | Repository Downloads API | Bearer token; app password with account name on 401 | Download endpoint at `api.bitbucket.org`; token is stripped on redirects. Bitbucket Downloads are files, not semantic releases, so version is inferred from the filename when present. |
| Gitea | `/api/v1/repos/{owner}/{repo}/releases` | Token header | Attached asset URL on the configured Gitea host. |
| Forgejo / Codeberg | Forgejo-compatible release API at `codeberg.org` for Codeberg and at the configured custom host for self-hosted Forgejo | Token header | Attached asset URL on the provider host. |
| Azure DevOps | No GitHub-style repository release attachment | Project PAT | A repository manifest can declare a build artifact's HTTPS download URL. The PAT is sent only to the project's `dev.azure.com` or `*.visualstudio.com` origin; external storage redirects receive no PAT. Automatic pipeline-to-repository artifact discovery is still needed. |
| AWS CodeCommit | CodeCommit has Git tags but no release attachment service | AWS credentials are separate from Git credentials | A repository manifest can declare a signed S3 URL. No AWS credential is sent to that URL. Automatic CodePipeline/S3 artifact discovery is still needed. |
| Custom | Gitea-compatible API at the project's configured HTTPS host, or a repository manifest | Project token | Token is sent only to the configured HTTPS origin. Other custom protocols need an adapter. |

The `local` source resolves its provider from the repository clone URL, with a saved snapshot as an offline fallback. Unsupported or missing remote metadata never erases a previously saved app listing.

Store names, categories, descriptions, artwork, and screenshots come from each repository's `release-desk-store.json` when supplied. A missing name falls back to a readable repository name; a missing category is `Other`. No application-specific name or category list is embedded in Store.

## Artifact declaration

Azure, CodeCommit, and custom CI systems do not have a universal way to associate a Git tag with one desktop installer. A cloned repository can declare that mapping in its existing `release-desk-store.json`:

```json
{
  "name": "Example App",
  "description": "Create and edit examples.",
  "category": "Productivity",
  "icon": "assets/icon-512.png",
  "images": ["assets/screenshots/main.png"],
  "bundle_name": "Example App.app",
  "releases": [
    {
      "tag": "v1.4.0",
      "notes": "Improved export.",
      "published_at": "2026-10-29T00:00:00Z",
      "assets": [
        {
          "name": "example-v1.4.0-macos.dmg",
          "url": "https://artifacts.example.com/example-v1.4.0-macos.dmg",
          "size": 12345678
        }
      ]
    }
  ]
}
```

The URL must use HTTPS without embedded credentials. A private Azure artifact may use a `dev.azure.com` URL and the project PAT. A private CodeCommit build may use a short-lived signed S3 URL from its pipeline. Signed URLs expire; the project must update its manifest or the provider adapter must issue fresh URLs. The installer name must identify its platform; `dmg` and `pkg` indicate macOS, `msi` and `exe` indicate Windows, and `appimage`, `deb`, and `rpm` indicate Linux.

## Credential and file safety

- The project credential is used only for its configured provider. Other sources can use existing OS Git credentials for their provider host; no credential is written into the SQLite store.
- Authenticated requests use an HTTPS origin allowlist. A redirect to another origin is followed without the credential.
- Store installs a local file only when it stays inside the project folder and matches the recorded size and shelf version when that metadata exists. A changed or missing file triggers a new download.
- Local artwork is read from a cloned repository. It chooses a high-resolution square icon or SVG; a generated monogram is used when no suitable image exists. Organization avatars are not used as app icons in Store.

## Adding a provider

Add a metadata adapter that returns `WorkspaceRelease` records with version, notes, time, and asset names, URLs, and sizes. Add a download adapter whose credentials are scoped to that provider's HTTPS origin. Keep discovery and download protocol details in `crates/release-desk-store`, then route the adapter from `workspace_releases.rs`. Both project Releases and Store must consume the same records and shelf paths. Add fixture tests for metadata mapping, auth redirect behavior, and local asset reuse. If the provider does not own a release artifact service, document its build or package source and support a repository artifact declaration until native discovery is defined.
