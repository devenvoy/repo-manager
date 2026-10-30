//! App Store discovery and application detail views.

use crate::colors::color;
use crate::components::avatar::repo_icon_with_provider;
use crate::components::motion_button::Button;
use crate::controllers::store_controller::{StoreActivity, StoreFilter, StoreSort};
use crate::store_catalog::StoreApp;
use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    Context, Entity, InteractiveElement as _, IntoElement, ObjectFit, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, StyledImage as _, div, img, px,
};
use release_desk_core::app_catalog::AppActionState;
use std::{collections::HashMap, path::Path};

fn matches_store_filter(filter: StoreFilter, state: Option<&AppActionState>) -> bool {
    match filter {
        StoreFilter::All => true,
        StoreFilter::Installed => state.is_some_and(AppActionState::is_installed),
        StoreFilter::Updates => state.is_some_and(AppActionState::requires_update),
        StoreFilter::Available => matches!(state, Some(AppActionState::Installable)),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn render_store<
    T: 'static,
    FSelect: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    FAction: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    FRelease: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    FBack: Fn(&mut T, &mut Context<T>) + Copy + 'static,
    FCategory: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    FFilter: Fn(StoreFilter, &mut T, &mut Context<T>) + Copy + 'static,
    FSort: Fn(StoreSort, &mut T, &mut Context<T>) + Copy + 'static,
    FRefresh: Fn(&mut T, &mut Context<T>) + Copy + 'static,
    FPreview: Fn(usize, &mut T, &mut Context<T>) + Copy + 'static,
>(
    items: &[StoreApp],
    selected_app_id: Option<&str>,
    selected_release: Option<&str>,
    search: &Entity<InputState>,
    category: &str,
    filter: StoreFilter,
    sort: StoreSort,
    install_states: &HashMap<String, AppActionState>,
    busy: bool,
    activities: &HashMap<String, StoreActivity>,
    cx: &Context<T>,
    on_select: FSelect,
    on_action: FAction,
    on_release: FRelease,
    on_back: FBack,
    on_category: FCategory,
    on_filter: FFilter,
    on_sort: FSort,
    on_refresh: FRefresh,
    on_preview: FPreview,
) -> impl IntoElement {
    let listings: Vec<&StoreApp> = items.iter().filter(|item| item.is_app_listing()).collect();
    if let Some(item) =
        selected_app_id.and_then(|id| listings.iter().copied().find(|item| item.key == id))
    {
        return render_detail(
            item,
            selected_release,
            install_states.get(&item.key),
            activities.get(&item.key).copied(),
            cx,
            on_action,
            on_release,
            on_back,
            on_preview,
        )
        .into_any_element();
    }

    let query = search.read(cx).value().trim().to_lowercase();
    let mut visible: Vec<&StoreApp> = listings
        .iter()
        .copied()
        .filter(|item| {
            (category == "All" || item.display_category() == category)
                && matches_store_filter(filter, install_states.get(&item.key))
                && (query.is_empty()
                    || item.display_name().to_lowercase().contains(&query)
                    || item.description.to_lowercase().contains(&query)
                    || item.display_category().to_lowercase().contains(&query)
                    || item.project_name.to_lowercase().contains(&query))
        })
        .collect();
    match sort {
        StoreSort::Name => visible.sort_by_key(|item| item.display_name().to_lowercase()),
        StoreSort::RecentlyUpdated => visible.sort_by(|a, b| {
            b.platform_release()
                .and_then(|release| release.published_at.as_deref())
                .cmp(
                    &a.platform_release()
                        .and_then(|release| release.published_at.as_deref()),
                )
        }),
    }
    let featured = visible.iter().copied().max_by_key(|item| {
        item.platform_release()
            .and_then(|release| release.published_at.as_deref())
    });
    let updates = listings
        .iter()
        .filter(|item| {
            install_states
                .get(&item.key)
                .is_some_and(AppActionState::requires_update)
        })
        .count();
    let installed = listings
        .iter()
        .filter(|item| {
            install_states
                .get(&item.key)
                .is_some_and(AppActionState::is_installed)
        })
        .count();
    let mut status_tabs = div().flex().gap_2();
    for (choice, label) in [
        (StoreFilter::All, "Discover".to_string()),
        (StoreFilter::Updates, format!("Updates  {updates}")),
        (StoreFilter::Installed, format!("Installed  {installed}")),
        (StoreFilter::Available, "Available".to_string()),
    ] {
        status_tabs = status_tabs.child(
            Button::new(format!("store-tab-{choice:?}"))
                .label(label)
                .outline()
                .disabled(filter == choice)
                .on_click(cx.listener(move |this, _, _, cx| on_filter(choice, this, cx))),
        );
    }
    let mut categories = vec!["All".to_string()];
    for item in &listings {
        let value = item.display_category();
        if !categories.contains(&value) {
            categories.push(value);
        }
    }
    categories[1..].sort();
    let mut category_tabs = div().flex().flex_wrap().gap_2();
    for value in categories {
        let selected = value == category;
        let id = format!("store-category-{value}");
        let selected_value = value.clone();
        category_tabs =
            category_tabs.child(
                Button::new(id)
                    .label(value)
                    .ghost()
                    .disabled(selected)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        on_category(selected_value.clone(), this, cx)
                    })),
            );
    }
    let mut cards = div().flex().flex_wrap().gap_3();
    for item in visible.iter().copied() {
        cards = cards.child(render_app_row(
            item,
            install_states.get(&item.key),
            activities.get(&item.key).copied(),
            cx,
            on_select,
            on_action,
        ));
    }
    div()
        .flex()
        .flex_col()
        .gap_5()
        .p_6()
        .overflow_y_scrollbar()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(11.))
                                .font_bold()
                                .text_color(cx.theme().primary)
                                .child("DISCOVER"),
                        )
                        .child(
                            div()
                                .text_size(px(30.))
                                .font_bold()
                                .child(format!("Apps for your {}", platform_label())),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(cx.theme().muted_foreground)
                                .child("Explore and keep up with apps from your projects."),
                        ),
                )
                .child(
                    Button::new("refresh-store")
                        .label(if busy { "Refreshing…" } else { "Refresh" })
                        .outline()
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, _, cx| on_refresh(this, cx))),
                ),
        )
        .child(status_tabs)
        .child(
            if filter == StoreFilter::All && query.is_empty() && category == "All" {
                featured
                    .map(|item| {
                        render_featured(
                            item,
                            install_states.get(&item.key),
                            activities.get(&item.key).copied(),
                            cx,
                            on_select,
                            on_action,
                        )
                        .into_any_element()
                    })
                    .unwrap_or_else(|| div().into_any_element())
            } else {
                div().into_any_element()
            },
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(div().flex_1().child(Input::new(search)))
                .child(
                    Button::new("sort-store-name")
                        .label("Name")
                        .outline()
                        .disabled(sort == StoreSort::Name)
                        .on_click(
                            cx.listener(move |this, _, _, cx| on_sort(StoreSort::Name, this, cx)),
                        ),
                )
                .child(
                    Button::new("sort-store-recent")
                        .label("Newest")
                        .outline()
                        .disabled(sort == StoreSort::RecentlyUpdated)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            on_sort(StoreSort::RecentlyUpdated, this, cx)
                        })),
                ),
        )
        .child(category_tabs)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(div().text_size(px(19.)).font_bold().child(
                    if filter == StoreFilter::Updates {
                        "Updates"
                    } else if filter == StoreFilter::Installed {
                        "Installed apps"
                    } else {
                        "Explore apps"
                    },
                ))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{} apps", visible.len())),
                ),
        )
        .child(if visible.is_empty() {
            div()
                .p_8()
                .rounded_xl()
                .bg(cx.theme().secondary)
                .child(if busy {
                    "Finding apps across your projects…"
                } else if listings.is_empty() {
                    "No apps saved yet. Open a project's Releases page or choose Refresh."
                } else {
                    "No apps match this view."
                })
                .into_any_element()
        } else {
            cards.into_any_element()
        })
        .into_any_element()
}

