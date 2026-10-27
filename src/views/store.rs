//! Local App Store view featuring hero spotlight card, bento grid, squircle icons,
//! and expansive detail view with 3-state action buttons.

use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{Context, IntoElement, ParentElement as _, Styled as _, div, px};
use release_desk_application::use_case::app_store::AppStoreMetrics;
use release_desk_core::app_catalog::{AppActionState, AppStoreItem};

pub fn render_store<T: 'static>(
    items: &[AppStoreItem],
    _metrics: AppStoreMetrics,
    selected_app_id: Option<&str>,
    cx: &Context<T>,
    on_select_app: impl Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    on_trigger_action: impl Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    on_close_detail: impl Fn(&mut T, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    let mut root = div().flex().flex_col().gap_6().p_6().overflow_y_scrollbar();

    // 1. HERO SPOTLIGHT CARD (ArtCraft Studio)
    root = root.child(render_hero_banner(cx, on_select_app, on_trigger_action));

    // 2. CATEGORY & TARGET BAR
    root = root.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .bg(cx.theme().primary)
                            .text_color(cx.theme().primary_foreground)
                            .text_size(px(11.))
                            .font_bold()
                            .child(format!("All Applications ({})", items.len())),
                    )
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .bg(cx.theme().muted)
                            .text_color(cx.theme().muted_foreground)
                            .text_size(px(11.))
                            .child("Creative & Media"),
                    )
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .bg(cx.theme().muted)
                            .text_color(cx.theme().muted_foreground)
                            .text_size(px(11.))
                            .child("Productivity"),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(cx.theme().muted_foreground)
                    .child("Install Directory: ~/Applications"),
            ),
    );

    // 3. BENTO GRID OF APPS
    let mut grid = div().flex().flex_wrap().gap_4();
    for item in items {
        grid = grid.child(render_app_card(item, cx, on_select_app, on_trigger_action));
    }
    root = root.child(grid);

    // 4. DETAIL MODAL (Overlay if an app is selected)
    if let Some(app_id) = selected_app_id {
        if let Some(item) = items.iter().find(|i| i.metadata.id == app_id) {
            root = root.child(render_detail_modal(
                item,
                cx,
                on_trigger_action,
                on_close_detail,
            ));
        }
    }

    root
}

fn render_hero_banner<T: 'static>(
    cx: &Context<T>,
    on_select_app: impl Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    on_trigger_action: impl Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .p_6()
        .rounded_xl()
        .bg(cx.theme().accent)
        .text_color(cx.theme().accent_foreground)
        .border_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_6()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_5()
                        .child(
                            div()
                                .w(px(72.))
                                .h(px(72.))
                                .rounded_xl()
                                .bg(cx.theme().primary)
                                .text_color(cx.theme().primary_foreground)
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(26.))
                                .font_bold()
                                .child("AC"),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_size(px(10.))
                                        .font_bold()
                                        .text_color(cx.theme().warning)
                                        .child("★ SPOTLIGHT APPLICATION"),
                                )
                                .child(
                                    div()
                                        .text_size(px(24.))
                                        .font_bold()
                                        .child("ArtCraft Studio"),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child("Local AI generative workstation with Metal GPU acceleration."),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child("v2.4.0 Universal • Project: ArtCraft • 428 MB"),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .child(
                            Button::new("hero-details")
                                .label("View Details")
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    on_select_app("artcraft".to_string(), this, cx);
                                })),
                        )
                        .child(
                            Button::new("hero-open")
                                .label("▶️ Open Application")
                                .primary()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    on_trigger_action("artcraft".to_string(), this, cx);
                                })),
                        ),
                ),
        )
}

