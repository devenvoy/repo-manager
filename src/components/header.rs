//! Workspace topbar header with clickable breadcrumbs, back/forward history navigation, and status.

use crate::components::ui_icons::{arrow_left_icon, arrow_right_icon};
use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};

#[allow(clippy::too_many_arguments)]
pub fn render_header<
    T: 'static,
    FBack: Fn(&mut T, &mut Context<T>) + Copy + 'static,
    FForward: Fn(&mut T, &mut Context<T>) + Copy + 'static,
    FNavWorkspace: Fn(&mut T, &mut Context<T>) + Copy + 'static,
    FNavProject: Fn(&mut T, &mut Context<T>) + Copy + 'static,
>(
    can_go_back: bool,
    can_go_forward: bool,
    location_title: &str,
    project_name: Option<&str>,
    status: &str,
    cx: &Context<T>,
    on_back: FBack,
    on_forward: FForward,
    on_nav_workspace: FNavWorkspace,
    on_nav_project: FNavProject,
) -> impl IntoElement {
    let mut back_btn = Button::new("nav-back").icon(arrow_left_icon());
    if can_go_back {
        back_btn = back_btn
            .ghost()
            .on_click(cx.listener(move |this, _, _, cx| on_back(this, cx)));
    } else {
        back_btn = back_btn.ghost().disabled(true);
    }

    let mut fwd_btn = Button::new("nav-fwd").icon(arrow_right_icon());
    if can_go_forward {
        fwd_btn = fwd_btn
            .ghost()
            .on_click(cx.listener(move |this, _, _, cx| on_forward(this, cx)));
    } else {
        fwd_btn = fwd_btn.ghost().disabled(true);
    }

    let mut crumbs = div().flex().items_center().gap_2().min_w_0();

    // Clickable "Workspace" root
    crumbs = crumbs.child(
        div()
            .id("crumb-workspace")
            .cursor_pointer()
            .text_size(px(12.))
            .text_color(cx.theme().muted_foreground)
            .hover(|s| s.text_color(cx.theme().foreground))
            .on_click(cx.listener(move |this, _, _, cx| on_nav_workspace(this, cx)))
            .child("Workspace"),
    );

    crumbs = crumbs.child(
        div()
            .text_size(px(12.))
            .text_color(cx.theme().muted_foreground)
            .child("›"),
    );

    // If inside a project, add clickable project breadcrumb
    if let Some(name) = project_name {
        crumbs = crumbs.child(
            div()
                .id("crumb-project")
                .cursor_pointer()
                .text_size(px(12.))
                .text_color(cx.theme().muted_foreground)
                .hover(|s| s.text_color(cx.theme().foreground))
                .on_click(cx.listener(move |this, _, _, cx| on_nav_project(this, cx)))
                .child(name.to_string()),
        );

        crumbs = crumbs.child(
            div()
                .text_size(px(12.))
                .text_color(cx.theme().muted_foreground)
                .child("›"),
        );
    }

    // Current page title (bold)
    crumbs = crumbs.child(
        div()
            .text_size(px(13.))
            .font_bold()
            .text_color(cx.theme().foreground)
            .child(location_title.to_string()),
    );

    div()
        .flex()
        .items_center()
        .justify_between()
        .px_5()
        .py_3()
        .h(px(52.))
        .bg(cx.theme().secondary)
        .border_b_1()
        .border_color(cx.theme().border)
        .w_full()
        .min_w_0()
        // Left side: Nav arrows + Breadcrumbs
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .min_w_0()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(back_btn)
                        .child(fwd_btn),
                )
                .child(crumbs),
        )
        // Right side: Status message (constrained, no clipping, no theme toggle)
        .child(
            div()
                .flex()
                .items_center()
                .max_w(px(360.))
                .overflow_hidden()
                .text_size(px(11.))
                .text_color(cx.theme().muted_foreground)
                .child(status.to_string()),
        )
}
