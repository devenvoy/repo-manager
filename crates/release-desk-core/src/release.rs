//! GitHub release domain models and selection rules.

use crate::error::ValidationError;
use crate::installer::{Asset, AssetConnection};
use crate::snapshot::Repository;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A validated release tag name (e.g. "v1.2.3").
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ReleaseTag(String);

impl ReleaseTag {
    pub fn new(raw: &str) -> Result<Self, ValidationError> {
        let tag = raw.trim();
        if tag.is_empty() {
            return Err(ValidationError::Empty("release tag"));
        }
        if tag.len() > 100 {
            return Err(ValidationError::TooLong {
                field: "release tag",
                max: 100,
            });
        }
        if tag.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(ValidationError::InvalidCharacter("release tag"));
        }
        Ok(Self(tag.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ReleaseTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Target CPU architecture for release assets.
#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Architecture {
    AppleSilicon,
    Intel,
    Universal,
    #[default]
    Unknown,
}

impl Architecture {
    pub fn from_filename(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        if lower.contains("arm64") || lower.contains("aarch64") {
            Self::AppleSilicon
        } else if lower.contains("x86_64") || lower.contains("x64") || lower.contains("intel") {
            Self::Intel
        } else if lower.contains("universal") {
            Self::Universal
        } else {
            Self::Unknown
        }
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::AppleSilicon => "arm64",
            Self::Intel => "x86_64",
            Self::Universal => "universal",
            Self::Unknown => "unknown",
        }
    }

    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::AppleSilicon => "Apple Silicon",
            Self::Intel => "Intel",
            Self::Universal => "Universal",
            Self::Unknown => "Universal / Other",
        }
    }
}

impl fmt::Display for Architecture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

/// Connection wrapper for paginated GraphQL releases.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReleaseConnection {
    pub nodes: Vec<Release>,
}

/// GitHub release entity.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub name: Option<String>,
    pub tag_name: String,
    pub description: Option<String>,
    pub is_draft: bool,
    pub is_prerelease: bool,
    pub published_at: Option<String>,
    pub created_at: String,
    pub url: String,
    pub release_assets: AssetConnection,
}

/// Choose the newest eligible release with at least one installer asset matching extension.
///
/// Deterministic sorting uses publication time when present (primary sort key);
/// created-at timestamp is the fallback for older unannotated records.
pub fn latest_installer<'a>(
    repo: &'a Repository,
    include_prereleases: bool,
    extension: &str,
) -> Option<(&'a Release, Vec<&'a Asset>)> {
    let extension = extension.trim_start_matches('.');
    repo.releases
        .as_ref()?
        .nodes
        .iter()
        .filter(|release| !release.is_draft && (include_prereleases || !release.is_prerelease))
        .filter_map(|release| {
            let assets: Vec<_> = release
                .release_assets
                .nodes
                .iter()
                .filter(|asset| {
                    asset
                        .name
                        .rsplit_once('.')
                        .is_some_and(|(_, suffix)| suffix.eq_ignore_ascii_case(extension))
                })
                .collect();
            (!assets.is_empty()).then_some((release, assets))
        })
        .max_by(|(a, _), (b, _)| {
            a.published_at
                .as_deref()
                .unwrap_or(&a.created_at)
                .cmp(b.published_at.as_deref().unwrap_or(&b.created_at))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_tag_validation() {
        assert_eq!(ReleaseTag::new("v2.0.0").unwrap().as_str(), "v2.0.0");
        assert!(ReleaseTag::new("").is_err());
        assert!(ReleaseTag::new("v 1.0").is_err());
    }

    #[test]
    fn architecture_parsing() {
        assert_eq!(
            Architecture::from_filename("app-arm64.dmg"),
            Architecture::AppleSilicon
        );
        assert_eq!(
            Architecture::from_filename("app-x86_64.dmg"),
            Architecture::Intel
        );
        assert_eq!(
            Architecture::from_filename("app-universal.pkg"),
            Architecture::Universal
        );
        assert_eq!(
            Architecture::from_filename("app-setup.exe"),
            Architecture::Unknown
        );
    }
}
