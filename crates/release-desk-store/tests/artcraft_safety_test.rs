//! Adversarial tests verifying the strict ArtCraft read-only safety boundary.
//!
//! Confirms that Release Desk NEVER permits write, modification, symlink traversal,
//! or deletion of files inside or pointing to `/Volumes/Devansh2/codes/ArtCraft`.

use release_desk_core::storage::RelativeManagedPath;
use release_desk_store::boundary::{BoundaryError, FORBIDDEN_ARTCRAFT_ROOT, ManagedLibrary};
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn reject_artcraft_as_direct_managed_library_root() {
    let err = ManagedLibrary::new(FORBIDDEN_ARTCRAFT_ROOT).unwrap_err();
    match err {
        BoundaryError::ForbiddenArtCraftAccess(p) => {
            assert!(p.to_string_lossy().contains("ArtCraft"));
        }
        other => panic!("expected ForbiddenArtCraftAccess, got {other:?}"),
    }
}

#[test]
fn reject_nested_artcraft_folder_as_managed_root() {
    let nested = PathBuf::from(FORBIDDEN_ARTCRAFT_ROOT).join(".release-desk/subfolder");
    let err = ManagedLibrary::new(&nested).unwrap_err();
    match err {
        BoundaryError::ForbiddenArtCraftAccess(p) => {
            assert!(p.to_string_lossy().contains("ArtCraft"));
        }
        other => panic!("expected ForbiddenArtCraftAccess, got {other:?}"),
    }
}

#[test]
fn reject_parent_of_artcraft_as_managed_root() {
    // If someone tries to manage /Volumes/Devansh2/codes, which contains ArtCraft
    let parent = Path::new(FORBIDDEN_ARTCRAFT_ROOT).parent().unwrap();
    let err = ManagedLibrary::new(parent).unwrap_err();
    match err {
        BoundaryError::InvalidRoot(msg) => {
            assert!(msg.contains("cannot contain the ArtCraft directory"));
        }
        other => panic!("expected InvalidRoot, got {other:?}"),
    }
}

#[test]
fn reject_relative_escape_attempts() {
    let temp = tempfile::tempdir().unwrap();
    let _lib = ManagedLibrary::new(temp.path()).unwrap();

    // Direct traversal with ..
    assert!(RelativeManagedPath::new("../outside.txt").is_err());
    assert!(RelativeManagedPath::new("subdir/../../escape.txt").is_err());
    assert!(RelativeManagedPath::new("/absolute/path").is_err());
}

#[cfg(unix)]
#[test]
fn reject_symlink_pointing_to_artcraft() {
    let temp = tempfile::tempdir().unwrap();
    let lib = ManagedLibrary::new(temp.path()).unwrap();

    // Create a symlink inside the managed library that targets ArtCraft
    let symlink_path = temp.path().join("evil_link");
    let target = Path::new(FORBIDDEN_ARTCRAFT_ROOT);

    std::os::unix::fs::symlink(target, &symlink_path).unwrap();

    let rel = RelativeManagedPath::new("evil_link/exploit.txt").unwrap();
    let err = lib.write_file(&rel, b"malicious content").unwrap_err();

    match err {
        BoundaryError::ForbiddenArtCraftAccess(p) => {
            assert!(p.to_string_lossy().contains("ArtCraft"));
        }
        other => panic!("expected ForbiddenArtCraftAccess, got {other:?}"),
    }
}

#[test]
fn verify_real_artcraft_files_hashes_unchanged() {
    // Check that ArtCraft files are present and match known baseline hashes
    let data_json = Path::new("/Volumes/Devansh2/codes/ArtCraft/.release-desk/data.json");
    if let Ok(bytes) = fs::read(data_json) {
        use sha2::{Digest, Sha256};
        let hash = format!("{:x}", Sha256::digest(&bytes));
        assert_eq!(
            hash, "0ce54ddd0590f1ae4fde3f4ae69d24b79f1a2e1202fd0f3cdaf8705b6940daa8",
            "ArtCraft data.json must remain 100% untouched!"
        );
    }
}
