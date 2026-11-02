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
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Animation, AnimationExt as _, Context, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, ScrollHandle, StatefulInteractiveElement as _, Styled as _, Window,
    deferred, div, px,
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
                if release.is_draft {
                    continue;
                }
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
        None => "Date unavailable".to_string(),
    }
}

fn detail_chip(
    label: &'static str,
    value: String,
    theme: &gpui_kit::component::Theme,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .py_1()
        .rounded_md()
        .bg(theme.secondary.opacity(0.2))
        .text_size(px(11.))
        .child(div().text_color(theme.muted_foreground).child(label))
        .child(div().font_medium().child(value))
}

fn release_detail_row(
    label: &str,
    version: &str,
    published_at: Option<&str>,
    size: Option<u64>,
    filename: &str,
    theme: &gpui_kit::component::Theme,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .flex_wrap()
        .gap_2()
        .child(
            div()
                .w(px(96.))
                .text_size(px(11.))
                .font_bold()
                .text_color(theme.muted_foreground)
                .child(label.to_string()),
        )
        .child(detail_chip("Version", version.to_string(), theme))
        .child(detail_chip("Released", format_date(published_at), theme))
        .child(detail_chip("Size", format_bytes(size), theme))
        .child(
            div()
                .text_size(px(11.))
                .text_color(theme.muted_foreground)
                .child(filename.to_string()),
        )
}

#[derive(Clone, Copy)]
pub(crate) struct ReleaseShelfControls<'a> {
    pub busy: bool,
    pub downloading: &'a std::collections::HashSet<String>,
    pub project_id: u128,
    pub(crate) queue: &'a [crate::release_queue::DownloadJob],
    pub queue_open: bool,
    pub extension: &'a str,
    pub type_menu_open: bool,
    pub scroll_handle: &'a ScrollHandle,
    pub scroll_generation: &'a std::rc::Rc<std::cell::Cell<u64>>,
}

pub type ReleaseDownloadRequest = (String, String, String, String, String, String, Option<u64>);

pub(crate) fn render_download_queue<T: 'static>(
    queue: &[crate::release_queue::DownloadJob],
    project_id: u128,
    open: bool,
    cx: &Context<T>,
    on_toggle: impl Fn(&mut T, &mut Context<T>) + Copy + 'static,
    on_cancel: impl Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    let project_jobs = queue
        .iter()
        .filter(|job| job.project_id == project_id)
        .collect::<Vec<_>>();
    let active = project_jobs.iter().filter(|job| job.is_active()).count();
    let queued = project_jobs
        .iter()
        .filter(|job| matches!(job.status, crate::release_queue::DownloadStatus::Queued))
        .count();
    let completed = project_jobs.len().saturating_sub(active);
    let mut root = div()
        .absolute()
        .bottom(px(18.))
        .right(px(20.))
        .flex()
        .flex_col()
        .items_end()
        .gap_2();
    if !project_jobs.is_empty() {
        root = root.child(
            Button::new("release-queue-snackbar")
                .icon(download_icon())
                .label(format!(
                    "Downloads · {} downloading · {queued} queued · {completed} done",
                    active.saturating_sub(queued)
                ))
                .primary()
                .on_click(cx.listener(move |this, _, _, cx| on_toggle(this, cx))),
        );
    }
    if open {
        let mut rows = div()
            .flex()
            .flex_col()
            .gap_2()
            .max_h(px(440.))
            .overflow_y_scrollbar();
        for job in project_jobs {
            let job_key = job.key.clone();
            let mut row = div()
                .flex()
                .flex_col()
                .gap_1()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .p_3()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .child(
                                    div()
                                        .font_medium()
                                        .text_size(px(13.))
                                        .child(job.repository.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(job.asset.clone()),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(match &job.status {
                                            crate::release_queue::DownloadStatus::Queued => {
                                                "Queued".to_string()
                                            }
                                            crate::release_queue::DownloadStatus::Downloading => {
                                                format!(
                                                    "Downloading · {} / {}",
                                                    format_bytes(Some(job.received)),
                                                    job.total
                                                        .map_or("Unknown size".to_string(), |v| {
                                                            format_bytes(Some(v))
                                                        })
                                                )
                                            }
                                            crate::release_queue::DownloadStatus::Complete => {
                                                "Complete".to_string()
                                            }
                                            crate::release_queue::DownloadStatus::Failed(error) => {
                                                format!("Failed · {error}")
                                            }
                                            crate::release_queue::DownloadStatus::Cancelled => {
                                                "Cancelled".to_string()
                                            }
                                        }),
                                )
                                .when(job.is_active(), |element| {
                                    element.child(
                                        Button::new(format!("cancel-{}", job.key))
                                            .label("Cancel")
                                            .outline()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                on_cancel(job_key.clone(), this, cx)
                                            })),
                                    )
                                }),
                        ),
                );
            if let Some(progress) = job.progress() {
                row = row.child(
                    div()
                        .h(px(4.))
                        .w_full()
                        .rounded_full()
                        .bg(cx.theme().secondary.opacity(0.4))
                        .child(
                            div()
                                .h(px(4.))
                                .rounded_full()
                                .bg(cx.theme().primary)
                                .w(px(320. * progress)),
                        ),
                );
            }
            rows = rows.child(row);
        }
        root = root.child(
            div()
                .id("release-queue-backdrop")
                .absolute()
                .inset_0()
                .bg(cx.theme().background.opacity(0.55))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(move |this, _, _, cx| on_toggle(this, cx)))
                .child(
                    div()
                        .id("release-queue-dialog")
                        .w(px(560.))
                        .max_h(px(560.))
                        .flex()
                        .flex_col()
                        .gap_3()
                        .rounded_2xl()
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().background)
                        .shadow_lg()
                        .p_5()
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(div().text_size(px(17.)).font_bold().child("Download queue"))
                                .child(
                                    Button::new("close-release-queue")
                                        .label("Close")
                                        .ghost()
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| on_toggle(this, cx)),
                                        ),
                                ),
                        )
                        .child(rows),
                ),
        );
    }
    root
}

