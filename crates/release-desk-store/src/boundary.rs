//! Managed library path resolver and ArtCraft write boundary guard.
//!
//! Enforces the immutable boundary rule: Release Desk NEVER writes, modifies,
//! renames, or deletes files under `/Volumes/Devansh2/codes/ArtCraft/`.
//! All managed writes are confined to the user-approved managed library root.

use release_desk_core::storage::RelativeManagedPath;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// The immutable legacy root directory that Release Desk is forbidden to modify.
pub const FORBIDDEN_ARTCRAFT_ROOT: &str = "/Volumes/Devansh2/codes/ArtCraft";

/// Boundary violations and security errors.
#[derive(Debug)]
pub enum BoundaryError {
    ForbiddenArtCraftAccess(PathBuf),
    EscapeAttempt(String),
    InvalidRoot(String),
    Io(io::Error),
}

impl fmt::Display for BoundaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForbiddenArtCraftAccess(path) => {
                write!(
                    f,
                    "SAFETY VIOLATION: write attempted to forbidden ArtCraft path: {}",
                    path.display()
                )
            }
            Self::EscapeAttempt(msg) => write!(f, "path escape attempt rejected: {msg}"),
            Self::InvalidRoot(msg) => write!(f, "invalid managed library root: {msg}"),
            Self::Io(err) => write!(f, "I/O error in managed library: {err}"),
        }
    }
}

impl std::error::Error for BoundaryError {}

impl From<io::Error> for BoundaryError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

/// A validated managed library directory capability.
///
/// Any filesystem mutation must pass through this struct to guarantee that
/// writes remain strictly within the designated library and never touch ArtCraft.
#[derive(Clone, Debug)]
pub struct ManagedLibrary {
    root: PathBuf,
    forbidden_root: PathBuf,
}

impl ManagedLibrary {
    /// Create a managed library rooted at `root_path`.
    pub fn new(root_path: impl AsRef<Path>) -> Result<Self, BoundaryError> {
        let forbidden = PathBuf::from(FORBIDDEN_ARTCRAFT_ROOT);
        let forbidden_canonical = forbidden.canonicalize().unwrap_or(forbidden.clone());

        let raw_root = root_path.as_ref();
        if !raw_root.is_absolute() {
            return Err(BoundaryError::InvalidRoot(
                "managed library path must be absolute".into(),
            ));
        }

        // Check if raw_root or its existing ancestors overlap with the forbidden path
        if raw_root == forbidden || raw_root == forbidden_canonical {
            return Err(BoundaryError::ForbiddenArtCraftAccess(
                raw_root.to_path_buf(),
            ));
        }

        if raw_root.starts_with(&forbidden) || raw_root.starts_with(&forbidden_canonical) {
            return Err(BoundaryError::ForbiddenArtCraftAccess(
                raw_root.to_path_buf(),
            ));
        }

        if forbidden.starts_with(raw_root) || forbidden_canonical.starts_with(raw_root) {
            return Err(BoundaryError::InvalidRoot(
                "managed library root cannot contain the ArtCraft directory".into(),
            ));
        }

        // Canonicalize root if it exists, or canonicalize the closest existing ancestor
        let canonical_root = if raw_root.exists() {
            raw_root.canonicalize()?
        } else {
            // Find existing ancestor
            let mut curr = raw_root.to_path_buf();
            while !curr.exists() {
                if let Some(parent) = curr.parent() {
                    curr = parent.to_path_buf();
                } else {
                    break;
                }
            }
            if curr.exists() {
                let ancestor_canon = curr.canonicalize()?;
                if ancestor_canon == forbidden_canonical
                    || ancestor_canon.starts_with(&forbidden_canonical)
                {
                    return Err(BoundaryError::ForbiddenArtCraftAccess(
                        raw_root.to_path_buf(),
                    ));
                }
            }
            raw_root.to_path_buf()
        };

        if canonical_root == forbidden_canonical || canonical_root.starts_with(&forbidden_canonical)
        {
            return Err(BoundaryError::ForbiddenArtCraftAccess(canonical_root));
        }

        Ok(Self {
            root: canonical_root,
            forbidden_root: forbidden_canonical,
        })
    }

    /// The canonical root directory of the managed library.
    pub fn root_path(&self) -> &Path {
        &self.root
    }

