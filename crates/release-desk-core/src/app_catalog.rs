//! App Store domain models and ArtCraft Studio application catalog registry.
//!
//! Provides the metadata specifications for runnable desktop applications,
//! version resolution, and 3-state action derivation (Install, Update, Open).

use crate::project::ProjectId;
use serde::{Deserialize, Serialize};

/// Action state of an application in the Local App Store.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppActionState {
    /// Not installed on the system; ready for 1-click install.
    Installable,
    /// Installed on the system, but an upstream newer release tag is available.
    UpdateAvailable {
        installed_version: String,
        latest_version: String,
    },
    /// Installed on the system and exactly matches the latest release tag.
    UpToDate { version: String },
}

impl AppActionState {
    /// Derive the 3-state action from the installed version and upstream release tag.
    pub fn derive(installed_version: Option<&str>, latest_version: Option<&str>) -> Self {
        match (installed_version, latest_version) {
            (None, _) => Self::Installable,
            (Some(installed), None) => Self::UpToDate {
                version: installed.to_string(),
            },
            (Some(installed), Some(latest)) => {
                let norm_installed = installed.trim_start_matches('v');
                let norm_latest = latest.trim_start_matches('v');
                let installed_parts = numeric_version(norm_installed);
                let latest_parts = numeric_version(norm_latest);
                if norm_installed == norm_latest
                    || installed == "installed"
                    || matches!((&installed_parts, &latest_parts), (Some(current), Some(next)) if next <= current)
                {
                    Self::UpToDate {
                        version: installed.to_string(),
                    }
                } else {
                    Self::UpdateAvailable {
                        installed_version: installed.to_string(),
                        latest_version: latest.to_string(),
                    }
                }
            }
        }
    }

    pub fn is_installed(&self) -> bool {
        matches!(self, Self::UpdateAvailable { .. } | Self::UpToDate { .. })
    }

    pub fn requires_update(&self) -> bool {
        matches!(self, Self::UpdateAvailable { .. })
    }
}

fn numeric_version(value: &str) -> Option<Vec<u64>> {
    let value = value.split(['-', '+']).next()?;
    value
        .split('.')
        .map(str::parse)
        .collect::<Result<Vec<_>, _>>()
        .ok()
}

/// Rich metadata defining a desktop application in the catalog.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppMetadata {
    pub id: String,
    pub name: String,
    pub category: String,
    pub category_name: String,
    pub description: String,
    pub size_estimate: String,
    pub icon_text: String,
    pub color_gradient: String,
    pub glow_color: String,
    pub changelog: Vec<String>,
    pub binary_name: String,
}

/// An entry displayed in the Local App Store, aggregated across projects.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppStoreItem {
    pub metadata: AppMetadata,
    pub project_id: ProjectId,
    pub project_name: String,
    pub repo_name: String,
    pub latest_release_tag: Option<String>,
    pub installed_version: Option<String>,
    pub state: AppActionState,
}

impl AppStoreItem {
    pub fn new(
        metadata: AppMetadata,
        project_id: ProjectId,
        project_name: String,
        repo_name: String,
        latest_release_tag: Option<String>,
        installed_version: Option<String>,
    ) -> Self {
        let state =
            AppActionState::derive(installed_version.as_deref(), latest_release_tag.as_deref());
        Self {
            metadata,
            project_id,
            project_name,
            repo_name,
            latest_release_tag,
            installed_version,
            state,
        }
    }
}

