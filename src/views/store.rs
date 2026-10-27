//! App Store grid and full detail page for the desktop design preview.

use crate::components::app_icon::app_icon;
use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use release_desk_core::app_catalog::{AppActionState, AppStoreItem};

pub fn render_store<
    T: 'static,
    FSelect: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    FAction: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    FBack: Fn(&mut T, &mut Context<T>) + Copy + 'static,
>(
    items: &[AppStoreItem],
    selected_app_id: Option<&str>,
    cx: &Context<T>,
    on_select: FSelect,
    on_action: FAction,
    on_back: FBack,
) -> impl IntoElement {
    if let Some(item) =
        selected_app_id.and_then(|id| items.iter().find(|item| item.metadata.id == id))
    {
        return render_detail(item, cx, on_action, on_back).into_any_element();
    }

    let mut grid = div().flex().flex_wrap().gap_4();
    for item in items {
        grid = grid.child(render_card(item, cx, on_select, on_action));
    }

    div()
        .flex()
        .flex_col()
        .gap_6()
        .p_6()
        .overflow_y_scrollbar()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().text_size(px(11.)).font_bold().text_color(cx.theme().primary).child("LOCAL APP STORE"))
                .child(div().text_size(px(27.)).font_bold().child("Apps from every project."))
                .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground).child("Installable releases from selected repositories will appear together here.")),
        )
        .child(if items.is_empty() {
            div()
                .p_8()
                .rounded_xl()
                .bg(cx.theme().secondary)
                .border_1()
                .border_color(cx.theme().border)
                .flex()
                .flex_col()
                .items_start()
                .gap_4()
                .child(div().text_size(px(11.)).font_bold().text_color(cx.theme().primary).child("LOCAL APP STORE"))
                .child(div().text_size(px(22.)).font_bold().child("No runnable applications yet."))
                .child(div().text_size(px(13.)).text_color(cx.theme().muted_foreground).child("Applications with release builds across all your projects appear together in this store. Create a project and track repositories with published releases to install, update, and launch them here."))
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap_4()
                .child(div().text_size(px(17.)).font_bold().child(format!("All applications  ·  {}", items.len())))
                .child(grid)
                .into_any_element()
        })
        .into_any_element()
}

fn render_card<
    T: 'static,
    FSelect: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    FAction: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
>(
    item: &AppStoreItem,
    cx: &Context<T>,
    on_select: FSelect,
    on_action: FAction,
) -> impl IntoElement {
    let id_for_card = item.metadata.id.clone();
    let id_for_action = item.metadata.id.clone();
    let action = action_label(&item.state);
    div()
        .id(format!("app-card-{}", item.metadata.id))
        .cursor_pointer()
        .w(px(268.))
        .p_4()
        .rounded_xl()
        .bg(cx.theme().secondary)
        .border_1()
        .border_color(cx.theme().border)
        .flex()
        .flex_col()
        .gap_3()
        .on_click(cx.listener(move |this, _, _, cx| on_select(id_for_card.clone(), this, cx)))
        .child(app_icon(&item.metadata.id))
        .child(
            div()
                .text_size(px(10.))
                .font_bold()
                .text_color(cx.theme().muted_foreground)
                .child(item.metadata.category_name.to_uppercase()),
        )
        .child(
            div()
                .text_size(px(16.))
                .font_bold()
                .child(item.metadata.name.clone()),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(cx.theme().muted_foreground)
                .child(item.metadata.description.clone()),
        )
        .child(
            div()
                .text_size(px(10.))
                .text_color(cx.theme().primary)
                .child(format!(
                    "{} · {}",
                    item.latest_release_tag.as_deref().unwrap_or("No release"),
                    item.project_name
                )),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pt_2()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(div().text_size(px(11.)).child("View details  →"))
                .child(
                    Button::new(format!("action-{}", item.metadata.id))
                        .label(action)
                        .primary()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            on_action(id_for_action.clone(), this, cx);
                        })),
                ),
        )
}

fn render_detail<
    T: 'static,
    FAction: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    FBack: Fn(&mut T, &mut Context<T>) + Copy + 'static,
>(
    item: &AppStoreItem,
    cx: &Context<T>,
    on_action: FAction,
    on_back: FBack,
) -> impl IntoElement {
    let id = item.metadata.id.clone();
    div()
        .flex()
        .flex_col()
        .gap_5()
        .p_6()
        .overflow_y_scrollbar()
        .child(Button::new("back-to-store").label("← All applications").ghost().on_click(cx.listener(move |this, _, _, cx| on_back(this, cx))))
        .child(
            div()
                .p_5()
                .rounded_xl()
                .bg(cx.theme().secondary)
                .flex()
                .items_center()
                .gap_5()
                .child(app_icon(&item.metadata.id))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(div().text_size(px(26.)).font_bold().child(item.metadata.name.clone()))
                        .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground).child(item.metadata.description.clone()))
                        .child(div().text_size(px(11.)).child(format!("{} · {} · {}", item.project_name, item.latest_release_tag.as_deref().unwrap_or("No release"), item.metadata.size_estimate))),
                )
                .child(Button::new("detail-action").label(action_label(&item.state)).primary().on_click(cx.listener(move |this, _, _, cx| on_action(id.clone(), this, cx)))),
        )
        .child(div().text_size(px(18.)).font_bold().child("Release details"))
        .child(div().p_5().rounded_xl().bg(cx.theme().secondary).text_size(px(12.)).child("Sample release tags and notes illustrate the layout. Live assets will come from the project's connected provider."))
        .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground).child("macOS · Windows · Linux release formats will be listed here when available."))
}

fn action_label(state: &AppActionState) -> &'static str {
    match state {
        AppActionState::Installable => "Install",
        AppActionState::UpdateAvailable { .. } => "Update",
        AppActionState::UpToDate { .. } => "Open",
    }
}
