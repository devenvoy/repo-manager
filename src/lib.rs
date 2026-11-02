//! Release Desk desktop GUI library components, state management, and views.

pub mod app;
pub mod colors;
pub mod components;
pub mod controllers;
pub mod git_credential_helper;
pub(crate) mod git_update_queue;
pub mod icon_catalog;
pub mod modules;
pub(crate) mod release_queue;
pub mod repository_git;
pub mod settings;
pub mod state;
pub mod store_catalog;
pub mod styles;
pub mod views;
pub mod workspace;
pub mod workspace_releases;
pub mod workspace_sources;

pub use app::ReleaseDesk;
