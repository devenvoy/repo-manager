//! Clearly labeled fictional catalog used while project discovery is being built.

use release_desk_core::app_catalog::{AppMetadata, AppStoreItem};
use release_desk_core::project::ProjectId;

pub fn sample_apps() -> Vec<AppStoreItem> {
    let samples = [
        (
            "prism",
            "Prism Studio",
            "Creative tools",
            "A desktop workspace for visual storytelling and live previews.",
            "188 MB",
            "Northstar Suite",
            "v2.8.0",
            None,
        ),
        (
            "quill",
            "Quill Notes",
            "Productivity",
            "A focused writing app with linked notes and offline notebooks.",
            "56 MB",
            "Northstar Suite",
            "v1.6.3",
            Some("v1.6.3"),
        ),
        (
            "atlas",
            "Atlas Maps",
            "Developer tools",
            "Explore geographic data layers and build map prototypes.",
            "92 MB",
            "Northstar Suite",
            "v0.9.5",
            None,
        ),
        (
            "orbit",
            "Orbit Tasks",
            "Productivity",
            "A lightweight task board for distributed teams.",
            "79 MB",
            "Orbit Team Tools",
            "v3.1.0",
            Some("v3.0.0"),
        ),
        (
            "relay",
            "Relay Client",
            "Developer tools",
            "Inspect API requests and live streams in a clear interface.",
            "72 MB",
            "Community Builds",
            "v1.2.1",
            None,
        ),
    ];
    samples
        .into_iter()
        .map(
            |(id, name, category, description, size, project, tag, installed)| {
                let metadata = AppMetadata {
                    id: id.into(),
                    name: name.into(),
                    category: category.to_lowercase(),
                    category_name: category.into(),
                    description: description.into(),
                    size_estimate: size.into(),
                    icon_text: id[..1].to_uppercase(),
                    color_gradient: String::new(),
                    glow_color: String::new(),
                    changelog: vec!["Fictional release notes for the UI preview.".into()],
                    binary_name: format!("{name}.app"),
                };
                AppStoreItem::new(
                    metadata,
                    ProjectId::from_u128(0),
                    project.into(),
                    id.into(),
                    Some(tag.into()),
                    installed.map(str::to_string),
                )
            },
        )
        .collect()
}
