//! Safe Git repository clone and fast-forward synchronization command.

use crate::context::CliContext;
use crate::output::{output_value, print_info};
use release_desk_application::use_case::discovery::TrackingStatus;
use release_desk_application::use_case::sync::SyncOutcome;
use release_desk_core::error::DomainError;
use release_desk_core::project::ProjectId;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
pub struct SyncReport {
    pub project_id: u128,
    pub outcomes: Vec<SyncItemOutput>,
}

#[derive(Serialize)]
pub struct SyncItemOutput {
    pub repo_name: String,
    pub path: PathBuf,
    pub outcome: String,
}

pub fn handle_sync(
    ctx: &CliContext,
    project_id: u128,
    repo_name: Option<String>,
    dry_run: bool,
    json: bool,
) -> Result<(), DomainError> {
    let proj_id = ProjectId::from_u128(project_id);
    let project = ctx
        .project_store
        .get_project(proj_id)?
        .ok_or_else(|| DomainError::NotFound {
            entity: "Project",
            id: project_id.to_string(),
        })?;

    let clone_root = project
        .clone_location
        .unwrap_or_else(|| ctx.library.root_path().join("repositories"));

    let mut outputs = Vec::new();

    let repo_names_to_sync: Vec<String> = if let Some(name) = repo_name {
        vec![name]
    } else {
        let records = ctx.tracking_store.list_project_repositories(proj_id)?;
        records
            .into_iter()
            .filter(|r| matches!(r.status, TrackingStatus::Tracked(ref prefs) if prefs.source))
            .map(|r| r.repo_name)
            .collect()
    };

    for name in repo_names_to_sync {
        let repo_path = clone_root.join(&name);
        let remote_url = format!("https://github.com/{}/{}.git", project.name, name);
        let default_branch = "main";

        let outcome_str = if dry_run {
            let state = ctx.git_port.inspect_clone(&repo_path, default_branch)?;
            format!("DryRun(state: {:?})", state)
        } else {
            let res = ctx.sync_repo_uc.execute(
                proj_id,
                &name,
                &remote_url,
                &repo_path,
                default_branch,
            )?;
            format_outcome(&res)
        };

        outputs.push(SyncItemOutput {
            repo_name: name,
            path: repo_path,
            outcome: outcome_str,
        });
    }

    let report = SyncReport {
        project_id,
        outcomes: outputs,
    };

    output_value(json, &report, |r| {
        let mode_str = if dry_run { " (DRY-RUN)" } else { "" };
        print_info(
            false,
            format!(
                "Synchronized {} repository/repositories for project {}{}:",
                r.outcomes.len(),
                r.project_id,
                mode_str
            ),
        );
        for item in &r.outcomes {
            print_info(
                false,
                format!("  • {:<25} => {}", item.repo_name, item.outcome),
            );
        }
    });

    Ok(())
}

fn format_outcome(outcome: &SyncOutcome) -> String {
    match outcome {
        SyncOutcome::Cloned { revision } => format!("Cloned at {}", revision),
        SyncOutcome::FastForwarded { revision } => format!("FastForwarded to {}", revision),
        SyncOutcome::AlreadyCurrent => "AlreadyCurrent".to_string(),
        SyncOutcome::Skipped { reason } => format!("Skipped ({})", reason),
    }
}
