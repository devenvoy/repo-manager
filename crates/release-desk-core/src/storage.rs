//! Storage boundaries, path containment, and backup models.

use crate::error::ValidationError;
use serde::{Deserialize, Serialize};
use std::fmt;
/// A strictly validated relative path inside managed storage.
///
/// Guaranteed never to escape its root: no leading slashes, no `..`, no `:` (Windows drives/streams),
/// and normalized to forward slashes.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct RelativeManagedPath(String);

impl RelativeManagedPath {
    pub fn new(raw: &str) -> Result<Self, ValidationError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(ValidationError::Empty("relative managed path"));
        }
        if trimmed.starts_with('/') || trimmed.starts_with('\\') {
            return Err(ValidationError::InvalidPath(
                "must be relative, not absolute",
            ));
        }
        if trimmed.contains(':') || trimmed.contains('\0') {
            return Err(ValidationError::InvalidPath(
                "contains forbidden characters",
            ));
        }
        let mut segments = Vec::new();
        for segment in trimmed.split(['/', '\\']) {
            if segment.is_empty() || segment == "." {
                continue;
            }
            if segment == ".." {
                return Err(ValidationError::InvalidPath(
                    "parent traversal (..) is strictly forbidden",
                ));
            }
            segments.push(segment);
        }
        if segments.is_empty() {
            return Err(ValidationError::InvalidPath("path resolves to empty"));
        }
        Ok(Self(segments.join("/")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RelativeManagedPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// SHA-256 cryptographic checksum.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct Sha256Checksum(String);

impl Sha256Checksum {
    pub fn new(raw: &str) -> Result<Self, ValidationError> {
        let trimmed = raw.trim();
        if trimmed.len() != 64 || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ValidationError::InvalidCharacter(
                "sha256 checksum must be 64 hex digits",
            ));
        }
        Ok(Self(trimmed.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Sha256Checksum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Human-friendly byte size formatting.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct FileSize(u64);

impl FileSize {
    pub const fn from_bytes(bytes: u64) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(self) -> u64 {
        self.0
    }

    pub fn to_human_string(self) -> String {
        if self.0 < 1024 {
            return format!("{} B", self.0);
        }
        let units = ["KB", "MB", "GB", "TB"];
        let mut n = self.0 as f64 / 1024.0;
        let mut i = 0;
        while n >= 1024.0 && i < units.len() - 1 {
            n /= 1024.0;
            i += 1;
        }
        if n >= 100.0 {
            format!("{:.0} {}", n, units[i])
        } else {
            format!("{:.1} {}", n, units[i])
        }
    }
}

impl fmt::Display for FileSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_human_string())
    }
}

/// Unique backup archive identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct BackupId(u128);

impl BackupId {
    pub const fn from_u128(val: u128) -> Self {
        Self(val)
    }

    pub const fn as_u128(self) -> u128 {
        self.0
    }
}

/// Scope of backup creation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupMode {
    #[default]
    Full,
    ConfigOnly,
    ExcludeClones,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_escaping_rejected() {
        assert!(RelativeManagedPath::new("/root/escape").is_err());
        assert!(RelativeManagedPath::new("a/b/../../escape").is_err());
        assert!(RelativeManagedPath::new("C:\\Windows").is_err());
        assert!(RelativeManagedPath::new("valid/path").is_ok());
    }

    #[test]
    fn size_human_formatting() {
        assert_eq!(FileSize::from_bytes(250).to_human_string(), "250 B");
        assert_eq!(FileSize::from_bytes(1024).to_human_string(), "1.0 KB");
        assert_eq!(
            FileSize::from_bytes(10 * 1024 * 1024).to_human_string(),
            "10.0 MB"
        );
    }
}