fn render_featured<
    T: 'static,
    FSelect: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    FAction: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
>(
    item: &StoreApp,
    state: Option<&AppActionState>,
    activity: Option<StoreActivity>,
    cx: &Context<T>,
    on_select: FSelect,
    on_action: FAction,
) -> impl IntoElement {
    let key = item.key.clone();
    let action_key = key.clone();
    div()
        .id("store-featured")
        .cursor_pointer()
        .p_6()
        .rounded_2xl()
        .bg(color("#18213B"))
        .text_color(color("#FFFFFF"))
        .flex()
        .items_center()
        .justify_between()
        .gap_6()
        .on_click(cx.listener(move |this, _, _, cx| on_select(key.clone(), this, cx)))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .max_w(px(650.))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_bold()
                        .text_color(color("#AFC7FF"))
                        .child("FEATURED RELEASE"),
                )
                .child(
                    div()
                        .text_size(px(27.))
                        .font_bold()
                        .child(item.display_name()),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(color("#E4EAF8"))
                        .line_clamp(2)
                        .child(item.description.clone()),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(color("#AFC7FF"))
                        .child(format!(
                            "{}  ·  {}",
                            item.display_category(),
                            item.project_name
                        )),
                )
                .child(
                    Button::new("featured-action")
                        .label(activity.map_or_else(
                            || action_label(item, state).to_string(),
                            StoreActivity::label,
                        ))
                        .primary()
                        .disabled(
                            activity.is_some()
                                || matches!(action_label(item, state), "Unavailable" | "Checking…"),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            on_action(action_key.clone(), this, cx);
                        })),
                )
                .child(activity_indicator(activity, cx)),
        )
        .child(repo_icon_with_provider(
            &item.display_name(),
            104.,
            item.icon_path.as_deref(),
            Some(&item.provider),
        ))
}

