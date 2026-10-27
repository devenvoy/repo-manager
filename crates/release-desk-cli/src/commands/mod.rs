//! CLI subcommand handlers.

pub mod auth;
pub mod backup;
pub mod discovery;
pub mod init;
pub mod installers;
pub mod migrate;
pub mod pr;
pub mod project;
pub mod sync;
pub mod track;

pub use auth::handle_auth;
pub use backup::handle_backup;
pub use discovery::handle_discovery;
pub use init::handle_init;
pub use installers::handle_installers;
pub use migrate::handle_migrate;
pub use pr::handle_pr;
pub use project::handle_project;
pub use sync::handle_sync;
pub use track::{handle_track, handle_untrack};
