//! Repository artwork utilities: image validation, dimension sniffing, and caching.
//!
//! The complex heuristic scoring engine has been removed. Icon resolution is now
//! delegated to the `icon_agent` module which uses an AI-powered pipeline for
//! accurate icon selection and persistent caching in `~/.release-desk/`.

use std::path::{Path, PathBuf};

/* ───────────────────── Image Validation ───────────────────── */

/// Safely slice up to `max_bytes` without panicking on multi-byte UTF-8 boundaries.
pub fn safe_slice(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        s
    } else {
        let mut end = max_bytes;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

/// Sniffs the resolution of an icon file (returns min dimension in pixels, or 2048 for SVG).
pub fn read_icon_resolution(path: &Path) -> u32 {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext.eq_ignore_ascii_case("svg") {
        return 2048;
    }
    read_icon_dimensions(path)
        .map(|dim| dim.w.min(dim.h))
        .unwrap_or(0)
}

/// Validates whether a file is an authentic, non-banner square icon (aspect ratio ≤ 1.25, min dim ≥ 32px).
pub fn is_valid_icon_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let file_name = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
    let lower_name = file_name.to_ascii_lowercase();

    // Reject obvious non-icon files by name
    if lower_name.contains("banner")
        || lower_name.contains("header")
        || lower_name.contains("wordmark")
        || lower_name.contains("horizontal")
        || lower_name.contains("screenshot")
        || lower_name.contains("preview")
        || lower_name.contains("social")
        || lower_name.contains("diagram")
    {
        return false;
    }

    if let Some(dim) = read_icon_dimensions(path) {
        let lo = dim.w.min(dim.h) as f32;
        let hi = dim.w.max(dim.h) as f32;
        if lo <= 0.0 {
            return false;
        }
        // Strict square aspect-ratio: max/min <= 1.25 (disqualifies wide banners!)
        if hi / lo > 1.25 {
            return false;
        }
        // Reject tiny low-res images (< 32px)
        if lo < 32.0 {
            return false;
        }
        return true;
    }

    // ICNS is not supported by the image crate, and SVG dimensions may be
    // omitted. Other raster formats must have decodable dimensions.
    if matches!(ext.to_ascii_lowercase().as_str(), "svg" | "icns")
        && (lower_name.contains("icon") || lower_name.contains("app"))
        && !lower_name.contains("logo")
        && !lower_name.contains("brand")
    {
        return true;
    }

    false
}

/// Returns true if a directory name should be skipped during icon scanning.
pub fn is_dir_blocked(dir: &str) -> bool {
    matches!(
        dir.to_ascii_lowercase().as_str(),
        ".git"
            | ".release-desk"
            | ".release_desk"
            | "node_modules"
            | "vendor"
            | "third_party"
            | "thirdparty"
            | "bower_components"
            | "pods"
            | "venv"
            | ".venv"
            | "__pycache__"
            | "site-packages"
            | "dist"
            | "out"
            | "target"
            | ".next"
            | ".nuxt"
            | "coverage"
            | "test"
            | "tests"
            | "__tests__"
            | "spec"
            | "e2e"
            | "fixtures"
            | "fixture"
            | "testdata"
            | "snapshots"
            | "__snapshots__"
            | "examples"
            | "example"
            | "samples"
            | "sample"
            | "demo"
            | "demos"
            | "screenshots"
            | "benchmarks"
    )
}

/// Discover the preferred artwork from a cloned repository and cache it.
pub fn find_local_repo_icon(repo_dir: &Path, repo_name: &str) -> Option<PathBuf> {
    let found = discover_local_icon(repo_dir, repo_name)?;
    persist_discovered_icon(&found, repo_dir, "local", "local", repo_name);
    Some(found)
}

fn persist_discovered_icon(
    found: &Path,
    repo_dir: &Path,
    provider: &str,
    owner: &str,
    repo_name: &str,
) {
    let project_dir = repo_dir.parent();
    let cached =
        crate::modules::icon_agent::cache_icon_in(found, project_dir, provider, owner, repo_name)
            .unwrap_or_else(|| found.to_path_buf());
    crate::modules::icon_agent::db::record_icon_for(
        project_dir,
        provider,
        owner,
        repo_name,
        &found.to_string_lossy(),
        &cached,
        read_icon_resolution(found),
    );
}

/// Respect an explicit Store icon, then use the shared repository scan policy.
fn discover_local_icon(repo_dir: &Path, repo_name: &str) -> Option<PathBuf> {
    let root = repo_dir.canonicalize().ok()?;

    // A repository's explicit Store artwork choice takes precedence.
    for manifest_name in ["release-desk-store.json", "store.json"] {
        let manifest_file = root.join(manifest_name);
        if let Ok(text) = std::fs::read_to_string(manifest_file)
            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
            && let Some(icon_rel) = json.get("icon").and_then(|value| value.as_str())
        {
            let candidate = root.join(icon_rel.trim_start_matches("./"));
            if let Ok(candidate) = candidate.canonicalize()
                && candidate.starts_with(&root)
                && is_valid_icon_file(&candidate)
            {
                return Some(candidate);
            }
        }
    }

    let candidates = crate::modules::icon_agent::scan_repo(&root, repo_name);
    let selected = crate::modules::icon_agent::policy::best_local_candidate(&candidates)?;
    root.join(selected.relative_path)
        .canonicalize()
        .ok()
        .filter(|path| path.starts_with(&root))
}

