//! Reusable badge, pill, and tag components.

use crate::components::ui_icons::check_icon;
use crate::styles::CssExt as _;
use gpui_kit::component::Theme;
use gpui_kit::{IntoElement, ParentElement as _, Styled as _, div, px, rgb};

/// Pill visual variant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PillVariant {
    Primary,
    Success,
    Warning,
    Danger,
    Neutral,
}

/// A compact status indicator pill with optional icon.
pub fn status_pill(
    text: &str,
    variant: PillVariant,
    show_check: bool,
    theme: &Theme,
) -> impl IntoElement {
    let mut pill = match variant {
        PillVariant::Primary => div().css_pill_primary(theme),
        PillVariant::Success => div().css_pill_success(theme),
        PillVariant::Warning => div().css_pill_warning(theme),
        PillVariant::Danger => div().css_pill_danger(theme),
        PillVariant::Neutral => div().css_pill_neutral(theme),
    }
    .flex()
    .items_center()
    .gap_1();

    if show_check {
        pill = pill.child(check_icon().w(px(10.)).h(px(10.)));
    }

    pill.child(text.to_string())
}

/// Version tag pill (e.g. `v0.10.0`).
pub fn version_pill(version: &str, theme: &Theme) -> impl IntoElement {
    div().css_pill_primary(theme).child(version.to_string())
}

/// Git branch chip (e.g. `main`, `master`).
pub fn branch_chip(branch: &str, theme: &Theme) -> impl IntoElement {
    div()
        .css_chip(theme)
        .child(if branch.is_empty() { "main" } else { branch }.to_string())
}

/// Language indicator chip with syntax color dot.
pub fn language_chip(language: &str, theme: &Theme) -> impl IntoElement {
    let dot_color = match language.to_lowercase().as_str() {
        "rust" => rgb(0xDEA584),
        "dart" => rgb(0x00B4AB),
        "python" => rgb(0x3572A5),
        "typescript" | "ts" => rgb(0x3178C6),
        "javascript" | "js" => rgb(0xF7DF1E),
        "swift" => rgb(0xF05138),
        "kotlin" => rgb(0xA97BFF),
        "go" => rgb(0x00ADD8),
        _ => rgb(0x8B949E),
    };

    div()
        .css_chip(theme)
        .flex()
        .items_center()
        .gap_1p5()
        .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(dot_color))
        .child(language.to_string())
}

/// Compact 18x18 native-style checkbox matching the HTML design preview.
pub fn checkbox(checked: bool, theme: &Theme) -> impl IntoElement {
    let mut box_el = div()
        .w(px(18.))
        .h(px(18.))
        .rounded(px(4.))
        .border_1()
        .border_color(if checked { theme.primary } else { theme.border })
        .bg(if checked {
            theme.primary
        } else {
            theme.background
        })
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0();

    if checked {
        box_el = box_el.child(
            check_icon()
                .w(px(12.))
                .h(px(12.))
                .text_color(theme.primary_foreground),
        );
    }

    box_el
}
