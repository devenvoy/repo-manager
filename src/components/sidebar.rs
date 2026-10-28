//! Navigation sidebar component for Release Desk matching the modern design prototype.

use crate::components::motion_button::Button;
use crate::components::ui_icons::{brand_logo, grid_icon, settings_icon, store_icon};
use crate::state::WorkspaceTab;
use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::{Context, IntoElement, ParentElement as _, Styled as _, div, px};

pub fn render_sidebar<T: 'static>(
    current_tab: WorkspaceTab,
    projects_count: usize,
    cx: &Context<T>,
    on_select_tab: impl Fn(&mut T, WorkspaceTab, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    let mut sidebar = div()
        .flex()
        .flex_col()
        .gap_4()
        .p_4()
        .w(px(220.))
        .h_full()
        .bg(cx.theme().secondary)
        .border_r_1()
        .border_color(cx.theme().border)
        // Brand Header
        .child(
            div()
                .flex()
                .items_center()
                .gap_2p5()
                .px_1()
                .py_2()
                .child(
                    div()
                        .w(px(28.))
                        .h(px(28.))
                        .rounded_lg()
                        .bg(cx.theme().primary)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(brand_logo()),
                )
                .child(
                    div()
                        .font_bold()
                        .text_size(px(15.))
                        .text_color(cx.theme().foreground)
                        .child("Release Desk"),
                ),
        )
        // Workspace Label
        .child(
            div()
                .text_size(px(10.))
                .font_bold()
                .text_color(cx.theme().muted_foreground)
                .px_2()
                .child("WORKSPACE"),
        );

    // Nav Items (Projects, App Store, Settings)
    let tabs = [
        (
            WorkspaceTab::ProjectHub,
            "nav-projects",
            "Projects",
            Some(projects_count),
        ),
        (WorkspaceTab::AppStore, "nav-store", "App Store", None),
        (WorkspaceTab::Settings, "nav-settings", "Settings", None),
    ];

    let mut nav_group = div().flex().flex_col().gap_1();

    for (tab, id, label, count) in tabs {
        let is_active = current_tab == tab;
        let mut btn = Button::new(id).label(label.to_string());

        btn = match tab {
            WorkspaceTab::ProjectHub => btn.icon(grid_icon()),
            WorkspaceTab::AppStore => btn.icon(store_icon()),
            _ => btn.icon(settings_icon()),
        };

        if is_active {
            btn = btn.primary();
        } else {
            btn = btn.ghost();
        }

        let nav_row = div()
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .child(btn.on_click(cx.listener(move |this, _, _, cx| {
                on_select_tab(this, tab, cx);
            })));

        let nav_row = if let Some(n) = count {
            nav_row.child(
                div()
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .bg(cx.theme().muted)
                    .text_size(px(10.))
                    .font_bold()
                    .text_color(cx.theme().muted_foreground)
                    .child(n.to_string()),
            )
        } else {
            nav_row
        };

        nav_group = nav_group.child(nav_row);
    }

    sidebar = sidebar.child(nav_group);
    sidebar
}
