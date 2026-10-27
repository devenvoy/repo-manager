//! Project management and local backup snapshot reconciliation commands.

use crate::args::ProjectSubcommand;
use crate::context::CliContext;
use crate::output::{output_value, print_info};
use release_desk_application::use_case::project::CreateProjectCommand;
use release_desk_core::error::DomainError;
use release_desk_core::project::{
    AccountIdentity, AccountKind, AccountLogin, ProjectId, ProjectName,
};
use release_desk_core::snapshot::MismatchReport;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
pub struct ProjectCreatedReport {
    pub id: u128,
    pub name: String,
    pub account: String,
    pub clone_location: Option<PathBuf>,
    pub backup_snapshot_path: Option<PathBuf>,
}

#[derive(Serialize)]
pub struct ReconcileOutput {
    pub report: MismatchReport,
    pub restored_records: Option<usize>,
    pub updated_backup_path: Option<PathBuf>,
}

pub fn handle_project(
    ctx: &CliContext,
    subcommand: ProjectSubcommand,
    json: bool,
) -> Result<(), DomainError> {
    match subcommand {
        ProjectSubcommand::Create {
            id,
            name,
            account,
            location,
        } => {
            let proj_id = ProjectId::from_u128(id);
            let proj_name = ProjectName::new(&name).map_err(DomainError::from)?;
            let account_login = AccountLogin::new(&account).map_err(DomainError::from)?;

            let cmd = CreateProjectCommand {
                id: proj_id,
                name: proj_name,
                account: AccountIdentity {
                    login: account_login,
                    kind: AccountKind::Organization,
                },
                clone_location: location.clone(),
            };

            let created = ctx.create_project_uc.execute(cmd)?;

            let backup_path = location
                .as_ref()
                .map(|loc| loc.join(".release-desk/data.json"));

            let report = ProjectCreatedReport {
                id: created.id.as_u128(),
                name: created.name.as_str().to_string(),
                account,
                clone_location: created.clone_location,
                backup_snapshot_path: backup_path,
            };

            output_value(json, &report, |r| {
                print_info(
                    false,
                    format!("✓ Project created successfully: {} (ID: {})", r.name, r.id),
                );
                if let Some(ref loc) = r.clone_location {
                    print_info(false, format!("  Clone location: {}", loc.display()));
                }
                if let Some(ref bp) = r.backup_snapshot_path {
                    print_info(
                        false,
                        format!("  Local backup snapshot initialized: {}", bp.display()),
                    );
                }
            });
            Ok(())
        }
        ProjectSubcommand::List => {
            let projects = ctx.list_projects_uc.execute()?;
            output_value(json, &projects, |list| {
                if list.is_empty() {
                    print_info(
                        false,
                        "No projects registered. Create one with `release-desk project create`",
                    );
                    return;
                }
                print_info(false, format!("Registered Projects ({})", list.len()));
                for p in list {
                    let loc_str = p
                        .clone_location
                        .as_ref()
                        .map(|l| format!(" [clone dir: {}]", l.display()))
                        .unwrap_or_default();
                    print_info(
                        false,
                        format!("  • {} (id: {}){}", p.name, p.id.as_u128(), loc_str),
                    );
                }
            });
            Ok(())
        }
        ProjectSubcommand::Reconcile {
            id,
            restore_db,
            sync_backup,
        } => {
            let proj_id = ProjectId::from_u128(id);
            let report = ctx.reconcile_uc.check_mismatch(proj_id)?;

            let mut restored = None;
            if restore_db {
                let count = ctx.reconcile_uc.restore_database_from_backup(proj_id)?;
                restored = Some(count);
            }

            let mut updated_backup = None;
            if sync_backup {
                let path = ctx.reconcile_uc.sync_backup_from_database(proj_id)?;
                updated_backup = Some(path);
            }

            let out = ReconcileOutput {
                report: report.clone(),
                restored_records: restored,
                updated_backup_path: updated_backup.clone(),
            };

            output_value(json, &out, |_| {
                if report.is_synchronized() {
                    print_info(
                        false,
                        "✓ Database and local data.json backup are perfectly in sync!",
                    );
                } else {
                    print_info(
                        false,
                        "⚠ Mismatch detected between database and local data.json backup:",
                    );
                    if !report.disk_snapshot_present {
                        print_info(false, "  • Local backup snapshot is absent on disk.");
                    }
                    if !report.missing_in_database.is_empty() {
                        print_info(
                            false,
                            format!(
                                "  • Repositories present in data.json but missing in database: {:?}",
                                report.missing_in_database
                            ),
                        );
                    }
                    if !report.missing_on_disk.is_empty() {
                        print_info(
                            false,
                            format!(
                                "  • Repositories in database but missing on disk data.json: {:?}",
                                report.missing_on_disk
                            ),
                        );
                    }
                    if !report.revision_mismatches.is_empty() {
                        print_info(
                            false,
                            format!(
                                "  • Revision mismatches: {} repository head commit divergence(s)",
                                report.revision_mismatches.len()
                            ),
                        );
                    }
                }

                if let Some(r_count) = restored {
                    print_info(
                        false,
                        format!("✓ Restored {r_count} missing repository records into database"),
                    );
                }
                if let Some(ref bp) = updated_backup {
                    print_info(
                        false,
                        format!(
                            "✓ Exported database state to local backup: {}",
                            bp.display()
                        ),
                    );
                }
            });

            Ok(())
        }
    }
}
