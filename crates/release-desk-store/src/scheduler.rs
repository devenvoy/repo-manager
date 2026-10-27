//! OS background daemon registration adapter (launchd / systemd).
//!
//! Implements `SchedulerPort` for registering scheduled background sync operations.

use release_desk_core::error::DomainError;
use release_desk_core::port::SchedulerPort;
use std::fs;
use std::path::PathBuf;

const DAEMON_LABEL: &str = "com.storytold.release-desk";

/// Scheduler port adapter managing OS background daemon services.
#[derive(Clone, Debug, Default)]
pub struct OsSchedulerPort {
    custom_plist_dir: Option<PathBuf>,
}

impl OsSchedulerPort {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a custom output directory for testing plist generation.
    pub fn with_custom_dir(dir: PathBuf) -> Self {
        Self {
            custom_plist_dir: Some(dir),
        }
    }

    fn plist_path(&self) -> Result<PathBuf, DomainError> {
        if let Some(ref dir) = self.custom_plist_dir {
            return Ok(dir.join(format!("{DAEMON_LABEL}.plist")));
        }

        #[cfg(target_os = "macos")]
        {
            let home = std::env::var("HOME").map_err(|_| DomainError::PortFailure {
                port: "Scheduler",
                message: "HOME environment variable not set".into(),
            })?;
            Ok(PathBuf::from(home)
                .join("Library/LaunchAgents")
                .join(format!("{DAEMON_LABEL}.plist")))
        }

        #[cfg(not(target_os = "macos"))]
        {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            Ok(PathBuf::from(home)
                .join(".config/systemd/user")
                .join("release-desk.service"))
        }
    }

    /// Render launchd XML plist content.
    pub fn render_launchd_plist(interval_seconds: u64) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{DAEMON_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>release-desk-cli</string>
        <string>sync</string>
        <string>--all</string>
    </array>
    <key>StartInterval</key>
    <integer>{interval_seconds}</integer>
    <key>RunAtLoad</key>
    <true/>
    <key>StandardErrorPath</key>
    <string>/tmp/release-desk-daemon.err</string>
    <key>StandardOutPath</key>
    <string>/tmp/release-desk-daemon.out</string>
</dict>
</plist>
"#
        )
    }
}

impl SchedulerPort for OsSchedulerPort {
    fn register_daemon(&self) -> Result<(), DomainError> {
        let plist_path = self.plist_path()?;
        if let Some(parent) = plist_path.parent() {
            fs::create_dir_all(parent).map_err(|e| DomainError::PortFailure {
                port: "Scheduler",
                message: format!("cannot create directory {}: {e}", parent.display()),
            })?;
        }

        let content = Self::render_launchd_plist(300);
        fs::write(&plist_path, content).map_err(|e| DomainError::PortFailure {
            port: "Scheduler",
            message: format!("cannot write plist to {}: {e}", plist_path.display()),
        })?;

        // On macOS, if not custom dir, invoke launchctl
        #[cfg(target_os = "macos")]
        if self.custom_plist_dir.is_none() {
            let _ = std::process::Command::new("launchctl")
                .args(["load", "-w", plist_path.to_str().unwrap_or_default()])
                .output();
        }

        Ok(())
    }

    fn unregister_daemon(&self) -> Result<(), DomainError> {
        let plist_path = self.plist_path()?;

        #[cfg(target_os = "macos")]
        if self.custom_plist_dir.is_none() {
            let _ = std::process::Command::new("launchctl")
                .args(["unload", plist_path.to_str().unwrap_or_default()])
                .output();
        }

        if plist_path.exists() {
            fs::remove_file(&plist_path).map_err(|e| DomainError::PortFailure {
                port: "Scheduler",
                message: format!("cannot remove plist at {}: {e}", plist_path.display()),
            })?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_and_register_plist_to_temp_dir() {
        let temp = tempfile::tempdir().unwrap();
        let scheduler = OsSchedulerPort::with_custom_dir(temp.path().to_path_buf());

        scheduler.register_daemon().unwrap();

        let plist_file = temp.path().join(format!("{DAEMON_LABEL}.plist"));
        assert!(plist_file.exists());

        let content = fs::read_to_string(&plist_file).unwrap();
        assert!(content.contains(DAEMON_LABEL));
        assert!(content.contains("StartInterval"));

        scheduler.unregister_daemon().unwrap();
        assert!(!plist_file.exists());
    }
}
