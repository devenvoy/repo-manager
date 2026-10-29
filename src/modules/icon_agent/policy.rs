//! Shared icon candidate policy for local discovery and Kev input.

use super::scanner::ImageCandidate;
use std::collections::HashSet;

/// Keep valid candidates, collapsing alternate exports of the same artwork.
/// A full SVG is the canonical asset when it accompanies raster sizes or a
/// `-small` SVG in the same directory. Different artwork remains available.
pub fn preferred_candidates(candidates: &[ImageCandidate]) -> Vec<ImageCandidate> {
    let full_svg_families: HashSet<String> = candidates
        .iter()
        .filter(|candidate| candidate.is_valid_icon && candidate.extension == "svg")
        .filter(|candidate| !is_compact_variant(&candidate.relative_path))
        .map(asset_family)
        .collect();

    let mut preferred: Vec<_> = candidates
        .iter()
        .filter(|candidate| {
            candidate.is_valid_icon
                && (candidate.extension == "svg" && !is_compact_variant(&candidate.relative_path)
                    || !full_svg_families.contains(&asset_family(candidate)))
        })
        .cloned()
        .collect();
    preferred.sort_by(|a, b| {
        b.relevance_score
            .cmp(&a.relevance_score)
            .then_with(|| a.relative_path.cmp(&b.relative_path))
    });
    preferred
}

pub fn best_local_candidate(candidates: &[ImageCandidate]) -> Option<ImageCandidate> {
    preferred_candidates(candidates).into_iter().next()
}

fn is_compact_variant(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(stem, _)| stem.ends_with("-small") || stem.ends_with("_small"))
}

fn asset_family(candidate: &ImageCandidate) -> String {
    let (stem, _) = candidate
        .relative_path
        .rsplit_once('.')
        .unwrap_or((&candidate.relative_path, ""));
    let mut stem = stem.to_ascii_lowercase();
    if stem.ends_with("-small") || stem.ends_with("_small") {
        stem.truncate(stem.len() - 6);
    }
    if let Some((base, size)) = stem.rsplit_once('-')
        && size
            .parse::<u32>()
            .is_ok_and(|size| size >= 32 && size == candidate.resolution)
    {
        stem = base.to_string();
    }
    for suffix in ["-macos", "-windows", "-linux"] {
        if stem.ends_with(suffix) {
            stem.truncate(stem.len() - suffix.len());
        }
    }
    stem
}
