//! Repository discovery command.

use crate::context::CliContext;
use crate::output::{output_value, print_info};
use release_desk_core::error::DomainError;
use release_desk_core::project::{AccountIdentity, AccountKind, AccountLogin, ProjectId};
use serde::Serialize;

#[derive(Serialize)]
pub struct DiscoveryItem {
    pub repo_id: u64,
    pub name: String,
    pub status: String,
}

pub fn handle_discovery(
    ctx: &CliContext,
    project_id: u128,
    account: &str,
    json: bool,
) -> Result<(), DomainError> {
    let proj_id = ProjectId::from_u128(project_id);
    let account_login = AccountLogin::new(account).map_err(DomainError::from)?;
    let account_id = AccountIdentity {
        login: account_login,
        kind: AccountKind::Organization,
    };

    let discovered = ctx.discover_uc.execute(proj_id, &account_id)?;

    let items: Vec<DiscoveryItem> = discovered
        .into_iter()
        .map(|r| DiscoveryItem {
            repo_id: r.identity.id.as_u64(),
            name: r.identity.name.as_str().to_string(),
            status: format!("{:?}", r.status),
        })
        .collect();

    output_value(json, &items, |list| {
        print_info(
            false,
            format!(
                "Discovered {} repositories for account '{}':",
                list.len(),
                account
            ),
        );
        for item in list {
            print_info(
                false,
                format!(
                    "  • {:<30} (ID: {}) [{}]",
                    item.name, item.repo_id, item.status
                ),
            );
        }
    });

    Ok(())
}
