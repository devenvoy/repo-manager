//! Installer assets, shelf status, and retention planning.

use crate::error::ValidationError;
use crate::job::DownloadProgress;
use crate::storage::RelativeManagedPath;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A normalized installer file extension (e.g. "dmg", "zip", "pkg").
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct InstallerExtension(String);

impl InstallerExtension {
    pub fn new(raw: &str) -> Result<Self, ValidationError> {
        let val = raw.trim().trim_start_matches('.').to_ascii_lowercase();
        if val.is_empty() {
            return Err(ValidationError::Empty("installer extension"));
        }
        if val.len() > 12 {
            return Err(ValidationError::TooLong {
                field: "installer extension",
                max: 12,
            });
        }
        if !val.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(ValidationError::InvalidCharacter("installer extension"));
        }
        Ok(Self(val))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InstallerExtension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Connection wrapper for GraphQL assets.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AssetConnection {
    pub nodes: Vec<Asset>,
}

/// Release asset item from GitHub API.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub name: String,
    pub size: u64,
    pub download_url: String,
}

/// Installer availability across GitHub metadata and local managed library.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum InstallerStatus {
    NoInstallerAvailable,
    New,
    UpdateAvailable,
    MissingLocally,
    Latest,
    Downloading(DownloadProgress),
    Failed(String),
}

/// Inputs required to derive installer shelf status.
pub struct InstallerObservation<'a> {
    pub latest_signature: Option<&'a str>,
    pub current_signature: Option<&'a str>,
    pub current_files_missing: bool,
    pub download: Option<DownloadProgress>,
    pub last_error: Option<&'a str>,
}

impl InstallerStatus {
    /// In-progress and failed jobs take visual precedence over steady-state.
    pub fn from_observation(observation: InstallerObservation<'_>) -> Self {
        if let Some(progress) = observation.download {
            return Self::Downloading(progress);
        }
        if let Some(error) = observation.last_error {
            return Self::Failed(error.to_owned());
        }
        let Some(latest) = observation.latest_signature else {
            return Self::NoInstallerAvailable;
        };
        let Some(current) = observation.current_signature else {
            return Self::New;
        };
        if current != latest {
            return Self::UpdateAvailable;
        }
        if observation.current_files_missing {
            return Self::MissingLocally;
        }
        Self::Latest
    }
}

/// Outcome of planning an installer rotation across active and archived storage.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RotationPlan {
    /// Active files to move to archive directory.
    pub to_archive: Vec<RelativeManagedPath>,
    /// Archived files to delete because they exceed retention limits.
    pub to_prune: Vec<RelativeManagedPath>,
    /// Incoming staged files to promote to active directory.
    pub to_install: Vec<RelativeManagedPath>,
}

/// Retention service calculating safe installer rotation and pruning.
pub struct RetentionPlanner;

impl RetentionPlanner {
    /// Compute which files should move to archive, which should be pruned, and which installed.
    pub fn plan_rotation(
        current_files: &[RelativeManagedPath],
        existing_archives: &[RelativeManagedPath],
        incoming_files: &[RelativeManagedPath],
        is_same_version: bool,
        retention_limit: usize,
    ) -> RotationPlan {
        let mut plan = RotationPlan {
            to_install: incoming_files.to_vec(),
            ..Default::default()
        };

        if is_same_version {
            return plan;
        }

        plan.to_archive = current_files.to_vec();
        let total_archive_count = existing_archives.len() + usize::from(!current_files.is_empty());

        if retention_limit == 0 {
            plan.to_prune.extend_from_slice(existing_archives);
            plan.to_prune.extend_from_slice(current_files);
        } else if total_archive_count > retention_limit {
            let excess = total_archive_count - retention_limit;
            for file in existing_archives.iter().take(excess) {
                plan.to_prune.push(file.clone());
            }
        }

        plan
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_normalization() {
        assert_eq!(InstallerExtension::new(".DMG").unwrap().as_str(), "dmg");
        assert_eq!(InstallerExtension::new("tar.gz").is_err(), true);
    }

    #[test]
    fn installer_status_derivation() {
        let status = InstallerStatus::from_observation(InstallerObservation {
            latest_signature: Some("v1|a1"),
            current_signature: None,
            current_files_missing: false,
            download: None,
            last_error: None,
        });
        assert_eq!(status, InstallerStatus::New);
    }
}
