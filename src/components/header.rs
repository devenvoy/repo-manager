//! Window titlebar and header status banner.

use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{Context, IntoElement, ParentElement as _, Styled as _, div};

pub fn render_header<T: 'static>(
    project_name: &str,
    status_message: &str,
    cx: &Context<T>,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .p_3()
        .bg(cx.theme().muted)
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div().flex().gap_2().items_center().child(
                div()
                    .font_bold()
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(format!("Project: {}", project_name)),
            ),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(status_message.to_string()),
        )
}