/// Render the release shelf matching the ArtCraft Release Desk layout.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_live_releases<T: 'static>(
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
    on_download_all: impl Fn(Vec<ReleaseDownloadRequest>, &mut T, &mut Window, &mut Context<T>)
    + Copy
    + 'static,
    on_toggle_queue: impl Fn(&mut T, &mut Context<T>) + Copy + 'static,
    on_cancel_queue_item: impl Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    let ReleaseShelfControls {
        busy,
        downloading,
        project_id,
        queue,
        queue_open,
        extension,
        type_menu_open,
        scroll_handle,
        scroll_generation,
    } = controls;
    let available_types = available_release_types(state, extension);
    let selected_type = release_asset_type_for_extension(extension);
    let selected_label = selected_type
        .map(|kind| kind.label.to_string())
        .unwrap_or_else(|| extension.to_ascii_uppercase());
    let bulk_requests = match state {
        ReleaseViewState::Loaded {
            repositories,
            presentations,
            ..
        } => repositories
            .iter()
            .zip(presentations)
            .filter_map(|(repository, presentation)| {
                if matches!(
                    presentation.local_state,
                    LocalAssetState::SizeMatches | LocalAssetState::PresentSizeUnknown
                ) {
                    return None;
                }
                let asset = presentation.asset.as_ref()?;
                let tag = presentation.latest_tag.as_ref()?;
                Some((
                    repository.provider.clone(),
                    repository.owner.clone(),
                    repository.repository.clone(),
                    tag.clone(),
                    asset.name.clone(),
                    asset.download_url.clone(),
                    asset.size,
                ))
            })
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };

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
                .child("DOWNLOAD FORMAT"),
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

    let scroll_handle = scroll_handle.clone();
    let _ = scroll_generation;
    let mut body = div()
        .id("release-list-scroll")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap_5()
        .overflow_y_scroll()
        .track_scroll(&scroll_handle)
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
                        .child(div().css_section_eyebrow(cx.theme()).child("DOWNLOADS"))
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
                                    "Checking…"
                                } else {
                                    "Check releases"
                                })
                                .outline()
                                .disabled(busy || matches!(state, ReleaseViewState::Loading))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    on_refresh(this, window, cx)
                                })),
                        )
                        .child(
                            Button::new("download-all-releases")
                                .icon(download_icon())
                                .label("Download all")
                                .primary()
                                .disabled(busy || bulk_requests.is_empty())
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    on_download_all(bulk_requests.clone(), this, window, cx);
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
                            .child("Find available downloads"),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(cx.theme().muted_foreground)
                            .text_center()
                            .child("Check releases to see available versions and files for your repositories."),
                    )
                    .child(
                        Button::new("shelf-initial-refresh")
                            .icon(refresh_icon())
                            .label("Check releases now")
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
                            .child("Checking releases and downloaded files…"),
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
            let mut list = div().flex().flex_col().gap_2();
            let mut matched_any = false;
            let mut fetch_errors = 0;

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
                return div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(body)
                    .vertical_scrollbar(&scroll_handle);
            }

            for ((repo_index, repository), presentation) in
                repositories.iter().enumerate().zip(presentations)
            {
                let Some(asset) = presentation.asset.as_ref() else {
                    if repository.result.is_err() {
                        fetch_errors += 1;
                    }
                    continue;
                };
                matched_any = true;
                let known = find_known_app(&repository.repository);
                let title = known
                    .as_ref()
                    .map(|app| app.name.clone())
                    .unwrap_or_else(|| repository.repository.clone());
                let latest_tag = presentation.latest_tag.as_deref().unwrap_or("Version unavailable");
                let current = presentation.downloaded.as_ref();
                let up_to_date = matches!(
                    presentation.local_state,
                    LocalAssetState::SizeMatches | LocalAssetState::PresentSizeUnknown
                );
                let status_text = if up_to_date {
                    "Latest downloaded"
                } else if presentation.update_available {
                    "New version available"
                } else {
                    "Ready to download"
                };
                let status_color = if up_to_date {
                    cx.theme().success
                } else if presentation.update_available {
                    cx.theme().warning
                } else {
                    cx.theme().primary
                };
                let provider = repository.provider.clone();
                let owner = repository.owner.clone();
                let repo = repository.repository.clone();
                let tag = latest_tag.to_string();
                let name = asset.name.clone();
                let url = asset.download_url.clone();
                let size = asset.size;
                let download_key = format!("{project_id}/{owner}/{repo}/{tag}/{name}");
                let is_downloading = downloading.contains(&download_key);
                let button_label = if is_downloading {
                    if presentation.update_available { "Updating…" } else { "Downloading…" }
                } else if up_to_date {
                    "Download again"
                } else {
                    "Download latest"
                };

                let header = div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap_3()
                    .child(
                        div().flex().items_center().gap_3()
                            .child(repo_icon_with_provider(
                                &repository.repository, 38., presentation.icon_path.as_deref(),
                                Some(&repository.provider),
                            ))
                            .child(div().flex().flex_col().gap_0p5()
                                .child(div().text_size(px(15.)).font_bold().child(title))
                                .child(div().text_size(px(11.)).text_color(cx.theme().muted_foreground)
                                    .child(repository.owner.clone())))
                    .child(div().px_2().py_1().rounded_full()
                                .bg(status_color.opacity(0.12))
                        .text_color(if is_downloading { cx.theme().primary } else { status_color }).text_size(px(11.)).font_bold()
                        .child(if is_downloading && presentation.update_available { "Updating" } else if is_downloading { "Downloading" } else { status_text })),
                    )
                    .child(Button::new(format!("shelf-dl-{repo_index}"))
                        .icon(download_icon())
                        .label(button_label)
                        .primary()
                        .disabled(is_downloading || busy)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            on_download(
                                provider.clone(), owner.clone(), repo.clone(), tag.clone(),
                                name.clone(), url.clone(), size, this, window, cx,
                            );
                        })));

                let latest_label = format!("Latest {selected_label}");
                let latest = release_detail_row(
                    &latest_label, latest_tag, presentation.published_at.as_deref(),
                    asset.size, &asset.name, cx.theme(),
                );
                let downloaded = if let Some(current) = current {
                    release_detail_row(
                        "Downloaded", &current.tag, current.published_at.as_deref(),
                        current.size, &current.name, cx.theme(),
                    ).into_any_element()
                } else {
                    div().flex().items_center().gap_2()
                        .child(div().w(px(96.)).text_size(px(11.)).font_bold()
                            .text_color(cx.theme().muted_foreground).child("Downloaded"))
                        .child(div().text_size(px(11.)).text_color(cx.theme().muted_foreground)
                            .child("No downloaded file yet"))
                        .into_any_element()
                };

                let mut card = div().css_card(cx.theme()).p_3()
                    .flex().flex_col().gap_2p5()
                    .child(header)
                    .child(latest)
                    .child(downloaded);
                if let Some((archived_name, archived_size)) = presentation.archived_file.as_ref() {
                    let archived_release = repository.result.as_ref().ok().and_then(|releases| {
                        releases.iter().find(|release| release.assets.iter().any(|asset| &asset.name == archived_name))
                    });
                    card = card.child(release_detail_row(
                        "Previous file",
                        archived_release.map_or("Unknown", |release| release.tag_name.as_str()),
                        archived_release.and_then(|release| release.published_at.as_deref()),
                        Some(*archived_size), archived_name, cx.theme(),
                    ));
                }
                list = list.child(card.with_animation(
                    format!("release-row-{}-{}-{}", repository.owner, repository.repository, extension),
                    Animation::new(std::time::Duration::from_millis(220))
                        .with_easing(gpui_kit::ease_in_out),
                    |row, progress| row.opacity(progress).top(px((1.0 - progress) * 6.0)),
                ));
            }

            if !matched_any {
                list = list.child(
                    div()
                        .css_empty_state(cx.theme())
                        .child(
                            div()
                                .font_bold()
                                .text_size(px(16.))
                                .child(format!("No {selected_label} downloads available")),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(cx.theme().muted_foreground)
                                .text_center()
                                .child(if fetch_errors > 0 {
                                    format!("No downloadable {selected_label} files were found. {fetch_errors} repositories could not be checked; try Check releases again.")
                                } else {
                                    "Choose another download format or check releases again.".to_string()
                                }),
                        ),
                );
            } else if fetch_errors > 0 {
                list = list.child(div().text_size(px(11.)).text_color(cx.theme().muted_foreground)
                    .child(format!("{fetch_errors} repositories could not be checked. Check releases to try again.")));
            }

            body = body.child(list);
        }
    }

    let mut shelf = div()
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .child(body)
        .vertical_scrollbar(&scroll_handle);
    shelf = shelf.child(render_download_queue(
        queue,
        project_id,
        queue_open,
        cx,
        on_toggle_queue,
        on_cancel_queue_item,
    ));
    shelf
}
