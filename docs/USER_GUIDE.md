# Release Desk User Guide & Operations Manual

Release Desk is a native Git release manager, installer library, and multi-host synchronization workspace for macOS and Linux. It provides three interfaces powered by a single core:
- **GPUI Desktop Application (`repo-manager`)**: Fluid macOS native GUI with dark/light appearance.
- **Interactive Terminal UI (`release-desk-tui`)**: Full-screen keyboard-driven terminal dashboard.
- **Scriptable CLI (`release-desk`)**: Scriptable command-line interface with JSON output.

---

## 1. Core Architecture & Storage Model

```
Managed Library (Default: ~/Library/Application Support/Release Desk)
├── release-desk.sqlite3               <- Main database (WAL mode, schema v3)
├── projects/                          <- Internal project records & metadata
└── archives/                          <- Compressed .tar.gz backups

Project Clone Directory (e.g., ~/Developer/MyProject)
├── .release-desk/
│   └── data.json                      <- Local backup snapshot (Disaster Recovery & Mismatch Engine)
├── Grouped Layout:
│   └── <organization_or_user>/        <- Per-user namespace
│       └── <repository>/              <- Git repository clone
└── Flat Layout:
    └── <repository>/                  <- Git repository clone directly under project

Local Applications Directory (Default: ~/Applications or /Applications)
└── <AppName>.app                      <- Headless 1-click installed desktop applications
```

### The Local Backup Snapshot Guarantee
Every project clone location automatically maintains a `<clone_location>/.release-desk/data.json` backup snapshot.
- If SQLite database records diverge or the database file is lost, the local backup snapshot can instantly restore your tracked state.
- If remote repositories or branches change, the mismatch detector highlights discrepancies in both the GUI, TUI, and CLI.

### ArtCraft Read-Only Boundary
Legacy ArtCraft directories (`/Volumes/Devansh2/codes/ArtCraft/`) are strictly protected. The application enforces an immutable boundary: ArtCraft files are linked read-only and never modified.

---

## 2. Getting Started

### 2.1 First-Run Setup & Onboarding via GUI
Launch the desktop application:
```bash
cargo run --release -p repo-manager
```
On first run, Release Desk presents the **First-Run Onboarding Screen**:
- Displays Terms of Service, Privacy Policy, and Local Storage Commitments.
- Click **"Accept & Continue"** to automatically initialize the local library in `~/Library/Application Support/Release Desk` and default applications in `~/Applications`.
- No blank input dialogs or startup blockers. You are immediately taken to the unified navigation workspace.

### 2.2 Fluid Top Navigation & Live Metric Badges
The top header provides instant switching between primary modes:
- **App Store**: Shows live update counts (e.g., `App Store (1 Update)`).
- **Project Hub**: Shows live git pull counts (e.g., `Project Hub (2 Pulls)`).
- **Layout Switcher**: Toggle between **Grouped** (`<project>/<user>/<repo>`) and **Flat** (`<project>/<repo>`) directory organization.

### 2.2 First-Run Setup via CLI
Initialize your library directly from the command line:
```bash
# Initialize clean library
release-desk init --library "$HOME/Release Desk Library"

# Initialize and link legacy ArtCraft snapshot read-only
release-desk init --library "$HOME/Release Desk Library" --snapshot "/Volumes/Devansh2/codes/ArtCraft/.release-desk/data.json"
```

---

## 3. Multi-Host Git Authentication

Release Desk supports GitHub, GitLab, Bitbucket, and Atlassian with secure credential storage in the macOS Keychain.

### 3.1 Token Authentication
```bash
# Authenticate GitHub
release-desk auth login --provider github --token ghp_yourPersonalAccessToken

# Authenticate GitLab
release-desk auth login --provider gitlab --token glpat_yourGitLabToken

# Authenticate Bitbucket
release-desk auth login --provider bitbucket --token yourBitbucketToken
```

### 3.2 Browser & Webview OAuth Login
Authenticate via your default web browser:
```bash
release-desk auth login --provider github --browser
```
*If a system browser is not available, Release Desk automatically opens an isolated modal webview window panel to complete authentication and closes upon success.*

### 3.3 Check Authentication Status
```bash
release-desk auth status
# Output JSON for automation:
release-desk auth status --json
```

---

## 4. Project Management & Disaster Recovery

### 4.1 Creating a Project with Local Backup Redundancy
```bash
release-desk project create \
  --id 1 \
  --name "Storytold" \
  --account "storytold" \
  --location "$HOME/Developer/Storytold"
```
This registers the project in SQLite and automatically writes `$HOME/Developer/Storytold/.release-desk/data.json`.

### 4.2 Checking Mismatch & Reconciling State
```bash
# Inspect discrepancies between SQLite and the local data.json backup
release-desk project reconcile --id 1

# Restore missing database records from local backup snapshot
release-desk project reconcile --id 1 --restore-db

# Update local backup snapshot from current database state
release-desk project reconcile --id 1 --sync-backup
```

---

## 5. Repository Discovery & Synchronization

### 5.1 Discovering & Tracking Repositories
```bash
# Discover repositories from account
release-desk discovery --project-id 1 --account "storytold"

# Track source code, installers, and pull requests
release-desk track --project-id 1 --repo-id 101 --source --installers --pr

# Stop tracking a repository
release-desk untrack --project-id 1 --repo-id 101
```

### 5.2 Safe Git Fast-Forward Sync
Release Desk enforces strict synchronization invariants:
- Clean default branches are fast-forwarded (`--ff-only`).
- Uncommitted local changes (dirty clones) are preserved and never overwritten.
- Detached HEAD or topic branch checkouts are safely skipped with warnings.

