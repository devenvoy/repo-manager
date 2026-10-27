//! Command line entry point for the Release Desk library.

use clap::{Parser, Subcommand};
use release_desk_core::{Repository, latest_installer};
use release_desk_git::{GitClient, SyncOutcome};
use release_desk_store::{Project, Store, default_config_path};
use serde_json::{Value, json};
use std::{
    error::Error,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

const ARTCRAFT_SNAPSHOT: &str = "/Volumes/Devansh2/codes/ArtCraft/.release-desk/data.json";

#[derive(Parser)]
#[command(
    name = "release-desk",
    version,
    about = "Manage repository releases and local library state"
)]
struct Args {
    /// Use a different machine-local configuration file.
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create the library and link the original ArtCraft snapshot read-only.
    Init {
        /// Parent folder for the new Release Desk Library.
        #[arg(long)]
        library: Option<PathBuf>,
        /// Existing ArtCraft data.json to link without editing it.
        #[arg(long, default_value = ARTCRAFT_SNAPSHOT)]
        snapshot: PathBuf,
    },
    /// List projects in this library.
    Projects {
        #[arg(long)]
        json: bool,
    },
    /// List repositories from the linked snapshot.
    Repos {
        #[arg(long, default_value = "ArtCraft")]
        project: String,
        #[arg(long)]
        archived: bool,
        #[arg(long)]
        json: bool,
    },
    /// Show release and installer assets for a repository.
    Releases {
        repository: String,
        #[arg(long, default_value = "ArtCraft")]
        project: String,
        #[arg(long)]
        json: bool,
    },
    /// Refresh the managed cache from the linked snapshot.
    Refresh {
        #[arg(long, default_value = "ArtCraft")]
        project: String,
    },
    /// Summarize project and repository status.
    Status {
        #[arg(long, default_value = "ArtCraft")]
        project: String,
        #[arg(long)]
        json: bool,
    },
    /// Clone or safely fast-forward one repository in the managed library.
    Sync {
        repository: String,
        #[arg(long, default_value = "ArtCraft")]
        project: String,
        /// Inspect without cloning, fetching, or changing files.
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("release-desk: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<(), Box<dyn Error>> {
    let config = args.config.map(Ok).unwrap_or_else(default_config_path)?;
    match args.command {
        Command::Init { library, snapshot } => {
            let library = match library {
                Some(path) => path,
                None => prompt_library_location()?,
            };
            let project = Store::initialize_at(&config, &library, &snapshot)?;
            println!(
                "Created Release Desk Library in {}",
                library.join("Release Desk Library").display()
            );
            println!(
                "Linked {} ({}) read-only",
                project.name,
                project.linked_snapshot.display()
            );
        }
        Command::Projects { json: as_json } => {
            let projects = Store::open_at(&config)?.projects()?;
            if as_json {
                output_json(&projects)?;
            } else {
                for project in projects {
                    println!(
                        "{}\t{}\t{}",
                        project.name,
                        project.account,
                        project.linked_snapshot.display()
                    );
                }
            }
        }
        Command::Repos {
            project,
            archived,
            json: as_json,
        } => {
            let (store, project) = open_project(&config, &project)?;
            let view = store.cached_snapshot(&project)?;
            if view.stale {
                eprintln!("release-desk: linked source is unavailable; showing cached data");
            }
            let repos: Vec<_> = view
                .snapshot
                .repos
                .iter()
                .filter(|repo| archived || !repo.is_archived)
                .collect();
            if as_json {
                output_json(
                    &repos
                        .iter()
                        .map(|repo| repository_json(repo))
                        .collect::<Vec<_>>(),
                )?;
            } else {
                for repo in repos {
                    let installer = latest_installer(repo, false, "dmg")
                        .map(|(release, _)| release.tag_name.as_str())
                        .unwrap_or("—");
                    println!(
                        "{}\t{}\t{}\t{}",
                        repo.name,
                        repo.primary_language
                            .as_ref()
                            .map_or("—", |language| &language.name),
                        repo.releases.nodes.len(),
                        installer
                    );
                }
            }
        }
        Command::Releases {
            repository,
            project,
            json: as_json,
        } => {
            let (store, project) = open_project(&config, &project)?;
            let view = store.cached_snapshot(&project)?;
            let repo = view
                .snapshot
                .repos
                .iter()
                .find(|repo| repo.name.eq_ignore_ascii_case(&repository))
                .ok_or_else(|| format!("repository not found: {repository}"))?;
            if as_json {
                let releases: Vec<_> = repo.releases.nodes.iter().map(|release| json!({
                    "tag": release.tag_name, "name": release.name, "draft": release.is_draft,
                    "prerelease": release.is_prerelease, "publishedAt": release.published_at,
                    "url": release.url,
                    "assets": release.release_assets.nodes.iter().map(|asset| json!({
                        "name": asset.name, "size": asset.size, "downloadUrl": asset.download_url
                    })).collect::<Vec<_>>()
                })).collect();
                output_json(&releases)?;
            } else {
                for release in &repo.releases.nodes {
                    println!(
                        "{}\t{}{}",
                        release.tag_name,
                        release.published_at.as_deref().unwrap_or("unpublished"),
                        if release.is_prerelease {
                            "\tprerelease"
                        } else {
                            ""
                        }
                    );
                    for asset in &release.release_assets.nodes {
                        println!("  {}\t{} bytes", asset.name, asset.size);
                    }
                }
            }
        }
        Command::Refresh { project } => {
            let (mut store, project) = open_project(&config, &project)?;
            let view = store.refresh_snapshot(&project)?;
            println!(
                "Refreshed {}: {} repositories",
                project.name,
                view.snapshot.repos.len()
            );
        }
        Command::Status {
            project,
            json: as_json,
        } => {
            let (store, project) = open_project(&config, &project)?;
            let view = store.cached_snapshot(&project)?;
            let repos = &view.snapshot.repos;
            let summary = json!({
                "project": project.name,
                "account": project.account,
                "library": store.library_root(),
                "snapshot": project.linked_snapshot,
                "generated": view.snapshot.generated,
                "stale": view.stale,
                "repositories": repos.len(),
                "archived": repos.iter().filter(|repo| repo.is_archived).count(),
                "releases": repos.iter().map(|repo| repo.releases.nodes.len()).sum::<usize>(),
                "repositoriesWithDmg": repos.iter().filter(|repo| latest_installer(repo, false, "dmg").is_some()).count()
            });
            if as_json {
                output_json(&summary)?;
            } else {
                println!("{} · {}", project.name, project.account);
                println!("Library: {}", store.library_root().display());
                println!(
                    "Snapshot: {}{}",
                    project.linked_snapshot.display(),
                    if view.stale { " (cached)" } else { "" }
                );
                println!(
                    "Repositories: {} · Archived: {} · Releases: {} · DMG ready: {}",
                    summary["repositories"],
                    summary["archived"],
                    summary["releases"],
                    summary["repositoriesWithDmg"]
                );
            }
        }
        Command::Sync {
            repository,
            project,
            dry_run,
            json: as_json,
        } => {
            let (store, project) = open_project(&config, &project)?;
            let view = store.cached_snapshot(&project)?;
            let repo = view
                .snapshot
                .repos
                .iter()
                .find(|repo| repo.name.eq_ignore_ascii_case(&repository))
                .ok_or_else(|| format!("repository not found: {repository}"))?;
            let branch = repo.default_branch_ref.as_ref().ok_or_else(|| {
                format!("{} has no default branch in the linked snapshot", repo.name)
            })?;
            let path = store.clone_path(&project, &repo.name)?;
            let git = GitClient::new();
            if dry_run {
                let state = git.inspect(&path, &branch.name)?;
                if as_json {
                    output_json(
                        &json!({"repository": repo.name, "path": path, "status": format!("{:?}", state.status),
                        "localRevision": state.local_revision, "remoteRevision": state.remote_revision,
                        "message": state.message}),
                    )?;
                } else {
                    println!("{}: {:?} at {}", repo.name, state.status, path.display());
                }
            } else {
                let outcome = git.sync(&repo.url, &path, &branch.name)?;
                if as_json {
                    let (status, detail) = match outcome {
                        SyncOutcome::Cloned { revision } => {
                            ("cloned", json!({"revision": revision}))
                        }
                        SyncOutcome::FastForwarded { from, to } => {
                            ("fastForwarded", json!({"from": from, "to": to}))
                        }
                        SyncOutcome::AlreadyCurrent => ("current", json!({})),
                        SyncOutcome::Skipped(state) => (
                            "skipped",
                            json!({"state": format!("{:?}", state.status), "message": state.message}),
                        ),
                    };
                    output_json(
                        &json!({"repository": repo.name, "path": path, "status": status, "detail": detail}),
                    )?;
                } else {
                    println!("{}: {:?}", repo.name, outcome);
                }
            }
        }
    }
    Ok(())
}

fn open_project(config: &Path, name: &str) -> Result<(Store, Project), Box<dyn Error>> {
    let store = Store::open_at(config)?;
    let project = store.project(name)?;
    Ok((store, project))
}

fn repository_json(repo: &Repository) -> Value {
    json!({
        "name": repo.name,
        "description": repo.description,
        "url": repo.url,
        "private": repo.is_private,
        "archived": repo.is_archived,
        "pushedAt": repo.pushed_at,
        "language": repo.primary_language.as_ref().map(|language| &language.name),
        "defaultBranch": repo.default_branch_ref.as_ref().map(|branch| &branch.name),
        "releaseCount": repo.releases.nodes.len(),
        "latestDmgRelease": latest_installer(repo, false, "dmg").map(|(release, _)| &release.tag_name)
    })
}

fn prompt_library_location() -> Result<PathBuf, Box<dyn Error>> {
    if !io::stdin().is_terminal() {
        return Err("--library is required when standard input is not a terminal".into());
    }
    print!("Choose the parent folder for Release Desk Library: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let path = input.trim();
    if path.is_empty() {
        return Err("a library folder is required".into());
    }
    Ok(PathBuf::from(path))
}

fn output_json(value: &impl serde::Serialize) -> Result<(), Box<dyn Error>> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
