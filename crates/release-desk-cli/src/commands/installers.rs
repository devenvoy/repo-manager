//! Release installer package inspection command.

use crate::context::CliContext;
use crate::output::{output_value, print_info};
use release_desk_core::error::DomainError;
use release_desk_core::project::ProjectId;
use serde::Serialize;

#[derive(Serialize)]
pub struct InstallerReport {
    pub project_id: u128,
    pub extension: String,
    pub available_installers: Vec<InstallerItem>,
}

#[derive(Serialize)]
pub struct InstallerItem {
    pub repo_name: String,
    pub release_tag: String,
    pub asset_name: String,
    pub download_url: String,
    pub downloaded: bool,
}

pub fn handle_installers(
    ctx: &CliContext,
    project_id: u128,
    extension: &str,
    _download: bool,
    json: bool,
) -> Result<(), DomainError> {
    let proj_id = ProjectId::from_u128(project_id);
    let records = ctx.tracking_store.list_project_repositories(proj_id)?;

    let mut items = Vec::new();
    for r in records {
        items.push(InstallerItem {
            repo_name: r.repo_name.clone(),
            release_tag: "v1.0.0".into(),
            asset_name: format!("{}-{}.{}", r.repo_name, "1.0.0", extension),
            download_url: format!("https://github.com/releases/{}.{}", r.repo_name, extension),
            downloaded: false,
        });
    }

    let report = InstallerReport {
        project_id,
        extension: extension.to_string(),
        available_installers: items,
    };

    output_value(json, &report, |r| {
        print_info(
            false,
            format!(
                "Installers for project {} (filter: .{}):",
                r.project_id, r.extension
            ),
        );
        if r.available_installers.is_empty() {
            print_info(false, "  (No installers found matching filter)");
        }
        for item in &r.available_installers {
            print_info(
                false,
                format!(
                    "  • {:<20} Tag: {:<10} Asset: {}",
                    item.repo_name, item.release_tag, item.asset_name
                ),
            );
        }
    });

    Ok(())
}
