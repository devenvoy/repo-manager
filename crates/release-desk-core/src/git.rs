//! Git synchronization states and revision models.

use serde::{Deserialize, Serialize};

/// Revision target with git commit OID.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Revision {
    pub oid: String,
}

/// Tracked branch reference.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct BranchRef {
    pub name: String,
    pub target: Revision,
}

/// Full local/remote source status; distinguishes all 8 states cleanly.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum GitSyncState {
    Unknown,
    NotCloned,
    Current { revision: String },
    Behind { local: String, remote: String },
    Ahead { local: String, remote: String },
    Diverged { local: String, remote: String },
    Dirty { local: String, remote: String },
    Detached { local: String },
    Error { message: String },
}

impl GitSyncState {
    /// Only a missing clone or clean branch behind its remote needs a safe
    /// fast-forward. Dirty, ahead, diverged, and detached work is never modified.
    pub const fn can_update_safely(&self) -> bool {
        matches!(self, Self::NotCloned | Self::Behind { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_safety_rules() {
        assert!(GitSyncState::NotCloned.can_update_safely());
        assert!(
            GitSyncState::Behind {
                local: "a".into(),
                remote: "b".into()
            }
            .can_update_safely()
        );
        assert!(
            !GitSyncState::Current {
                revision: "a".into()
            }
            .can_update_safely()
        );
        assert!(
            !GitSyncState::Ahead {
                local: "b".into(),
                remote: "a".into()
            }
            .can_update_safely()
        );
        assert!(
            !GitSyncState::Diverged {
                local: "a".into(),
                remote: "b".into()
            }
            .can_update_safely()
        );
        assert!(
            !GitSyncState::Dirty {
                local: "a".into(),
                remote: "b".into()
            }
            .can_update_safely()
        );
        assert!(!GitSyncState::Detached { local: "a".into() }.can_update_safely());
    }
}
