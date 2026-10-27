//! Bundled provider logos, rendered without a network dependency.

use crate::workspace::ProviderChoice;
use gpui_kit::component::Icon;
use gpui_kit::{Styled as _, px};

pub fn provider_icon(provider: ProviderChoice) -> Icon {
    let data: &[u8] = match provider {
        ProviderChoice::GitHub => include_bytes!("../../resources/provider-icons/github.svg"),
        ProviderChoice::GitLab => include_bytes!("../../resources/provider-icons/gitlab.svg"),
        ProviderChoice::Bitbucket => include_bytes!("../../resources/provider-icons/bitbucket.svg"),
        ProviderChoice::AzureDevOps => include_bytes!("../../resources/provider-icons/azuredevops.svg"),
        ProviderChoice::Gitea => include_bytes!("../../resources/provider-icons/gitea.svg"),
        ProviderChoice::Forgejo => include_bytes!("../../resources/provider-icons/forgejo.svg"),
        ProviderChoice::Codeberg => include_bytes!("../../resources/provider-icons/codeberg.svg"),
        ProviderChoice::AwsCodeCommit => include_bytes!("../../resources/provider-icons/amazonaws.svg"),
        ProviderChoice::Custom => br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#354ee8" d="M4 5h16v14H4z"/><path fill="white" d="m9 8-4 4 4 4 1.4-1.4L7.8 12l2.6-2.6zm6 0-1.4 1.4 2.6 2.6-2.6 2.6L15 16l4-4z"/></svg>"##,
    };
    Icon::default().data(data).w(px(25.)).h(px(25.))
}
