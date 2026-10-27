//! Desktop application settings panel matching the modern design prototype.

use crate::components::toggle_button;
use crate::components::ui_icons::shield_icon;
use crate::settings::{Appearance, HubViewMode, Settings, StartView};
use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{Context, IntoElement, ParentElement as _, Styled as _, Window, div, px};

pub fn render_settings<
    T: 'static,
    FTheme: Fn(&mut T, Appearance, &Window, &mut Context<T>) + Copy + 'static,
    FStartView: Fn(&mut T, StartView, &mut Context<T>) + Copy + 'static,
    FLayout: Fn(&mut T, HubViewMode, &mut Context<T>) + Copy + 'static,
    FSave: Fn(&mut T, &mut Context<T>) + Copy + 'static,
>(
    settings: &Settings,
    cx: &Context<T>,
    on_theme: FTheme,
    on_start_view: FStartView,
    on_layout: FLayout,
    on_save: FSave,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_6()
        .p_6()
        .overflow_y_scrollbar()
        .size_full()
        // Header
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
                        .child("APPLICATION PREFERENCES"),
                )
                .child(div().text_size(px(27.)).font_bold().child("Settings"))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(cx.theme().muted_foreground)
                        .child("Manage visual appearance, storage locations, and Git defaults on this machine."),
                ),
        )
        // Appearance & Visual Style
        .child(
            div()
                .p_5()
                .rounded_xl()
                .bg(cx.theme().secondary)
                .border_1()
                .border_color(cx.theme().border)
                .flex()
                .flex_col()
                .gap_3()
                .child(div().text_size(px(15.)).font_bold().child("Visual Appearance"))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(cx.theme().muted_foreground)
                        .child("Select your preferred studio interface appearance:"),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            toggle_button(
                                "set-theme-light",
                                "Light Studio",
                                settings.appearance == Appearance::Light,
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                on_theme(this, Appearance::Light, window, cx);
                            })),
                        )
                        .child(
                            toggle_button(
                                "set-theme-dark",
                                "Dark Studio",
                                settings.appearance == Appearance::Dark,
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                on_theme(this, Appearance::Dark, window, cx);
                            })),
                        )
                        .child(
                            toggle_button(
                                "set-theme-system",
                                "Follow System (Auto)",
                                settings.appearance == Appearance::System,
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                on_theme(this, Appearance::System, window, cx);
                            })),
                        ),
                ),
        )
        // Default Workspace Start View
        .child(
            div()
                .p_5()
                .rounded_xl()
                .bg(cx.theme().secondary)
                .border_1()
                .border_color(cx.theme().border)
                .flex()
                .flex_col()
                .gap_3()
                .child(div().text_size(px(15.)).font_bold().child("Startup Navigation"))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(cx.theme().muted_foreground)
                        .child("Choose which view opens when launching Release Desk:"),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            toggle_button(
                                "start-projects",
                                "Project Hub",
                                settings.start_view == StartView::ProjectHub,
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                on_start_view(this, StartView::ProjectHub, cx);
                            })),
                        )
                        .child(
                            toggle_button(
                                "start-store",
                                "App Store",
                                settings.start_view == StartView::AppStore,
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                on_start_view(this, StartView::AppStore, cx);
                            })),
                        ),
                ),
        )
        // Default Layout Preference
        .child(
            div()
                .p_5()
                .rounded_xl()
                .bg(cx.theme().secondary)
                .border_1()
                .border_color(cx.theme().border)
                .flex()
                .flex_col()
                .gap_3()
                .child(div().text_size(px(15.)).font_bold().child("Default Project Disk Layout"))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(cx.theme().muted_foreground)
                        .child("Layout preference for newly created projects on disk:"),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            toggle_button(
                                "layout-grouped",
                                "Grouped (<project>/<owner>/<repo>)",
                                settings.hub_view_mode == HubViewMode::Grouped,
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                on_layout(this, HubViewMode::Grouped, cx);
                            })),
                        )
                        .child(
                            toggle_button(
                                "layout-flat",
                                "Flat (<project>/<repo>)",
                                settings.hub_view_mode == HubViewMode::Flat,
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                on_layout(this, HubViewMode::Flat, cx);
                            })),
                        ),
                ),
        )
        // System & Security
        .child(
            div()
                .p_5()
                .rounded_xl()
                .bg(cx.theme().muted)
                .border_1()
                .border_color(cx.theme().border)
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(shield_icon())
                        .child(div().text_size(px(15.)).font_bold().child("Security & Local Invariants")),
                )
                .child(setting_info("Applications Directory", &settings.applications_dir, cx))
                .child(setting_info("Managed Library", &settings.library_dir, cx))
                .child(setting_info("Credential Security", "Encrypted in macOS Keychain per project", cx))
                .child(setting_info("Offline Redundancy", "Automatic .release-desk/data.json per clone folder", cx)),
        )
        .child(
            Button::new("save-settings-btn")
                .label("Save Preferences")
                .primary()
                .on_click(cx.listener(move |this, _, _, cx| on_save(this, cx))),
        )
}

fn setting_info<T: 'static>(label: &str, value: &str, cx: &Context<T>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .py_1p5()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .text_size(px(12.))
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
        .child(
            div()
                .text_size(px(12.))
                .font_bold()
                .text_color(cx.theme().foreground)
                .child(value.to_string()),
        )
}
