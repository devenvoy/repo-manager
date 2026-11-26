//! Name-confirmation dialog for destructive repository actions.

use crate::components::motion_button::Button;
use crate::views::project_repository_status::RepositoryAction;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    Context, Entity, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _,
    Styled as _, Window, div, px,
};

pub(crate) fn render<T: 'static>(
    repository: &str,
    action: RepositoryAction,
    contents_preview: Option<&[String]>,
    input: &Entity<InputState>,
    cx: &Context<T>,
    on_cancel: impl Fn(&mut T, &mut Context<T>) + Copy + 'static,
    on_confirm: impl Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    let is_non_git_trash = contents_preview.is_some();
    let title = if action == RepositoryAction::RemoveStaleLocks {
        "Remove stale Git lock files?"
    } else if is_non_git_trash {
        "Move incomplete folder to Trash?"
    } else {
        "Delete repository checkout?"
    };
    let action_text = if action == RepositoryAction::RemoveStaleLocks {
        "delete detected .git lock files (index.lock, FETCH_HEAD.lock, shallow.lock, or refs locks). Only proceed if no Git operation is running"
    } else if is_non_git_trash {
        "move the existing non-repository folder (including user files) to the macOS Trash"
    } else if action == RepositoryAction::DeleteFromDisk {
        "delete the checkout from disk. This cannot be undone"
    } else {
        "remove it from this project and delete the checkout from disk. This cannot be undone"
    };
    let confirm_label = if action == RepositoryAction::RemoveStaleLocks {
        "Remove stale locks"
    } else if is_non_git_trash {
        "Move to Trash"
    } else {
        "Delete checkout"
    };
    let mut modal = div()
        .w(px(500.))
        .occlude()
        .p_6()
        .rounded_xl()
        .border_1()
        .border_color(cx.theme().danger.opacity(0.55))
        .bg(cx.theme().secondary)
        .flex()
        .flex_col()
        .gap_4()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .child(div().font_bold().text_size(px(20.)).child(title))
        .child(
            div()
                .text_size(px(13.))
                .text_color(cx.theme().muted_foreground)
                .child(format!(
                    "This will {action_text}. Type {repository} to confirm."
                )),
        );
    if let Some(items) = contents_preview.filter(|items| !items.is_empty()) {
        let preview_height = (items.len().max(1) as f32 * 22.0 + 44.0).clamp(72.0, 200.0);
        modal = modal.child(
            div()
                .h(px(preview_height))
                .overflow_y_scrollbar()
                .p_3()
                .pb_4()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background.opacity(0.5))
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(px(11.))
                        .font_bold()
                        .text_color(cx.theme().muted_foreground)
                        .child("Folder contents:"),
                )
                .children(items.iter().map(|entry| {
                    div()
                        .text_size(px(12.))
                        .text_color(cx.theme().foreground)
                        .child(entry.clone())
                })),
        );
    }
    let modal = modal.child(Input::new(input)).child(
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
                    .label(confirm_label)
                    .danger()
                    .on_click(cx.listener(move |this, _, window, cx| on_confirm(this, window, cx))),
            ),
    );
    div()
        .occlude()
        .absolute()
        .inset_0()
        .bg(cx.theme().background.opacity(0.72))
        .flex()
        .items_center()
        .justify_center()
        .p_5()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .child(modal)
}
