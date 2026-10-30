use super::{SourceAdapter, SourceRequest};
use crate::source_fetcher::FetchedProfile;
use release_desk_core::error::DomainError;

pub(super) struct GitHubSource;

impl SourceAdapter for GitHubSource {
    fn discover(
        &self,
        request: &SourceRequest<'_>,
    ) -> Result<(FetchedProfile, Option<String>), DomainError> {
        crate::source_fetcher::fetch_source_input(request.input, "github", request.credential)
    }
}