    /// Resolve a relative managed path to an absolute path, verifying boundary safety.
    pub fn resolve_path(&self, relative: &RelativeManagedPath) -> Result<PathBuf, BoundaryError> {
        let rel_path = Path::new(relative.as_str());

        // Validate components: no prefixes, no '..' traversal, no root components
        for component in rel_path.components() {
            match component {
                Component::Normal(_) => {}
                Component::CurDir => {}
                _ => {
                    return Err(BoundaryError::EscapeAttempt(format!(
                        "forbidden component in relative path: {:?}",
                        component
                    )));
                }
            }
        }

        let full_path = self.root.join(rel_path);

        // Security check: if the path or any existing prefix is a symlink, resolve it and check forbidden
        let mut check_ancestor = full_path.clone();
        while !check_ancestor.exists() {
            if let Some(parent) = check_ancestor.parent() {
                check_ancestor = parent.to_path_buf();
            } else {
                break;
            }
        }

        if check_ancestor.exists() {
            let canon = check_ancestor.canonicalize()?;
            if canon == self.forbidden_root || canon.starts_with(&self.forbidden_root) {
                return Err(BoundaryError::ForbiddenArtCraftAccess(canon));
            }
        }

        Ok(full_path)
    }

    /// Ensure that the parent directory for a managed relative path exists under the library.
    pub fn ensure_parent_dir(
        &self,
        relative: &RelativeManagedPath,
    ) -> Result<PathBuf, BoundaryError> {
        let full_path = self.resolve_path(relative)?;
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(full_path)
    }

    /// Write bytes into a relative managed file path with atomic staging.
    pub fn write_file(
        &self,
        relative: &RelativeManagedPath,
        content: &[u8],
    ) -> Result<PathBuf, BoundaryError> {
        let full_path = self.ensure_parent_dir(relative)?;

        // Verify again before write
        if full_path.starts_with(&self.forbidden_root) {
            return Err(BoundaryError::ForbiddenArtCraftAccess(full_path));
        }

        // Write via tempfile in same directory and atomic rename
        let parent = full_path.parent().unwrap_or(&self.root);
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        use std::io::Write;
        temp.write_all(content)?;
        temp.flush()?;
        temp.persist(&full_path).map_err(|e| e.error)?;

        Ok(full_path)
    }

    /// Read bytes from a managed relative file.
    pub fn read_file(&self, relative: &RelativeManagedPath) -> Result<Vec<u8>, BoundaryError> {
        let full_path = self.resolve_path(relative)?;
        Ok(fs::read(full_path)?)
    }

    /// Delete a file inside the managed library.
    pub fn delete_file(&self, relative: &RelativeManagedPath) -> Result<(), BoundaryError> {
        let full_path = self.resolve_path(relative)?;
        if full_path.exists() {
            // Assert not forbidden
            let canon = full_path.canonicalize()?;
            if canon == self.forbidden_root || canon.starts_with(&self.forbidden_root) {
                return Err(BoundaryError::ForbiddenArtCraftAccess(canon));
            }
            fs::remove_file(full_path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_artcraft_as_root() {
        let err = ManagedLibrary::new(FORBIDDEN_ARTCRAFT_ROOT).unwrap_err();
        match err {
            BoundaryError::ForbiddenArtCraftAccess(_) => {}
            other => panic!("expected ForbiddenArtCraftAccess, got {other:?}"),
        }
    }

    #[test]
    fn rejects_nested_path_inside_artcraft() {
        let path = format!("{FORBIDDEN_ARTCRAFT_ROOT}/nested/store");
        let err = ManagedLibrary::new(path).unwrap_err();
        match err {
            BoundaryError::ForbiddenArtCraftAccess(_) => {}
            other => panic!("expected ForbiddenArtCraftAccess, got {other:?}"),
        }
    }

    #[test]
    fn accepts_clean_temp_dir() {
        let temp = tempfile::tempdir().unwrap();
        let lib = ManagedLibrary::new(temp.path()).unwrap();
        assert_eq!(lib.root_path(), temp.path().canonicalize().unwrap());
    }

    #[test]
    fn writes_and_reads_safe_relative_path() {
        let temp = tempfile::tempdir().unwrap();
        let lib = ManagedLibrary::new(temp.path()).unwrap();

        let rel = RelativeManagedPath::new("installers/test.dmg").unwrap();
        lib.write_file(&rel, b"sample bytes").unwrap();

        let read = lib.read_file(&rel).unwrap();
        assert_eq!(read, b"sample bytes");

        lib.delete_file(&rel).unwrap();
        assert!(!lib.resolve_path(&rel).unwrap().exists());
    }
}
