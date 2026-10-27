//! Master-detail repository browser view.

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::Button;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{Context, IntoElement, ParentElement as _, Styled as _, div, px};
use release_desk_core::latest_installer;
use release_desk_core::snapshot::Repository;

pub fn render_repositories<T: 'static>(
    repos: &[Repository],
    selected_index: Option<usize>,
    show_archived: bool,
    include_prereleases: bool,
    extension: &str,
    cx: &Context<T>,
    on_select: impl Fn(&mut T, usize, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    let mut list = div()
        .flex()
        .flex_col()
        .gap_1()
        .w(px(280.))
        .h_full()
        .overflow_y_scrollbar()
        .p_3();

    for (index, repo) in repos.iter().enumerate() {
        if repo.is_archived && !show_archived {
            continue;
        }
        let is_selected = Some(index) == selected_index;
        let label = if is_selected {
            format!("▶ {}", repo.name)
        } else {
            repo.name.clone()
        };

        list = list.child(
            Button::new(format!("repo-{index}"))
                .label(label)
                .on_click(cx.listener(move |this, _, _, cx| on_select(this, index, cx))),
        );
    }

    let detail = render_repo_detail(
        selected_index.and_then(|i| repos.get(i)),
        include_prereleases,
        extension,
    );

    div()
        .flex()
        .size_full()
        .child(list)
        .child(div().flex_1().h_full().overflow_y_scrollbar().child(detail))
}

fn render_repo_detail(
    repo_opt: Option<&Repository>,
    include_prereleases: bool,
    extension: &str,
) -> impl IntoElement {
    let mut detail = div().flex().flex_col().gap_3().p_6();
    let Some(repo) = repo_opt else {
        return detail.child("Select a repository from the list on the left.");
    };

    detail = detail
        .child(
            div()
                .font_bold()
                .text_size(px(22.))
                .child(repo.name.clone()),
        )
        .child(repo.description.clone().unwrap_or_default())
        .child(format!("URL: {}", repo.url))
        .child(format!(
            "Visibility: {}",
            if repo.is_private { "Private" } else { "Public" }
        ))
        .child(format!(
            "Default branch: {}",
            repo.default_branch_ref
                .as_ref()
                .map_or("Unknown", |branch| &branch.name)
        ))
        .child(format!(
            "Pushed: {}",
            repo.pushed_at.as_deref().unwrap_or("Unknown")
        ));

    if let Some((release, assets)) = latest_installer(repo, include_prereleases, extension) {
        detail = detail
            .child(format!("Latest installer release: {}", release.tag_name))
            .child(format!(
                "Published: {}",
                release
                    .published_at
                    .as_deref()
                    .unwrap_or(&release.created_at)
            ))
            .child(format!("Release URL: {}", release.url));

        for asset in assets {
            detail = detail.child(format!("  • {} ({} bytes)", asset.name, asset.size));
        }
    } else {
        detail = detail.child(format!("No .{} installer release found.", extension));
    }

    detail
}