/// Returns the full suite of known ArtCraft Studio applications.
pub fn default_artcraft_catalog() -> Vec<AppMetadata> {
    vec![
        AppMetadata {
            id: "artcraft".to_string(),
            name: "ArtCraft Studio".to_string(),
            category: "creative".to_string(),
            category_name: "AI Creative Studio".to_string(),
            description:
                "AI-assisted digital art workstation with real-time diffusion preview and layers."
                    .to_string(),
            size_estimate: "428 MB".to_string(),
            icon_text: "AC".to_string(),
            color_gradient: "from-purple-600 via-indigo-600 to-pink-600".to_string(),
            glow_color: "rgba(168, 85, 247, 0.4)".to_string(),
            changelog: vec![
                "Metal 3 GPU hardware acceleration for local generative pipeline.".to_string(),
                "Real-time latents visualizer with dynamic prompt interpolation.".to_string(),
                "Added brush stabilizer and high-precision stylus pressure curve.".to_string(),
                "Memory management fix when loading multi-gigabyte safetensors models.".to_string(),
            ],
            binary_name: "ArtCraft Studio.app".to_string(),
        },
        AppMetadata {
            id: "photocraft".to_string(),
            name: "PhotoCraft".to_string(),
            category: "creative".to_string(),
            category_name: "Image Editor".to_string(),
            description:
                "Pro RAW photo development, color grading curves, and batch image processor."
                    .to_string(),
            size_estimate: "184 MB".to_string(),
            icon_text: "PC".to_string(),
            color_gradient: "from-cyan-500 to-blue-600".to_string(),
            glow_color: "rgba(6, 182, 212, 0.4)".to_string(),
            changelog: vec![
                "Added native Apple Silicon hardware acceleration for ProRes and AV1 encoding."
                    .to_string(),
                "New Curves adjustment tool with real-time RGB histogram preview.".to_string(),
                "Fixed crash when importing RAW files from Sony A7 IV.".to_string(),
                "Reduced memory footprint during 8K panorama stitching by 34%.".to_string(),
            ],
            binary_name: "PhotoCraft.app".to_string(),
        },
        AppMetadata {
            id: "gridcraft".to_string(),
            name: "GridCraft".to_string(),
            category: "productivity".to_string(),
            category_name: "Spreadsheets".to_string(),
            description:
                "High-speed analytical spreadsheet engine with Python/WASM formula execution."
                    .to_string(),
            size_estimate: "64 MB".to_string(),
            icon_text: "GC".to_string(),
            color_gradient: "from-emerald-500 to-teal-600".to_string(),
            glow_color: "rgba(16, 185, 129, 0.4)".to_string(),
            changelog: vec![
                "Zero-latency calculation engine compiled to WebAssembly with SIMD.".to_string(),
                "Pivot table multi-dimensional filtering with cross-sheet links.".to_string(),
                "Export directly to SQLite, Parquet, and Excel formats.".to_string(),
            ],
            binary_name: "GridCraft.app".to_string(),
        },
        AppMetadata {
            id: "filmcraft".to_string(),
            name: "FilmCraft".to_string(),
            category: "creative".to_string(),
            category_name: "Video Editor".to_string(),
            description:
                "Timeline-based video editor with multi-track audio and color LUT grading."
                    .to_string(),
            size_estimate: "310 MB".to_string(),
            icon_text: "FC".to_string(),
            color_gradient: "from-rose-500 to-amber-600".to_string(),
            glow_color: "rgba(244, 63, 94, 0.4)".to_string(),
            changelog: vec![
                "Multi-track timeline with magnetic snapping and ripple edit.".to_string(),
                "Native 3D LUT grading support with live vector scope.".to_string(),
                "Background render queue with hardware H.265 export.".to_string(),
            ],
            binary_name: "FilmCraft.app".to_string(),
        },
        AppMetadata {
            id: "vectorcraft".to_string(),
            name: "VectorCraft".to_string(),
            category: "creative".to_string(),
            category_name: "Vector Illustration".to_string(),
            description: "Precision vector design, bezier pen tools, SVG exports, and iconography."
                .to_string(),
            size_estimate: "92 MB".to_string(),
            icon_text: "VC".to_string(),
            color_gradient: "from-orange-500 to-red-600".to_string(),
            glow_color: "rgba(249, 115, 22, 0.4)".to_string(),
            changelog: vec![
                "Sub-pixel bezier pen algorithm with smooth tangent snapping.".to_string(),
                "Boolean shape combination engine with non-destructive history.".to_string(),
                "SVG and PDF vector asset export with automated artboard slicing.".to_string(),
            ],
            binary_name: "VectorCraft.app".to_string(),
        },
        AppMetadata {
            id: "pdfcraft".to_string(),
            name: "PdfCraft".to_string(),
            category: "productivity".to_string(),
            category_name: "PDF Workbench".to_string(),
            description:
                "Merge, annotate, redact, and cryptographically sign PDF documents locally."
                    .to_string(),
            size_estimate: "45 MB".to_string(),
            icon_text: "PDF".to_string(),
            color_gradient: "from-red-600 to-rose-700".to_string(),
            glow_color: "rgba(225, 29, 72, 0.4)".to_string(),
            changelog: vec![
                "Added local OCR engine for text search inside scanned PDF documents.".to_string(),
                "Cryptographic digital signatures with local certificate store.".to_string(),
                "Batch page reordering, splitting, and redaction tools.".to_string(),
            ],
            binary_name: "PdfCraft.app".to_string(),
        },
        AppMetadata {
            id: "soundcraft".to_string(),
            name: "SoundCraft".to_string(),
            category: "creative".to_string(),
            category_name: "Audio Workstation".to_string(),
            description:
                "Spatial sound design, multi-track audio recording, and VST/AU synth host."
                    .to_string(),
            size_estimate: "210 MB".to_string(),
            icon_text: "SC".to_string(),
            color_gradient: "from-violet-600 to-purple-700".to_string(),
            glow_color: "rgba(139, 92, 246, 0.4)".to_string(),
            changelog: vec![
                "Ultra-low latency audio engine using macOS CoreAudio.".to_string(),
                "Spatial 3D binaural panner with dynamic reverb modeling.".to_string(),
                "Support for AudioUnit v3 and VST3 third-party plugins.".to_string(),
            ],
            binary_name: "SoundCraft.app".to_string(),
        },
        AppMetadata {
            id: "cadcraft".to_string(),
            name: "CADCraft".to_string(),
            category: "devtools".to_string(),
            category_name: "3D CAD & Drafting".to_string(),
            description: "Parametric 3D solid modeling, STEP/IGES viewer, and mesh export tools."
                .to_string(),
            size_estimate: "315 MB".to_string(),
            icon_text: "CAD".to_string(),
            color_gradient: "from-blue-600 to-indigo-700".to_string(),
            glow_color: "rgba(59, 130, 246, 0.4)".to_string(),
            changelog: vec![
                "Parametric sketch constraints engine with real-time solver.".to_string(),
                "STEP and IGES industrial CAD file importer.".to_string(),
                "Direct mesh export for 3D printing (STL, 3MF).".to_string(),
            ],
            binary_name: "CADCraft.app".to_string(),
        },
    ]
}

