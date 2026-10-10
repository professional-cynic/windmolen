// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 professional-cynic

//! libadwaita colour palette (1.6+ named colours) for light and dark styles.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const WHITE: Self = Self::rgb(0xff, 0xff, 0xff);
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const TRANSPARENT: Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self { a, ..Self::rgb(r, g, b) }
    }

    pub const fn hex(v: u32) -> Self {
        Self::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    /// `alpha(color, a)` in libadwaita terms: scales the existing alpha.
    pub fn alpha(self, a: f32) -> Self {
        Self { a: self.a * a, ..self }
    }

    /// Mixes `other` over `self` with weight `t` (0 = self, 1 = other).
    pub fn mix(self, other: Self, t: f32) -> Self {
        let l = |a: f32, b: f32| a + (b - a) * t;
        Self { r: l(self.r, other.r), g: l(self.g, other.g), b: l(self.b, other.b), a: l(self.a, other.a) }
    }

    fn to_oklab(self) -> [f32; 3] {
        let lin = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        let (r, g, b) = (lin(self.r), lin(self.g), lin(self.b));
        let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
        let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
        let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
        [
            0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
            1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
            0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
        ]
    }

    fn from_oklab([l, a, b]: [f32; 3], alpha: f32) -> Self {
        let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
        let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
        let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
        let r = 4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_;
        let g = -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_;
        let bl = -0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_;
        let enc = |c: f32| {
            let c = c.clamp(0.0, 1.0);
            if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
        };
        Self { r: enc(r), g: enc(g), b: enc(bl), a: alpha }
    }
}

/// The libadwaita accent colours, in the order used for mark slots too.
pub const ACCENTS: [Color; 9] = [
    Color::hex(0x3584e4), // blue
    Color::hex(0x2190a4), // teal
    Color::hex(0x3a944a), // green
    Color::hex(0xc88800), // yellow
    Color::hex(0xed5b00), // orange
    Color::hex(0xe62d42), // red
    Color::hex(0xd56199), // pink
    Color::hex(0x9141ac), // purple
    Color::hex(0x6f8396), // slate
];

/// Picks the libadwaita accent closest to an arbitrary system colour, the
/// same way libadwaita does on platforms with free-form accents.
pub fn nearest_accent(c: Color) -> Color {
    let [_, a, b] = c.to_oklab();
    let hue = b.atan2(a);
    let chroma = a.hypot(b);
    if chroma < 0.04 {
        return ACCENTS[8];
    }
    *ACCENTS[..8]
        .iter()
        .min_by(|x, y| {
            let d = |c: &Color| {
                let [_, a, b] = c.to_oklab();
                let diff = (b.atan2(a) - hue).abs();
                diff.min(std::f32::consts::TAU - diff)
            };
            d(x).total_cmp(&d(y))
        })
        .unwrap()
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub dark: bool,
    pub window_fg: Color,
    pub view_bg: Color,
    pub view_fg: Color,
    pub headerbar_bg: Color,
    pub headerbar_fg: Color,
    pub headerbar_backdrop: Color,
    pub headerbar_shade: Color,
    pub card_bg: Color,
    pub card_shade: Color,
    pub dialog_bg: Color,
    pub popover_bg: Color,
    pub sidebar_bg: Color,
    pub sidebar_backdrop: Color,
    pub sidebar_border: Color,
    pub accent_bg: Color,
    pub accent_fg: Color,
    /// The accent as a foreground colour on window backgrounds.
    pub accent: Color,
    /// Red text and tints of destructive buttons.
    pub destructive: Color,
    pub osd_bg: Color,
    pub osd_fg: Color,
    pub dimming: Color,
}

pub const DIM_OPACITY: f32 = 0.55;
pub const BORDER_OPACITY: f32 = 0.15;
pub const DISABLED_OPACITY: f32 = 0.5;
/// Disabled flat buttons fade further, having no background to dim.
pub const STRONG_DISABLED_OPACITY: f32 = 0.3;

