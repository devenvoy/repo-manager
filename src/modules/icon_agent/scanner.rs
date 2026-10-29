//! Repository image scanner — Tool 1 of the icon agent pipeline.
//!
//! Walks a repository tree and collects candidate image files with metadata.
//! Feeds the AI selector with structured context about what images exist.

use crate::modules::artwork::{is_dir_blocked, is_valid_icon_file, read_icon_resolution};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::Path;

/// A candidate image discovered in a repository.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageCandidate {
    /// Path relative to repo root.
    pub relative_path: String,
    /// File name only.
    pub file_name: String,
    /// Parent directory name.
    pub parent_dir: String,
    /// File size in bytes.
    pub file_size: u64,
    /// Image resolution (min dimension, 0 if unknown).
    pub resolution: u32,
    /// File extension (lowercase).
    pub extension: String,
    /// Whether the file passes basic icon validation.
    pub is_valid_icon: bool,
    /// Heuristic relevance score (higher = more likely to be an app icon).
    pub relevance_score: i32,
}

/// Image file extensions to look for.
const IMAGE_EXTENSIONS: &[&str] = &["png", "svg", "ico", "icns", "webp", "jpg", "jpeg"];

/// Scan a repository directory for candidate images.
///
/// Returns a sorted list (highest relevance first) of image candidates
/// suitable for feeding to the AI selector.
pub fn scan_repo(repo_dir: &Path, repo_name: &str) -> Vec<ImageCandidate> {
    if !repo_dir.is_dir() {
        return Vec::new();
    }

    let root = repo_dir
        .canonicalize()
        .ok()
        .unwrap_or_else(|| repo_dir.to_path_buf());
    let clean_name = repo_name.to_lowercase();

    let mut candidates = Vec::new();
    let mut queue = VecDeque::from([(root.clone(), 0u8)]);

    while let Some((dir, depth)) = queue.pop_front() {
        // Cap depth to avoid huge repos
        if candidates.len() >= 500 || depth > 6 {
            break;
        }

        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
                continue;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            if path.is_dir() {
                if !is_dir_blocked(&name) {
                    queue.push_back((path, depth + 1));
                }
                continue;
            }

            if !path.is_file() {
                continue;
            }

            // Check extension
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();

            if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }

            let Ok(rel) = path.strip_prefix(&root) else {
                continue;
            };
            let relative_path = rel.to_string_lossy().replace('\\', "/");

            let parent_dir = rel
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            let file_size = path.metadata().map(|m| m.len()).unwrap_or(0);
            let resolution = read_icon_resolution(&path);
            let is_valid = is_valid_icon_file(&path);

            let relevance = compute_relevance(
                &name,
                &parent_dir,
                &ext,
                &clean_name,
                resolution,
                is_valid,
                depth,
            );

            candidates.push(ImageCandidate {
                relative_path,
                file_name: name,
                parent_dir,
                file_size,
                resolution,
                extension: ext,
                is_valid_icon: is_valid,
                relevance_score: relevance,
            });
        }
    }

    // Sort by relevance descending
    candidates.sort_by_key(|a| std::cmp::Reverse(a.relevance_score));
    candidates
}

/// Compute a simple relevance score for a candidate.
/// This is NOT the old heuristic scoring engine — it's a lightweight signal
/// to pre-rank candidates before feeding them to the AI selector.
fn compute_relevance(
    file_name: &str,
    parent_dir: &str,
    ext: &str,
    repo_name: &str,
    resolution: u32,
    is_valid: bool,
    depth: u8,
) -> i32 {
    let mut score: i32 = 0;
    let lower_name = file_name.to_ascii_lowercase();

    // Valid icon bonus
    if is_valid {
        score += 100;
    }

    // Extension quality
    match ext {
        "svg" => score += 30,
        "png" => score += 25,
        "webp" => score += 15,
        "ico" => score += 12,
        "icns" => score += 10,
        _ => {}
    }

    // Resolution bonus
    if ext != "svg" {
        // SVG's synthetic resolution is not a measure of artwork quality.
        if resolution >= 512 {
            score += 80;
        } else if resolution >= 256 {
            score += 50;
        } else if resolution >= 128 {
            score += 25;
        } else if resolution > 0 && resolution < 48 {
            score -= 50;
        }
    }

    // Name contains repo name (strong signal)
    if lower_name.contains(repo_name) {
        score += 120;
    }

    // Name signals
    if lower_name.contains("icon")
        || lower_name.contains("appicon")
        || lower_name.contains("app-icon")
    {
        score += 60;
    }
    if is_valid && lower_name.contains("logo") && resolution >= 512 {
        score += 60;
    }
    if lower_name.contains("launcher") {
        score += 30;
    }

    // Directory signals
    let lower_dir = parent_dir.to_ascii_lowercase();
    if lower_dir == "app-icon" || lower_dir == "appicon" || lower_dir == "app_icon" {
        score += 80;
    } else if lower_dir == "icons" {
        score += 40;
    } else if lower_dir == "assets" || lower_dir == "resources" {
        score += 10;
    }

    // Penalize banners/screenshots/logos
    if lower_name.contains("banner")
        || lower_name.contains("header")
        || lower_name.contains("screenshot")
        || lower_name.contains("social")
        || lower_name.contains("preview")
    {
        score -= 200;
    }

    // Penalize deep paths
    score -= (depth as i32) * 5;

    score
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_empty_dir() {
        let temp = tempfile::tempdir().unwrap();
        let candidates = scan_repo(temp.path(), "test-repo");
        assert!(candidates.is_empty());
    }

    #[test]
    fn scan_finds_icon_in_well_known_path() {
        let temp = tempfile::tempdir().unwrap();
        let icon_dir = temp.path().join("assets").join("app-icon");
        std::fs::create_dir_all(&icon_dir).unwrap();

        let icon_file = icon_dir.join("myapp-1024.png");
        image::RgbaImage::new(512, 512).save(&icon_file).unwrap();

        // Create a banner (non-square) that should score lower
        let banner_file = temp.path().join("banner.png");
        std::fs::write(&banner_file, b"not a real image").unwrap();

        let candidates = scan_repo(temp.path(), "myapp");
        assert!(!candidates.is_empty());
        assert_eq!(candidates[0].file_name, "myapp-1024.png");
        assert!(candidates[0].relevance_score > 0);
    }

    #[test]
    fn scan_does_not_reselect_previous_cached_icons() {
        let repo = tempfile::tempdir().unwrap();
        let cache = repo.path().join(".release-desk/icons");
        std::fs::create_dir_all(&cache).unwrap();
        image::RgbaImage::new(512, 512)
            .save(cache.join("app-512.png"))
            .unwrap();
        assert!(scan_repo(repo.path(), "app").is_empty());
    }
}
