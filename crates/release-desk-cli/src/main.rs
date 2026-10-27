//! Release Desk native command-line interface.

mod args;
mod commands;
mod context;
mod output;

use args::{Cli, Command};
use clap::Parser;
use context::CliContext;
use output::print_error;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args = Cli::parse();
    let json = args.json;

    match execute(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            if json {
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "error": true,
                        "message": err.to_string(),
                    })
                );
            } else {
                print_error(err);
            }
            ExitCode::FAILURE
        }
    }
}

fn execute(args: Cli) -> Result<(), release_desk_core::error::DomainError> {
    let json = args.json;
    let library_override = args.library.as_deref();

    match args.command {
        Command::Init { library, snapshot } => {
            commands::handle_init(library, snapshot, json)?;
        }
        Command::Project { subcommand } => {
            let ctx = CliContext::new(library_override)?;
            commands::handle_project(&ctx, subcommand, json)?;
        }
        Command::Auth { subcommand } => {
            let ctx = CliContext::new(library_override)?;
            commands::handle_auth(&ctx, subcommand, json)?;
        }
        Command::Discovery {
            project_id,
            account,
        } => {
            let ctx = CliContext::new(library_override)?;
            commands::handle_discovery(&ctx, project_id, &account, json)?;
        }
        Command::Track {
            project_id,
            repo_id,
            source,
            installers,
            pr,
        } => {
            let ctx = CliContext::new(library_override)?;
            commands::handle_track(&ctx, project_id, repo_id, source, installers, pr, json)?;
        }
        Command::Untrack {
            project_id,
            repo_id,
        } => {
            let ctx = CliContext::new(library_override)?;
            commands::handle_untrack(&ctx, project_id, repo_id, json)?;
        }
        Command::Sync {
            project_id,
            repo,
            dry_run,
        } => {
            let ctx = CliContext::new(library_override)?;
            commands::handle_sync(&ctx, project_id, repo, dry_run, json)?;
        }
        Command::Installers {
            project_id,
            extension,
            download,
        } => {
            let ctx = CliContext::new(library_override)?;
            commands::handle_installers(&ctx, project_id, &extension, download, json)?;
        }
        Command::Pr { project_id, status } => {
            let ctx = CliContext::new(library_override)?;
            commands::handle_pr(&ctx, project_id, status, json)?;
        }
        Command::Backup { output } => {
            let ctx = CliContext::new(library_override)?;
            commands::handle_backup(&ctx, output, json)?;
        }
        Command::Migrate { subcommand } => {
            let ctx = CliContext::new(library_override)?;
            commands::handle_migrate(&ctx, subcommand, json)?;
        }
    }

    Ok(())
}
