//! Kev Decision Model Tool (`jaredpalmer/kev-0.8b`).
//!
//! An ultra-fast decision model tool implementing the TypeSafe System One
//! API (`POST /v1/systemone`). The remote model returns choice probabilities
//! in a single forward pass.
//! When it is unavailable, scanner relevance supplies a local ranking.
//!
//! Used as the final decision:
//! - Evaluates candidate images
//! - Returns remote probabilities or local ranking weights
//! - Filters to Top 10 candidates
//! - Selects the top candidate.

use super::policy::preferred_candidates;
use super::scanner::ImageCandidate;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A candidate with remote probability or local normalized ranking weight.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScoredCandidate {
    pub candidate: ImageCandidate,
    pub probability: f64,
}

/// Result of running the Kev decision tool on candidate files.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KevDecisionResult {
    /// The selected candidate.
    pub top_choice: ImageCandidate,
    /// Remote probability or local normalized ranking weight of the selection.
    pub top_probability: f64,
    /// Top 10 candidates and their remote probabilities or local weights.
    pub top_10: Vec<ScoredCandidate>,
    /// Full candidate count before filtering.
    pub total_scanned: usize,
    /// Backend used ("systemone_http" or "local_ranker").
    pub backend: String,
}

/// Kev client configuration.
#[derive(Clone, Debug)]
pub struct KevClient {
    /// Base URL for Kev server (default: http://127.0.0.1:8009)
    pub base_url: String,
    /// Model name (default: "jaredpalmer/kev-0.8b")
    pub model_name: String,
    /// Timeout in milliseconds
    pub timeout_ms: u64,
}

impl Default for KevClient {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:8009".to_string(),
            model_name: "jaredpalmer/kev-0.8b".to_string(),
            timeout_ms: 3000,
        }
    }
}

