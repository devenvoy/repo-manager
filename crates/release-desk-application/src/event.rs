//! Application event stream for reactive UI and logging subscriptions.

use release_desk_core::installer::InstallerStatus;
use release_desk_core::job::{DownloadProgress, JobId, JobKind, JobPriority};
use release_desk_core::project::ProjectId;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// Typed application-level domain and execution events.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum AppEvent {
    JobQueued {
        job_id: JobId,
        kind: JobKind,
        priority: JobPriority,
    },
    JobStarted {
        job_id: JobId,
        kind: JobKind,
    },
    JobProgressUpdate {
        job_id: JobId,
        progress: DownloadProgress,
        message: String,
    },
    JobCompleted {
        job_id: JobId,
        kind: JobKind,
    },
    JobFailed {
        job_id: JobId,
        kind: JobKind,
        error: String,
    },
    JobCancelled {
        job_id: JobId,
    },
    SyncStarted {
        project_id: ProjectId,
        repository: String,
    },
    SyncFinished {
        project_id: ProjectId,
        repository: String,
        outcome: String,
    },
    InstallerStatusChanged {
        repository: String,
        status: InstallerStatus,
    },
    PullRequestsRefreshed {
        repository: String,
        count: usize,
    },
    AuthStarted {
        provider: String,
        method: String,
    },
    AuthFallbackToWebview {
        provider: String,
        reason: String,
    },
    AuthSucceeded {
        provider: String,
        account: String,
    },
    AuthFailed {
        provider: String,
        error: String,
    },
}

/// Broadcast channel for publishing application events to multiple subscribers (GUI, TUI, CLI).
#[derive(Clone, Debug)]
pub struct EventBroadcaster {
    sender: broadcast::Sender<AppEvent>,
}

impl Default for EventBroadcaster {
    fn default() -> Self {
        Self::new(256)
    }
}

impl EventBroadcaster {
    /// Create a new event broadcaster with a given buffer capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Broadcast an event to all active subscribers. Returns number of active receivers.
    pub fn broadcast(&self, event: AppEvent) -> usize {
        self.sender.send(event).unwrap_or(0)
    }

    /// Create a new subscription receiver for observing application events.
    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.sender.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn event_broadcasting_and_reception() {
        let broadcaster = EventBroadcaster::new(16);
        let mut rx = broadcaster.subscribe();

        let event = AppEvent::JobStarted {
            job_id: JobId::from_u128(42),
            kind: JobKind::SyncMetadata,
        };

        let count = broadcaster.broadcast(event.clone());
        assert_eq!(count, 1);

        let received = rx.recv().await.expect("receive event");
        assert_eq!(received, event);
    }
}