/// Returns sample suite applications from the design prototype.
pub fn extended_sample_catalog() -> Vec<AppMetadata> {
    vec![
        AppMetadata {
            id: "prism".to_string(),
            name: "Prism Studio".to_string(),
            category: "creative".to_string(),
            category_name: "Creative Tools".to_string(),
            description:
                "Desktop workspace for visual storytelling, moodboards, and real-time previews."
                    .to_string(),
            size_estimate: "188 MB".to_string(),
            icon_text: "PS".to_string(),
            color_gradient: "from-indigo-600 to-violet-700".to_string(),
            glow_color: "rgba(99, 102, 241, 0.4)".to_string(),
            changelog: vec![
                "New canvas presets, faster preview rendering, and refined export controls."
                    .to_string(),
                "Stability improvements and color profile fixes.".to_string(),
            ],
            binary_name: "Prism Studio.app".to_string(),
        },
        AppMetadata {
            id: "quill".to_string(),
            name: "Quill Notes".to_string(),
            category: "productivity".to_string(),
            category_name: "Productivity".to_string(),
            description:
                "A focused writing app with linked notes and a clean distraction-free editor."
                    .to_string(),
            size_estimate: "56 MB".to_string(),
            icon_text: "QN".to_string(),
            color_gradient: "from-amber-500 to-orange-600".to_string(),
            glow_color: "rgba(245, 158, 11, 0.4)".to_string(),
            changelog: vec![
                "Improved search index and offline notebooks sync.".to_string(),
                "Distraction-free typewriter scrolling mode.".to_string(),
            ],
            binary_name: "Quill Notes.app".to_string(),
        },
        AppMetadata {
            id: "atlas".to_string(),
            name: "Atlas Maps".to_string(),
            category: "devtools".to_string(),
            category_name: "Developer Tools".to_string(),
            description: "Explore geographic data layers and build map-driven prototypes."
                .to_string(),
            size_estimate: "92 MB".to_string(),
            icon_text: "AM".to_string(),
            color_gradient: "from-teal-500 to-cyan-600".to_string(),
            glow_color: "rgba(20, 184, 166, 0.4)".to_string(),
            changelog: vec![
                "Better vector tile layer controls and GeoJSON export support.".to_string(),
                "Hardware-accelerated MapLibre renderer integration.".to_string(),
            ],
            binary_name: "Atlas Maps.app".to_string(),
        },
        AppMetadata {
            id: "orbit".to_string(),
            name: "Orbit Tasks".to_string(),
            category: "productivity".to_string(),
            category_name: "Task Board".to_string(),
            description: "A lightweight task board for small teams working across time zones."
                .to_string(),
            size_estimate: "79 MB".to_string(),
            icon_text: "OT".to_string(),
            color_gradient: "from-sky-500 to-blue-600".to_string(),
            glow_color: "rgba(14, 165, 233, 0.4)".to_string(),
            changelog: vec![
                "Timeline view and smart recurring tasks engine.".to_string(),
                "Offline-first sync with Conflict-free Replicated Data Types.".to_string(),
            ],
            binary_name: "Orbit Tasks.app".to_string(),
        },
        AppMetadata {
            id: "relay".to_string(),
            name: "Relay Client".to_string(),
            category: "devtools".to_string(),
            category_name: "API Workbench".to_string(),
            description:
                "A clear, fast client for inspecting API requests, WebSockets, and live streams."
                    .to_string(),
            size_estimate: "72 MB".to_string(),
            icon_text: "RC".to_string(),
            color_gradient: "from-fuchsia-600 to-pink-600".to_string(),
            glow_color: "rgba(217, 70, 239, 0.4)".to_string(),
            changelog: vec![
                "New streaming request history and keyboard shortcuts.".to_string(),
                "gRPC reflection and automated protobuf decoding.".to_string(),
            ],
            binary_name: "Relay Client.app".to_string(),
        },
    ]
}

