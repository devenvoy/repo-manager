//! Floating pull queue snackbar and queue detail dialog.

use crate::components::motion_button::Button;
use crate::git_update_queue::{GitUpdateJob, GitUpdateStatus};
use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};

pub(crate) fn render<T: 'static>(
    queue: &[GitUpdateJob],
    open: bool,
    cx: &Context<T>,
    on_toggle: impl Fn(&mut T, &mut Context<T>) + Copy + 'static,
    on_cancel: impl Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    on_cancel_all: impl Fn(&mut T, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    let total = queue.len();
    let finished = queue.iter().filter(|job| job.status.is_terminal()).count();
    let active = total.saturating_sub(finished);
    let cancellable = queue
        .iter()
        .filter(|job| {
            job.status.is_active()
                && !matches!(&job.status, GitUpdateStatus::Running(stage) if stage == "Applying fast-forward")
        })
        .count();
    let ratio = if total == 0 {
        0.0
    } else {
        finished as f32 / total as f32
    };

    let mut root = div()
        .absolute()
        .inset_0()
        .items_end()
        .justify_end()
        .p_5()
        .flex()
        .flex_col()
        .gap_2();
    if total > 0 {
        root = root.child(
            Button::new("git-update-queue-snackbar")
                .label(if active > 0 {
                    format!("Updating {active} repositories · {finished}/{total} complete")
                } else {
                    format!("Repository updates finished · {finished}/{total}")
                })
                .primary()
                .on_click(cx.listener(move |this, _, _, cx| on_toggle(this, cx))),
        );
    }

    if open {
        let mut rows = div()
            .flex()
            .flex_col()
            .gap_2()
            .max_h(px(440.))
            .overflow_y_scrollbar();
        for job in queue {
            let key = job.key.clone();
            let state = match &job.status {
                GitUpdateStatus::Queued => "Waiting".to_string(),
                GitUpdateStatus::Running(stage) => stage.clone(),
                GitUpdateStatus::Complete(message) => message.clone(),
                GitUpdateStatus::Skipped(message) => format!("Skipped · {message}"),
                GitUpdateStatus::Failed(error) => format!("Failed · {error}"),
                GitUpdateStatus::Cancelled => "Cancelled".to_string(),
            };
            let mut row = div()
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .p_3()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .font_medium()
                                .text_size(px(13.))
                                .child(job.repository.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child(state),
                        ),
                );
            let cancellable = job.status.is_active()
                && !matches!(&job.status, GitUpdateStatus::Running(stage) if stage == "Applying fast-forward");
            if cancellable {
                row = row.child(
                    Button::new(format!("cancel-git-update-{}", job.key))
                        .label("Cancel")
                        .outline()
                        .on_click(
                            cx.listener(move |this, _, _, cx| on_cancel(key.clone(), this, cx)),
                        ),
                );
            }
            rows = rows.child(row);
        }

        let mut actions = div().flex().items_center().justify_between().gap_3();
        actions = actions.child(
            div()
                .text_size(px(12.))
                .text_color(cx.theme().muted_foreground)
                .child(if active == 0 {
                    format!("{finished} of {total} jobs finished")
                } else {
                    format!("Updating {active} repositories · {finished}/{total} finished")
                }),
        );
        if cancellable > 0 {
            actions = actions.child(
                Button::new("cancel-all-git-updates")
                    .label("Cancel remaining")
                    .outline()
                    .on_click(cx.listener(move |this, _, _, cx| on_cancel_all(this, cx))),
            );
        }

        root = root.child(
            div()
                .id("git-update-queue-backdrop")
                .absolute()
                .inset_0()
                .bg(cx.theme().background.opacity(0.55))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(move |this, _, _, cx| on_toggle(this, cx)))
                .child(
                    div()
                        .id("git-update-queue-dialog")
                        .w(px(560.))
                        .max_h(px(560.))
                        .flex()
                        .flex_col()
                        .gap_3()
                        .rounded_2xl()
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().background)
                        .shadow_lg()
                        .p_5()
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(17.))
                                        .font_bold()
                                        .child("Repository update queue"),
                                )
                                .child(
                                    Button::new("close-git-update-queue")
                                        .label("Close")
                                        .ghost()
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| on_toggle(this, cx)),
                                        ),
                                ),
                        )
                        .child(actions)
                        .child(
                            div()
                                .h(px(5.))
                                .w_full()
                                .rounded_full()
                                .bg(cx.theme().secondary.opacity(0.45))
                                .child(
                                    div()
                                        .h(px(5.))
                                        .rounded_full()
                                        .bg(cx.theme().primary)
                                        .w(px(510. * ratio)),
                                ),
                        )
                        .child(rows),
                ),
        );
    }
    root
}
