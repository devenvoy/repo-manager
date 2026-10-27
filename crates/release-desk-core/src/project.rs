//! Project and GitHub account domain models.

use crate::error::{ValidationError, validate_name};
use serde::{Deserialize, Serialize};
use std::fmt;

/// A user-facing project name. Names are never used as filesystem components.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct ProjectName(String);

impl ProjectName {
    pub fn new(raw: &str) -> Result<Self, ValidationError> {
        validate_name("project name", raw, 80, |c| !c.is_control()).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProjectName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A GitHub account login used in URLs and API lookups.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct AccountLogin(String);

impl AccountLogin {
    pub fn new(raw: &str) -> Result<Self, ValidationError> {
        validate_name("account login", raw, 39, |c| {
            c.is_ascii_alphanumeric() || c == '-'
        })
        .map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AccountLogin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Stable application project identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct ProjectId(u128);

impl ProjectId {
    pub const fn from_u128(value: u128) -> Self {
        Self(value)
    }

    pub const fn as_u128(self) -> u128 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AccountKind {
    Organization,
    User,
}

/// An account selected as a repository source.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AccountIdentity {
    pub login: AccountLogin,
    pub kind: AccountKind,
}

/// Project identity and its user-facing label.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectIdentity {
    pub id: ProjectId,
    pub name: ProjectName,
}

/// Independent tracking choices for one repository in one project.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TrackingPreferences {
    pub source: bool,
    pub installers: bool,
    pub pull_requests: bool,
}

impl TrackingPreferences {
    pub const fn all() -> Self {
        Self {
            source: true,
            installers: true,
            pull_requests: true,
        }
    }

    pub const fn is_tracked(self) -> bool {
        self.source || self.installers || self.pull_requests
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_and_account_name_validation() {
        assert_eq!(ProjectName::new(" ArtCraft ").unwrap().as_str(), "ArtCraft");
        assert_eq!(
            AccountLogin::new("storytold").unwrap().as_str(),
            "storytold"
        );
        assert!(ProjectName::new("").is_err());
        assert!(AccountLogin::new("invalid/user").is_err());
    }

    #[test]
    fn tracking_preferences_independence() {
        let prefs = TrackingPreferences {
            source: false,
            installers: true,
            pull_requests: false,
        };
        assert!(prefs.is_tracked());
        assert!(!TrackingPreferences::default().is_tracked());
    }
}
