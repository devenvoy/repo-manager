//! Interactive import review and confirmation dialog.
//!
//! Allows users to inspect discovered sources, edit or confirm the detected handle,
//! review avatar and provider details, and selectively choose repositories to import.

use crate::components::avatar::source_avatar;
use crate::components::badge::{branch_chip, checkbox, language_chip, version_pill};
use crate::components::motion_button::Button;
use crate::components::provider_icon::provider_icon;
use crate::modules::importer::DiscoveredWorkspace;
use crate::styles::CssExt as _;
use gpui_kit::base::{Disableable as _, StyledExt as _};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    AppContext as _, Context, ElementId, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use std::path::PathBuf;

/// State for the active import confirmation dialog.
pub struct ImportDialogState {
    pub project_id: u128,
    pub project_dir: PathBuf,
    pub discovered: DiscoveredWorkspace,
    pub handle_input: Entity<InputState>,
    pub selected_indices: Vec<bool>,
    pub avatar_path: Option<PathBuf>,
}

impl ImportDialogState {
    pub fn new<T: 'static>(
        project_id: u128,
        project_dir: PathBuf,
        discovered: DiscoveredWorkspace,
        window: &mut Window,
        cx: &mut Context<T>,
    ) -> Self {
        let repo_count = discovered.repositories.len();
        let handle_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Organization or user handle"));
        handle_input.update(cx, |input, cx| {
            input.set_value(discovered.source_handle.clone(), window, cx);
        });

        let avatar_path = crate::settings::settings_path().ok().and_then(|p| {
            p.parent().map(|dir| {
                let safe = |part: &str| {
                    part.chars()
                        .map(|ch| {
                            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                                ch
                            } else {
                                '_'
                            }
                        })
                        .collect::<String>()
                };
                dir.join("avatars").join(format!(
                    "{}-{}.png",
                    safe(&discovered.provider),
                    safe(&discovered.source_handle)
                ))
            })
        });

        Self {
            project_id,
            project_dir,
            discovered,
            handle_input,
            selected_indices: vec![true; repo_count],
            avatar_path,
        }
    }

    pub fn selected_count(&self) -> usize {
        self.selected_indices.iter().filter(|&&s| s).count()
    }

    pub fn toggle_repo(&mut self, index: usize) {
        if let Some(val) = self.selected_indices.get_mut(index) {
            *val = !*val;
        }
    }

    pub fn set_all_selected(&mut self, selected: bool) {
        self.selected_indices.fill(selected);
    }
}

