//! CLI argument definitions and clap parser schemas.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "release-desk",
    version,
    about = "Native Git release manager and multi-host synchronizer"
)]
pub struct Cli {
    /// Override the managed library root directory.
    #[arg(long, global = true)]
    pub library: Option<PathBuf>,

    /// Emit machine-readable JSON output instead of human-readable text.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Initialize a Release Desk library and optionally link a project snapshot read-only.
    Init {
        /// Parent folder for the new Release Desk Library.
        #[arg(long)]
        library: Option<PathBuf>,
        /// Project snapshot file to link without modifying.
        #[arg(long)]
        snapshot: Option<PathBuf>,
    },
    /// Manage projects, clone directories, and local backup snapshots.
    Project {
        #[command(subcommand)]
        subcommand: ProjectSubcommand,
    },
    /// Multi-host Git authentication (GitHub, GitLab, Bitbucket).
    Auth {
        #[command(subcommand)]
        subcommand: AuthSubcommand,
    },
    /// Discover repositories for a project account.
    Discovery {
        /// The target project ID.
        #[arg(long)]
        project_id: u128,
        /// Account login (user or org) to discover.
        #[arg(long)]
        account: String,
    },
    /// Set or update repository tracking preferences.
    Track {
        #[arg(long)]
        project_id: u128,
        #[arg(long)]
        repo_id: u64,
        #[arg(long, default_value_t = true)]
        source: bool,
        #[arg(long, default_value_t = true)]
        installers: bool,
        #[arg(long, default_value_t = true)]
        pr: bool,
    },
    /// Untrack or ignore a repository.
    Untrack {
        #[arg(long)]
        project_id: u128,
        #[arg(long)]
        repo_id: u64,
    },
    /// Safely clone or fast-forward repositories.
    Sync {
        #[arg(long)]
        project_id: u128,
        /// Name of the specific repository to sync (syncs all tracked if omitted).
        #[arg(long)]
        repo: Option<String>,
        /// Dry-run mode: inspect without cloning, fetching, or modifying disk.
        #[arg(long)]
        dry_run: bool,
    },
    /// Inspect or download release installer packages.
    Installers {
        #[arg(long)]
        project_id: u128,
        /// Target file extension (dmg, pkg, zip, etc.).
        #[arg(long, default_value = "dmg")]
        extension: String,
        /// Download the installer asset if available.
        #[arg(long)]
        download: bool,
    },
    /// List and filter pull requests.
    Pr {
        #[arg(long)]
        project_id: u128,
        /// Filter by state (open, closed, merged).
        #[arg(long)]
        status: Option<String>,
    },
    /// Create a compressed .tar.gz backup of the managed library.
    Backup {
        /// Destination path for the backup archive.
        #[arg(long)]
        output: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub enum ProjectSubcommand {
    /// Create a new project with optional clone directory and local backup snapshot.
    Create {
        /// Stable numeric project ID.
        #[arg(long)]
        id: u128,
        /// Project display name.
        #[arg(long)]
        name: String,
        /// Source account login.
        #[arg(long)]
        account: String,
        /// Directory where repos will be cloned. Automatically maintains .release-desk/data.json backup.
        #[arg(long)]
        location: Option<PathBuf>,
    },
    /// List all registered projects.
    List,
    /// Reconcile project clone folder's data.json backup against the database.
    Reconcile {
        /// Target project ID.
        #[arg(long)]
        id: u128,
        /// Restore missing repositories in database from the local data.json backup.
        #[arg(long)]
        restore_db: bool,
        /// Export current database state to the local data.json backup snapshot.
        #[arg(long)]
        sync_backup: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum AuthSubcommand {
    /// Log in to a Git host provider.
    Login {
        /// Git provider (github, gitlab, bitbucket).
        #[arg(long, default_value = "github")]
        provider: String,
        /// Read a Personal Access Token from standard input.
        #[arg(long)]
        token_stdin: bool,
        /// Authenticate via system browser with automatic webview modal fallback.
        #[arg(long)]
        browser: bool,
    },
    /// Inspect active authentication status across providers.
    Status,
}
