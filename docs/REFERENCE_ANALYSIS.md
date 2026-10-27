# Reference behavior and migration notes

The existing tools are `release-desk.html` and `storytold-sync.sh` in the
ArtCraft folder supplied by the user. The separate agent prompt describes the
intended replacement; it is not evidence that a feature already exists.

## Shell sync

- Defaults to the `storytold` organization and clones beside the script.
  `STORYTOLD_ORG` and `STORYTOLD_BASE_DIR` override those defaults.
- Gets a token from `GITHUB_TOKEN`, `GH_TOKEN`, or `gh auth token`. An optional
  `~/.storytold-sync/env` file is sourced. GraphQL snapshot generation requires
  a token, even though the warning suggests public-only operation without one.
- Fetches 50 repositories per GraphQL page, including metadata, the default
  branch head, up to six releases, and up to 30 assets per release. It writes
  `.release-desk/data.json` using a temporary file and rename.
- Excludes archived repositories from Git sync. Clones missing ones; for an
  existing clone, fetches and prunes `origin`, then fast-forwards only if the
  checked-out branch is the default branch and the working tree is clean.
  Failed clones and fetches are logged and the loop continues.
- A directory lock prevents overlapping runs, but a crashed run can leave a
  stale lock. Cron entries can be added, listed, and removed. `*/3` is a
  day-of-month expression, so it does not mean a fixed 72-hour interval.
- The script does not download or archive installers and does not fetch pull
  requests. It can open the browser dashboard after syncing.

## Browser dashboard

- TypeScript is transpiled in the browser by a compiler loaded from a CDN.
  Fonts, Markdown rendering, and sanitization also depend on external assets.
- Reads the shell snapshot from a user-selected code folder. Otherwise it can
  query GitHub GraphQL with a token, fetching up to 10 pages of 50 repositories.
  The organization, token, filters, and concurrency preference are stored in
  browser local storage. API responses are cached for ten minutes.
- Can show archived repositories, include or exclude pre-releases, filter and
  select repositories, display release notes, and show API quota headers.
- Chooses the first non-draft release in API order that has `.dmg` assets,
  skipping pre-releases when disabled. Since the API query orders releases by
  creation time, this is not necessarily the latest by publication time.
- Can compare a local `.git/HEAD` value to the remote default-branch commit.
  `notcloned` means no `.git` directory; `current` means equal commit IDs;
  `behind` means different IDs; all other cases are `unknown`. The `behind`
  label does not distinguish ahead or diverged histories. The page copies Git
  commands; it does not run them.
- Installer status is `loading`, `none`, `nofolder`, `new`, `update`, `missing`,
  or `latest`. A release signature combines tag and asset IDs. The page starts
  browser downloads, then imports files whose names and sizes match **all**
  current release DMGs. It stages imports, moves the previous current files to
  `old/`, and records current and old filenames in `.state/*.manifest`. It keeps
  only one previous version. This is not a verified network downloader.
- Pull request types and rendering code exist, but the data is initialized to
  empty and never fetched. The detail view does not call the PR renderer.
- The repository-list REST fallback exists as a function but is not called by
  the loading path, so token-free use requires the shell snapshot.

## Current desktop milestone

The new app reads the legacy snapshot without modifying it. It applies a
release-selection rule based on publication time (created time as fallback),
matching the new product specification. It already has a persistent Settings
panel for snapshot path, pre-release preference, archived visibility, and
installer extension. GitHub authentication, managed downloads, Git sync, PRs,
and project history belong to later milestones.
