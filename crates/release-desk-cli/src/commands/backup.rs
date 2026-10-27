//! Library backup archive creation command.

use crate::context::CliContext;
use crate::output::{output_value, print_info};
use release_desk_core::error::DomainError;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
pub struct BackupReport {
    pub source_dir: PathBuf,
    pub output_file: PathBuf,
    pub bytes_written: u64,
}

pub fn handle_backup(ctx: &CliContext, output: PathBuf, json: bool) -> Result<(), DomainError> {
    let source_dir = ctx.library.root_path().to_path_buf();
    let bytes = ctx.archive_port.create_archive(&source_dir, &output)?;

    let report = BackupReport {
        source_dir,
        output_file: output,
        bytes_written: bytes,
    };

    output_value(json, &report, |r| {
        print_info(
            false,
            format!(
                "✓ Backup archive created successfully: {} ({} bytes)",
                r.output_file.display(),
                r.bytes_written
            ),
        );
    });

    Ok(())
}
