//! Read-only legacy snapshot types and parser.

use crate::git::BranchRef;
use crate::release::ReleaseConnection;
use serde::Deserialize;

/// The read-only snapshot written by `storytold-sync.sh`.
#[derive(Debug, Deserialize)]
pub struct Snapshot {
    pub org: String,
    pub generated: String,
    pub repos: Vec<Repository>,
}

/// A repository as captured in the legacy snapshot JSON.
#[derive(Debug, Deserialize)]
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
    pub releases: ReleaseConnection,
}

/// Primary programming language metadata.
#[derive(Debug, Deserialize)]
pub struct Language {
    pub name: String,
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
