//! User-visible state for queued repository pulls.

use std::sync::{Arc, atomic::AtomicBool};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GitUpdateStatus {
    Queued,
    Running(String),
    Complete(String),
    Skipped(String),
    Failed(String),
    Cancelled,
}

impl GitUpdateStatus {
    pub(crate) fn is_active(&self) -> bool {
        matches!(self, Self::Queued | Self::Running(_))
    }

    pub(crate) fn is_terminal(&self) -> bool {
        !self.is_active()
    }
}

#[derive(Clone)]
pub(crate) struct GitUpdateJob {
    pub(crate) key: String,
    pub(crate) repository: String,
    pub(crate) status: GitUpdateStatus,
    pub(crate) cancel: Arc<AtomicBool>,
}
