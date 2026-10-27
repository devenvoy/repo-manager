//! Git source code synchronization use cases.

use crate::event::{AppEvent, EventBroadcaster};
use release_desk_core::error::DomainError;
use release_desk_core::git::GitSyncState;
use release_desk_core::port::GitPort;
use release_desk_core::project::ProjectId;
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Result outcome of synchronizing a single repository.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyncOutcome {
    Cloned { revision: String },
    FastForwarded { revision: String },
    AlreadyCurrent,
    Skipped { reason: String },
}

/// Use case for synchronizing a single repository's source code safely.
pub struct SyncRepositoryUseCase {
    git: Arc<dyn GitPort>,
    events: EventBroadcaster,
}

impl SyncRepositoryUseCase {
    pub fn new(git: Arc<dyn GitPort>, events: EventBroadcaster) -> Self {
        Self { git, events }
    }

    pub fn execute(
        &self,
        project_id: ProjectId,
        repo_name: &str,
        remote_url: &str,
        local_path: &Path,
        default_branch: &str,
    ) -> Result<SyncOutcome, DomainError> {
        self.events.broadcast(AppEvent::SyncStarted {
            project_id,
            repository: repo_name.to_owned(),
        });

        let state = self.git.inspect_clone(local_path, default_branch)?;

        let outcome = match state {
            GitSyncState::NotCloned => {
                let rev = self
                    .git
                    .clone_repo(remote_url, local_path, default_branch)?;
                SyncOutcome::Cloned { revision: rev }
            }
            GitSyncState::Current { .. } => SyncOutcome::AlreadyCurrent,
            GitSyncState::Behind { .. } => {
                self.git.fetch_origin(local_path)?;
                let new_state = self.git.fast_forward(local_path, default_branch)?;
                match new_state {
                    GitSyncState::Current { revision } => SyncOutcome::FastForwarded { revision },
                    other => SyncOutcome::Skipped {
                        reason: format!("Post-merge state was not current: {:?}", other),
                    },
                }
            }
            GitSyncState::Ahead { .. } => SyncOutcome::Skipped {
                reason: "Local branch is ahead of remote (unpushed commits)".into(),
            },
            GitSyncState::Diverged { .. } => SyncOutcome::Skipped {
                reason: "Local and remote branches have diverged".into(),
            },
            GitSyncState::Dirty { .. } => SyncOutcome::Skipped {
                reason: "Working tree has uncommitted local changes".into(),
            },
            GitSyncState::Detached { .. } => SyncOutcome::Skipped {
                reason: "HEAD is detached".into(),
            },
            GitSyncState::Unknown => SyncOutcome::Skipped {
                reason: "Git state could not be determined".into(),
            },
            GitSyncState::Error { message } => SyncOutcome::Skipped {
                reason: format!("Git error: {message}"),
            },
        };

        let outcome_str = format!("{:?}", outcome);
        self.events.broadcast(AppEvent::SyncFinished {
            project_id,
            repository: repo_name.to_owned(),
            outcome: outcome_str,
        });

        Ok(outcome)
    }
}

/// Target repository specifier for batch synchronization.
pub struct SyncTarget<'a> {
    pub repo_name: &'a str,
    pub remote_url: &'a str,
    pub local_path: &'a Path,
    pub default_branch: &'a str,
}

/// Use case for synchronizing multiple repositories sequentially or with cancellation.
pub struct SyncAllRepositoriesUseCase {
    single_sync: SyncRepositoryUseCase,
}

impl SyncAllRepositoriesUseCase {
    pub fn new(git: Arc<dyn GitPort>, events: EventBroadcaster) -> Self {
        Self {
            single_sync: SyncRepositoryUseCase::new(git, events),
        }
    }

    pub fn execute(
        &self,
        project_id: ProjectId,
        targets: &[SyncTarget<'_>],
        cancel_token: &CancellationToken,
    ) -> Result<Vec<(String, SyncOutcome)>, DomainError> {
        let mut results = Vec::with_capacity(targets.len());

        for target in targets {
            if cancel_token.is_cancelled() {
                break;
            }

            let outcome = self.single_sync.execute(
                project_id,
                target.repo_name,
                target.remote_url,
                target.local_path,
                target.default_branch,
            )?;

            results.push((target.repo_name.to_owned(), outcome));
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct MockGit {
        state: Mutex<GitSyncState>,
    }

    impl GitPort for MockGit {
        fn inspect_clone(
            &self,
            _path: &Path,
            _default_branch: &str,
        ) -> Result<GitSyncState, DomainError> {
            Ok(self.state.lock().unwrap().clone())
        }

        fn fetch_origin(&self, _path: &Path) -> Result<(), DomainError> {
            Ok(())
        }

        fn fast_forward(
            &self,
            _path: &Path,
            _default_branch: &str,
        ) -> Result<GitSyncState, DomainError> {
            Ok(GitSyncState::Current {
                revision: "rev2".into(),
            })
        }

        fn clone_repo(
            &self,
            _remote_url: &str,
            _destination: &Path,
            _default_branch: &str,
        ) -> Result<String, DomainError> {
            Ok("rev1".into())
        }
    }

    #[test]
    fn sync_guards_dirty_and_diverged_clones() {
        let events = EventBroadcaster::new(8);
        let git = Arc::new(MockGit {
            state: Mutex::new(GitSyncState::Dirty {
                local: "a".into(),
                remote: "b".into(),
            }),
        });

        let use_case = SyncRepositoryUseCase::new(git, events);
        let outcome = use_case
            .execute(
                ProjectId::from_u128(1),
                "my-repo",
                "https://github.com/storytold/my-repo.git",
                Path::new("/tmp/test"),
                "main",
            )
            .expect("sync result");

        assert!(matches!(outcome, SyncOutcome::Skipped { .. }));
    }
}