/// libadwaita's `--standalone-color-oklab`: the lightness at which a
/// background colour also reads as text on the window background.
fn standalone(bg: Color, dark: bool) -> Color {
    let lab = bg.to_oklab();
    let l = if dark { lab[0].max(0.85) } else { lab[0].min(0.5) };
    Color::from_oklab([l, lab[1], lab[2]], 1.0)
}

impl Theme {
    pub fn new(dark: bool, accent_bg: Color) -> Self {
        let accent = standalone(accent_bg, dark);
        // --destructive-bg-color: red 3 (light) and red 4 (dark).
        let destructive = standalone(Color::hex(if dark { 0xc01c28 } else { 0xe01b24 }), dark);
        let fg_light = Color::rgba(0, 0, 6, 0.8);
        if dark {
            Self {
                dark,
                window_fg: Color::WHITE,
                view_bg: Color::hex(0x1d1d20),
                view_fg: Color::WHITE,
                headerbar_bg: Color::hex(0x2e2e32),
                headerbar_fg: Color::WHITE,
                headerbar_backdrop: Color::hex(0x222226),
                headerbar_shade: Color::rgba(0, 0, 6, 0.36),
                card_bg: Color::rgba(255, 255, 255, 0.08),
                card_shade: Color::rgba(0, 0, 6, 0.36),
                dialog_bg: Color::hex(0x36363a),
                popover_bg: Color::hex(0x36363a),
                sidebar_bg: Color::hex(0x2e2e32),
                sidebar_backdrop: Color::hex(0x28282c),
                sidebar_border: Color::rgba(0, 0, 6, 0.36),
                accent_bg,
                accent_fg: Color::WHITE,
                accent,
                destructive,
                osd_bg: Color::rgba(0, 0, 0, 0.7),
                osd_fg: Color::rgba(255, 255, 255, 0.9),
                // --shade-color at twice its opacity.
                dimming: Color::rgba(0, 0, 6, 0.5),
            }
        } else {
            Self {
                dark,
                window_fg: fg_light,
                view_bg: Color::WHITE,
                view_fg: fg_light,
                headerbar_bg: Color::WHITE,
                headerbar_fg: fg_light,
                headerbar_backdrop: Color::hex(0xfafafb),
                headerbar_shade: Color::rgba(0, 0, 6, 0.12),
                card_bg: Color::WHITE,
                card_shade: Color::rgba(0, 0, 6, 0.07),
                dialog_bg: Color::hex(0xfafafb),
                popover_bg: Color::WHITE,
                sidebar_bg: Color::hex(0xebebed),
                sidebar_backdrop: Color::hex(0xf2f2f4),
                sidebar_border: Color::rgba(0, 0, 6, 0.07),
                accent_bg,
                accent_fg: Color::WHITE,
                accent,
                destructive,
                osd_bg: Color::rgba(0, 0, 0, 0.7),
                osd_fg: Color::rgba(255, 255, 255, 0.9),
                dimming: Color::rgba(0, 0, 6, 0.14),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_blue_matches_libadwaita() {
        // libadwaita documents #0461be (light) and #81d0ff (dark) for blue.
        let near = |c: Color, hex: u32| {
            let e = Color::hex(hex);
            (c.r - e.r).abs() < 0.03 && (c.g - e.g).abs() < 0.03 && (c.b - e.b).abs() < 0.03
        };
        assert!(near(Theme::new(false, ACCENTS[0]).accent, 0x0461be));
        assert!(near(Theme::new(true, ACCENTS[0]).accent, 0x81d0ff));
        // And #c30000 / #ff938c for the destructive colour.
        assert!(near(Theme::new(false, ACCENTS[0]).destructive, 0xc30000));
        assert!(near(Theme::new(true, ACCENTS[0]).destructive, 0xff938c));
    }

    #[test]
    fn accent_mapping() {
        assert_eq!(nearest_accent(Color::hex(0x0078d4)), ACCENTS[0]); // Windows default blue
        assert_eq!(nearest_accent(Color::hex(0xe81123)), ACCENTS[5]); // red
        assert_eq!(nearest_accent(Color::hex(0x107c10)), ACCENTS[2]); // green
        assert_eq!(nearest_accent(Color::hex(0x808080)), ACCENTS[8]); // grey → slate
    }
}
