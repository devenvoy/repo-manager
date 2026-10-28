//! Modular avatar rendering component for user, organization, and repository profiles.

use crate::components::app_icon::{app_badge, repo_icon};
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::Theme;
use gpui_kit::{IntoElement, ParentElement as _, Styled as _, div, img, px};
use std::path::Path;

/// Renders a circular user or organization avatar with image or monogram fallback.
pub fn source_avatar(
    handle: &str,
    avatar_file: Option<&Path>,
    size: f32,
    theme: &Theme,
) -> impl IntoElement {
    if let Some(path) = avatar_file {
        if path.is_file() {
            return div()
                .w(px(size))
                .h(px(size))
                .rounded_full()
                .border_2()
                .border_color(theme.border)
                .overflow_hidden()
                .child(img(path.to_path_buf()).size_full().rounded_full())
                .into_any_element();
        }
    }

    let initial = handle
        .chars()
        .find(|c| c.is_alphanumeric())
        .unwrap_or('S')
        .to_uppercase()
        .to_string();

    let font_size = (size * 0.42).max(11.);

    div()
        .w(px(size))
        .h(px(size))
        .rounded_full()
        .border_2()
        .border_color(theme.primary.opacity(0.4))
        .bg(theme.primary.opacity(0.12))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(font_size))
        .font_bold()
        .text_color(theme.primary)
        .child(initial)
        .into_any_element()
}

/// Renders a repository badge icon (with real artwork if available, or procedural gradient monogram).
pub fn repo_avatar(name: &str, size: f32, icon_path: Option<&Path>) -> impl IntoElement {
    repo_icon(name, size, icon_path)
}

/// Backward compatible repo avatar fallback.
pub fn repo_avatar_default(name: &str, size: f32) -> impl IntoElement {
    app_badge(name, size)
}
