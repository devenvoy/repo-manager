//! Installer download, verification, and shelf management use cases.

use release_desk_core::error::DomainError;
use release_desk_core::installer::{
    InstallerObservation, InstallerStatus, RetentionPlanner, RotationPlan,
};
use release_desk_core::storage::{RelativeManagedPath, Sha256Checksum};
use std::sync::Arc;

/// Abstract port for streaming binary installer asset downloads.
pub trait DownloadPort: Send + Sync {
    fn download_to_file(
        &self,
        url: &str,
        expected_size: u64,
        destination: &RelativeManagedPath,
    ) -> Result<Sha256Checksum, DomainError>;
}

/// Use case for deriving installer availability across repository releases.
pub struct CheckInstallerStatusUseCase;
pub type CheckInstallerUpdatesUseCase = CheckInstallerStatusUseCase;

impl CheckInstallerStatusUseCase {
    pub fn execute(observation: InstallerObservation<'_>) -> InstallerStatus {
        InstallerStatus::from_observation(observation)
    }
}

/// Command input for downloading and rotating an installer.
pub struct InstallAssetCommand {
    pub download_url: String,
    pub expected_size: u64,
    pub expected_checksum: Option<Sha256Checksum>,
    pub staged_path: RelativeManagedPath,
    pub current_files: Vec<RelativeManagedPath>,
    pub archived_files: Vec<RelativeManagedPath>,
    pub is_same_version: bool,
    pub retention_limit: usize,
}

/// Outcome of a successful installer download and rotation planning.
pub struct InstallAssetOutcome {
    pub computed_checksum: Sha256Checksum,
    pub rotation_plan: RotationPlan,
}

/// Use case for downloading an installer asset and planning atomic rotation.
pub struct DownloadAndPlanInstallerUseCase {
    downloader: Arc<dyn DownloadPort>,
}
pub type DownloadInstallerUseCase = DownloadAndPlanInstallerUseCase;

impl DownloadAndPlanInstallerUseCase {
    pub fn new(downloader: Arc<dyn DownloadPort>) -> Self {
        Self { downloader }
    }

    pub fn execute(&self, cmd: InstallAssetCommand) -> Result<InstallAssetOutcome, DomainError> {
        let computed_checksum = self.downloader.download_to_file(
            &cmd.download_url,
            cmd.expected_size,
            &cmd.staged_path,
        )?;

        // If an expected checksum was supplied, verify it matches
        if let Some(ref expected) = cmd.expected_checksum {
            if computed_checksum != *expected {
                return Err(DomainError::Validation(
                    release_desk_core::error::ValidationError::InvalidCharacter(
                        "checksum mismatch: downloaded file is corrupt",
                    ),
                ));
            }
        }

        let incoming = [cmd.staged_path];
        let rotation_plan = RetentionPlanner::plan_rotation(
            &cmd.current_files,
            &cmd.archived_files,
            &incoming,
            cmd.is_same_version,
            cmd.retention_limit,
        );

        Ok(InstallAssetOutcome {
            computed_checksum,
            rotation_plan,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockDownloader {
        checksum: Sha256Checksum,
    }

    impl DownloadPort for MockDownloader {
        fn download_to_file(
            &self,
            _url: &str,
            _expected_size: u64,
            _destination: &RelativeManagedPath,
        ) -> Result<Sha256Checksum, DomainError> {
            Ok(self.checksum.clone())
        }
    }

    #[test]
    fn download_and_plan_rotation() {
        let sha =
            Sha256Checksum::new("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
                .unwrap();
        let downloader = Arc::new(MockDownloader {
            checksum: sha.clone(),
        });
        let use_case = DownloadAndPlanInstallerUseCase::new(downloader);

        let staged = RelativeManagedPath::new("staging/app.dmg").unwrap();
        let current = vec![RelativeManagedPath::new("current/old-app.dmg").unwrap()];

        let outcome = use_case
            .execute(InstallAssetCommand {
                download_url: "https://example.com/app.dmg".into(),
                expected_size: 1024,
                expected_checksum: Some(sha.clone()),
                staged_path: staged.clone(),
                current_files: current.clone(),
                archived_files: vec![],
                is_same_version: false,
                retention_limit: 1,
            })
            .expect("download and plan");

        assert_eq!(outcome.computed_checksum, sha);
        assert_eq!(outcome.rotation_plan.to_install, vec![staged]);
        assert_eq!(outcome.rotation_plan.to_archive, current);
    }
}
