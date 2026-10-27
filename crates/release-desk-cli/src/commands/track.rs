//! Repository tracking preference commands.

use crate::context::CliContext;
use crate::output::{output_value, print_info};
use release_desk_application::use_case::discovery::TrackingStatus;
use release_desk_core::error::DomainError;
use release_desk_core::project::{ProjectId, TrackingPreferences};
use release_desk_core::repository::GitHubRepositoryId;
use serde::Serialize;

#[derive(Serialize)]
pub struct TrackReport {
    pub project_id: u128,
    pub repo_id: u64,
    pub status: String,
}

pub fn handle_track(
    ctx: &CliContext,
    project_id: u128,
    repo_id: u64,
    source: bool,
    installers: bool,
    pr: bool,
    json: bool,
) -> Result<(), DomainError> {
    let proj_id = ProjectId::from_u128(project_id);
    let github_id = GitHubRepositoryId::from_u64(repo_id);

    let prefs = TrackingPreferences {
        source,
        installers,
        pull_requests: pr,
    };

    ctx.track_uc
        .execute(proj_id, github_id, TrackingStatus::Tracked(prefs))?;

    let report = TrackReport {
        project_id,
        repo_id,
        status: format!("{:?}", prefs),
    };

    output_value(json, &report, |r| {
        print_info(
            false,
            format!(
                "✓ Repository {} tracking updated for project {}: {}",
                r.repo_id, r.project_id, r.status
            ),
        );
    });

    Ok(())
}

pub fn handle_untrack(
    ctx: &CliContext,
    project_id: u128,
    repo_id: u64,
    json: bool,
) -> Result<(), DomainError> {
    let proj_id = ProjectId::from_u128(project_id);
    let github_id = GitHubRepositoryId::from_u64(repo_id);

    ctx.track_uc
        .execute(proj_id, github_id, TrackingStatus::Ignored)?;

    let report = TrackReport {
        project_id,
        repo_id,
        status: "Ignored".to_string(),
    };

    output_value(json, &report, |r| {
        print_info(
            false,
            format!(
                "✓ Repository {} ignored/untracked for project {}",
                r.repo_id, r.project_id
            ),
        );
    });

    Ok(())
}
