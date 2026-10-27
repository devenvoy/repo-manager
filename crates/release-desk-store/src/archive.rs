//! Portable tar.gz archive packaging with path traversal protection.
//!
//! Implements `ArchivePort` for creating and restoring full project backups.

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use release_desk_core::error::DomainError;
use release_desk_core::port::ArchivePort;
use std::fs::{self, File};
use std::path::{Component, Path};

/// Tar/Gzip backup archive packager implementing `ArchivePort`.
#[derive(Clone, Copy, Debug, Default)]
pub struct TarGzArchivePort;

impl TarGzArchivePort {
    pub fn new() -> Self {
        Self
    }
}

impl ArchivePort for TarGzArchivePort {
    fn create_archive(&self, source_dir: &Path, output_file: &Path) -> Result<u64, DomainError> {
        if !source_dir.is_dir() {
            return Err(DomainError::PortFailure {
                port: "Archive",
                message: format!("source is not a directory: {}", source_dir.display()),
            });
        }

        if let Some(parent) = output_file.parent() {
            fs::create_dir_all(parent).map_err(|e| DomainError::PortFailure {
                port: "Archive",
                message: format!("cannot create output parent: {e}"),
            })?;
        }

        let out_file = File::create(output_file).map_err(|e| DomainError::PortFailure {
            port: "Archive",
            message: format!("cannot create archive file: {e}"),
        })?;

        let enc = GzEncoder::new(out_file, Compression::default());
        let mut tar = tar::Builder::new(enc);

        tar.append_dir_all(".", source_dir)
            .map_err(|e| DomainError::PortFailure {
                port: "Archive",
                message: format!("cannot archive directory: {e}"),
            })?;

        let enc = tar.into_inner().map_err(|e| DomainError::PortFailure {
            port: "Archive",
            message: format!("tar write error: {e}"),
        })?;

        enc.finish().map_err(|e| DomainError::PortFailure {
            port: "Archive",
            message: format!("compression finish error: {e}"),
        })?;

        let metadata = fs::metadata(output_file).map_err(|e| DomainError::PortFailure {
            port: "Archive",
            message: format!("cannot read archive metadata: {e}"),
        })?;

        Ok(metadata.len())
    }

    fn extract_archive(&self, archive_file: &Path, target_dir: &Path) -> Result<(), DomainError> {
        if !archive_file.is_file() {
            return Err(DomainError::PortFailure {
                port: "Archive",
                message: format!("archive file not found: {}", archive_file.display()),
            });
        }

        fs::create_dir_all(target_dir).map_err(|e| DomainError::PortFailure {
            port: "Archive",
            message: format!("cannot create target extraction directory: {e}"),
        })?;

        let canon_target = target_dir
            .canonicalize()
            .map_err(|e| DomainError::PortFailure {
                port: "Archive",
                message: format!("cannot canonicalize target directory: {e}"),
            })?;

        let file = File::open(archive_file).map_err(|e| DomainError::PortFailure {
            port: "Archive",
            message: format!("cannot open archive file: {e}"),
        })?;

        let dec = GzDecoder::new(file);
        let mut archive = tar::Archive::new(dec);

        for entry_result in archive.entries().map_err(|e| DomainError::PortFailure {
            port: "Archive",
            message: format!("cannot read tar entries: {e}"),
        })? {
            let mut entry = entry_result.map_err(|e| DomainError::PortFailure {
                port: "Archive",
                message: format!("corrupt tar entry: {e}"),
            })?;

            let entry_path = entry.path().map_err(|e| DomainError::PortFailure {
                port: "Archive",
                message: format!("invalid entry path: {e}"),
            })?;

            // Traversal protection check
            for comp in entry_path.components() {
                match comp {
                    Component::Normal(_) | Component::CurDir => {}
                    _ => {
                        return Err(DomainError::PortFailure {
                            port: "Archive",
                            message: format!("unsafe component in archive entry: {:?}", comp),
                        });
                    }
                }
            }

            let destination = canon_target.join(entry_path);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|e| DomainError::PortFailure {
                    port: "Archive",
                    message: format!("cannot create directory: {e}"),
                })?;
            }

            entry
                .unpack(&destination)
                .map_err(|e| DomainError::PortFailure {
                    port: "Archive",
                    message: format!("cannot unpack entry: {e}"),
                })?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_and_extract_roundtrip() {
        let temp_src = tempfile::tempdir().unwrap();
        let file1 = temp_src.path().join("hello.txt");
        fs::write(&file1, b"world").unwrap();

        let sub = temp_src.path().join("subdir");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("nested.txt"), b"nested content").unwrap();

        let temp_dest = tempfile::tempdir().unwrap();
        let archive_path = temp_dest.path().join("backup.tar.gz");

        let archiver = TarGzArchivePort::new();
        let size = archiver
            .create_archive(temp_src.path(), &archive_path)
            .unwrap();
        assert!(size > 0);

        let extract_dir = temp_dest.path().join("extracted");
        archiver
            .extract_archive(&archive_path, &extract_dir)
            .unwrap();

        assert_eq!(
            fs::read_to_string(extract_dir.join("hello.txt")).unwrap(),
            "world"
        );
        assert_eq!(
            fs::read_to_string(extract_dir.join("subdir/nested.txt")).unwrap(),
            "nested content"
        );
    }
}
