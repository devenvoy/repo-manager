//! Release shelf backed by fetched provider data and ArtCraft application catalog.

use crate::components::app_icon::app_badge;
use crate::components::motion_button::Button;
use crate::components::ui_icons::{check_icon, download_icon, refresh_icon};
use crate::styles::CssExt as _;
use crate::workspace_releases::ReleaseViewState;
use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{Context, IntoElement, ParentElement as _, Styled as _, Window, div, px};
use release_desk_core::app_catalog::find_known_app;
use release_desk_store::workspace_releases::{
    LocalAssetState, local_asset_state, newer_release_with_local_older,
};
use std::path::Path;

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

/// Render release shelf matching the ArtCraft Release Desk layout.
pub fn render_live_releases<T: 'static>(
    state: &ReleaseViewState,
    project_directory: &Path,
    busy: bool,
    cx: &Context<T>,
    on_refresh: impl Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
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
                    Button::new("refresh-releases")
                        .icon(refresh_icon())
                        .label(if busy || matches!(state, ReleaseViewState::Loading) {
                            "Fetching…"
                        } else {
                            "Refresh releases"
                        })
                        .outline()
                        .disabled(busy || matches!(state, ReleaseViewState::Loading))
                        .on_click(
                            cx.listener(move |this, _, window, cx| on_refresh(this, window, cx)),
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
                            .child("Click Refresh releases above to fetch upstream versions and DMGs for your repositories."),
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
                            .child("Fetching release catalog and verifying local DMGs…"),
                    ),
            )
        }
        ReleaseViewState::Loaded(repositories) if repositories.is_empty() => {
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
        ReleaseViewState::Loaded(repositories) => {
            let mut list = div().flex().flex_col().gap_4();

            for (repo_index, repository) in repositories.iter().enumerate() {
                let known = find_known_app(&repository.repository);
                let display_title = known
                    .as_ref()
                    .map(|k| k.name.clone())
                    .unwrap_or_else(|| repository.repository.clone());
                let category_name = known
                    .as_ref()
                    .map(|k| k.category_name.clone())
                    .unwrap_or_else(|| "Desktop App".to_string());

                let releases = match &repository.result {
                    Ok(rels) => rels,
                    Err(err) => {
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
                                        .child(app_badge(&repository.repository, 36.))
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
                };

                let active_releases: Vec<_> = releases.iter().filter(|r| !r.is_draft).collect();
                let latest_release = active_releases.first().copied();

                // Find candidate DMG asset or first asset
                let target_asset = latest_release.and_then(|rel| {
                    rel.assets
                        .iter()
                        .find(|a| a.name.ends_with(".dmg"))
                        .or_else(|| rel.assets.first())
                });

                // Compute local asset state
                let local_state = match (latest_release, target_asset) {
                    (Some(rel), Some(asset)) => local_asset_state(
                        project_directory,
                        &repository.owner,
                        &repository.repository,
                        &rel.tag_name,
                        &asset.name,
                        asset.size,
                    ),
                    _ => LocalAssetState::Missing,
                };

                let is_up_to_date = matches!(
                    local_state,
                    LocalAssetState::SizeMatches | LocalAssetState::PresentSizeUnknown
                );

                let newer_update = latest_release.and_then(|_| {
                    newer_release_with_local_older(
                        project_directory,
                        &repository.owner,
                        &repository.repository,
                        releases,
                    )
                });

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
                        .child("DMG ready")
                } else {
                    div()
                        .px_2p5()
                        .py_1()
                        .rounded_full()
                        .bg(cx.theme().secondary.opacity(0.2))
                        .text_color(cx.theme().muted_foreground)
                        .text_size(px(11.))
                        .font_bold()
                        .child("No DMG")
                };

                let mut card_header_actions = div().flex().items_center().gap_2();

                if let (Some(rel), Some(asset)) = (latest_release, target_asset) {
                    let provider = repository.provider.clone();
                    let owner = repository.owner.clone();
                    let repo = repository.repository.clone();
                    let tag = rel.tag_name.clone();
                    let name = asset.name.clone();
                    let url = asset.download_url.clone();
                    let size = asset.size;

                    let btn_label = if is_up_to_date {
                        "Re-add file"
                    } else {
                        "Download DMG"
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
                            .child(app_badge(&repository.repository, 44.))
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
                                                    latest_release
                                                        .and_then(|r| r.published_at.as_deref())
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

                // Slot 1: Newest on GitHub
                let slot1 = div()
                    .flex_1()
                    .min_w(px(200.))
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary.opacity(0.04))
                    .overflow_hidden()
                    .flex()
                    .items_stretch()
                    .child(div().w(px(3.5)).flex_shrink_0().bg(cx.theme().primary))
                    .child(
                        div()
                            .p_3p5()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .font_bold()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("NEWEST ON GITHUB"),
                            )
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_bold()
                                    .child(
                                        latest_release
                                            .map(|r| r.tag_name.clone())
                                            .unwrap_or_else(|| "None".to_string()),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        target_asset.map_or("No package assets".to_string(), |a| {
                                            format!("{} · {}", a.name, format_bytes(a.size))
                                        }),
                                    ),
                            ),
                    );

                // Slot 2: On the shelf
                let slot2 = div()
                    .flex_1()
                    .min_w(px(200.))
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary.opacity(0.04))
                    .overflow_hidden()
                    .flex()
                    .items_stretch()
                    .child(
                        div()
                            .w(px(3.5))
                            .flex_shrink_0()
                            .bg(if is_up_to_date {
                                cx.theme().success
                            } else {
                                cx.theme().primary.opacity(0.4)
                            }),
                    )
                    .child(
                        div()
                            .p_3p5()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .font_bold()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("ON THE SHELF"),
                            )
                            .child(div().text_size(px(15.)).font_bold().child(if is_up_to_date {
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
                                        "Click Download DMG to store locally".to_string()
                                    }),
                            ),
                    );

                // Slot 3: Previous build in old/
                let older_tag = newer_update.map(|(_, older)| older);
                let slot3 = div()
                    .flex_1()
                    .min_w(px(200.))
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary.opacity(0.04))
                    .overflow_hidden()
                    .flex()
                    .items_stretch()
                    .child(
                        div()
                            .w(px(3.5))
                            .flex_shrink_0()
                            .bg(cx.theme().muted_foreground.opacity(0.3)),
                    )
                    .child(
                        div()
                            .p_3p5()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .font_bold()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("PREVIOUS BUILD IN OLD/"),
                            )
                            .child(div().text_size(px(15.)).font_bold().child(
                                older_tag
                                    .map_or("None in shelf".to_string(), |t| format!("Archived {t}")),
                            ))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if older_tag.is_some() {
                                        "Previous build preserved in old/ folder"
                                    } else {
                                        "Earlier versions are automatically kept in old/"
                                    }),
                            ),
                    );

                let slots_row = div().flex().flex_wrap().gap_3().child(slot1).child(slot2).child(slot3);

                // Release notes / changelog block
                let notes_block = latest_release.and_then(|rel| {
                    let text = rel
                        .description
                        .as_deref()
                        .filter(|d| !d.trim().is_empty())
                        .or_else(|| {
                            known
                                .as_ref()
                                .and_then(|k| k.changelog.first().map(|s| s.as_str()))
                        })?;

                    if text.is_empty() {
                        return None;
                    }

                    Some(
                        div()
                            .p_3()
                            .rounded_lg()
                            .bg(cx.theme().secondary.opacity(0.03))
                            .border_1()
                            .border_color(cx.theme().border)
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .font_bold()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("RELEASE NOTES"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(text.to_string()),
                            ),
                    )
                });

                let mut card = div()
                    .css_card_padded(cx.theme())
                    .shadow_xs()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(card_head)
                    .child(slots_row);

                if let Some(notes) = notes_block {
                    card = card.child(notes);
                }

                list = list.child(card);
            }

            body = body.child(list);
        }
    }

    body
}
