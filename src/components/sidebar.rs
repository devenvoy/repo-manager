//! Navigation sidebar component for the desktop GUI.

use crate::state::WorkspaceTab;
use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{Context, IntoElement, ParentElement as _, Styled as _, div, px};

pub fn render_sidebar<T: 'static>(
    current_tab: WorkspaceTab,
    ready: bool,
    cx: &Context<T>,
    on_select_tab: impl Fn(&mut T, WorkspaceTab, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    let mut sidebar = div()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .w(px(210.))
        .h_full()
        .bg(cx.theme().secondary)
        .child(
            div()
                .font_bold()
                .text_sm()
                .text_color(cx.theme().foreground)
                .child("RELEASE DESK"),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(if ready { "PROJECT WORKSPACE" } else { "SETUP" }),
        );

    let tabs = [
        (WorkspaceTab::Overview, "overview", "Overview"),
        (WorkspaceTab::Repositories, "repositories", "Repositories"),
        (WorkspaceTab::Installers, "installers", "Installers"),
        (WorkspaceTab::PullRequests, "pull-requests", "Pull Requests"),
        (WorkspaceTab::Reconcile, "reconcile", "Backup & Reconcile"),
        (WorkspaceTab::Settings, "settings", "Settings"),
    ];

    for (tab, id, label) in tabs {
        let is_active = current_tab == tab;
        let button_label = if is_active {
            format!("● {}", label)
        } else {
            label.to_string()
        };

        let mut btn = Button::new(id).label(button_label);
        if is_active {
            btn = btn.primary();
        }

        sidebar = sidebar.child(btn.on_click(cx.listener(move |this, _, _, cx| {
            if ready {
                on_select_tab(this, tab, cx);
            }
        })));
    }

    sidebar
}
