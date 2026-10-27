//! Pull requests inspection view.

use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{Context, IntoElement, ParentElement as _, Styled as _, div, px};

pub fn render_pull_requests<T: 'static>(cx: &Context<T>) -> impl IntoElement {
    let prs = [
        (
            "#42",
            "[OPEN]",
            "Implement safe fast-forward sync guard",
            "dev-core",
        ),
        (
            "#41",
            "[MERGED]",
            "Support multi-host OAuth and token authentication",
            "dev-auth",
        ),
        (
            "#40",
            "[CLOSED]",
            "Deprecated monolithic storage script",
            "dev-arch",
        ),
    ];

    div()
        .flex()
        .flex_col()
        .gap_4()
        .p_6()
        .overflow_y_scrollbar()
        .size_full()
        .child(div().font_bold().text_size(px(24.)).child("Pull Requests"))
        .child("Tracked pull requests across active repositories.")
        .child(div().flex().flex_col().gap_2().children(prs.iter().map(
            |(num, status, title, author)| {
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .p_3()
                    .rounded_lg()
                    .bg(cx.theme().secondary)
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(div().font_bold().child(*num))
                            .child(div().font_bold().child(*status))
                            .child(title.to_string()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("Author: {}", author)),
                    )
            },
        )))
}
