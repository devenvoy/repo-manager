//! Application core: use cases, event stream, and job coordinator for Release Desk.

pub mod event;
pub mod job_coordinator;
pub mod provider_registry;
pub mod testing;
pub mod use_case;

pub use event::{AppEvent, EventBroadcaster};
pub use job_coordinator::JobCoordinator;
pub use provider_registry::ProviderRegistry;
