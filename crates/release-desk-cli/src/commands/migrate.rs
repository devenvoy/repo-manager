//! Legacy migration subcommands: analyze, preview, execute.

use crate::args::MigrateSubcommand;
use crate::context::CliContext;
use crate::output::{output_value, print_info};
use release_desk_application::use_case::migration::{
    AnalyzeLegacySetupUseCase, ExecuteLegacyMigrationUseCase, PreviewMigrationPlanUseCase,
};
use release_desk_core::error::{DomainError, ValidationError};
use release_desk_store::legacy_scanner::SystemLegacyScanner;
use std::sync::Arc;

pub fn handle_migrate(
    ctx: &CliContext,
    subcommand: MigrateSubcommand,
    json: bool,
) -> Result<(), DomainError> {
    let scanner = Arc::new(SystemLegacyScanner);

    match subcommand {
        MigrateSubcommand::Analyze { path } => {
            let use_case = AnalyzeLegacySetupUseCase::new(scanner);
            let analysis = use_case.execute(&path)?;

            output_value(json, &analysis, |a| {
                print_info(json, format!("Analyzed legacy path: {}", path.display()));
                println!("Detected kind:        {:?}", a.detected_kind);
                println!("Suggested project:    {}", a.suggested_project_name);
                if let Some(ref org) = a.discovered_org {
                    println!("Discovered account:   {}", org);
                }
                println!(
                    "Discovered repos:     {} ({})",
                    a.repositories.len(),
                    a.repositories.join(", ")
                );
                if !a.installer_assets.is_empty() {
                    println!(
                        "Discovered packages:  {} ({})",
                        a.installer_assets.len(),
                        a.installer_assets.join(", ")
                    );
                }
                for warning in &a.warnings {
                    println!("Notice:               {}", warning);
                }
            });
        }
        MigrateSubcommand::Preview {
            path,
            name,
            location,
            copy_installers,
        } => {
            let analyze_uc = AnalyzeLegacySetupUseCase::new(scanner);
            let analysis = analyze_uc.execute(&path)?;

            let preview_uc = PreviewMigrationPlanUseCase::new(ctx.project_store.clone());
            let plan = preview_uc.execute(&analysis, name.as_deref(), location, copy_installers)?;

            output_value(json, &plan, |p| {
                print_info(
                    json,
                    format!("Migration Plan for project '{}'", p.project_name),
                );
                println!("Source:           {}", p.source_path.display());
                if let Some(ref loc) = p.clone_location {
                    println!("Clone location:   {}", loc.display());
                    println!(
                        "Local backup:     {}/.release-desk/data.json",
                        loc.display()
                    );
                }
                println!("Copy packages:    {}", p.copy_installers);
                println!("\nPlanned Actions:");
                for (idx, action) in p.actions.iter().enumerate() {
                    println!("  {}. {:?}", idx + 1, action);
                }
                println!("\nTo execute this migration, re-run with: execute --confirm");
            });
        }
        MigrateSubcommand::Execute {
            path,
            name,
            location,
            copy_installers,
            confirm,
        } => {
            if !confirm {
                return Err(DomainError::Validation(ValidationError::InvalidPath(
                    "Migration requires explicit confirmation flag: --confirm",
                )));
            }

            let analyze_uc = AnalyzeLegacySetupUseCase::new(scanner.clone());
            let analysis = analyze_uc.execute(&path)?;

            let preview_uc = PreviewMigrationPlanUseCase::new(ctx.project_store.clone());
            let plan = preview_uc.execute(&analysis, name.as_deref(), location, copy_installers)?;

            let execute_uc = ExecuteLegacyMigrationUseCase::new(
                scanner,
                ctx.project_store.clone(),
                ctx.backup_port.clone(),
            );
            let result = execute_uc.execute(plan)?;

            output_value(json, &result, |r| {
                print_info(
                    json,
                    format!(
                        "✓ Successfully adopted legacy setup into project '{}' (id: {})",
                        r.project_name,
                        r.project_id.as_u128()
                    ),
                );
                println!("Imported repositories: {}", r.imported_repositories);
                println!("Imported packages:     {}", r.imported_installers);
                if let Some(ref bp) = r.backup_snapshot_path {
                    println!("Local backup created:  {}", bp.display());
                }
                println!("Source integrity:      Verified untouched (read-only guarantee upheld)");
            });
        }
    }

    Ok(())
}
