//! Headless macOS DMG application installer and launcher engine.
//!
//! Executes automated 1-click mounting, verification, extraction to `~/Applications`,
//! and unmounting of release disk images.

use release_desk_core::error::DomainError;
use std::fs;
use std::path::{Path, PathBuf};

/// Abstract port for mounting and unmounting disk images (`.dmg`).
pub trait DiskImagePort: Send + Sync {
    /// Attach the disk image read-only without opening a Finder window, returning its mount point.
    fn attach(&self, dmg_path: &Path) -> Result<PathBuf, DomainError>;

    /// Detach and eject the mounted volume.
    fn detach(&self, mount_point: &Path) -> Result<(), DomainError>;
}

/// System adapter that executes native macOS `hdiutil` commands.
#[derive(Clone, Debug, Default)]
pub struct HdiutilAdapter;

impl DiskImagePort for HdiutilAdapter {
    fn attach(&self, dmg_path: &Path) -> Result<PathBuf, DomainError> {
        let output = std::process::Command::new("hdiutil")
            .args(["attach", "-nobrowse", "-readonly", "-plist"])
            .arg(dmg_path)
            .output()
            .map_err(|e| DomainError::PortFailure {
                port: "hdiutil",
                message: format!("cannot execute hdiutil attach: {e}"),
            })?;

        if !output.status.success() {
            return Err(DomainError::PortFailure {
                port: "hdiutil",
                message: format!(
                    "hdiutil attach failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            });
        }

        // Parse plist output to find mount-point
        let out_str = String::from_utf8_lossy(&output.stdout);
        if let Some(pos) = out_str.find("<key>mount-point</key>") {
            let after = &out_str[pos..];
            if let (Some(s_start), Some(s_end)) = (after.find("<string>"), after.find("</string>"))
            {
                let mnt = &after[s_start + 8..s_end];
                return Ok(PathBuf::from(mnt.trim()));
            }
        }

        // Fallback guess: /Volumes/<dmg_stem>
        let stem = dmg_path.file_stem().unwrap_or_default().to_string_lossy();
        Ok(PathBuf::from(format!("/Volumes/{stem}")))
    }

    fn detach(&self, mount_point: &Path) -> Result<(), DomainError> {
        let output = std::process::Command::new("hdiutil")
            .args(["detach", "-force"])
            .arg(mount_point)
            .output()
            .map_err(|e| DomainError::PortFailure {
                port: "hdiutil",
                message: format!("cannot execute hdiutil detach: {e}"),
            })?;

        if !output.status.success() {
            return Err(DomainError::PortFailure {
                port: "hdiutil",
                message: format!(
                    "hdiutil detach failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            });
        }

        Ok(())
    }
}

/// Execution steps reported during the installation process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstallStep {
    VerifyingAsset,
    MountingDiskImage,
    CopyingBundle { bundle_name: String },
    UnmountingDiskImage,
    Completed { installed_path: PathBuf },
}

/// Coordinates the end-to-end installation of a macOS application from a DMG asset.
pub struct InstallMacOsAppUseCase<D: DiskImagePort> {
    disk_image: D,
    applications_dir: PathBuf,
}

impl<D: DiskImagePort> InstallMacOsAppUseCase<D> {
    pub fn new(disk_image: D, applications_dir: PathBuf) -> Self {
        Self {
            disk_image,
            applications_dir,
        }
    }

    /// Install the `.app` bundle from `dmg_path` into `applications_dir`.
    pub fn install_from_dmg(
        &self,
        dmg_path: &Path,
        expected_app_name: &str,
    ) -> Result<PathBuf, DomainError> {
        if !dmg_path.exists() {
            return Err(DomainError::NotFound {
                entity: "DMG asset",
                id: dmg_path.display().to_string(),
            });
        }

        // 1. Mount disk image
        let mount_point = self.disk_image.attach(dmg_path)?;

        // Guard unmount with RAII or deferred cleanup
        let result = self.extract_and_copy(&mount_point, expected_app_name);

        // 2. Unmount disk image
        let detach_res = self.disk_image.detach(&mount_point);

        // Check outcomes
        let installed_path = result?;
        detach_res?;

        Ok(installed_path)
    }

    fn extract_and_copy(
        &self,
        mount_point: &Path,
        expected_app_name: &str,
    ) -> Result<PathBuf, DomainError> {
        let app_bundle_name = if expected_app_name.ends_with(".app") {
            expected_app_name.to_string()
        } else {
            format!("{expected_app_name}.app")
        };

        let source_app = mount_point.join(&app_bundle_name);
        if !source_app.exists() {
            return Err(DomainError::PortFailure {
                port: "Installer",
                message: format!(
                    "bundle {app_bundle_name} not found in mounted volume {}",
                    mount_point.display()
                ),
            });
        }

        // Ensure applications directory exists
        fs::create_dir_all(&self.applications_dir).map_err(|e| DomainError::PortFailure {
            port: "Installer",
            message: format!("cannot create target applications dir: {e}"),
        })?;

        let destination = self.applications_dir.join(&app_bundle_name);

        // Remove old version if present
        if destination.exists() {
            fs::remove_dir_all(&destination).map_err(|e| DomainError::PortFailure {
                port: "Installer",
                message: format!("cannot remove existing app bundle: {e}"),
            })?;
        }

        // Recursive copy
        copy_dir_all(&source_app, &destination).map_err(|e| DomainError::PortFailure {
            port: "Installer",
            message: format!("cannot copy app bundle to target directory: {e}"),
        })?;

        Ok(destination)
    }

    /// Launch installed application using system `open` command.
    pub fn launch_installed_app(&self, app_path: &Path) -> Result<(), DomainError> {
        #[cfg(target_os = "macos")]
        {
            let status = std::process::Command::new("open")
                .arg(app_path)
                .status()
                .map_err(|e| DomainError::PortFailure {
                    port: "Launcher",
                    message: format!("cannot execute open command: {e}"),
                })?;

            if !status.success() {
                return Err(DomainError::PortFailure {
                    port: "Launcher",
                    message: format!("failed to launch app at {}", app_path.display()),
                });
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = app_path;
        }

        Ok(())
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct MockDiskImage {
        mount_point: PathBuf,
        detached: Arc<AtomicBool>,
    }

    impl DiskImagePort for MockDiskImage {
        fn attach(&self, _dmg_path: &Path) -> Result<PathBuf, DomainError> {
            Ok(self.mount_point.clone())
        }

        fn detach(&self, _mount_point: &Path) -> Result<(), DomainError> {
            self.detached.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn test_headless_dmg_install_flow() {
        let temp = tempfile::tempdir().unwrap();
        let dmg_file = temp.path().join("PhotoCraft-v2.4.0.dmg");
        fs::write(&dmg_file, b"fake dmg content").unwrap();

        let fake_mount = temp.path().join("fake_volume");
        let fake_app = fake_mount.join("PhotoCraft.app/Contents");
        fs::create_dir_all(&fake_app).unwrap();
        fs::write(fake_app.join("Info.plist"), b"<xml>version</xml>").unwrap();

        let apps_dir = temp.path().join("Applications");
        let detached_flag = Arc::new(AtomicBool::new(false));

        let disk_port = MockDiskImage {
            mount_point: fake_mount,
            detached: detached_flag.clone(),
        };

        let use_case = InstallMacOsAppUseCase::new(disk_port, apps_dir.clone());
        let installed = use_case
            .install_from_dmg(&dmg_file, "PhotoCraft")
            .expect("install successful");

        assert_eq!(installed, apps_dir.join("PhotoCraft.app"));
        assert!(installed.exists());
        assert!(installed.join("Contents/Info.plist").exists());
        assert!(
            detached_flag.load(Ordering::SeqCst),
            "disk image was detached"
        );
    }
}
