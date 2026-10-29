//! Integration tests for headless macOS DMG installer engine.

use release_desk_application::use_case::macos_installer::{DiskImagePort, InstallMacOsAppUseCase};
use release_desk_core::error::DomainError;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

struct MockDiskImageAdapter {
    mount_point: PathBuf,
    attached: Arc<AtomicBool>,
    detached: Arc<AtomicBool>,
}

impl DiskImagePort for MockDiskImageAdapter {
    fn attach(&self, _dmg_path: &Path) -> Result<PathBuf, DomainError> {
        self.attached.store(true, Ordering::SeqCst);
        Ok(self.mount_point.clone())
    }

    fn detach(&self, _mount_point: &Path) -> Result<(), DomainError> {
        self.detached.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn test_end_to_end_headless_dmg_install_and_update() {
    let temp = tempfile::tempdir().unwrap();
    let dmg_path = temp.path().join("PhotoCraft-v2.4.0.dmg");
    fs::write(&dmg_path, b"mock dmg bytes").unwrap();

    let fake_mount = temp.path().join("fake_volume");
    let bundle_dir = fake_mount.join("PhotoCraft.app/Contents");
    fs::create_dir_all(&bundle_dir).unwrap();
    fs::write(bundle_dir.join("Info.plist"), b"<plist version=\"2.4.0\"/>").unwrap();
    fs::write(bundle_dir.join("MacOS"), b"executable").unwrap();

    let target_apps_dir = temp.path().join("Applications");
    let attached = Arc::new(AtomicBool::new(false));
    let detached = Arc::new(AtomicBool::new(false));

    let disk_port = MockDiskImageAdapter {
        mount_point: fake_mount.clone(),
        attached: attached.clone(),
        detached: detached.clone(),
    };

    let installer = InstallMacOsAppUseCase::new(disk_port, target_apps_dir.clone());

    // 1. Initial 1-click Install
    let installed_app = installer
        .install_from_dmg(&dmg_path, "PhotoCraft")
        .expect("installation success");

    assert!(attached.load(Ordering::SeqCst), "disk image was attached");
    assert!(detached.load(Ordering::SeqCst), "disk image was detached");
    assert_eq!(installed_app, target_apps_dir.join("PhotoCraft.app"));
    assert!(installed_app.join("Contents/Info.plist").exists());

    // 2. Perform Update (Overwrites existing app cleanly)
    fs::write(bundle_dir.join("Info.plist"), b"<plist version=\"2.4.1\"/>").unwrap();

    let updated_app = installer
        .install_from_dmg(&dmg_path, "PhotoCraft")
        .expect("update success");

    let updated_plist = fs::read_to_string(updated_app.join("Contents/Info.plist")).unwrap();
    assert!(updated_plist.contains("2.4.1"));
}

#[test]
fn test_install_fails_on_missing_dmg() {
    let temp = tempfile::tempdir().unwrap();
    let non_existent = temp.path().join("Missing.dmg");

    let disk_port = MockDiskImageAdapter {
        mount_point: temp.path().join("mnt"),
        attached: Arc::new(AtomicBool::new(false)),
        detached: Arc::new(AtomicBool::new(false)),
    };

    let installer = InstallMacOsAppUseCase::new(disk_port, temp.path().join("Apps"));
    let err = installer.install_from_dmg(&non_existent, "App");
    assert!(err.is_err());
    assert!(matches!(err.unwrap_err(), DomainError::NotFound { .. }));
}

#[cfg(unix)]
#[test]
fn failed_update_preserves_installed_app() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let dmg_path = temp.path().join("Broken.dmg");
    fs::write(&dmg_path, b"mock dmg").unwrap();
    let mount = temp.path().join("mount");
    let new_bundle = mount.join("Example.app");
    fs::create_dir_all(&new_bundle).unwrap();
    symlink("missing-file", new_bundle.join("broken-link")).unwrap();

    let apps_dir = temp.path().join("Applications");
    let existing = apps_dir.join("Example.app");
    fs::create_dir_all(&existing).unwrap();
    fs::write(existing.join("existing.txt"), b"keep this app").unwrap();
    let disk = MockDiskImageAdapter {
        mount_point: mount,
        attached: Arc::new(AtomicBool::new(false)),
        detached: Arc::new(AtomicBool::new(false)),
    };
    let installer = InstallMacOsAppUseCase::new(disk, apps_dir);
    assert!(installer.install_from_dmg(&dmg_path, "Example").is_err());
    assert_eq!(
        fs::read(existing.join("existing.txt")).unwrap(),
        b"keep this app"
    );
}
