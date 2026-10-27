//! Background jobs, execution priorities, and retry backoff.

use crate::error::ValidationError;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique job execution identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct JobId(u128);

impl JobId {
    pub const fn from_u128(val: u128) -> Self {
        Self(val)
    }

    pub const fn as_u128(self) -> u128 {
        self.0
    }
}

/// Kinds of asynchronous jobs managed by the application.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    SyncMetadata,
    FastForwardSource,
    DownloadInstaller,
    BackupProject,
    HealthCheck,
}

/// Job queue priority.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobPriority {
    Low,
    #[default]
    Normal,
    High,
    Critical,
}

/// Current execution state of an asynchronous job.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Pending,
    Running,
    Completed,
    Failed(String),
    Cancelled,
}

/// A download currently in progress.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub received: u64,
    pub total: Option<u64>,
}

/// Unique idempotency key to prevent duplicate job queueing.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    pub fn new(raw: &str) -> Result<Self, ValidationError> {
        let key = raw.trim();
        if key.is_empty() {
            return Err(ValidationError::Empty("idempotency key"));
        }
        if key.len() > 128 {
            return Err(ValidationError::TooLong {
                field: "idempotency key",
                max: 128,
            });
        }
        if !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':' | '.'))
        {
            return Err(ValidationError::InvalidCharacter("idempotency key"));
        }
        Ok(Self(key.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for IdempotencyKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Exponential backoff calculator with bounded limits.
pub struct RetryBackoff;

impl RetryBackoff {
    /// Compute exponential backoff delay in seconds: `base_secs * 2^(attempt - 1)`
    /// clamped to `max_secs`.
    pub fn calculate_backoff_secs(attempt: u32, base_secs: u64, max_secs: u64) -> u64 {
        if attempt == 0 {
            return 0;
        }
        let shift = (attempt - 1).min(31);
        let multiplier = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
        base_secs.saturating_mul(multiplier).min(max_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idempotency_key_rules() {
        assert!(IdempotencyKey::new("sync:project_1:metadata").is_ok());
        assert!(IdempotencyKey::new("").is_err());
        assert!(IdempotencyKey::new("invalid key with space").is_err());
    }

    #[test]
    fn backoff_progression() {
        assert_eq!(RetryBackoff::calculate_backoff_secs(0, 5, 60), 0);
        assert_eq!(RetryBackoff::calculate_backoff_secs(1, 5, 60), 5);
        assert_eq!(RetryBackoff::calculate_backoff_secs(2, 5, 60), 10);
        assert_eq!(RetryBackoff::calculate_backoff_secs(3, 5, 60), 20);
        assert_eq!(RetryBackoff::calculate_backoff_secs(5, 5, 60), 60); // clamped
    }
}
