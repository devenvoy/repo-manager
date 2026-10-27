//! Theme tokens, color palettes, and styling definitions adhering to UX-SPEC.md.

use gpui_kit::Hsla;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    System,
    Light,
    Dark,
}

/// Palette tokens for light and dark appearance modes.
pub struct ThemeColors {
    pub canvas: Hsla,
    pub surface: Hsla,
    pub sidebar: Hsla,
    pub muted: Hsla,
    pub fg_primary: Hsla,
    pub fg_muted: Hsla,
    pub border: Hsla,
    pub accent: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub danger: Hsla,
}

impl ThemeColors {
    pub fn dark() -> Self {
        Self {
            canvas: rgb(0x0f, 0x17, 0x2a),
            surface: rgb(0x1e, 0x29, 0x3b),
            sidebar: rgb(0x09, 0x0d, 0x16),
            muted: rgb(0x33, 0x41, 0x55),
            fg_primary: rgb(0xf8, 0xfa, 0xfc),
            fg_muted: rgb(0x94, 0xa3, 0xb8),
            border: rgb(0x33, 0x41, 0x55),
            accent: rgb(0x4f, 0x6d, 0xf7),
            success: rgb(0x22, 0xc5, 0x5e),
            warning: rgb(0xf5, 0x9e, 0x0b),
            danger: rgb(0xef, 0x44, 0x44),
        }
    }

    pub fn light() -> Self {
        Self {
            canvas: rgb(0xf5, 0xf6, 0xf8),
            surface: rgb(0xff, 0xff, 0xff),
            sidebar: rgb(0xf0, 0xf2, 0xf6),
            muted: rgb(0xe8, 0xec, 0xf3),
            fg_primary: rgb(0x17, 0x21, 0x32),
            fg_muted: rgb(0x69, 0x75, 0x89),
            border: rgb(0xe1, 0xe6, 0xee),
            accent: rgb(0x35, 0x4e, 0xe8),
            success: rgb(0x11, 0x81, 0x5a),
            warning: rgb(0xa4, 0x5d, 0x17),
            danger: rgb(0xbd, 0x3b, 0x43),
        }
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Hsla {
    let rf = r as f32 / 255.0;
    let gf = g as f32 / 255.0;
    let bf = b as f32 / 255.0;

    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let delta = max - min;

    let l = (max + min) / 2.0;

    let (h, s) = if delta == 0.0 {
        (0.0, 0.0)
    } else {
        let s = if l > 0.5 {
            delta / (2.0 - max - min)
        } else {
            delta / (max + min)
        };

        let h = if max == rf {
            (gf - bf) / delta + (if gf < bf { 6.0 } else { 0.0 })
        } else if max == gf {
            (bf - rf) / delta + 2.0
        } else {
            (rf - gf) / delta + 4.0
        };

        (h / 6.0, s)
    };

    Hsla { h, s, l, a: 1.0 }
}
