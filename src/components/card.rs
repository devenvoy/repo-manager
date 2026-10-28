//! Modular card and section container components.

use crate::styles::CssExt as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::Theme;
use gpui_kit::{AnyElement, IntoElement, ParentElement as _, Styled as _, div, px};

/// Metric summary counter card used in overview and headers.
pub fn summary_stat_card(label: &str, count: usize, theme: &Theme) -> impl IntoElement {
    div()
        .css_card(theme)
        .flex_1()
        .min_w(px(150.))
        .p_4()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_size(px(25.))
                .font_bold()
                .child(count.to_string()),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme.muted_foreground)
                .child(label.to_string()),
        )
}

/// Key-value property row used in setting cards.
pub fn property_row(label: &str, value: &str, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .py_2p5()
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .text_size(px(13.))
                .text_color(theme.muted_foreground)
                .child(label.to_string()),
        )
        .child(
            div()
                .text_size(px(13.))
                .font_bold()
                .child(value.to_string()),
        )
}

/// Standardized section header with category eyebrow, heading, and subtitle.
pub fn section_header(
    eyebrow: &str,
    heading: &str,
    subtitle: &str,
    actions: Option<AnyElement>,
    theme: &Theme,
) -> impl IntoElement {
    let mut header = div()
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
                .child(div().css_section_eyebrow(theme).child(eyebrow.to_string()))
                .child(div().css_heading_title().child(heading.to_string()))
                .child(div().css_body_muted(theme).child(subtitle.to_string())),
        );

    if let Some(act) = actions {
        header = header.child(act);
    }

    header
}
