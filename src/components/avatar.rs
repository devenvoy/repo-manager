//! Modular avatar rendering component for user, organization, and repository profiles.

use crate::colors::rgb;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::Theme;
use gpui_kit::{
    IntoElement, ObjectFit, ParentElement as _, Styled as _, StyledImage as _, div, img, px, white,
};
use std::path::Path;

/// Extracts clean 1-2 letter initials for a repository (e.g. "PC" for PhotoCraft).
pub fn name_initials(name: &str) -> String {
    let clean = name.trim();
    if clean.is_empty() {
        return "R".to_string();
    }
    let parts: Vec<&str> = clean
        .split(|c: char| !c.is_alphanumeric())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() >= 2 {
        let first = parts[0]
            .chars()
            .find(|c| c.is_alphanumeric())
            .unwrap_or('A');
        let second = parts[1]
            .chars()
            .find(|c| c.is_alphanumeric())
            .unwrap_or('B');
        format!("{first}{second}").to_uppercase()
    } else {
        let uppers: Vec<char> = clean.chars().filter(|c| c.is_uppercase()).collect();
        if uppers.len() >= 2 {
            format!("{}{}", uppers[0], uppers[1])
        } else {
            clean
                .chars()
                .filter(|c| c.is_alphanumeric())
                .take(2)
                .collect::<String>()
                .to_uppercase()
        }
    }
}

/// Renders a repository initials badge on a solid, non-gradient background.
pub fn repo_initials_badge(name: &str, size_px: f32) -> impl IntoElement + 'static {
    let initials = name_initials(name);
    let radius = (size_px * 0.24).round();
    let font_size = (size_px * 0.40).round().max(10.);

    div()
        .w(px(size_px))
        .h(px(size_px))
        .rounded(px(radius))
        .flex_shrink_0()
        .bg(rgb(0x27272A)) // Solid dark neutral background - NO gradient!
        .border_1()
        .border_color(white().opacity(0.12))
        .shadow_sm()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_size(px(font_size))
                .font_bold()
                .text_color(white().opacity(0.92))
                .child(initials),
        )
}

/// Backward compatibility placeholder: renders a solid initials badge.
pub fn app_badge(name: &str, size_px: f32) -> impl IntoElement + 'static {
    repo_initials_badge(name, size_px)
}

/// Renders a circular user or organization avatar with image or initial fallback.
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

/// Renders a repository badge icon (with real image if available, or clean solid initials fallback).
pub fn repo_icon_with_provider(
    name: &str,
    size_px: f32,
    icon_path: Option<&Path>,
    _provider: Option<&str>,
) -> impl IntoElement + 'static {
    let radius = (size_px * 0.24).round();

    if let Some(path) = icon_path {
        let fallback_name = name.to_string();
        return div()
            .w(px(size_px))
            .h(px(size_px))
            .rounded(px(radius))
            .flex_shrink_0()
            .overflow_hidden()
            .shadow_sm()
            .child(
                img(path.to_path_buf())
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .with_fallback(move || {
                        repo_initials_badge(&fallback_name, size_px).into_any_element()
                    }),
            )
            .into_any_element();
    }

    // Default when no icon image is available: show initials without gradient background
    repo_initials_badge(name, size_px).into_any_element()
}

/// Renders a real image repository icon if available on disk or cached,
/// otherwise falls back to a solid initials badge.
pub fn repo_icon(name: &str, size_px: f32, icon_path: Option<&Path>) -> impl IntoElement + 'static {
    repo_icon_with_provider(name, size_px, icon_path, None)
}

/// Renders a repository badge icon (with real artwork if available, or solid initials fallback).
pub fn repo_avatar(name: &str, size: f32, icon_path: Option<&Path>) -> impl IntoElement {
    repo_icon(name, size, icon_path)
}

/// Backward compatible repo avatar fallback.
pub fn repo_avatar_default(name: &str, size: f32) -> impl IntoElement {
    repo_initials_badge(name, size)
}
