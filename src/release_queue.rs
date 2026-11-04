//! Release download queue state shared by the release controller and shelf UI.

use std::sync::{Arc, atomic::AtomicBool};

type DownloadTask = Box<dyn FnOnce() + Send + 'static>;

/// Submit work to a process-wide pool with exactly two blocking I/O workers.
pub(crate) fn submit(task: impl FnOnce() + Send + 'static) {
    static SENDER: std::sync::OnceLock<std::sync::mpsc::Sender<DownloadTask>> =
        std::sync::OnceLock::new();
    let sender = SENDER.get_or_init(|| {
        let (sender, receiver) = std::sync::mpsc::channel::<DownloadTask>();
        let receiver = Arc::new(std::sync::Mutex::new(receiver));
        for index in 0..2 {
            let receiver = Arc::clone(&receiver);
            std::thread::Builder::new()
                .name(format!("release-download-{index}"))
                .spawn(move || {
                    loop {
                        let next = receiver
                            .lock()
                            .expect("download worker receiver lock poisoned")
                            .recv();
                        match next {
                            Ok(task) => task(),
                            Err(_) => break,
                        }
                    }
                })
                .expect("failed to start release download worker");
        }
        sender
    });
    let _ = sender.send(Box::new(task));
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DownloadStatus {
    Queued,
    Downloading,
    Cancelling,
    Complete,
    Failed(String),
    Cancelled,
}

#[derive(Clone)]
pub struct DownloadJob {
    pub key: String,
    pub project_id: u128,
    pub repository: String,
    pub asset: String,
    pub status: DownloadStatus,
    pub received: u64,
    pub total: Option<u64>,
    pub cancel: Arc<AtomicBool>,
}

pub(crate) enum DownloadUpdate {
    Progress(u64, Option<u64>),
    Finished(Result<std::path::PathBuf, String>),
}

impl DownloadJob {
    pub fn progress(&self) -> Option<f32> {
        self.total
            .filter(|total| *total > 0)
            .map(|total| (self.received as f32 / total as f32).clamp(0.0, 1.0))
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self.status,
            DownloadStatus::Queued | DownloadStatus::Downloading | DownloadStatus::Cancelling
        )
    }
}
