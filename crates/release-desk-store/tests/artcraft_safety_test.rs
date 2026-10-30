//! Generic managed-library safety checks for all projects.
use release_desk_core::storage::RelativeManagedPath;
use release_desk_store::boundary::{BoundaryError, ManagedLibrary};

#[test]
fn rejects_path_traversal() {
    assert!(RelativeManagedPath::new("../outside.txt").is_err());
    assert!(RelativeManagedPath::new("/absolute/path").is_err());
}

#[cfg(unix)]
#[test]
fn rejects_symlinks_to_external_and_missing_directories() {
    let library = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let lib = ManagedLibrary::new(library.path()).unwrap();
    for (name, target) in [
        ("existing", outside.path().to_path_buf()),
        ("missing", outside.path().join("missing")),
    ] {
        std::os::unix::fs::symlink(target, library.path().join(name)).unwrap();
        let relative = RelativeManagedPath::new(&format!("{name}/new.txt")).unwrap();
        assert!(matches!(
            lib.write_file(&relative, b"content"),
            Err(BoundaryError::EscapeAttempt(_))
        ));
    }
    assert!(!outside.path().join("new.txt").exists());
}
