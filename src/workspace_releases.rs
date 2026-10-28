//! Release data shown in a project workspace.
use release_desk_store::workspace_releases::WorkspaceRelease;

/// A repository's most recent release fetch and its result.
#[derive(Clone, Debug)]
pub struct RepositoryReleases {
    pub provider: String,
    pub owner: String,
    pub repository: String,
    pub result: Result<Vec<WorkspaceRelease>, String>,
}

/// Data exists only after a real fetch; errors remain attached to their repository.
#[derive(Clone, Debug, Default)]
pub enum ReleaseViewState {
    #[default]
    NotLoaded,
    Loading,
    Loaded(Vec<RepositoryReleases>),
}
