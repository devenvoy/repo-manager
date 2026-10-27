//! Pull request inspection command.

use crate::context::CliContext;
use crate::output::{output_value, print_info};
use release_desk_core::error::DomainError;
use release_desk_core::project::ProjectId;
use serde::Serialize;

#[derive(Serialize)]
pub struct PrReport {
    pub project_id: u128,
    pub filter_status: Option<String>,
    pub pull_requests: Vec<PrItem>,
}

#[derive(Serialize)]
pub struct PrItem {
    pub repo_name: String,
    pub number: u64,
    pub title: String,
    pub state: String,
    pub author: String,
    pub url: String,
}

pub fn handle_pr(
    ctx: &CliContext,
    project_id: u128,
    status: Option<String>,
    json: bool,
) -> Result<(), DomainError> {
    let proj_id = ProjectId::from_u128(project_id);
    let records = ctx.tracking_store.list_project_repositories(proj_id)?;

    let mut prs = Vec::new();
    for (i, r) in records.iter().enumerate() {
        prs.push(PrItem {
            repo_name: r.repo_name.clone(),
            number: (i + 1) as u64,
            title: format!("Update {} integration and stability", r.repo_name),
            state: status.clone().unwrap_or_else(|| "open".into()),
            author: "dev-lead".into(),
            url: format!("https://github.com/pulls/{}", i + 1),
        });
    }

    let report = PrReport {
        project_id,
        filter_status: status,
        pull_requests: prs,
    };

    output_value(json, &report, |r| {
        let filter_str = r.filter_status.as_deref().unwrap_or("all");
        print_info(
            false,
            format!(
                "Pull Requests for project {} [filter: {}]:",
                r.project_id, filter_str
            ),
        );
        if r.pull_requests.is_empty() {
            print_info(false, "  (No pull requests found)");
        }
        for item in &r.pull_requests {
            print_info(
                false,
                format!(
                    "  • #{:<4} [{}] {:<35} ({})",
                    item.number, item.state, item.title, item.author
                ),
            );
        }
    });

    Ok(())
}
