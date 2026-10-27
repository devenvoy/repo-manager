//! Read-only legacy snapshot types and parser.

use crate::git::BranchRef;
use crate::release::ReleaseConnection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The snapshot written by sync operations or legacy scripts.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Snapshot {
    pub org: String,
    pub generated: String,
    pub repos: Vec<Repository>,
}

/// A repository as captured in the snapshot JSON.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    pub name: String,
    pub description: Option<String>,
    pub url: String,
    pub is_private: bool,
    pub is_archived: bool,
    pub pushed_at: Option<String>,
    pub primary_language: Option<Language>,
    pub default_branch_ref: Option<BranchRef>,
    #[serde(default)]
    pub releases: Option<ReleaseConnection>,
}

/// Primary programming language metadata.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Language {
    pub name: String,
}

/// Detailed discrepancy report comparing a project folder's data.json backup against SQLite.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct MismatchReport {
    pub project_root: PathBuf,
    pub disk_snapshot_present: bool,
    pub matched_repositories: Vec<String>,
    pub missing_in_database: Vec<String>,
    pub missing_on_disk: Vec<String>,
    pub revision_mismatches: Vec<RevisionMismatch>,
}

impl MismatchReport {
    /// Returns true if the database and the project folder data.json are in complete sync.
    pub fn is_synchronized(&self) -> bool {
        self.disk_snapshot_present
            && self.missing_in_database.is_empty()
            && self.missing_on_disk.is_empty()
            && self.revision_mismatches.is_empty()
    }
}

/// A revision or head commit discrepancy for a specific repository.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RevisionMismatch {
    pub repository: String,
    pub disk_revision: Option<String>,
    pub db_revision: Option<String>,
}

/// Parse a snapshot without performing I/O. Rejects non-snapshot JSON documents.
pub fn parse_snapshot(text: &str) -> Result<Snapshot, serde_json::Error> {
    serde_json::from_str(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_snapshot() {
        let json = r#"{"org":"storytold","generated":"2026-10-27T00:00:00Z","repos":[]}"#;
        let snap = parse_snapshot(json).expect("valid snapshot");
        assert_eq!(snap.org, "storytold");
        assert!(snap.repos.is_empty());
    }

    #[test]
    fn reject_invalid_snapshot() {
        assert!(parse_snapshot("{}").is_err());
    }
}
