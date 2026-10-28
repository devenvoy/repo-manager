//! Release shelf backed only by fetched provider data.
use crate::components::motion_button::Button;
use crate::components::ui_icons::{download_icon, refresh_icon};
use crate::workspace_releases::ReleaseViewState;
use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{Context, IntoElement, ParentElement as _, Styled as _, Window, div, px};
use release_desk_store::workspace_releases::{
    LocalAssetState, local_asset_state, newer_release_with_local_older,
};
use std::path::Path;

/// Render releases and emit refresh/download actions. Downloads are performed by the caller.
pub fn render_live_releases<T: 'static>(
    state: &ReleaseViewState,
    project_directory: &Path,
    busy: bool,
    cx: &Context<T>,
    on_refresh: impl Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    on_download: impl Fn(
        String,
        String,
        String,
        String,
        String,
        String,
        Option<u64>,
        &mut T,
        &mut Window,
        &mut Context<T>,
    ) + Copy
    + 'static,
) -> impl IntoElement {
    let mut body = div().flex().flex_col().gap_4().child(
        div().flex().items_center().justify_between().child(
            div().flex().flex_col().gap_1().child(div().text_size(px(10.)).font_bold().text_color(cx.theme().primary).child("RELEASE SHELF"))
                .child(div().text_size(px(29.)).font_bold().child("Releases & downloads"))
                .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground).child("Published releases and downloadable assets for selected repositories."))
        ).child(Button::new("refresh-releases").icon(refresh_icon()).label(if busy || matches!(state, ReleaseViewState::Loading) { "Fetching…" } else { "Refresh releases" }).outline().disabled(busy || matches!(state, ReleaseViewState::Loading)).on_click(cx.listener(move |this, _, window, cx| on_refresh(this, window, cx))))
    );
    match state {
        ReleaseViewState::NotLoaded => {
            body =
                body.child(
                    div().p_5().rounded_lg().bg(cx.theme().background).child(
                        "Select repositories in Sources, then refresh to fetch their releases.",
                    ),
                )
        }
        ReleaseViewState::Loading => {
            body = body.child(div().p_5().child("Fetching releases from the provider…"))
        }
        ReleaseViewState::Loaded(repositories) if repositories.is_empty() => {
            body = body.child(
                div()
                    .p_5()
                    .child("No repositories selected. Choose repositories in Sources first."),
            )
        }
        ReleaseViewState::Loaded(repositories) => {
            let published_count = repositories
                .iter()
                .filter(|repository| {
                    repository
                        .result
                        .as_ref()
                        .is_ok_and(|releases| releases.iter().any(|release| !release.is_draft))
                })
                .count();
            if published_count == 0
                && repositories
                    .iter()
                    .all(|repository| repository.result.is_ok())
            {
                body = body.child(
                    div().py_6().border_t_1().border_color(cx.theme().border)
                        .flex().flex_col().gap_2()
                        .child(div().font_bold().text_size(px(16.)).child("No published releases yet"))
                        .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground)
                            .child("The selected repositories have no published releases from their providers."))
                );
            }
            for (repo_index, repository) in repositories.iter().enumerate() {
                if let Err(error) = &repository.result {
                    body =
                        body.child(
                            div()
                                .py_3()
                                .border_t_1()
                                .border_color(cx.theme().border)
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(div().font_bold().text_size(px(13.)).child(format!(
                                    "{}/{}",
                                    repository.owner, repository.repository
                                )))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(cx.theme().danger)
                                        .child(format!("Could not check releases: {error}")),
                                ),
                        );
                    continue;
                }
                if repository
                    .result
                    .as_ref()
                    .is_ok_and(|releases| releases.iter().all(|release| release.is_draft))
                {
                    continue;
                }
                let mut card = div()
                    .py_4()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .w(px(34.))
                                    .h(px(34.))
                                    .rounded_lg()
                                    .bg(cx.theme().primary.opacity(0.1))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(crate::components::provider_icon::provider_icon(
                                        match repository.provider.as_str() {
                                            "github" => crate::workspace::ProviderChoice::GitHub,
                                            "gitlab" => crate::workspace::ProviderChoice::GitLab,
                                            "bitbucket" => {
                                                crate::workspace::ProviderChoice::Bitbucket
                                            }
                                            _ => crate::workspace::ProviderChoice::Custom,
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(div().font_bold().text_size(px(17.)).child(format!(
                                        "{}/{}",
                                        repository.owner, repository.repository
                                    )))
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(cx.theme().muted_foreground)
                                            .child(repository.provider.clone()),
                                    ),
                            ),
                    );
                match &repository.result {
                    Err(error) => {
                        card = card.child(
                            div()
                                .text_size(px(12.))
                                .child(format!("Could not fetch releases: {error}")),
                        )
                    }
                    Ok(releases) if releases.iter().all(|release| release.is_draft) => {
                        card = card.child(
                            div()
                                .text_size(px(12.))
                                .text_color(cx.theme().muted_foreground)
                                .child("No published releases returned by the provider."),
                        )
                    }
                    Ok(releases) => {
                        if let Some((newer, older)) = newer_release_with_local_older(
                            project_directory,
                            &repository.owner,
                            &repository.repository,
                            releases,
                        ) {
                            card = card.child(div().p_3().rounded_lg().bg(cx.theme().primary.opacity(0.08)).text_size(px(12.)).child(format!("Newer release {newer} is available · local files exist for {older}")));
                        }
                        for (rel_index, release) in releases
                            .iter()
                            .filter(|release| !release.is_draft)
                            .enumerate()
                        {
                            let mut entry = div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .pt_3()
                                .border_t_1()
                                .border_color(cx.theme().border)
                                .child(div().font_bold().child(format!(
                                    "{}{}",
                                    release.tag_name,
                                    if release.is_prerelease == Some(true) {
                                        "  ·  Pre-release"
                                    } else {
                                        ""
                                    }
                                )));
                            if let Some(date) = &release.published_at {
                                entry = entry.child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("Release date {date}")),
                                );
                            }
                            if let Some(notes) = &release.description {
                                if !notes.trim().is_empty() {
                                    entry =
                                        entry.child(div().text_size(px(12.)).child(notes.clone()));
                                }
                            }
                            if release.assets.is_empty() {
                                entry = entry.child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child("No downloadable assets."),
                                );
                            }
                            for (asset_index, asset) in release.assets.iter().enumerate() {
                                let local = local_asset_state(
                                    project_directory,
                                    &repository.owner,
                                    &repository.repository,
                                    &release.tag_name,
                                    &asset.name,
                                    asset.size,
                                );
                                let local_label = match local {
                                    LocalAssetState::Missing => "Not downloaded",
                                    LocalAssetState::SizeMatches => {
                                        "Local file size matches · contents unverified"
                                    }
                                    LocalAssetState::SizeDiffers => {
                                        "Local file has a different size"
                                    }
                                    LocalAssetState::PresentSizeUnknown => {
                                        "Local file exists · size and contents unverified"
                                    }
                                    LocalAssetState::Unavailable => "Local file status unavailable",
                                };
                                let provider = repository.provider.clone();
                                let owner = repository.owner.clone();
                                let repo = repository.repository.clone();
                                let tag = release.tag_name.clone();
                                let name = asset.name.clone();
                                let url = asset.download_url.clone();
                                let size = asset.size;
                                entry = entry.child(
                                    div()
                                        .flex()
                                        .flex_wrap()
                                        .justify_between()
                                        .items_center()
                                        .gap_3()
                                        .py_2()
                                        .border_t_1()
                                        .border_color(cx.theme().border)
                                        .child(
                                            div()
                                                .min_w_0()
                                                .flex()
                                                .flex_col()
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .text_size(px(13.))
                                                        .font_bold()
                                                        .child(asset.name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(11.))
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(format!(
                                                            "{} · {}",
                                                            asset
                                                                .size
                                                                .map(|size| format!(
                                                                    "{:.1} MB",
                                                                    size as f64 / 1_000_000.0
                                                                ))
                                                                .unwrap_or_else(
                                                                    || "Size unknown".into()
                                                                ),
                                                            local_label
                                                        )),
                                                ),
                                        )
                                        .child(
                                            Button::new(format!(
                                                "asset-{repo_index}-{rel_index}-{asset_index}"
                                            ))
                                            .icon(download_icon())
                                            .label("Download")
                                            .outline()
                                            .disabled(busy || local != LocalAssetState::Missing)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                on_download(
                                                    provider.clone(),
                                                    owner.clone(),
                                                    repo.clone(),
                                                    tag.clone(),
                                                    name.clone(),
                                                    url.clone(),
                                                    size,
                                                    this,
                                                    window,
                                                    cx,
                                                )
                                            })),
                                        ),
                                );
                            }
                            card = card.child(entry);
                        }
                    }
                }
                body = body.child(card);
            }
        }
    }
    body
}
