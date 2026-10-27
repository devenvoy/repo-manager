//! Pull request domain model and state tracking.

use crate::project::AccountLogin;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Pull request number identifier.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct PullRequestNumber(u64);

impl PullRequestNumber {
    pub const fn from_u64(val: u64) -> Self {
        Self(val)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for PullRequestNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// GitHub Pull Request state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PullRequestState {
    Open,
    Merged,
    Closed,
    Draft,
}

impl PullRequestState {
    /// Derive accurate PR state from GitHub status fields.
    pub fn from_flags(state: &str, draft: bool, merged_at: Option<&str>) -> Self {
        if merged_at.is_some() {
            Self::Merged
        } else if state.eq_ignore_ascii_case("open") {
            if draft { Self::Draft } else { Self::Open }
        } else {
            Self::Closed
        }
    }

    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Merged => "Merged",
            Self::Closed => "Closed",
            Self::Draft => "Draft",
        }
    }
}

impl fmt::Display for PullRequestState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

/// GitHub pull request label.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PullRequestLabel {
    pub name: String,
    pub color: String,
}

/// Pull request domain entity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub state: PullRequestState,
    pub draft: bool,
    pub author: Option<AccountLogin>,
    pub head_ref: String,
    pub base_ref: String,
    pub html_url: String,
    pub created_at: String,
    pub updated_at: String,
    pub merged_at: Option<String>,
    pub labels: Vec<PullRequestLabel>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_request_states() {
        assert_eq!(
            PullRequestState::from_flags("open", false, None),
            PullRequestState::Open
        );
        assert_eq!(
            PullRequestState::from_flags("open", true, None),
            PullRequestState::Draft
        );
        assert_eq!(
            PullRequestState::from_flags("closed", false, Some("2026-10-27T00:00:00Z")),
            PullRequestState::Merged
        );
        assert_eq!(
            PullRequestState::from_flags("closed", false, None),
            PullRequestState::Closed
        );
    }
}