fn render_app_row<
    T: 'static,
    FSelect: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    FAction: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
>(
    item: &StoreApp,
    state: Option<&AppActionState>,
    activity: Option<StoreActivity>,
    cx: &Context<T>,
    on_select: FSelect,
    on_action: FAction,
) -> impl IntoElement {
    let key = item.key.clone();
    let action_key = key.clone();
    div()
        .id(format!("app-row-{key}"))
        .cursor_pointer()
        .w(px(510.))
        .p_4()
        .rounded_xl()
        .bg(cx.theme().secondary)
        .border_1()
        .border_color(cx.theme().border)
        .flex()
        .items_center()
        .gap_4()
        .on_click(cx.listener(move |this, _, _, cx| on_select(key.clone(), this, cx)))
        .child(repo_icon_with_provider(
            &item.display_name(),
            64.,
            item.icon_path.as_deref(),
            Some(&item.provider),
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(px(16.))
                        .font_bold()
                        .child(item.display_name()),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(cx.theme().muted_foreground)
                        .truncate()
                        .child(format!(
                            "{}  ·  {}",
                            item.display_category(),
                            item.project_name
                        )),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_clamp(2)
                        .child(item.description.clone()),
                )
                .child(activity_indicator(activity, cx)),
        )
        .child(
            Button::new(format!("app-action-{action_key}"))
                .label(activity.map_or_else(
                    || action_label(item, state).to_string(),
                    StoreActivity::label,
                ))
                .primary()
                .disabled(
                    activity.is_some()
                        || matches!(action_label(item, state), "Unavailable" | "Checking…"),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    on_action(action_key.clone(), this, cx);
                })),
        )
}

#[allow(clippy::too_many_arguments)]
fn render_detail<
    T: 'static,
    FAction: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    FRelease: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    FBack: Fn(&mut T, &mut Context<T>) + Copy + 'static,
    FPreview: Fn(usize, &mut T, &mut Context<T>) + Copy + 'static,
