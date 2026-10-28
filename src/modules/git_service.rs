//! Git orchestration service for repository verification, clones, and pulls.

use crate::repository_git::{
    RepositoryStatus, RepositoryTarget, check_for_updates, clone_target as git_clone_target,
    inspect, targets, update,
};
use crate::workspace::WorkspaceProject;
use release_desk_git::SyncOutcome;

/// Inspection mode for repository workspace.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InspectMode {
    LocalOnly,
    CheckRemoteUpdates,
    PullTarget(usize, usize),
}

/// Run Git status checks on a background thread.
pub fn run_status_check(
    project: &WorkspaceProject,
    mode: InspectMode,
    previous_statuses: &[RepositoryStatus],
) -> Result<(Vec<RepositoryStatus>, Option<String>), String> {
    let resolved_targets = targets(project)?;
    let mut update_message = None;

    let statuses = resolved_targets
        .iter()
        .map(|target| match mode {
            InspectMode::PullTarget(si, ri)
                if si == target.source_index && ri == target.repo_index =>
            {
                let outcome_msg = match update(target) {
                    Ok(outcome) => match outcome {
                        SyncOutcome::FastForwarded { .. } => {
                            format!("{}: updated successfully (fast-forwarded)", target.name)
                        }
                        SyncOutcome::AlreadyCurrent => {
                            format!("{}: already up to date", target.name)
                        }
                        SyncOutcome::Skipped(state) => {
                            format!("{}: skipped ({:?})", target.name, state.status)
                        }
                        SyncOutcome::Cloned { .. } => {
                            format!("{}: cloned successfully", target.name)
                        }
                    },
                    Err(err) => format!("{}: failed to update ({err})", target.name),
                };
                update_message = Some(outcome_msg);
                check_for_updates(target)
            }
            InspectMode::LocalOnly => inspect(target),
            InspectMode::CheckRemoteUpdates => check_for_updates(target),
            InspectMode::PullTarget(_, _) => previous_statuses
                .iter()
                .find(|s| {
                    s.source_index == target.source_index && s.repo_index == target.repo_index
                })
                .cloned()
                .unwrap_or_else(|| inspect(target)),
        })
        .collect();

    Ok((statuses, update_message))
}

/// Clone a single repository target.
pub fn clone_target(target: &RepositoryTarget) -> Result<SyncOutcome, String> {
    git_clone_target(target)
}

/// Find all repository targets that have not yet been cloned.
pub fn missing_targets(project: &WorkspaceProject) -> Result<Vec<RepositoryTarget>, String> {
    let all = targets(project)?;
    let missing = all
        .into_iter()
        .filter(|t| !t.path.exists() || !t.path.join(".git").exists())
        .collect();
    Ok(missing)
}

/// Clone all missing repositories in a project.
pub fn clone_all_missing(project: &WorkspaceProject) -> Result<usize, String> {
    let missing = missing_targets(project)?;
    let count = missing.len();
    for target in missing {
        git_clone_target(&target)?;
    }
    Ok(count)
}
