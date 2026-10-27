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
            canvas: rgb(0x0d, 0x11, 0x17),
            surface: rgb(0x16, 0x1b, 0x22),
            sidebar: rgb(0x09, 0x0d, 0x13),
            muted: rgb(0x21, 0x26, 0x2d),
            fg_primary: rgb(0xf0, 0xf6, 0xfc),
            fg_muted: rgb(0x8d, 0x96, 0xa0),
            border: rgb(0x30, 0x36, 0x3d),
            accent: rgb(0x2f, 0x81, 0xf7),
            success: rgb(0x23, 0x86, 0x36),
            warning: rgb(0xd2, 0x99, 0x22),
            danger: rgb(0xf8, 0x51, 0x49),
        }
    }

    pub fn light() -> Self {
        Self {
            canvas: rgb(0xf6, 0xf8, 0xfa),
            surface: rgb(0xff, 0xff, 0xff),
            sidebar: rgb(0xf0, 0xf2, 0xf5),
            muted: rgb(0xea, 0xee, 0xf2),
            fg_primary: rgb(0x1f, 0x23, 0x28),
            fg_muted: rgb(0x65, 0x6d, 0x76),
            border: rgb(0xd0, 0xd7, 0xde),
            accent: rgb(0x09, 0x69, 0xda),
            success: rgb(0x1a, 0x7f, 0x37),
            warning: rgb(0x9a, 0x67, 0x00),
            danger: rgb(0xcf, 0x22, 0x2e),
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
