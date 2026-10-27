//! Repository domain model and identity rules.

use crate::error::{ValidationError, validate_name};
use crate::project::AccountLogin;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A GitHub repository display name; it is never joined directly to an arbitrary path.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct RepositoryName(String);

impl RepositoryName {
    pub fn new(raw: &str) -> Result<Self, ValidationError> {
        let name = validate_name("repository name", raw, 100, |c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')
        })?;
        if name == "." || name == ".." {
            return Err(ValidationError::InvalidCharacter("repository name"));
        }
        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RepositoryName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Stable GitHub identity; repository names can change without changing this value.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct GitHubRepositoryId(u64);

impl GitHubRepositoryId {
    pub const fn from_u64(value: u64) -> Self {
        Self(value)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

/// Repository identity keeps the immutable GitHub ID alongside its current
/// owner and name, which may change after a transfer or rename.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RepositoryIdentity {
    pub id: GitHubRepositoryId,
    pub owner: AccountLogin,
    pub name: RepositoryName,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_name_validation() {
        assert_eq!(
            RepositoryName::new("repo-name_1.0").unwrap().as_str(),
            "repo-name_1.0"
        );
        assert!(RepositoryName::new(".").is_err());
        assert!(RepositoryName::new("..").is_err());
        assert!(RepositoryName::new("has space").is_err());
    }

    #[test]
    fn repository_identity_survives_rename() {
        let id = GitHubRepositoryId::from_u64(100);
        let original = RepositoryIdentity {
            id,
            owner: AccountLogin::new("storytold").unwrap(),
            name: RepositoryName::new("old-repo").unwrap(),
        };
        let renamed = RepositoryIdentity {
            name: RepositoryName::new("new-repo").unwrap(),
            ..original
        };
        assert_eq!(renamed.id.as_u64(), 100);
        assert_eq!(renamed.name.as_str(), "new-repo");
    }
}