>(
    item: &StoreApp,
    selected_release: Option<&str>,
    state: Option<&AppActionState>,
    activity: Option<StoreActivity>,
    cx: &Context<T>,
    on_action: FAction,
    on_release: FRelease,
    on_back: FBack,
    on_preview: FPreview,
) -> impl IntoElement {
    let key = item.key.clone();
    let release = selected_release
        .and_then(|tag| item.releases.iter().find(|release| release.tag == tag))
        .or_else(|| item.platform_release())
        .or_else(|| item.latest());
    let mut versions = div()
        .w(px(190.))
        .h(px(330.))
        .overflow_y_scrollbar()
        .flex()
        .flex_col()
        .gap_2();
    for entry in &item.releases {
        let tag = entry.tag.clone();
        let active = release.is_some_and(|selected| selected.tag == tag);
        versions = versions.child(
            Button::new(format!("release-{tag}"))
                .label(tag.clone())
                .outline()
                .disabled(active)
                .on_click(cx.listener(move |this, _, _, cx| on_release(tag.clone(), this, cx))),
        );
    }
    let mut notes = div()
        .flex_1()
        .h(px(330.))
        .p_5()
        .rounded_xl()
        .bg(cx.theme().secondary)
        .overflow_y_scrollbar()
        .flex()
        .flex_col()
        .gap_3();
    if let Some(release) = release {
        notes = notes
            .child(
                div()
                    .text_size(px(17.))
                    .font_bold()
                    .child(format!("What's new in {}", release.tag)),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        release
                            .published_at
                            .as_deref()
                            .map(format_date)
                            .unwrap_or_default(),
                    ),
            );
        let lines = note_lines(&release.notes);
        if lines.is_empty() {
            notes = notes.child(div().text_size(px(13.)).child("No release notes provided."));
        } else {
            for line in lines {
                notes = notes.child(div().text_size(px(13.)).child(line));
            }
        }
    }
    let mut artwork = div().flex().flex_wrap().gap_3();
    for (index, path) in item.images.iter().enumerate() {
        artwork = artwork.child(
            div()
                .id(format!("store-preview-{index}"))
                .cursor_pointer()
                .w(px(320.))
                .h(px(200.))
                .rounded_xl()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary)
                .overflow_hidden()
                .on_click(cx.listener(move |this, _, _, cx| on_preview(index, this, cx)))
                .child(img(path.clone()).size_full().object_fit(ObjectFit::Contain)),
        );
    }
    div()
        .flex()
        .flex_col()
        .gap_6()
        .p_6()
        .overflow_y_scrollbar()
        .child(
            Button::new("back-to-store")
                .label("← Discover")
                .ghost()
                .on_click(cx.listener(move |this, _, _, cx| on_back(this, cx))),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_5()
                .child(repo_icon_with_provider(
                    &item.display_name(),
                    104.,
                    item.icon_path.as_deref(),
                    Some(&item.provider),
                ))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(29.))
                                .font_bold()
                                .child(item.display_name()),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{}  ·  {}",
                                    item.display_category(),
                                    item.project_name
                                )),
                        )
                        .child(div().text_size(px(13.)).child(item.description.clone()))
                        .child(activity_indicator(activity, cx)),
                )
                .child(
                    Button::new("store-detail-action")
                        .label(activity.map_or_else(
                            || action_label(item, state).to_string(),
                            StoreActivity::label,
                        ))
                        .primary()
                        .disabled(
                            activity.is_some()
                                || matches!(action_label(item, state), "Unavailable" | "Checking…"),
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| on_action(key.clone(), this, cx)),
                        ),
                ),
        )
        .child(div().h(px(1.)).bg(cx.theme().border))
        .child(
            div()
                .flex()
                .gap_6()
                .child(detail_fact(
                    "VERSION",
                    item.platform_release()
                        .map_or("—", |release| release.tag.as_str()),
                    cx,
                ))
                .child(detail_fact("CATEGORY", &item.display_category(), cx))
                .child(detail_fact("SOURCE", &item.project_name, cx)),
        )
        .child(if item.images.is_empty() {
            div().into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(div().text_size(px(19.)).font_bold().child("Preview"))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(cx.theme().muted_foreground)
                        .child("Click an image to view it larger"),
                )
                .child(artwork)
                .into_any_element()
        })
        .child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(div().text_size(px(19.)).font_bold().child("What's New"))
                .child(div().flex().gap_4().child(versions).child(notes)),
        )
}

