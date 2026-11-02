//! Name-confirmation dialog for destructive repository actions.

use crate::components::motion_button::Button;
use crate::views::project_repository_status::RepositoryAction;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::{Context, Entity, IntoElement, ParentElement as _, Styled as _, Window, div, px};

pub(crate) fn render<T: 'static>(
    repository: &str,
    action: RepositoryAction,
    input: &Entity<InputState>,
    cx: &Context<T>,
    on_cancel: impl Fn(&mut T, &mut Context<T>) + Copy + 'static,
    on_confirm: impl Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    let action_text = if action == RepositoryAction::DeleteFromDisk {
        "delete the checkout from disk"
    } else {
        "remove it from this project and delete the checkout from disk"
    };
    div()
        .absolute()
        .inset_0()
        .bg(cx.theme().background.opacity(0.72))
        .flex()
        .items_center()
        .justify_center()
        .p_5()
        .child(
            div()
                .w(px(480.))
                .p_6()
                .rounded_xl()
                .border_1()
                .border_color(cx.theme().danger.opacity(0.55))
                .bg(cx.theme().secondary)
                .flex()
                .flex_col()
                .gap_4()
                .child(div().font_bold().text_size(px(20.)).child("Delete repository checkout?"))
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("This will {action_text}. This cannot be undone. Type {repository} to confirm.")),
                )
                .child(Input::new(input))
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("cancel-repository-delete")
                                .label("Cancel")
                                .outline()
                                .on_click(cx.listener(move |this, _, _, cx| on_cancel(this, cx))),
                        )
                        .child(
                            Button::new("confirm-repository-delete")
                                .label("Delete checkout")
                                .danger()
                                .on_click(cx.listener(move |this, _, window, cx| on_confirm(this, window, cx))),
                        ),
                ),
        )
}
