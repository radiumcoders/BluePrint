//! portboard's look: a blueprint. White sheets drafted in blue ink on dot
//! grid paper, set entirely in a technical mono.

use std::borrow::Cow;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, Rgba, px, rgb};

use crate::ansi;

/// The one typeface: clean, technical, monospaced.
pub const MONO: &str = "IBM Plex Mono";

/// Drafting paper behind the sheets.
pub const PAPER: u32 = 0xf4f7fd;
pub const SHEET: u32 = 0xffffff;
/// The ink. Everything structural is drawn in it.
pub const BLUE: u32 = 0x0552e1;
pub const BLUE_DEEP: u32 = 0x0340b0;
pub const TEXT: u32 = 0x0b1f4d;
pub const MUTED: u32 = 0x5a6b8c;
pub const FAINT: u32 = 0x9aa8c2;
pub const GREEN: u32 = 0x0e9f6e;
pub const AMBER: u32 = 0xd98206;
pub const RED: u32 = 0xd92d20;

pub fn c(hex: u32) -> Rgba {
    rgb(hex)
}

pub fn alpha(hex: u32, a: f32) -> Rgba {
    let mut c = rgb(hex);
    c.a = a;
    c
}

/// Blue at low opacity, for hover and selection fills.
pub fn wash(a: f32) -> Rgba {
    alpha(BLUE, a)
}

/// Register the bundled fonts and restyle gpui-component widgets (inputs).
pub fn install(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../../assets/fonts/IBMPlexMono-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/IBMPlexMono-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/IBMPlexMono-SemiBold.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/IBMPlexMono-Bold.ttf")),
    ];
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        eprintln!("portboard: couldn't load bundled fonts: {e}");
    }

    Theme::change(ThemeMode::Light, None, cx);
    let t = Theme::global_mut(cx);
    t.font_family = MONO.into();
    t.mono_font_family = MONO.into();
    t.font_size = px(13.5);
    t.radius = px(4.);
    t.foreground = Hsla::from(c(TEXT));
    t.background = Hsla::from(c(SHEET));
    t.caret = Hsla::from(c(BLUE));
    t.selection = Hsla::from(wash(0.22));
    t.muted_foreground = Hsla::from(c(FAINT));
    t.border = Hsla::from(alpha(BLUE, 0.35));
    t.input = Hsla::from(alpha(BLUE, 0.35));
    t.ring = Hsla::from(c(BLUE));
}

/// Log colors, darkened to read on white paper.
pub fn ansi_color(color: ansi::Color) -> Rgba {
    const BASIC: [u32; 8] = [0x3a4560, 0xc62828, 0x1b7f4b, 0xa86400, 0x0552e1, 0x8e2bc9, 0x0b7f8f, 0x6b7896];
    const BRIGHT: [u32; 8] = [0x6b7896, 0xe0443a, 0x22995c, 0xc07a00, 0x2f6ff0, 0xa64ee0, 0x12919f, 0x8a96b0];
    // Light colors meant for dark terminals would vanish on white.
    let darken = |r: u32, g: u32, b: u32| {
        let lum = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
        let k = if lum > 150. { 150. / lum } else { 1. };
        rgb((((r as f32 * k) as u32) << 16) | (((g as f32 * k) as u32) << 8) | (b as f32 * k) as u32)
    };
    match color {
        ansi::Color::Basic(i) => rgb(BASIC[i as usize % 8]),
        ansi::Color::Bright(i) => rgb(BRIGHT[i as usize % 8]),
        ansi::Color::Indexed(i) if i < 8 => rgb(BASIC[i as usize]),
        ansi::Color::Indexed(i) if i < 16 => rgb(BRIGHT[i as usize - 8]),
        ansi::Color::Indexed(i) if i >= 232 => {
            let v = 8 + (i as u32 - 232) * 10;
            darken(v, v, v)
        }
        ansi::Color::Indexed(i) => {
            // 6x6x6 color cube.
            let i = i as u32 - 16;
            let level = |n: u32| if n == 0 { 0 } else { 55 + n * 40 };
            darken(level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        ansi::Color::Rgb(r, g, b) => darken(r as u32, g as u32, b as u32),
    }
}