pub fn render_store_preview<
    T: 'static,
    FChange: Fn(Option<usize>, &mut T, &mut Context<T>) + Copy + 'static,
>(
    path: &Path,
    app_name: &str,
    index: usize,
    count: usize,
    cx: &Context<T>,
    on_change: FChange,
) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .bg(color("#101827").opacity(0.97))
        .text_color(color("#FFFFFF"))
        .flex()
        .flex_col()
        .p_6()
        .gap_4()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(18.))
                        .font_bold()
                        .child(format!("{app_name} preview")),
                )
                .child(
                    Button::new("close-store-preview")
                        .label("Close ✕")
                        .outline()
                        .on_click(cx.listener(move |this, _, _, cx| on_change(None, this, cx))),
                ),
        )
        .child(
            div().flex_1().min_h_0().min_w_0().child(
                img(path.to_path_buf())
                    .size_full()
                    .object_fit(ObjectFit::Contain),
            ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .gap_4()
                .child(
                    Button::new("previous-store-preview")
                        .label("← Previous")
                        .outline()
                        .disabled(index == 0)
                        .on_click(
                            cx.listener(move |this, _, _, cx| on_change(Some(index - 1), this, cx)),
                        ),
                )
                .child(format!("{} of {count}", index + 1))
                .child(
                    Button::new("next-store-preview")
                        .label("Next →")
                        .outline()
                        .disabled(index + 1 >= count)
                        .on_click(
                            cx.listener(move |this, _, _, cx| on_change(Some(index + 1), this, cx)),
                        ),
                ),
        )
}

fn detail_fact<T: 'static>(label: &str, value: &str, cx: &Context<T>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_size(px(10.))
                .font_bold()
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
        .child(div().text_size(px(12.)).child(value.to_string()))
}

fn activity_indicator<T: 'static>(
    activity: Option<StoreActivity>,
    cx: &Context<T>,
) -> impl IntoElement {
    let mut indicator = div().flex().flex_col().gap_1();
    if let Some(activity) = activity {
        indicator = indicator.child(div().text_size(px(11.)).font_bold().child(activity.label()));
        if let StoreActivity::Downloading(Some(percent)) = activity {
            indicator = indicator.child(
                div()
                    .w(px(160.))
                    .h(px(5.))
                    .rounded_full()
                    .bg(cx.theme().border)
                    .child(
                        div()
                            .w(px(160. * f32::from(percent) / 100.))
                            .h(px(5.))
                            .rounded_full()
                            .bg(cx.theme().primary),
                    ),
            );
        }
    }
    indicator
}

fn format_date(value: &str) -> String {
    let mut parts = value.get(..10).unwrap_or(value).split('-');
    let (Some(year), Some(month), Some(day)) = (parts.next(), parts.next(), parts.next()) else {
        return value.to_string();
    };
    let month = match month {
        "01" => "Jan",
        "02" => "Feb",
        "03" => "Mar",
        "04" => "Apr",
        "05" => "May",
        "06" => "Jun",
        "07" => "Jul",
        "08" => "Aug",
        "09" => "Sep",
        "10" => "Oct",
        "11" => "Nov",
        "12" => "Dec",
        _ => return value.to_string(),
    };
    format!("{month} {}, {year}", day.trim_start_matches('0'))
}