/* ───────────────────── Header Sniffing (≤ 8 KB) ───────────────────── */

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Dim {
    pub w: u32,
    pub h: u32,
}

fn read_icon_dimensions(path: &Path) -> Option<Dim> {
    let ext = path.extension()?.to_str()?;
    if ext.eq_ignore_ascii_case("svg") {
        use std::io::Read;
        let mut file = std::fs::File::open(path).ok()?;
        let mut bytes = Vec::new();
        file.by_ref().take(8192).read_to_end(&mut bytes).ok()?;
        return parse_svg_dimensions(&String::from_utf8_lossy(&bytes));
    }
    let (w, h) = image::image_dimensions(path).ok()?;
    Some(Dim { w, h })
}

pub fn parse_svg_dimensions(text: &str) -> Option<Dim> {
    let lower = text.to_ascii_lowercase();

    // 1. Check viewBox="x y w h" or viewBox="x, y, w, h"
    if let Some(pos) = lower.find("viewbox") {
        if let Some(eq_rel) = text[pos..].find('=') {
            let val_slice = text[pos + eq_rel + 1..].trim_start();
            if let Some(quote) = val_slice.chars().next() {
                if quote == '"' || quote == '\'' {
                    let inner = &val_slice[1..];
                    if let Some(close) = inner.find(quote) {
                        let raw = &inner[..close];
                        let nums: Vec<f32> = raw
                            .split(|c: char| c == ',' || c.is_whitespace())
                            .filter(|s| !s.is_empty())
                            .filter_map(|s| s.parse::<f32>().ok())
                            .collect();
                        if nums.len() == 4 && nums[2] > 0.0 && nums[3] > 0.0 {
                            return Some(Dim {
                                w: nums[2].round() as u32,
                                h: nums[3].round() as u32,
                            });
                        }
                    }
                }
            }
        }
    }

    // 2. Check <svg ... width="..." height="...">
    if let Some(svg_start) = lower.find("<svg") {
        let tag = &text[svg_start..];
        let tag_end = tag.find('>').unwrap_or(tag.len().min(2000));
        let tag_content = &tag[..tag_end];
        let parse_attr = |attr: &str| -> Option<f32> {
            let lower_tag = tag_content.to_ascii_lowercase();
            let pos = lower_tag.find(attr)?;
            let after = &tag_content[pos + attr.len()..];
            let eq_rel = after.find('=')?;
            let val_slice = after[eq_rel + 1..].trim_start();
            let quote = val_slice.chars().next()?;
            if quote == '"' || quote == '\'' {
                let inner = &val_slice[1..];
                let close = inner.find(quote)?;
                let raw_val = inner[..close]
                    .trim()
                    .trim_end_matches("px")
                    .trim_end_matches("pt")
                    .trim_end_matches("em");
                raw_val.parse::<f32>().ok()
            } else {
                None
            }
        };

        if let (Some(w), Some(h)) = (parse_attr("width"), parse_attr("height")) {
            if w > 0.0 && h > 0.0 {
                return Some(Dim {
                    w: w.round() as u32,
                    h: h.round() as u32,
                });
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_svg_dimension_parsing() {
        let svg_square = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512"><path d="..."/></svg>"#;
        assert_eq!(
            parse_svg_dimensions(svg_square),
            Some(Dim { w: 512, h: 512 })
        );

        let svg_banner = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0, 0, 1200, 300"><text>ARTCRAFT</text></svg>"#;
        assert_eq!(
            parse_svg_dimensions(svg_banner),
            Some(Dim { w: 1200, h: 300 })
        );

        let svg_attrs = r#"<svg width="800px" height="150px"><rect/></svg>"#;
        assert_eq!(
            parse_svg_dimensions(svg_attrs),
            Some(Dim { w: 800, h: 150 })
        );

        let svg_pt = r#"<svg width="64" height="64"><rect/></svg>"#;
        assert_eq!(parse_svg_dimensions(svg_pt), Some(Dim { w: 64, h: 64 }));
    }

    #[test]
    fn test_is_valid_icon_file_rejects_banners() {
        let temp = tempfile::tempdir().unwrap();
        let banner_svg = temp.path().join("artcraft-banner.svg");
        std::fs::write(
            &banner_svg,
            r#"<svg viewBox="0 0 1200 300"><text>ARTCRAFT</text></svg>"#,
        )
        .unwrap();
        assert!(!is_valid_icon_file(&banner_svg));

        let square_svg = temp.path().join("photocraft-icon.svg");
        std::fs::write(
            &square_svg,
            r#"<svg viewBox="0 0 512 512"><path d="..."/></svg>"#,
        )
        .unwrap();
        assert!(is_valid_icon_file(&square_svg));

        let named_banner = temp.path().join("hero_banner.png");
        std::fs::write(&named_banner, b"fake content").unwrap();
        assert!(!is_valid_icon_file(&named_banner));
    }
}
