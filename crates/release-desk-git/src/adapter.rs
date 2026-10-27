//! System Git adapter implementing domain `GitPort`.

use crate::{CloneState, CloneStatus, GitClient, SyncOutcome};
use release_desk_core::error::DomainError;
use release_desk_core::git::GitSyncState;
use release_desk_core::port::GitPort;
use std::path::Path;

/// An adapter wrapping system Git execution for the domain GitPort.
#[derive(Clone, Debug, Default)]
pub struct SystemGitPort {
    client: GitClient,
}

impl SystemGitPort {
    pub fn new() -> Self {
        Self {
            client: GitClient::new(),
        }
    }
}

fn map_state_to_domain(state: CloneState) -> GitSyncState {
    match state.status {
        CloneStatus::NotCloned => GitSyncState::NotCloned,
        CloneStatus::Current => GitSyncState::Current {
            revision: state.local_revision.unwrap_or_default(),
        },
        CloneStatus::Behind => GitSyncState::Behind {
            local: state.local_revision.unwrap_or_default(),
            remote: state.remote_revision.unwrap_or_default(),
        },
        CloneStatus::Ahead => GitSyncState::Ahead {
            local: state.local_revision.unwrap_or_default(),
            remote: state.remote_revision.unwrap_or_default(),
        },
        CloneStatus::Diverged => GitSyncState::Diverged {
            local: state.local_revision.unwrap_or_default(),
            remote: state.remote_revision.unwrap_or_default(),
        },
        CloneStatus::Dirty => GitSyncState::Dirty {
            local: state.local_revision.unwrap_or_default(),
            remote: state.remote_revision.unwrap_or_default(),
        },
        CloneStatus::Detached => GitSyncState::Detached {
            local: state.local_revision.unwrap_or_default(),
        },
        CloneStatus::OtherBranch => GitSyncState::Error {
            message: format!("checked out on non-default branch: {:?}", state.branch),
        },
        CloneStatus::Error => GitSyncState::Error {
            message: state.message.unwrap_or_else(|| "git error".into()),
        },
    }
}

impl GitPort for SystemGitPort {
    fn inspect_clone(
        &self,
        path: &Path,
        default_branch: &str,
    ) -> Result<GitSyncState, DomainError> {
        let state =
            self.client
                .inspect(path, default_branch)
                .map_err(|e| DomainError::PortFailure {
                    port: "Git",
                    message: e.to_string(),
                })?;
        Ok(map_state_to_domain(state))
    }

    fn fetch_origin(&self, path: &Path) -> Result<(), DomainError> {
        self.client
            .fetch(path)
            .map_err(|e| DomainError::PortFailure {
                port: "Git",
                message: e.to_string(),
            })
    }

    fn fast_forward(&self, path: &Path, default_branch: &str) -> Result<GitSyncState, DomainError> {
        let origin =
            self.client
                .inspect(path, default_branch)
                .map_err(|e| DomainError::PortFailure {
                    port: "Git",
                    message: e.to_string(),
                })?;

        // If not cloned or error, fail early
        if origin.status == CloneStatus::NotCloned || origin.status == CloneStatus::Error {
            return Err(DomainError::PortFailure {
                port: "Git",
                message: "cannot fast-forward: clone does not exist or has errors".into(),
            });
        }

        let remote_url = match self.client.inspect(path, default_branch) {
            Ok(_) => {
                // Fetch origin remote URL
                let out = std::process::Command::new("git")
                    .current_dir(path)
                    .args(["remote", "get-url", "origin"])
                    .output();
                match out {
                    Ok(o) if o.status.success() => {
                        String::from_utf8_lossy(&o.stdout).trim().to_string()
                    }
                    _ => {
                        return Err(DomainError::PortFailure {
                            port: "Git",
                            message: "could not read remote.origin.url".into(),
                        });
                    }
                }
            }
            Err(e) => {
                return Err(DomainError::PortFailure {
                    port: "Git",
                    message: e.to_string(),
                });
            }
        };

        let outcome = self
            .client
            .sync(&remote_url, path, default_branch)
            .map_err(|e| DomainError::PortFailure {
                port: "Git",
                message: e.to_string(),
            })?;

        match outcome {
            SyncOutcome::FastForwarded { to, .. } => Ok(GitSyncState::Current { revision: to }),
            SyncOutcome::AlreadyCurrent => {
                let state = self.client.inspect(path, default_branch).map_err(|e| {
                    DomainError::PortFailure {
                        port: "Git",
                        message: e.to_string(),
                    }
                })?;
                Ok(GitSyncState::Current {
                    revision: state.local_revision.unwrap_or_default(),
                })
            }
            SyncOutcome::Cloned { revision } => Ok(GitSyncState::Current { revision }),
            SyncOutcome::Skipped(state) => Ok(map_state_to_domain(state)),
        }
    }

    fn clone_repo(
        &self,
        remote_url: &str,
        destination: &Path,
        default_branch: &str,
    ) -> Result<String, DomainError> {
        let outcome = self
            .client
            .sync(remote_url, destination, default_branch)
            .map_err(|e| DomainError::PortFailure {
                port: "Git",
                message: e.to_string(),
            })?;

        match outcome {
            SyncOutcome::Cloned { revision } => Ok(revision),
            SyncOutcome::AlreadyCurrent => {
                let state = self
                    .client
                    .inspect(destination, default_branch)
                    .map_err(|e| DomainError::PortFailure {
                        port: "Git",
                        message: e.to_string(),
                    })?;
                Ok(state.local_revision.unwrap_or_default())
            }
            SyncOutcome::FastForwarded { to, .. } => Ok(to),
            SyncOutcome::Skipped(s) => Err(DomainError::PortFailure {
                port: "Git",
                message: format!("clone skipped or failed: {:?}", s.status),
            }),
        }
    }
}
