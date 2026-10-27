//! Window titlebar and header status banner with bidirectional view switcher.

use crate::state::WorkspaceTab;
use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{Context, IntoElement, ParentElement as _, Styled as _, div, px};

pub fn render_header<T: 'static>(
    project_name: &str,
    status_message: &str,
    active_tab: WorkspaceTab,
    updates_count: usize,
    pulls_count: usize,
    cx: &Context<T>,
    on_tab_switch: impl Fn(WorkspaceTab, &mut T, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    let store_badge = if updates_count > 0 {
        format!("App Store ({updates_count} updates)")
    } else {
        "App Store".to_string()
    };

    let hub_badge = if pulls_count > 0 {
        format!("Project Hub ({pulls_count} pulls)")
    } else {
        "Project Hub".to_string()
    };

    div()
        .flex()
        .items_center()
        .justify_between()
        .px_4()
        .py_2p5()
        .bg(cx.theme().secondary)
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .flex()
                .items_center()
                .gap_4()
                .child(
                    div()
                        .font_bold()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(format!("Release Desk • {}", project_name)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .p_1()
                        .rounded_lg()
                        .bg(cx.theme().muted)
                        .child(if active_tab == WorkspaceTab::AppStore {
                            Button::new("nav-store")
                                .label(store_badge)
                                .primary()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    on_tab_switch(WorkspaceTab::AppStore, this, cx);
                                }))
                        } else {
                            Button::new("nav-store")
                                .label(store_badge)
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    on_tab_switch(WorkspaceTab::AppStore, this, cx);
                                }))
                        })
                        .child(if active_tab == WorkspaceTab::ProjectHub {
                            Button::new("nav-hub")
                                .label(hub_badge)
                                .primary()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    on_tab_switch(WorkspaceTab::ProjectHub, this, cx);
                                }))
                        } else {
                            Button::new("nav-hub")
                                .label(hub_badge)
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    on_tab_switch(WorkspaceTab::ProjectHub, this, cx);
                                }))
                        }),
                ),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(cx.theme().muted_foreground)
                .child(status_message.to_string()),
        )
}
