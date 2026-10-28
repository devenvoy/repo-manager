//! GPUI color helpers backed by the `bigcolor` parser.

use bigcolor::BigColor;
use gpui_kit::{Hsla, Rgba};

/// Parse a CSS color string into a GPUI HSLA color.
///
/// Accepts every format supported by `bigcolor`, including hex, `rgb()`,
/// `rgba()`, `hsl()`, `hsla()`, and named colors.
pub fn color(value: &str) -> Hsla {
    let color = BigColor::new(value);
    let hsl = color.to_hsl();

    Hsla {
        h: hsl.h / 360.0,
        s: hsl.s,
        l: hsl.l,
        a: hsl.a,
    }
}

/// Build a GPUI RGBA color from a `0xRRGGBB` literal.
pub fn rgb(hex: u32) -> Rgba {
    let color = BigColor::new(format!("#{hex:06x}"));
    let rgb = color.to_rgb();

    Rgba {
        r: f32::from(rgb.r) / 255.0,
        g: f32::from(rgb.g) / 255.0,
        b: f32::from(rgb.b) / 255.0,
        a: rgb.a,
    }
}

/// Build a GPUI HSLA color from a hue in turns and saturation, lightness, and
/// alpha in the `0..=1` range.
pub fn hsla(h: f32, s: f32, l: f32, a: f32) -> Hsla {
    let color = BigColor::from_hsl(h * 360.0, s, l, a);
    let hsl = color.to_hsl();

    Hsla {
        h: hsl.h / 360.0,
        s: hsl.s,
        l: hsl.l,
        a: hsl.a,
    }
}

#[cfg(test)]
mod tests {
    use super::{color, hsla, rgb};

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.01,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn rgb_parses_hex_literals() {
        let color = rgb(0x0F_17_2A);

        assert_close(color.r, 0x0F as f32 / 255.0);
        assert_close(color.g, 0x17 as f32 / 255.0);
        assert_close(color.b, 0x2A as f32 / 255.0);
        assert_close(color.a, 1.0);
    }

    #[test]
    fn color_parses_css_formats() {
        let hsl = color("hsl(232, 90%, 83%)");
        assert_close(hsl.h, 231.538 / 360.0);
        assert_close(hsl.s, 0.886);
        assert_close(hsl.l, 0.827);
        assert_close(hsl.a, 1.0);

        let rgba = color("rgba(56, 189, 248, 0.15)");
        assert_close(rgba.a, 0.15);
    }

    #[test]
    fn hsla_matches_equivalent_css_color() {
        let from_components = hsla(232.0 / 360.0, 0.90, 0.83, 0.9);
        let from_css = color("hsla(232, 90%, 83%, 0.9)");

        assert_close(from_components.h, from_css.h);
        assert_close(from_components.s, from_css.s);
        assert_close(from_components.l, from_css.l);
        assert_close(from_components.a, from_css.a);
    }
}