/// Matches a repository name against the known app catalog.
pub fn find_known_app(repo_name: &str) -> Option<AppMetadata> {
    let lower = repo_name.to_lowercase();
    default_artcraft_catalog()
        .into_iter()
        .chain(extended_sample_catalog())
        .find(|app| app.id == lower || app.name.to_lowercase() == lower || lower.contains(&app.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_action_state_derivation() {
        assert_eq!(
            AppActionState::derive(None, Some("v2.4.0")),
            AppActionState::Installable
        );
        assert_eq!(
            AppActionState::derive(Some("v2.3.9"), Some("v2.4.0")),
            AppActionState::UpdateAvailable {
                installed_version: "v2.3.9".to_string(),
                latest_version: "v2.4.0".to_string(),
            }
        );
        assert_eq!(
            AppActionState::derive(Some("v2.4.0"), Some("2.4.0")),
            AppActionState::UpToDate {
                version: "v2.4.0".to_string(),
            }
        );
        assert_eq!(
            AppActionState::derive(Some("v1.0.0"), None),
            AppActionState::UpToDate {
                version: "v1.0.0".to_string(),
            }
        );
        assert!(matches!(
            AppActionState::derive(Some("installed"), Some("v2.0.0")),
            AppActionState::UpToDate { .. }
        ));
        assert!(matches!(
            AppActionState::derive(Some("2.0.0"), Some("v1.9.0")),
            AppActionState::UpToDate { .. }
        ));
    }

    #[test]
    fn test_find_known_app() {
        let app = find_known_app("PhotoCraft").expect("find photocraft");
        assert_eq!(app.id, "photocraft");
        assert_eq!(app.category, "creative");

        let app2 = find_known_app("storytold/gridcraft").expect("find gridcraft");
        assert_eq!(app2.id, "gridcraft");
    }
}
