//! Release Desk desktop GUI library components, state management, and views.

pub mod app;
pub mod colors;
pub mod components;
pub mod controllers;
pub mod git_credential_helper;
pub mod modules;
pub mod repository_git;
pub mod settings;
pub mod state;
pub mod styles;
pub mod views;
pub mod workspace;
pub mod workspace_releases;
pub mod workspace_sources;

pub use app::ReleaseDesk;
