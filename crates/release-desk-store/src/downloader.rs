//! Streaming HTTP installer asset downloader with live SHA-256 verification.
//!
//! Downloads binary installer assets directly into the managed library staging directory,
//! ensuring that partial or corrupted downloads never overwrite active releases.

use crate::boundary::ManagedLibrary;
use release_desk_application::use_case::installer::DownloadPort;
use release_desk_core::error::DomainError;
use release_desk_core::storage::{RelativeManagedPath, Sha256Checksum};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::time::Duration;

/// HTTP installer downloader implementing `DownloadPort`.
#[derive(Clone)]
pub struct HttpDownloadPort {
    library: ManagedLibrary,
    client: reqwest::blocking::Client,
}

impl HttpDownloadPort {
    /// Create a new downloader bounded to the given managed library.
    pub fn new(library: ManagedLibrary) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(300))
            .user_agent("Release-Desk-Downloader/1.0")
            .build()
            .unwrap_or_default();

        Self { library, client }
    }
}

impl DownloadPort for HttpDownloadPort {
    fn download_to_file(
        &self,
        url: &str,
        _expected_size: u64,
        destination: &RelativeManagedPath,
    ) -> Result<Sha256Checksum, DomainError> {
        let full_path =
            self.library
                .ensure_parent_dir(destination)
                .map_err(|e| DomainError::PortFailure {
                    port: "Download",
                    message: e.to_string(),
                })?;

        let parent = full_path
            .parent()
            .unwrap_or_else(|| self.library.root_path());
        let mut temp =
            tempfile::NamedTempFile::new_in(parent).map_err(|e| DomainError::PortFailure {
                port: "Download",
                message: format!("cannot create staging file: {e}"),
            })?;

        let mut response = self
            .client
            .get(url)
            .send()
            .map_err(|e| DomainError::PortFailure {
                port: "Download",
                message: format!("HTTP request failed: {e}"),
            })?;

        if !response.status().is_success() {
            return Err(DomainError::PortFailure {
                port: "Download",
                message: format!("HTTP status {}: {}", response.status(), url),
            });
        }

        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];

        loop {
            let bytes_read = response
                .read(&mut buffer)
                .map_err(|e| DomainError::PortFailure {
                    port: "Download",
                    message: format!("error reading HTTP stream: {e}"),
                })?;

            if bytes_read == 0 {
                break;
            }

            hasher.update(&buffer[..bytes_read]);
            temp.write_all(&buffer[..bytes_read])
                .map_err(|e| DomainError::PortFailure {
                    port: "Download",
                    message: format!("error writing to staging file: {e}"),
                })?;
        }

        temp.flush().map_err(|e| DomainError::PortFailure {
            port: "Download",
            message: format!("cannot flush staging file: {e}"),
        })?;

        // Atomically persist to destination path
        temp.persist(&full_path)
            .map_err(|e| DomainError::PortFailure {
                port: "Download",
                message: format!("cannot persist downloaded file: {}", e.error),
            })?;

        let hex = format!("{:x}", hasher.finalize());
        Sha256Checksum::new(&hex).map_err(DomainError::from)
    }
}
