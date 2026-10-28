//! Overview dashboard view summarizing project status and release metrics.

use crate::components::motion_button::Button;
use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{Context, IntoElement, ParentElement as _, Styled as _, div, px};

/// Aggregated project statistics for the overview dashboard.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OverviewMetrics {
    pub repos_count: usize,
    pub releases_count: usize,
    pub with_installers_count: usize,
    pub archived_count: usize,
    pub backup_synced: bool,
}

pub fn render_overview<T: 'static>(
    project_name: &str,
    metrics: OverviewMetrics,
    cx: &Context<T>,
    on_explore: impl Fn(&mut T, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_5()
        .p_6()
        .overflow_y_scrollbar()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_6()
                .rounded_lg()
                .bg(cx.theme().accent)
                .text_color(cx.theme().accent_foreground)
                .child(
                    div()
                        .text_size(px(28.))
                        .font_bold()
                        .child(format!("{} overview", project_name)),
                )
                .child("Release activity, installer shelf, and local data.json backup redundancy."),
        )
        .child(
            div()
                .flex()
                .gap_4()
                .child(metric_card("Repositories", metrics.repos_count, cx))
                .child(metric_card("Releases", metrics.releases_count, cx))
                .child(metric_card(
                    "With Installers",
                    metrics.with_installers_count,
                    cx,
                ))
                .child(metric_card("Archived", metrics.archived_count, cx))
                .child(backup_card(metrics.backup_synced, cx)),
        )
        .child(
            Button::new("browse-repositories")
                .primary()
                .label("Explore repositories")
                .on_click(cx.listener(move |this, _, _, cx| on_explore(this, cx))),
        )
}

fn metric_card<T: 'static>(label: &'static str, value: usize, cx: &Context<T>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_4()
        .flex_1()
        .bg(cx.theme().secondary)
        .text_color(cx.theme().foreground)
        .rounded_lg()
        .child(
            div()
                .text_size(px(28.))
                .font_bold()
                .child(value.to_string()),
        )
        .child(div().text_sm().child(label))
}

fn backup_card<T: 'static>(synced: bool, cx: &Context<T>) -> impl IntoElement {
    let (status_text, subtext) = if synced {
        ("IN SYNC", "Local data.json matches DB")
    } else {
        ("MISMATCH", "Check Backup & Reconcile")
    };

    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_4()
        .flex_1()
        .bg(cx.theme().secondary)
        .text_color(cx.theme().foreground)
        .rounded_lg()
        .child(div().text_size(px(20.)).font_bold().child(status_text))
        .child(div().text_sm().child(subtext))
}
