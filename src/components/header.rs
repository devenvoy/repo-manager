//! Compact workspace header shared by Projects and App Store with appearance toggle.

use super::toggle_button;
use crate::settings::Appearance;
use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{Context, IntoElement, ParentElement as _, Styled as _, Window, div, px};

pub fn render_header<
    T: 'static,
    FTheme: Fn(&mut T, Appearance, &Window, &mut Context<T>) + Copy + 'static,
>(
    location: &str,
    status: &str,
    current_theme: Appearance,
    cx: &Context<T>,
    on_theme: FTheme,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .px_5()
        .py_3()
        .bg(cx.theme().secondary)
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(cx.theme().muted_foreground)
                        .child("Workspace  ›"),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .font_bold()
                        .text_color(cx.theme().foreground)
                        .child(location.to_string()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(cx.theme().muted_foreground)
                        .child(status.to_string()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .p_1()
                        .rounded_lg()
                        .bg(cx.theme().muted)
                        .child(
                            toggle_button(
                                "theme-light",
                                "Light",
                                current_theme == Appearance::Light,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    on_theme(this, Appearance::Light, window, cx);
                                },
                            )),
                        )
                        .child(
                            toggle_button("theme-dark", "Dark", current_theme == Appearance::Dark)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    on_theme(this, Appearance::Dark, window, cx);
                                })),
                        )
                        .child(
                            toggle_button(
                                "theme-system",
                                "Auto",
                                current_theme == Appearance::System,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    on_theme(this, Appearance::System, window, cx);
                                },
                            )),
                        ),
                ),
        )
}
