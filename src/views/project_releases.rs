//! Release shelf backed by fetched provider data and ArtCraft application catalog.

use crate::components::avatar::repo_icon_with_provider;
use crate::components::icons::{check_icon, chevron_down_icon, download_icon, refresh_icon};
use crate::components::motion_button::Button;
use crate::styles::CssExt as _;
use crate::workspace_releases::ReleaseViewState;
use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    Context, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, deferred, div, px,
};
use release_desk_core::app_catalog::find_known_app;
use release_desk_store::workspace_releases::{
    LocalAssetState, RELEASE_ASSET_TYPES, ReleaseAssetType, release_asset_type,
    release_asset_type_for_extension,
};

fn available_release_types(
    state: &ReleaseViewState,
    selected: &str,
) -> Vec<&'static ReleaseAssetType> {
    let mut available = Vec::new();
    if let Some(repositories) = state.repositories() {
        for repository in repositories {
            let Ok(releases) = &repository.result else {
                continue;
            };
            for release in releases {
                for asset in &release.assets {
                    if let Some(kind) = release_asset_type(&asset.name) {
                        if !available.iter().any(|existing: &&ReleaseAssetType| {
                            existing.extension == kind.extension
                        }) {
                            available.push(kind);
                        }
                    }
                }
            }
        }
    }
    if let Some(kind) = release_asset_type_for_extension(selected) {
        if !available
            .iter()
            .any(|existing: &&ReleaseAssetType| existing.extension == kind.extension)
        {
            available.push(kind);
        }
    }
    available.sort_by_key(|kind| {
        RELEASE_ASSET_TYPES
            .iter()
            .position(|candidate| candidate.extension == kind.extension)
            .unwrap_or(usize::MAX)
    });
    available
}

fn format_bytes(bytes: Option<u64>) -> String {
    match bytes {
        Some(b) if b >= 1_000_000_000 => format!("{:.1} GB", b as f64 / 1_000_000_000.0),
        Some(b) if b >= 1_000_000 => format!("{} MB", b / 1_000_000),
        Some(b) if b >= 1_000 => format!("{} KB", b / 1_000),
        Some(b) => format!("{b} B"),
        None => "Unknown size".to_string(),
    }
}

fn format_date(raw: Option<&str>) -> String {
    match raw {
        Some(d) => {
            if d.len() >= 10 {
                let parts: Vec<&str> = d[..10].split('-').collect();
                if parts.len() == 3 {
                    let month = match parts[1] {
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
                        _ => parts[1],
                    };
                    return format!("{month} {}, {}", parts[2].trim_start_matches('0'), parts[0]);
                }
            }
            d.to_string()
        }
        None => "Recent".to_string(),
    }
}

#[derive(Clone, Copy)]
pub struct ReleaseShelfControls<'a> {
    pub busy: bool,
    pub extension: &'a str,
    pub type_menu_open: bool,
}