fn note_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .filter_map(|line| {
            let cleaned = line
                .trim()
                .trim_start_matches('#')
                .trim()
                .trim_start_matches(['*', '-', '•'])
                .trim()
                .replace("**", "");
            let cleaned = cleaned
                .split(" in https://")
                .next()
                .unwrap_or(&cleaned)
                .trim();
            let lower = cleaned.to_ascii_lowercase();
            if cleaned.is_empty()
                || cleaned.starts_with("https://")
                || lower.starts_with("full changelog")
                || lower == "what's changed"
                || lower.starts_with("community batch")
                || lower.contains(" test")
                || lower.contains("runtime limit")
                || ["ci:", "chore:", "build:", "test:", "docs:", "refactor:"]
                    .iter()
                    .any(|prefix| lower.starts_with(prefix))
            {
                None
            } else {
                let cleaned = cleaned.split(" by @").next().unwrap_or(cleaned).trim();
                let cleaned = cleaned
                    .trim_end_matches(|ch: char| ch.is_whitespace() || ch == '.')
                    .trim();
                let cleaned = cleaned.rsplit_once(" (#").map_or(cleaned, |(text, _)| text);
                let cleaned = cleaned
                    .split_once(": ")
                    .filter(|(prefix, _)| {
                        let prefix = prefix.to_ascii_lowercase();
                        prefix.starts_with("fix")
                            || prefix.starts_with("feat")
                            || prefix.starts_with("m7")
                    })
                    .map_or(cleaned, |(_, text)| text);
                if cleaned.is_empty() {
                    None
                } else {
                    let mut chars = cleaned.chars();
                    let first = chars.next()?;
                    Some(format!("{}{}", first.to_uppercase(), chars.as_str()))
                }
            }
        })
        .take(12)
        .collect()
}

fn action_label(item: &StoreApp, state: Option<&AppActionState>) -> &'static str {
    match state {
        None => "Checking…",
        Some(AppActionState::UpToDate { .. }) => "Open",
        Some(AppActionState::Installable | AppActionState::UpdateAvailable { .. }) => {
            if item.platform_asset().is_none() {
                "Unavailable"
            } else if cfg!(target_os = "macos")
                && item
                    .platform_asset()
                    .is_some_and(|asset| asset.name.to_ascii_lowercase().ends_with(".dmg"))
            {
                if state.is_some_and(AppActionState::is_installed) {
                    "Update"
                } else {
                    "Install"
                }
            } else {
                "Download"
            }
        }
    }
}

fn platform_label() -> &'static str {
    match std::env::consts::OS {
        "macos" => "Mac",
        "windows" => "Windows PC",
        "linux" => "Linux desktop",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::{format_date, matches_store_filter, note_lines};
    use crate::controllers::store_controller::StoreFilter;
    use release_desk_core::app_catalog::AppActionState;

    #[test]
    fn filters_use_cached_state_and_do_not_assume_unknown_is_installable() {
        let update = AppActionState::UpdateAvailable {
            installed_version: "1".into(),
            latest_version: "2".into(),
        };
        let installed = AppActionState::UpToDate {
            version: "1".into(),
        };
        assert!(matches_store_filter(StoreFilter::All, None));
        assert!(!matches_store_filter(StoreFilter::Installed, None));
        assert!(!matches_store_filter(StoreFilter::Updates, None));
        assert!(!matches_store_filter(StoreFilter::Available, None));
        assert!(matches_store_filter(StoreFilter::Updates, Some(&update)));
        assert!(matches_store_filter(StoreFilter::Installed, Some(&update)));
        assert!(matches_store_filter(
            StoreFilter::Installed,
            Some(&installed)
        ));
        assert!(matches_store_filter(
            StoreFilter::Available,
            Some(&AppActionState::Installable)
        ));
    }

    #[test]
    fn release_notes_hide_markdown_noise_and_raw_links() {
        let notes = "## What's Changed\n* Fixed export crash by @dev in https://github.com/x/y/pull/1\n\n**Full Changelog**: https://github.com/x/y/compare/1...2";
        assert_eq!(note_lines(notes), vec!["Fixed export crash"]);
    }

    #[test]
    fn release_notes_skip_developer_noise_and_dates_are_readable() {
        let notes = "## What's Changed\n* ci: update build job by @dev\n* M7: home tests wait for files\n* Community batch 1: 22 reviewed fixes\n* fix(ui): page stays open after double-click (#42) by @dev in https://github.com/x/y/pull/42";
        assert_eq!(
            note_lines(notes),
            vec!["Page stays open after double-click"]
        );
        assert_eq!(format_date("2026-10-18T04:05:18Z"), "Oct 18, 2026");
    }
}
