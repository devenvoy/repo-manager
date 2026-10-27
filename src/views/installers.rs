//! Release installer shelf and asset overview view.

use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{Context, IntoElement, ParentElement as _, Styled as _, div, px};
use release_desk_core::latest_installer;
use release_desk_core::snapshot::Repository;

pub fn render_installers<T: 'static>(
    repos: &[Repository],
    include_prereleases: bool,
    extension: &str,
    cx: &Context<T>,
) -> impl IntoElement {
    let mut shelf = div()
        .flex()
        .flex_col()
        .gap_4()
        .p_6()
        .overflow_y_scrollbar()
        .size_full()
        .child(
            div()
                .font_bold()
                .text_size(px(24.))
                .child("Installer Shelf"),
        )
        .child(
            "Eligible releases across all repositories. Downloads and cache live safely in the managed library.",
        );

    let mut count = 0;
    for repo in repos {
        if let Some((release, assets)) = latest_installer(repo, include_prereleases, extension) {
            count += 1;
            shelf = shelf.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_4()
                    .rounded_lg()
                    .bg(cx.theme().secondary)
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .child(
                                div()
                                    .font_bold()
                                    .child(format!("{}  ·  {}", repo.name, release.tag_name)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        release
                                            .published_at
                                            .as_deref()
                                            .unwrap_or(&release.created_at)
                                            .to_string(),
                                    ),
                            ),
                    )
                    .child(div().flex().gap_3().children(assets.iter().map(|asset| {
                        div()
                            .p_2()
                            .rounded_md()
                            .bg(cx.theme().muted)
                            .text_xs()
                            .child(format!("{} ({} MB)", asset.name, asset.size / 1_000_000))
                    }))),
            );
        }
    }

    if count == 0 {
        shelf = shelf.child("No installer releases found matching current filter.");
    }

    shelf
}