```bash
# Preview sync without modifying disk
release-desk sync --project-id 1 --dry-run

# Synchronize all tracked repositories
release-desk sync --project-id 1

# Synchronize a specific repository
release-desk sync --project-id 1 --repo "engine"
```

---

## 6. Release Installers & Pull Requests

### 6.1 Inspecting & Downloading Installers
Release Desk maintains a structured shelf for release assets (`.dmg`, `.pkg`, `.zip`):
```bash
# List eligible installers
release-desk installers --project-id 1 --extension dmg

# Download newest published installer into managed library
release-desk installers --project-id 1 --extension dmg --download
```

### 6.2 Tracking Pull Requests
```bash
# List all pull requests
release-desk pr --project-id 1

# Filter by open PRs
release-desk pr --project-id 1 --status open
```

---

## 7. Project-Centric Git Fleet Manager (Project Hub)

Release Desk treats projects as independent workspaces bound to specific disk locations, isolated credentials, and multiple user/org sources.

### 7.1 Project-Scoped Auth & Multi-User Ingestion
- **Per-Project Isolation**: Project A can authenticate with GitHub using Account X, while Project B connects to GitLab using Account Y.
- **Multiple Profile Ingestion**: Ingest repositories from multiple creators or organizations into the same project by pasting handles or full URLs (e.g., `https://github.com/storytold` or `storytold`).
- **Repository Picker**: Interactive checklist allowing individual selection or `Select All` / `Deselect All` with language, stars, visibility, and description indicators.
- **Automated Clone Planning**:
  - **Grouped Layout**: `<project_path>/<user_handle>/<repo_name>` (prevents name collisions between different users).
  - **Flat Layout**: `<project_path>/<repo_name>` (streamlined single-level hierarchy).
- **Offline Redundancy**: Writes `<project_path>/.release-desk/data.json` snapshot automatically upon configuration change.

---

## 8. Local App Store & 1-Click Headless DMG Installer

The **Local App Store** acts as an aggregator across all managed projects and built-in production suites (ArtCraft), presenting an Apple App Store-grade experience.

### 8.1 Visual Experience & Bento Grid
- **Spotlight Hero**: Features trending releases with gradient highlights and instant action buttons.
- **Bento Grid**: Organized card layout with category tags, semantic version badges, release sizes, and rich squircle app icons.
- **Rich Detail Modal**: Detailed overview with full release notes, hardware architecture specs (Apple Silicon / Intel / Universal), feature checklists, and system requirements.

### 8.2 3-State Smart Action Button
The system probes local directories (`~/Applications` and `/Applications`) to dynamically compute the application state:
1. **`Install`**: App is not present locally. Clicking downloads and executes headless installation.
2. **`Update`**: App is installed, but a newer release tag is available. Clicking triggers clean in-place upgrade.
3. **`Open`**: App is installed and at the latest version. Clicking launches the native macOS app immediately (`open -a`).

### 8.3 Headless 1-Click macOS DMG Installer Architecture
Release Desk includes a native installer engine that bypasses manual Finder mounting:
1. Mounts DMG quietly in the background: `hdiutil attach -nobrowse -readonly <dmg_path>`.
2. Verifies cryptographic checksum (SHA-256) of the asset against known release metadata.
3. Copies `<App>.app` bundle into `~/Applications` with preserved file permissions.
4. Detaches mounted disk image: `hdiutil detach <mount_point> -force`.
5. Launches or notifies user upon successful installation.

---

## 9. Legacy Migration & Adoption Engine

Adopt legacy ArtCraft workspaces, custom sync shell scripts, or standalone `data.json` files without touching the original source directory.

### 9.1 Step 1: Read-Only Analysis
```bash
release-desk migrate analyze --path "/path/to/legacy-workspace"
```
Outputs detected setup kind (`ArtCraftWorkspace`, `SnapshotFile`, `ShellScript`, `CloneDirectory`), discovered organization, repositories, and installer packages.

### 9.2 Step 2: Non-Destructive Preview
```bash
release-desk migrate preview \
  --path "/path/to/legacy-workspace" \
  --name "AdoptedProject" \
  --location "$HOME/Developer/AdoptedProject" \
  --copy-installers
```
Displays all planned migration actions before any execution.

### 9.3 Step 3: Verified Execution
```bash
release-desk migrate execute \
  --path "/path/to/legacy-workspace" \
  --name "AdoptedProject" \
  --location "$HOME/Developer/AdoptedProject" \
  --copy-installers \
  --confirm
```
Execution performs:
1. Pre-flight SHA-256 fingerprinting of the legacy source.
2. Registration of the project in SQLite.
3. Import of repositories and tracking preferences.
4. Creation of `<clone_location>/.release-desk/data.json` local backup snapshot.
5. Post-flight SHA-256 fingerprint verification ensuring 0 source modifications.

---

## 10. Terminal User Interface (TUI) Keybindings

Launch the interactive TUI:
```bash
cargo run --release -p release-desk-tui
```

| Key | Action |
|---|---|
| `Tab` / `BackTab` | Cycle through navigation tabs (Repositories, Installers, PRs, Reconcile, Projects, Auth, Help) |
| `1` – `7` | Direct jump to tab |
| `/` | Focus repository search input |
| `Esc` | Clear search / dismiss help modal |
| `r` | In Reconcile view: Restore database from local backup snapshot |
| `b` | In Reconcile view: Export current database to local backup snapshot |
| `s` | Trigger repository synchronization |
| `?` | Toggle Help modal overlay |
| `q` / `Ctrl+C` | Safely restore terminal raw mode and quit |

---

## 11. Creating Compressed Library Backups
```bash
release-desk backup --output "$HOME/Desktop/ReleaseDesk_Backup.tar.gz"
```
Generates a portable, compressed `.tar.gz` archive containing the SQLite database, migrations, and managed project metadata.