impl KevClient {
    pub fn new(base_url: &str, model_name: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            model_name: model_name.to_string(),
            timeout_ms: 3000,
        }
    }

    /// Run the Kev-0.8b decision tool on image candidates.
    /// Returns the Top 10 candidates and the selected icon.
    pub fn rank_and_filter_top10(
        &self,
        candidates: &[ImageCandidate],
        repo_name: &str,
    ) -> Option<KevDecisionResult> {
        if candidates.is_empty() {
            return None;
        }

        let preferred = preferred_candidates(candidates);
        if preferred.is_empty() {
            return None;
        }

        // 1. Try remote / local Kev System One server (POST /v1/systemone)
        if let Some(mut res) = self.call_system_one(&preferred, repo_name) {
            res.total_scanned = candidates.len();
            return Some(res);
        }

        // 2. Reuse scanner relevance when the model is unavailable.
        let mut res = self.evaluate_locally(&preferred);
        res.total_scanned = candidates.len();
        Some(res)
    }

    /// Query Kev via TypeSafe System One API contract: POST /v1/systemone
    fn call_system_one(
        &self,
        candidates: &[ImageCandidate],
        repo_name: &str,
    ) -> Option<KevDecisionResult> {
        let url = format!("{}/v1/systemone", self.base_url.trim_end_matches('/'));

        let mut criteria_map = HashMap::new();
        for (i, c) in candidates.iter().enumerate().take(30) {
            let desc = format!(
                "Path: '{}', File: '{}', Dir: '{}', Format: {}, Resolution: {}, Size: {} bytes, Valid square icon: {}",
                c.relative_path,
                c.file_name,
                c.parent_dir,
                c.extension,
                if c.extension.eq_ignore_ascii_case("svg") {
                    "vector (no raster resolution)".to_string()
                } else {
                    format!("{}x{} pixels", c.resolution, c.resolution)
                },
                c.file_size,
                c.is_valid_icon
            );
            criteria_map.insert(format!("cand_{i}"), desc);
        }

        let payload = serde_json::json!({
            "model": self.model_name,
            "state": format!(
                "Software repository: '{repo_name}'. Task: Select the primary application icon for this repository. \
                 First match the repository's authentic square application icon or logo. When the same artwork has a full SVG and raster size exports, select the full SVG; raster pixel count or file size does not make it higher quality than its vector source. \
                 Never select a '-small' SVG when the full SVG exists. If no suitable full SVG exists, prefer a high-resolution raster image (at least 512px) over a low-resolution one. \
                 Ignore banners, wide headers, documentation screenshots, and icons of other tools."
            ),
            "questions": {
                "primary_icon": {
                    "type": "choice",
                    "instructions": format!("Which candidate is the authentic square application icon or logo for '{repo_name}'? For matching exports, choose the valid full SVG before any PNG size variant, then the best high-resolution raster if no full SVG exists. Do not use file bytes or SVG viewport dimensions as a cross-format quality score."),
                    "criteria": criteria_map
                }
            }
        });

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_millis(self.timeout_ms))
            .build()
            .ok()?;

        let resp = client.post(&url).json(&payload).send().ok()?;
        if !resp.status().is_success() {
            return None;
        }

        let body: serde_json::Value = resp.json().ok()?;
        let answer = body.get("answers")?.get("primary_icon")?;

        // Extract probabilities map
        let mut scored: Vec<ScoredCandidate> = Vec::new();

        if let Some(probs) = answer.get("probabilities").and_then(|p| p.as_object()) {
            for (key, prob_val) in probs {
                if let Some(idx_str) = key.strip_prefix("cand_") {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        if idx < candidates.len() {
                            let prob = prob_val.as_f64().unwrap_or(0.0);
                            scored.push(ScoredCandidate {
                                candidate: candidates[idx].clone(),
                                probability: prob,
                            });
                        }
                    }
                }
            }
        }

        if scored.is_empty() {
            return None;
        }

        // Sort by probability descending
        scored.sort_by(|a, b| {
            b.probability
                .partial_cmp(&a.probability)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let top_choice = scored[0].candidate.clone();
        let top_probability = scored[0].probability;
        let top_10: Vec<ScoredCandidate> = scored.into_iter().take(10).collect();

        Some(KevDecisionResult {
            top_choice,
            top_probability,
            top_10,
            total_scanned: candidates.len(),
            backend: "systemone_http".to_string(),
        })
    }

    /// Use scanner relevance when Kev is unavailable. These weights are local
    /// ranking weights, not model-calibrated probabilities.
    fn evaluate_locally(&self, candidates: &[ImageCandidate]) -> KevDecisionResult {
        let max_score = candidates
            .iter()
            .map(|candidate| candidate.relevance_score)
            .max()
            .unwrap_or(0) as f64;
        let weights: Vec<f64> = candidates
            .iter()
            .map(|candidate| ((candidate.relevance_score as f64 - max_score) / 40.0).exp())
            .collect();
        let total_weight: f64 = weights.iter().sum();
        let top_10 = candidates
            .iter()
            .zip(weights)
            .take(10)
            .map(|(candidate, weight)| ScoredCandidate {
                candidate: candidate.clone(),
                probability: weight / total_weight,
            })
            .collect::<Vec<_>>();
        let top_choice = candidates[0].clone();
        let top_probability = top_10[0].probability;

        KevDecisionResult {
            top_choice,
            top_probability,
            top_10,
            total_scanned: candidates.len(),
            backend: "local_ranker".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kev_limits_to_top_10() {
        let client = KevClient::default();

        let mut candidates = Vec::new();
        for i in 0..25 {
            candidates.push(ImageCandidate {
                relative_path: format!("assets/icons/icon_{i}.png"),
                file_name: format!("icon_{i}.png"),
                parent_dir: "icons".to_string(),
                file_size: 1000 + i as u64 * 100,
                resolution: 128 + i as u32 * 10,
                extension: "png".to_string(),
                is_valid_icon: true,
                relevance_score: i * 10,
            });
        }

        let result = client
            .rank_and_filter_top10(&candidates, "my-app")
            .expect("kev result");

        assert_eq!(result.total_scanned, 25);
        assert_eq!(
            result.top_10.len(),
            10,
            "Kev decision tool must filter down to top 10"
        );
    }
}
