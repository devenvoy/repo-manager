//! Managed library path resolver and filesystem boundary guard.

use release_desk_core::storage::RelativeManagedPath;
use std::{
    fmt, fs, io,
    path::{Component, Path, PathBuf},
};

#[derive(Debug)]
pub enum BoundaryError {
    EscapeAttempt(String),
    InvalidRoot(String),
    Io(io::Error),
}

impl fmt::Display for BoundaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EscapeAttempt(message) => write!(f, "path escape attempt rejected: {message}"),
            Self::InvalidRoot(message) => write!(f, "invalid managed library root: {message}"),
            Self::Io(error) => write!(f, "I/O error in managed library: {error}"),
        }
    }
}
impl std::error::Error for BoundaryError {}
impl From<io::Error> for BoundaryError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Writes are restricted to the configured library for every project.
#[derive(Clone, Debug)]
pub struct ManagedLibrary {
    root: PathBuf,
}

impl ManagedLibrary {
    pub fn new(root_path: impl AsRef<Path>) -> Result<Self, BoundaryError> {
        let root = root_path.as_ref();
        if !root.is_absolute()
            || root
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(BoundaryError::InvalidRoot(
                "root must be absolute and contain no parent traversal".into(),
            ));
        }
        let mut ancestor = root;
        while !ancestor.exists() {
            ancestor = ancestor
                .parent()
                .ok_or_else(|| BoundaryError::InvalidRoot("no existing root ancestor".into()))?;
        }
        let canonical = ancestor.canonicalize()?.join(
            root.strip_prefix(ancestor)
                .map_err(|_| BoundaryError::InvalidRoot("cannot resolve root".into()))?,
        );
        Ok(Self { root: canonical })
    }

    pub fn root_path(&self) -> &Path {
        &self.root
    }

    pub fn resolve_path(&self, relative: &RelativeManagedPath) -> Result<PathBuf, BoundaryError> {
        let mut full_path = self.root.clone();
        for part in Path::new(relative.as_str()).components() {
            match part {
                Component::Normal(name) => full_path.push(name),
                Component::CurDir => continue,
                _ => {
                    return Err(BoundaryError::EscapeAttempt(
                        "invalid relative component".into(),
                    ));
                }
            }
            match fs::symlink_metadata(&full_path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(BoundaryError::EscapeAttempt(
                        "managed paths cannot traverse symbolic links".into(),
                    ));
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(full_path)
    }

    pub fn ensure_parent_dir(
        &self,
        relative: &RelativeManagedPath,
    ) -> Result<PathBuf, BoundaryError> {
        let path = self.resolve_path(relative)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        self.resolve_path(relative)
    }
    pub fn write_file(
        &self,
        relative: &RelativeManagedPath,
        content: &[u8],
    ) -> Result<PathBuf, BoundaryError> {
        let path = self.ensure_parent_dir(relative)?;
        let parent = path.parent().unwrap_or(&self.root);
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        use std::io::Write;
        temp.write_all(content)?;
        temp.flush()?;
        self.resolve_path(relative)?;
        temp.persist(&path).map_err(|error| error.error)?;
        Ok(path)
    }
    pub fn read_file(&self, relative: &RelativeManagedPath) -> Result<Vec<u8>, BoundaryError> {
        Ok(fs::read(self.resolve_path(relative)?)?)
    }
    pub fn delete_file(&self, relative: &RelativeManagedPath) -> Result<(), BoundaryError> {
        let path = self.resolve_path(relative)?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
