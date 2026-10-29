//! Icon caching — Tool 3 of the icon agent pipeline.
//!
//! Copies confirmed icon files to the hidden `.release-desk/icons/` folder.
//! Uses the project's folder if available (`<project_dir>/.release-desk/icons/`),
//! or defaults to `~/.release-desk/icons/`.
//!
//! Note: Only image files are copied to disk; image data is never stored in the database.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Get or create the hidden `.release-desk/` directory for a project or home.
pub fn release_desk_dir(project_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = project_dir {
        if !dir.is_dir() {
            return None;
        }
        let desk = dir.join(".release-desk");
        if std::fs::symlink_metadata(&desk).is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return None;
        }
        std::fs::create_dir_all(&desk).ok()?;
        return Some(desk);
    }
    if let Ok(home) = std::env::var("HOME") {
        let desk = PathBuf::from(home).join(".release-desk");
        if std::fs::create_dir_all(&desk).is_ok() && desk.is_dir() {
            let probe = desk.join(".probe");
            if std::fs::write(&probe, b"1").is_ok() {
                let _ = std::fs::remove_file(&probe);
                return Some(desk);
            }
        }
    }
    let fallback = std::env::temp_dir().join("release-desk-icons");
    let _ = std::fs::create_dir_all(&fallback);
    Some(fallback)
}

/// The directory where icon files are cached.
pub fn icons_dir(project_dir: Option<&Path>) -> Option<PathBuf> {
    let desk = release_desk_dir(project_dir)?;
    let dir = desk.join("icons");
    if std::fs::symlink_metadata(&dir).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return None;
    }
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Cache an icon under a content-specific name so image views see changed artwork.
///
/// Returns the path to the cached file on disk, or None if caching failed.
pub fn cache_icon(
    source_path: &Path,
    provider: &str,
    owner: &str,
    repo_name: &str,
) -> Option<PathBuf> {
    cache_icon_in(source_path, None, provider, owner, repo_name)
}

/// Cache an icon file inside a specific project's `.release-desk/icons/` directory.
pub fn cache_icon_in(
    source_path: &Path,
    project_dir: Option<&Path>,
    provider: &str,
    owner: &str,
    repo_name: &str,
) -> Option<PathBuf> {
    if !source_path.is_file() {
        return None;
    }

    let ext = source_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png");

    let cache_dir = icons_dir(project_dir)?;
    let safe_name = format!(
        "{}_{}_{}",
        sanitize(provider),
        sanitize(owner),
        sanitize(repo_name)
    );
    let bytes = std::fs::read(source_path).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    let target = cache_dir.join(format!("{safe_name}--{:016x}.{ext}", hasher.finish()));
    if std::fs::read(&target).ok().as_deref() != Some(bytes.as_slice()) {
        std::fs::write(&target, bytes).ok()?;
    }
    Some(target)
}

/// Remove a cached icon from disk.
pub fn remove_cached_icon(
    project_dir: Option<&Path>,
    provider: &str,
    owner: &str,
    repo_name: &str,
) -> bool {
    let Some(cache_dir) = icons_dir(project_dir) else {
        return false;
    };

    let safe_name = format!(
        "{}_{}_{}",
        sanitize(provider),
        sanitize(owner),
        sanitize(repo_name)
    );

    if let Ok(entries) = std::fs::read_dir(&cache_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(&format!("{safe_name}--"))
                || ["png", "svg", "webp", "ico", "icns"]
                    .iter()
                    .any(|ext| name == format!("{safe_name}.{ext}"))
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    true
}

/// Purge all cached icons from disk.
pub fn clear_icon_cache(project_dir: Option<&Path>) {
    if let Some(dir) = icons_dir(project_dir) {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Find an already-cached icon by looking in the icons directory on disk.
pub fn find_cached(
    project_dir: Option<&Path>,
    provider: &str,
    owner: &str,
    repo_name: &str,
) -> Option<PathBuf> {
    let cache_dir = icons_dir(project_dir)?;
    let safe_name = format!(
        "{}_{}_{}",
        sanitize(provider),
        sanitize(owner),
        sanitize(repo_name)
    );

    std::fs::read_dir(cache_dir)
        .ok()?
        .flatten()
        .find_map(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            (name.starts_with(&format!("{safe_name}--"))
                || ["png", "svg", "webp", "ico", "icns"]
                    .iter()
                    .any(|ext| name == format!("{safe_name}.{ext}")))
            .then(|| entry.path())
        })
}

/// Sanitize a string for use in file names.
fn sanitize(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_and_find_in_project_dir() {
        let temp = tempfile::tempdir().unwrap();
        let proj_dir = temp.path();

        let source = proj_dir.join("test-icon.png");
        std::fs::write(&source, b"fake png data for testing").unwrap();

        let cached = cache_icon_in(&source, Some(proj_dir), "github", "storytold", "photocraft");
        assert!(cached.is_some());
        let path = cached.unwrap();
        assert!(path.is_file());
        assert!(path.starts_with(proj_dir.join(".release-desk").join("icons")));

        let found = find_cached(Some(proj_dir), "github", "storytold", "photocraft");
        assert_eq!(found, Some(path));
    }

    #[test]
    fn sanitize_edge_cases() {
        assert_eq!(sanitize(""), "");
        assert_eq!(sanitize("a.b.c"), "a_b_c");
        assert_eq!(sanitize("hello world!"), "hello_world_");
    }

    #[test]
    fn changed_artwork_gets_a_new_path_even_at_the_same_size() {
        let project = tempfile::tempdir().unwrap();
        let source = project.path().join("app.svg");
        std::fs::write(&source, b"<svg>first</svg>").unwrap();
        let first = cache_icon_in(&source, Some(project.path()), "github", "owner", "app").unwrap();
        std::fs::write(&source, b"<svg>other</svg>").unwrap();
        let second =
            cache_icon_in(&source, Some(project.path()), "github", "owner", "app").unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read(first).unwrap(), b"<svg>first</svg>");
        assert_eq!(std::fs::read(second).unwrap(), b"<svg>other</svg>");
    }
}
