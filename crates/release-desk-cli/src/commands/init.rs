//! Library initialization command.

use crate::context::CliContext;
use crate::output::{output_value, print_info};
use release_desk_core::error::DomainError;
use release_desk_core::project::{
    AccountIdentity, AccountKind, AccountLogin, ProjectId, ProjectName,
};
use release_desk_core::snapshot::parse_snapshot;
use serde::Serialize;
use std::fs;
use std::path::PathBuf;

#[derive(Serialize)]
pub struct InitReport {
    pub library_path: PathBuf,
    pub linked_snapshot: Option<PathBuf>,
    pub initialized: bool,
    pub message: String,
}

pub fn handle_init(
    library: Option<PathBuf>,
    snapshot: Option<PathBuf>,
    json: bool,
) -> Result<(), DomainError> {
    let ctx = CliContext::new(library.as_deref())?;
    let lib_path = ctx.library.root_path().to_path_buf();

    let mut linked_path = None;
    if let Some(ref snap_path) = snapshot {
        if snap_path.exists() {
            let content = fs::read_to_string(snap_path).map_err(|e| DomainError::PortFailure {
                port: "SnapshotRead",
                message: format!("cannot read snapshot {}: {e}", snap_path.display()),
            })?;

            let snap = parse_snapshot(&content).map_err(|e| DomainError::PortFailure {
                port: "SnapshotParse",
                message: format!("failed to parse {}: {e}", snap_path.display()),
            })?;

            let project_name = ProjectName::new(&snap.org).unwrap_or_else(|_| {
                ProjectName::new("DefaultProject").expect("valid project name")
            });
            let account_login = AccountLogin::new(&snap.org)
                .unwrap_or_else(|_| AccountLogin::new("default-org").expect("valid account login"));

            let proj_id = ProjectId::from_u128(1);
            let account = AccountIdentity {
                login: account_login,
                kind: AccountKind::Organization,
            };

            // Register initial project if not already present
            if ctx.project_store.get_project(proj_id)?.is_none() {
                let _ = ctx.create_project_uc.execute(
                    release_desk_application::use_case::project::CreateProjectCommand {
                        id: proj_id,
                        name: project_name,
                        account,
                        clone_location: None,
                    },
                );
            }

            linked_path = Some(snap_path.clone());
        }
    }

    let report = InitReport {
        library_path: lib_path.clone(),
        linked_snapshot: linked_path,
        initialized: true,
        message: format!("Library initialized at {}", lib_path.display()),
    };

    output_value(json, &report, |r| {
        print_info(false, format!("✓ {}", r.message));
        if let Some(ref s) = r.linked_snapshot {
            print_info(
                false,
                format!("  Linked snapshot (read-only): {}", s.display()),
            );
        }
    });

    Ok(())
}
