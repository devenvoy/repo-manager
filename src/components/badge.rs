//! Reusable badge, pill, and tag components.

use crate::colors::rgb;
use crate::components::icons::{branch_icon, check_icon, globe_icon, lock_icon};
use crate::icon_catalog::{Language, LanguageColor, language_icon_for_name};
use crate::styles::CssExt as _;
use gpui_kit::component::Theme;
use gpui_kit::{IntoElement, ParentElement as _, Styled as _, div, px};

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
    .gap_1p5();

    if show_check {
        pill = pill.child(check_icon().w(px(13.)).h(px(13.)));
    }

    pill.child(text.to_string())
}

/// Version tag pill (e.g. `v0.10.0`).
pub fn version_pill(version: &str, theme: &Theme) -> impl IntoElement {
    div().css_pill_primary(theme).child(version.to_string())
}

/// Git branch chip (e.g. `main`, `master`) with branch icon.
pub fn branch_chip(branch: &str, theme: &Theme) -> impl IntoElement {
    div()
        .css_chip(theme)
        .flex()
        .items_center()
        .gap_2()
        .child(branch_icon().w(px(13.)).h(px(13.)))
        .child(if branch.is_empty() { "main" } else { branch }.to_string())
}

/// Visibility chip (Public / Private) with globe or lock icon.
pub fn visibility_chip(is_private: bool, theme: &Theme) -> impl IntoElement {
    div()
        .css_chip(theme)
        .flex()
        .items_center()
        .gap_2()
        .child(if is_private {
            lock_icon().w(px(13.)).h(px(13.))
        } else {
            globe_icon().w(px(13.)).h(px(13.))
        })
        .child(if is_private { "Private" } else { "Public" }.to_string())
}

/// Language indicator chip with syntax color dot.
pub fn language_chip(language: &str, theme: &Theme) -> impl IntoElement {
    let dot_color = rgb(Language::from_name(language)
        .map_or(LanguageColor::Unknown, Language::color)
        .rgb());

    div()
        .css_chip(theme)
        .flex()
        .items_center()
        .gap_2()
        .child(language_icon_for_name(language).w(px(14.)).h(px(14.)))
        .child(div().w(px(8.)).h(px(8.)).rounded_full().bg(dot_color))
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
