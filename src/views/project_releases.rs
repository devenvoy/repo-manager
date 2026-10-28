//! Release shelf backed by fetched provider data and ArtCraft application catalog.

use crate::components::app_icon::repo_icon;
use crate::components::icons::{check_icon, download_icon, refresh_icon};
use crate::components::motion_button::Button;
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

fn package_kind(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    if lower.ends_with(".dmg") {
        "DMG"
    } else if lower.ends_with(".pkg") {
        "PKG"
    } else if lower.ends_with(".exe") {
        "EXE"
    } else if lower.ends_with(".msi") {
        "MSI"
    } else if lower.ends_with(".appimage") {
        "AppImage"
    } else if lower.ends_with(".deb") {
        "DEB"
    } else if lower.ends_with(".rpm") {
        "RPM"
    } else if lower.ends_with(".apk") {
        "APK"
    } else if lower.ends_with(".aab") {
        "AAB"
    } else if lower.ends_with(".ipa") {
        "IPA"
    } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        "Tarball"
    } else if lower.ends_with(".tar.xz") {
        "Tarball (xz)"
    } else if lower.ends_with(".zip") {
        "ZIP"
    } else {
        "Package"
    }
}

fn is_supported_package(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".dmg")
        || lower.ends_with(".pkg")
        || lower.ends_with(".exe")
        || lower.ends_with(".msi")
        || lower.ends_with(".appimage")
        || lower.ends_with(".deb")
        || lower.ends_with(".rpm")
        || lower.ends_with(".apk")
        || lower.ends_with(".aab")
        || lower.ends_with(".ipa")
        || lower.ends_with(".tar.gz")
        || lower.ends_with(".tgz")
        || lower.ends_with(".tar.xz")
        || lower.ends_with(".zip")
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

                let repo_dir = crate::repository_git::resolve_target_path(
                    project_directory,
                    release_desk_core::project::DiskLayout::Flat,
                    &repository.owner,
                    &repository.repository,
                );
                let icon_path = crate::components::app_icon::find_local_repo_icon(
                    &repo_dir,
                    &repository.repository,
                )
                .or_else(|| {
                    crate::components::app_icon::find_cached_repo_icon(
                        &repository.repository,
                        Some(&repository.owner),
                    )
                });

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
                                        .child(repo_icon(
                                            &repository.repository,
                                            36.,
                                            icon_path.as_deref(),
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
                };

                let active_releases: Vec<_> = releases.iter().filter(|r| !r.is_draft).collect();
                let latest_release = active_releases.first().copied();

                // Find candidate package asset
                let target_asset = latest_release.and_then(|rel| {
                    rel.assets
                        .iter()
                        .find(|a| is_supported_package(&a.name))
                        .or_else(|| rel.assets.first())
                });
                let pkg_kind = target_asset
                    .map(|a| package_kind(&a.name))
                    .unwrap_or("Package");

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
                        .child("No package")
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
                            .child(repo_icon(
                                &repository.repository,
                                44.,
                                icon_path.as_deref(),
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
                let archived_files = release_desk_store::workspace_releases::local_archived_files(
                    project_directory,
                    &repository.repository,
                );
                let (slot3_label, slot3_subtext) = if let Some((arch_name, arch_size)) = archived_files.first() {
                    (
                        format!("{arch_name} · {}", format_bytes(Some(*arch_size))),
                        "Archived build preserved in old/ folder".to_string(),
                    )
                } else if let Some((_, older)) = newer_update {
                    (
                        format!("Archived {older}"),
                        "Previous build preserved in old/ folder".to_string(),
                    )
                } else {
                    (
                        "None in shelf".to_string(),
                        "Earlier versions are automatically kept in old/".to_string(),
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

                // Release notes / changelog block
                let notes_block = latest_release.and_then(|rel| {
                    let text = rel
                        .description
                        .as_deref()
                        .filter(|d| !d.trim().is_empty())?;

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
