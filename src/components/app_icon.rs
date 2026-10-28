//! Fictional app icons and rich procedural app badges for Release Desk.

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::Icon;
use gpui_kit::{
    IntoElement, ParentElement as _, Styled as _, div, linear_color_stop, linear_gradient, px, rgb,
    white,
};
use release_desk_core::app_catalog::find_known_app;

pub fn app_icon(id: &str) -> Icon {
    let data: &[u8] = match id {
        "prism" => include_bytes!("../../resources/app-icons/prism.svg"),
        "quill" => include_bytes!("../../resources/app-icons/quill.svg"),
        "atlas" => include_bytes!("../../resources/app-icons/atlas.svg"),
        "orbit" => include_bytes!("../../resources/app-icons/orbit.svg"),
        _ => include_bytes!("../../resources/app-icons/relay.svg"),
    };
    Icon::default().data(data).w(px(44.)).h(px(44.))
}

/// Palette pair: start and end colors for gradient badges.
fn app_palette(id: &str) -> (gpui_kit::Rgba, gpui_kit::Rgba) {
    match id {
        "photocraft" => (rgb(0x06B6D4), rgb(0x2563EB)), // Cyan to Blue
        "artcraft" => (rgb(0x9333EA), rgb(0xDB2777)),   // Purple to Pink
        "gridcraft" => (rgb(0x10B981), rgb(0x0D9488)),  // Emerald to Teal
        "filmcraft" => (rgb(0xF43F5E), rgb(0xD97706)),  // Rose to Amber
        "vectorcraft" => (rgb(0xF97316), rgb(0xDC2626)), // Orange to Red
        "pdfcraft" => (rgb(0xE11D48), rgb(0xBE123C)),   // Red to Rose
        "soundcraft" => (rgb(0x7C3AED), rgb(0x6D28D9)), // Violet to Deep Purple
        "cadcraft" => (rgb(0x2563EB), rgb(0x4338CA)),   // Blue to Indigo
        "prism" => (rgb(0x4F46E5), rgb(0x7C3AED)),      // Indigo to Violet
        "quill" => (rgb(0xD97706), rgb(0xEA580C)),      // Amber to Orange
        "atlas" => (rgb(0x0D9488), rgb(0x0284C7)),      // Teal to Sky
        "orbit" => (rgb(0x0284C7), rgb(0x2563EB)),      // Sky to Blue
        "relay" => (rgb(0xC026D3), rgb(0xDB2777)),      // Fuchsia to Pink
        _ => {
            let hash = id.bytes().fold(0u32, |acc, b| acc.wrapping_add(b as u32));
            match hash % 6 {
                0 => (rgb(0x3B82F6), rgb(0x1D4ED8)),
                1 => (rgb(0x10B981), rgb(0x047857)),
                2 => (rgb(0x8B5CF6), rgb(0x6D28D9)),
                3 => (rgb(0xF59E0B), rgb(0xD97706)),
                4 => (rgb(0xEC4899), rgb(0xBE185D)),
                _ => (rgb(0x06B6D4), rgb(0x0E7490)),
            }
        }
    }
}

/// Extracts a 1-3 letter monogram for the app icon badge.
fn app_initials(name: &str) -> String {
    let clean = name.trim();
    if clean.is_empty() {
        return "R".to_string();
    }
    let parts: Vec<&str> = clean
        .split(|c: char| !c.is_alphanumeric())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() >= 2 {
        let first = parts[0].chars().next().unwrap_or('A');
        let second = parts[1].chars().next().unwrap_or('B');
        format!("{first}{second}").to_uppercase()
    } else {
        // If single word with CamelCase like PhotoCraft
        let uppers: Vec<char> = clean.chars().filter(|c| c.is_uppercase()).collect();
        if uppers.len() >= 2 {
            format!("{}{}", uppers[0], uppers[1])
        } else {
            clean.chars().take(2).collect::<String>().to_uppercase()
        }
    }
}

/// Render an eye-catching, modern application icon badge with dynamic gradients and typography.
pub fn app_badge(name: &str, size_px: f32) -> impl IntoElement + 'static {
    let known = find_known_app(name);
    let (icon_text, (col_start, col_end)) = if let Some(app) = &known {
        (app.icon_text.clone(), app_palette(&app.id))
    } else {
        let clean_id = name.to_lowercase().replace(' ', "-");
        (app_initials(name), app_palette(&clean_id))
    };

    let font_size = (size_px * 0.42).round();
    let radius = (size_px * 0.28).round();

    div()
        .w(px(size_px))
        .h(px(size_px))
        .rounded(px(radius))
        .flex_shrink_0()
        .bg(linear_gradient(
            135.,
            linear_color_stop(col_start, 0.),
            linear_color_stop(col_end, 1.),
        ))
        .border_1()
        .border_color(white().opacity(0.2))
        .shadow_sm()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_size(px(font_size))
                .font_bold()
                .text_color(white())
                .child(icon_text),
        )
}