fn render_app_card<T: 'static>(
    item: &AppStoreItem,
    cx: &Context<T>,
    on_select_app: impl Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    on_trigger_action: impl Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    let app_id = item.metadata.id.clone();
    let app_id_for_btn = app_id.clone();
    let app_id_for_card = app_id.clone();

    let (btn_label, is_primary) = match &item.state {
        AppActionState::Installable => ("📥 Install", true),
        AppActionState::UpdateAvailable { .. } => ("🔄 Update", true),
        AppActionState::UpToDate { .. } => ("▶️ Open", false),
    };

    let badge_text = match &item.state {
        AppActionState::Installable => item.metadata.size_estimate.clone(),
        AppActionState::UpdateAvailable {
            installed_version,
            latest_version,
        } => {
            format!("{installed_version} → {latest_version}")
        }
        AppActionState::UpToDate { version } => format!("✓ {version}"),
    };

    div()
        .w(px(270.))
        .p_4()
        .rounded_xl()
        .bg(cx.theme().secondary)
        .border_1()
        .border_color(cx.theme().border)
        .flex()
        .flex_col()
        .justify_between()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_start()
                        .justify_between()
                        .child(
                            div()
                                .w(px(48.))
                                .h(px(48.))
                                .rounded_lg()
                                .bg(cx.theme().secondary)
                                .text_color(cx.theme().secondary_foreground)
                                .flex()
                                .items_center()
                                .justify_center()
                                .font_bold()
                                .text_size(px(16.))
                                .child(item.metadata.icon_text.clone()),
                        )
                        .child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded_md()
                                .bg(cx.theme().muted)
                                .text_color(cx.theme().muted_foreground)
                                .text_size(px(10.))
                                .font_bold()
                                .child(badge_text),
                        ),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .font_bold()
                        .text_color(cx.theme().muted_foreground)
                        .child(item.metadata.category_name.to_uppercase()),
                )
                .child(
                    div()
                        .text_size(px(15.))
                        .font_bold()
                        .text_color(cx.theme().foreground)
                        .child(item.metadata.name.clone()),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(cx.theme().muted_foreground)
                        .child(item.metadata.description.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pt_2()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    Button::new(format!("btn-detail-{}", app_id))
                        .label("Details")
                        .ghost()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            on_select_app(app_id_for_card.clone(), this, cx);
                        })),
                )
                .child(if is_primary {
                    Button::new(format!("btn-act-{}", app_id))
                        .label(btn_label)
                        .primary()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            on_trigger_action(app_id_for_btn.clone(), this, cx);
                        }))
                } else {
                    Button::new(format!("btn-act-{}", app_id))
                        .label(btn_label)
                        .secondary()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            on_trigger_action(app_id_for_btn.clone(), this, cx);
                        }))
                }),
        )
}

fn render_detail_modal<T: 'static>(
    item: &AppStoreItem,
    cx: &Context<T>,
    on_trigger_action: impl Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    on_close_detail: impl Fn(&mut T, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    let app_id = item.metadata.id.clone();
    let (btn_label, _) = match &item.state {
        AppActionState::Installable => ("📥 Install Application", true),
        AppActionState::UpdateAvailable { .. } => ("🔄 Update Application", true),
        AppActionState::UpToDate { .. } => ("▶️ Open Application", false),
    };

    div()
        .p_6()
        .rounded_2xl()
        .bg(cx.theme().secondary)
        .border_2()
        .border_color(cx.theme().primary)
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .flex()
                .items_start()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .gap_4()
                        .child(
                            div()
                                .w(px(64.))
                                .h(px(64.))
                                .rounded_xl()
                                .bg(cx.theme().primary)
                                .text_color(cx.theme().primary_foreground)
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(24.))
                                .font_bold()
                                .child(item.metadata.icon_text.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_size(px(22.))
                                        .font_bold()
                                        .child(item.metadata.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(item.metadata.description.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!(
                                            "Project: {} • Size: {}",
                                            item.project_name, item.metadata.size_estimate
                                        )),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Button::new("modal-action-btn")
                                .label(btn_label)
                                .primary()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    on_trigger_action(app_id.clone(), this, cx);
                                })),
                        )
                        .child(
                            Button::new("modal-close-btn")
                                .label("✕ Close")
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    on_close_detail(this, cx);
                                })),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_4()
                .rounded_lg()
                .bg(cx.theme().muted)
                .child(
                    div()
                        .text_size(px(13.))
                        .font_bold()
                        .child("What's New in this Release:"),
                )
                .children(item.metadata.changelog.iter().map(|line| {
                    div()
                        .text_size(px(11.))
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("• {line}"))
                })),
        )
}