/// Render the release shelf matching the ArtCraft Release Desk layout.
pub fn render_live_releases<T: 'static>(
    state: &ReleaseViewState,
    controls: ReleaseShelfControls<'_>,
    cx: &Context<T>,
    on_refresh: impl Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    on_toggle_type_menu: impl Fn(&mut T, &mut Context<T>) + Copy + 'static,
    on_select_type: impl Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    on_download: impl Fn(
        String,
        String,
        String,
        String,
        String,
        String,
        Option<u64>,
        &mut T,
        &mut Window,
        &mut Context<T>,
    ) + Copy
    + 'static,
) -> impl IntoElement {
    let ReleaseShelfControls {
        busy,
        extension,
        type_menu_open,
    } = controls;
    let available_types = available_release_types(state, extension);
    let selected_type = release_asset_type_for_extension(extension);
    let selected_label = selected_type
        .map(|kind| kind.label.to_string())
        .unwrap_or_else(|| extension.to_ascii_uppercase());

    let mut type_menu_items = div().flex().flex_col().gap_0p5();
    for kind in &available_types {
        let is_selected = kind.extension == extension;
        let kind_label = kind.label.to_string();
        let kind_extension = kind.extension.to_string();
        let mut item = div()
            .id(ElementId::Name(
                format!("release-type-{}", kind.extension).into(),
            ))
            .flex()
            .items_center()
            .justify_between()
            .px_2p5()
            .py_1p5()
            .rounded_md()
            .cursor_pointer()
            .bg(if is_selected {
                cx.theme().primary.opacity(0.12)
            } else {
                cx.theme().background
            })
            .hover(|style| style.bg(cx.theme().secondary.opacity(0.2)))
            .on_click(cx.listener(move |this, _, _, cx| {
                on_select_type(kind_extension.clone(), this, cx);
            }))
            .child(
                div()
                    .text_size(px(12.5))
                    .font_medium()
                    .text_color(if is_selected {
                        cx.theme().primary
                    } else {
                        cx.theme().foreground
                    })
                    .child(kind_label),
            );
        if is_selected {
            item = item.child(
                check_icon()
                    .w(px(13.))
                    .h(px(13.))
                    .text_color(cx.theme().primary),
            );
        }
        type_menu_items = type_menu_items.child(item);
    }

    let mut type_selector = div()
        .relative()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .text_size(px(10.))
                .font_bold()
                .text_color(cx.theme().muted_foreground)
                .child("FILE TYPE"),
        )
        .child(
            div()
                .id("release-type-trigger")
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_1p5()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary.opacity(0.12))
                .hover(|style| style.bg(cx.theme().secondary.opacity(0.25)))
                .cursor_pointer()
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_medium()
                        .text_color(cx.theme().foreground)
                        .child(selected_label.clone()),
                )
                .child(
                    chevron_down_icon()
                        .w(px(12.))
                        .h(px(12.))
                        .text_color(cx.theme().muted_foreground),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    on_toggle_type_menu(this, cx);
                })),
        );

    if type_menu_open && !available_types.is_empty() {
        type_selector = type_selector.child(
            deferred(
                div()
                    .child(
                        div()
                            .id("release-type-backdrop")
                            .occlude()
                            .absolute()
                            .top(px(-1000.))
                            .left(px(-2000.))
                            .w(px(5000.))
                            .h(px(5000.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                on_toggle_type_menu(this, cx);
                            })),
                    )
                    .child(
                        div()
                            .occlude()
                            .absolute()
                            .top_full()
                            .right_0()
                            .mt_1p5()
                            .w(px(180.))
                            .rounded_xl()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().background)
                            .shadow_lg()
                            .p_1p5()
                            .child(type_menu_items),
                    ),
            )
            .priority(10),
        );
    }

    let mut body = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap_5()
        .overflow_y_scrollbar()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().css_section_eyebrow(cx.theme()).child("RELEASE SHELF"))
                        .child(div().css_heading_title().child("Releases & downloads"))
                        .child(div().css_body_muted(cx.theme()).child(
                            "Published releases and downloadable assets for your applications.",
                        )),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(type_selector)
                        .child(
                            Button::new("refresh-releases")
                                .icon(refresh_icon())
                                .label(if busy || matches!(state, ReleaseViewState::Loading) {
                                    "Fetching…"
                                } else {
                                    "Refresh releases"
                                })
                                .outline()
                                .disabled(busy || matches!(state, ReleaseViewState::Loading))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    on_refresh(this, window, cx)
                                })),
                        ),
                ),
        );

    match state {
        ReleaseViewState::NotLoaded => {
            body = body.child(
                div()
                    .css_empty_state(cx.theme())
                    .child(
                        div()
                            .font_bold()
                            .text_size(px(16.))
                            .child("Release shelf ready"),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(cx.theme().muted_foreground)
                            .text_center()
                            .child("Click Refresh releases above to fetch upstream versions and files for your repositories."),
                    )
                    .child(
                        Button::new("shelf-initial-refresh")
                            .icon(refresh_icon())
                            .label("Refresh releases now")
                            .primary()
                            .on_click(cx.listener(move |this, _, window, cx| on_refresh(this, window, cx))),
                    ),
            )
        }
        ReleaseViewState::Loading => {
            body = body.child(
                div()
                    .css_empty_state(cx.theme())
                    .child(refresh_icon().w(px(18.)).h(px(18.)))
                    .child(
                        div()
                            .text_size(px(14.))
                            .text_color(cx.theme().muted_foreground)
                            .child("Fetching release catalog and verifying local files…"),
                    ),
            )
        }
        ReleaseViewState::Loaded { repositories, .. } if repositories.is_empty() => {
            body = body.child(
                div()
                    .css_empty_state(cx.theme())
                    .child(
                        div()
                            .font_bold()
                            .text_size(px(16.))
                            .child("No repositories tracked"),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                "Select repositories in Sources, then return here to view their releases.",
                            ),
                    ),
            )
        }
        ReleaseViewState::Loaded {
            repositories,
            presentations,
            extension: loaded_extension,
        } => {
            let mut list = div().flex().flex_col().gap_4();
            let mut matched_any = false;

            if loaded_extension != extension || presentations.len() != repositories.len() {
                body = body.child(
                    div()
                        .css_empty_state(cx.theme())
                        .child(refresh_icon().w(px(18.)).h(px(18.)))
                        .child(
                            div()
                                .text_size(px(14.))
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("Preparing {selected_label} shelf…")),
                        ),
                );
                return body;
            }

            for ((repo_index, repository), presentation) in
                repositories.iter().enumerate().zip(presentations)
            {
                let known = find_known_app(&repository.repository);
                let display_title = known
                    .as_ref()
                    .map(|k| k.name.clone())
                    .unwrap_or_else(|| repository.repository.clone());
                let category_name = known
                    .as_ref()
                    .map(|k| k.category_name.clone())
                    .unwrap_or_else(|| "Desktop App".to_string());

                let icon_path = presentation.icon_path.as_deref();

                if let Err(err) = &repository.result {
                    list = list.child(
                            div()
                                .p_5()
                                .rounded_xl()
                                .border_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().background)
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_3()
                                        .child(repo_icon_with_provider(
                                            &repository.repository,
                                            36.,
                                            icon_path,
                                            Some(&repository.provider),
                                        ))
                                        .child(
                                            div()
                                                .font_bold()
                                                .text_size(px(15.))
                                                .child(display_title),
                                        )
                                        .child(
                                            div()
                                                .px_2()
                                                .py_0p5()
                                                .rounded_md()
                                                .bg(cx.theme().danger.opacity(0.1))
                                                .text_color(cx.theme().danger)
                                                .text_size(px(11.))
                                                .font_bold()
                                                .child("Sync Error"),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(cx.theme().danger)
                                        .child(format!("Could not check releases: {err}")),
                                ),
                    );
                    continue;
                }

                let latest_tag = presentation.latest_tag.as_deref();
                let latest_published_at = presentation.published_at.as_deref();
                let target_asset = presentation.asset.as_ref();
                let pkg_kind = presentation.pkg_kind.as_str();
                matched_any |= target_asset.is_some();

                let local_state = presentation.local_state;
                let is_up_to_date = matches!(
                    local_state,
                    LocalAssetState::SizeMatches | LocalAssetState::PresentSizeUnknown
                );

                let newer_update = presentation.newer_update.as_ref();

                let status_chip = if is_up_to_date {
                    div()
                        .px_2p5()
                        .py_1()
                        .rounded_full()
                        .bg(cx.theme().success.opacity(0.12))
                        .text_color(cx.theme().success)
                        .text_size(px(11.))
                        .font_bold()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .child(check_icon().w(px(11.)).h(px(11.)))
                        .child("Up to date")
                } else if newer_update.is_some() {
                    div()
                        .px_2p5()
                        .py_1()
                        .rounded_full()
                        .bg(cx.theme().warning.opacity(0.12))
                        .text_color(cx.theme().warning)
                        .text_size(px(11.))
                        .font_bold()
                        .child("Update available")
                } else if target_asset.is_some() {
                    div()
                        .px_2p5()
                        .py_1()
                        .rounded_full()
                        .bg(cx.theme().primary.opacity(0.12))
                        .text_color(cx.theme().primary)
                        .text_size(px(11.))
                        .font_bold()
                        .child(format!("{pkg_kind} ready"))
                } else {
                    div()
                        .px_2p5()
                        .py_1()
                        .rounded_full()
                        .bg(cx.theme().secondary.opacity(0.2))
                        .text_color(cx.theme().muted_foreground)
                        .text_size(px(11.))
                        .font_bold()
                        .child(format!("No {pkg_kind}"))
                };

                let mut card_header_actions = div().flex().items_center().gap_2();

                if let (Some(tag), Some(asset)) = (latest_tag, target_asset) {
                    let provider = repository.provider.clone();
                    let owner = repository.owner.clone();
                    let repo = repository.repository.clone();
                    let tag = tag.to_string();
                    let name = asset.name.clone();
                    let url = asset.download_url.clone();
                    let size = asset.size;

                    let btn_label = if is_up_to_date {
                        "Re-add file".to_string()
                    } else {
                        format!("Download {pkg_kind}")
                    };

                    card_header_actions = card_header_actions.child(
                        Button::new(format!("shelf-dl-{repo_index}"))
                            .icon(download_icon())
                            .label(btn_label)
                            .primary()
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                on_download(
                                    provider.clone(),
                                    owner.clone(),
                                    repo.clone(),
                                    tag.clone(),
                                    name.clone(),
                                    url.clone(),
                                    size,
                                    this,
                                    window,
                                    cx,
                                );
                            })),
                    );
                }

                // Header Row
                let card_head = div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(repo_icon_with_provider(
                                &repository.repository,
                                44.,
                                icon_path,
                                Some(&repository.provider),
                            ))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .font_bold()
                                            .text_size(px(16.))
                                            .child(display_title),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!(
                                                "{} · published {}",
                                                repository.owner,
                                                format_date(
                                                    latest_published_at
                                                )
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_md()
                                    .bg(cx.theme().secondary.opacity(0.1))
                                    .text_color(cx.theme().muted_foreground)
                                    .text_size(px(11.))
                                    .font_bold()
                                    .child(category_name),
                            )
                            .child(status_chip),
                    )
                    .child(card_header_actions);

                // Slot 1: Newest Upstream
                let slot1 = div()
                    .flex_1()
                    .min_w(px(200.))
                    .rounded_xl()
                    .border_1()
                    .border_color(cx.theme().primary.opacity(0.35))
                    .bg(cx.theme().primary.opacity(0.05))
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(10.))
                            .font_bold()
                            .text_color(cx.theme().primary)
                            .child("NEWEST UPSTREAM"),
                    )
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_bold()
                            .child(
                                latest_tag
                                    .map(str::to_string)
                                    .unwrap_or_else(|| "None".to_string()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                target_asset.map_or(format!("No {pkg_kind} assets"), |a| {
                                    format!("{} · {}", a.name, format_bytes(a.size))
                                }),
                            ),
                    );

                // Slot 2: Selected Asset
                let slot2 = div()
                    .flex_1()
                    .min_w(px(200.))
                    .rounded_xl()
                    .border_1()
                    .border_color(if is_up_to_date {
                        cx.theme().success.opacity(0.4)
                    } else {
                        cx.theme().border
                    })
                    .bg(if is_up_to_date {
                        cx.theme().success.opacity(0.05)
                    } else {
                        cx.theme().secondary.opacity(0.04)
                    })
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(10.))
                            .font_bold()
                            .text_color(if is_up_to_date {
                                cx.theme().success
                            } else {
                                cx.theme().muted_foreground
                            })
                            .child("SELECTED ASSET"),
                    )
                    .child(div().text_size(px(16.)).font_bold().child(if is_up_to_date {
                        target_asset
                            .map(|a| a.name.clone())
                            .unwrap_or_else(|| "Downloaded".to_string())
                    } else {
                        "Not downloaded".to_string()
                    }))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child(if is_up_to_date {
                                target_asset.map_or("Ready to install".to_string(), |a| {
                                    format!("{} · Ready in project", format_bytes(a.size))
                                })
                            } else {
                                format!("Click Download {pkg_kind} to store locally")
                            }),
                    );

                // Slot 3: Previous build in old/
                let shelf_folder = selected_type.map_or("Release Dmgs", |kind| kind.folder);
                let (slot3_label, slot3_subtext) = if let Some((arch_name, arch_size)) = presentation.archived_file.as_ref() {
                    (
                        format!("{arch_name} · {}", format_bytes(Some(*arch_size))),
                        format!("Archived build preserved in {shelf_folder}/old"),
                    )
                } else if let Some((_, older)) = newer_update {
                    (
                        format!("Archived {older}"),
                        format!("Previous build preserved in {shelf_folder}/old"),
                    )
                } else {
                    (
                        "None in shelf".to_string(),
                        format!("Earlier versions are kept in {shelf_folder}/old"),
                    )
                };

                let slot3 = div()
                    .flex_1()
                    .min_w(px(200.))
                    .rounded_xl()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary.opacity(0.04))
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(10.))
                            .font_bold()
                            .text_color(cx.theme().muted_foreground)
                            .child("PREVIOUS BUILD (OLD/)"),
                    )
                    .child(div().text_size(px(16.)).font_bold().child(slot3_label))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child(slot3_subtext),
                    );

                let slots_row = div().flex().flex_wrap().gap_3().child(slot1).child(slot2).child(slot3);

                let card = div()
                    .css_card_padded(cx.theme())
                    .shadow_xs()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(card_head)
                    .child(slots_row);

                list = list.child(card);
            }

            if !matched_any {
                list = list.child(
                    div()
                        .css_empty_state(cx.theme())
                        .child(
                            div()
                                .font_bold()
                                .text_size(px(16.))
                                .child(format!("No {selected_label} releases found")),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(cx.theme().muted_foreground)
                                .text_center()
                                .child("Choose another file type above or refresh the release catalog."),
                        ),
                );
            }

            body = body.child(list);
        }
    }

    body
}