/// Render the interactive import modal overlay.
pub fn render_import_dialog<T: 'static>(
    state: &ImportDialogState,
    cx: &Context<T>,
    on_toggle: impl Fn(usize, &mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    on_select_all: impl Fn(bool, &mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    on_confirm: impl Fn(String, &mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    on_cancel: impl Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    let all_selected = state.selected_indices.iter().all(|&s| s);
    let selected_count = state.selected_count();
    let total_count = state.discovered.repositories.len();

    // Backdrop with modular CSS styling
    div()
        .css_modal_backdrop()
        .child(
            // Dialog container
            div()
                .css_modal_dialog(cx.theme())
                // Dialog Header
                .child(
                    div()
                        .p_5()
                        .border_b_1()
                        .border_color(cx.theme().border)
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
                                        .flex()
                                        .items_center()
                                        .gap_2p5()
                                        .child(
                                            div()
                                                .text_size(px(18.))
                                                .font_bold()
                                                .child("Import Discovered Repositories"),
                                        )
                                        .child(
                                            if state.discovered.detected_from_data_json {
                                                div().css_pill_success(cx.theme()).child("Snapshot: .release-desk/data.json")
                                            } else {
                                                div().css_pill_primary(cx.theme()).child("Discovered Git Checkouts")
                                            },
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!(
                                            "Review and confirm metadata found in {}",
                                            state.project_dir.display()
                                        )),
                                ),
                        )
                        .child(
                            Button::new("import-close-btn")
                                .label("✕")
                                .ghost()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    on_cancel(this, window, cx);
                                })),
                        ),
                )
                // Source Profile Review Card
                .child(
                    div()
                        .css_card_subtle(cx.theme())
                        .mx_5()
                        .mt_4()
                        .flex()
                        .items_center()
                        .gap_4()
                        .child(source_avatar(
                            &state.discovered.source_handle,
                            state.avatar_path.as_deref(),
                            52.,
                            cx.theme(),
                        ))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap_1p5()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .font_bold()
                                                .text_color(cx.theme().muted_foreground)
                                                .child("CONFIRM SOURCE USER / ORG"),
                                        )
                                        .child(
                                            div()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded_md()
                                                .bg(cx.theme().background)
                                                .border_1()
                                                .border_color(cx.theme().border)
                                                .text_size(px(9.))
                                                .font_bold()
                                                .flex()
                                                .items_center()
                                                .gap_1()
                                                .child(provider_icon(crate::workspace::ProviderChoice::GitHub).w(px(12.)).h(px(12.)))
                                                .child("GitHub"),
                                        ),
                                )
                                .child(Input::new(&state.handle_input)),
                        ),
                )
                // Repositories List Toolbar
                .child(
                    div()
                        .px_5()
                        .pt_4()
                        .pb_2()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_bold()
                                .text_color(cx.theme().foreground)
                                .child(format!(
                                    "Discovered Repositories ({selected_count} of {total_count} selected)"
                                )),
                        )
                        .child(
                            Button::new("toggle-all-repos")
                                .label(if all_selected { "Deselect All" } else { "Select All" })
                                .ghost()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    on_select_all(!all_selected, this, window, cx);
                                })),
                        ),
                )
                // Repositories Scrollable List Container (.repo-pick style)
                .child(
                    div()
                        .mx_5()
                        .my_1()
                        .flex_1()
                        .min_h_0()
                        .rounded_xl()
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().background)
                        .overflow_y_scrollbar()
                        .child({
                            let mut list = div().flex().flex_col();

                            for (idx, repo) in state.discovered.repositories.iter().enumerate() {
                                let is_selected = state.selected_indices.get(idx).copied().unwrap_or(false);
                                let name_clone = repo.name.clone();

                                let mut title_row = div().flex().items_center().gap_2().child(
                                    div()
                                        .font_bold()
                                        .text_size(px(13.))
                                        .truncate()
                                        .child(name_clone),
                                );
                                if repo.is_private {
                                    title_row = title_row.child(
                                        div()
                                            .css_chip(cx.theme())
                                            .child("Private"),
                                    );
                                }

                                let mut meta_row = div().flex().items_center().gap_1p5();
                                if let Some(ref ver) = repo.latest_version {
                                    meta_row = meta_row.child(version_pill(ver, cx.theme()));
                                }
                                meta_row = meta_row.child(branch_chip(&repo.default_branch, cx.theme()));
                                if let Some(ref lang) = repo.primary_language {
                                    meta_row = meta_row.child(language_chip(lang, cx.theme()));
                                }

                                let repo_dir = state.project_dir.join(&repo.name);
                                let icon_path = crate::components::app_icon::find_local_repo_icon(&repo_dir, &repo.name)
                                    .or_else(|| crate::components::app_icon::find_cached_repo_icon(&repo.name, Some(&state.discovered.source_handle)));

                                let item = div()
                                    .id(ElementId::Name(format!("pick-row-{idx}").into()))
                                    .px_3p5()
                                    .py_2p5()
                                    .border_b_1()
                                    .border_color(cx.theme().border.opacity(0.5))
                                    .bg(if is_selected {
                                        cx.theme().primary.opacity(0.04)
                                    } else {
                                        cx.theme().background
                                    })
                                    .hover(|s| s.bg(cx.theme().muted.opacity(0.25)))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        on_toggle(idx, this, window, cx);
                                    }))
                                    .child(checkbox(is_selected, cx.theme()))
                                    .child(crate::components::app_icon::repo_icon(&repo.name, 30., icon_path.as_deref()))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap_0p5()
                                            .child(title_row)
                                            .child(
                                                div()
                                                    .text_size(px(11.))
                                                    .text_color(cx.theme().muted_foreground)
                                                    .line_clamp(1)
                                                    .child(
                                                        repo.description
                                                            .clone()
                                                            .unwrap_or_else(|| "Application repository".to_string()),
                                                    ),
                                            ),
                                    )
                                    .child(meta_row);

                                list = list.child(item);
                            }

                            list
                        }),
                )
                // Dialog Footer Action Bar
                .child(
                    div()
                        .p_4()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().background)
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{selected_count} repositories will be managed")),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    Button::new("cancel-import-dialog")
                                        .label("Cancel")
                                        .outline()
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            on_cancel(this, window, cx);
                                        })),
                                )
                                .child(
                                    Button::new("confirm-import-dialog")
                                        .label(format!("Import {selected_count} Repositories"))
                                        .primary()
                                        .disabled(selected_count == 0)
                                        .on_click(cx.listener({
                                            move |this, _, window, cx| {
                                                on_confirm(String::new(), this, window, cx);
                                            }
                                        })),
                                ),
                        ),
                ),
        )
}
