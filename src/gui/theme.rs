//! The look: white ink on a blue field with a fine grid, set in Geist
//! with Geist Mono for data.

use std::borrow::Cow;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, Rgba, px, rgb};

use crate::ansi;

/// The interface face.
pub const SANS: &str = "Geist";
/// The data face: URLs, ports, paths, logs, form values and tags.
pub const MONO: &str = "Geist Mono";

/// The blueprint field everything is drawn on.
pub const FIELD: u32 = 0x0552e1;
/// The ink: every line, heading and link is drawn in it.
pub const INK: u32 = 0xffffff;
/// Blue text on white ink (tags, primary buttons).
pub const ON_INK: u32 = 0x0552e1;
pub const TEXT: u32 = 0xffffff;
/// White at 72% and 45% over the field, kept opaque for crisp text.
pub const MUTED: u32 = 0xb9cff7;
pub const FAINT: u32 = 0x76a0ee;
pub const GREEN: u32 = 0x7dffb0;
pub const AMBER: u32 = 0xffd166;
pub const RED: u32 = 0xff8a80;

pub fn c(hex: u32) -> Rgba {
    rgb(hex)
}

pub fn alpha(hex: u32, a: f32) -> Rgba {
    let mut c = rgb(hex);
    c.a = a;
    c
}

/// White ink at low opacity, for hover and selection fills.
pub fn wash(a: f32) -> Rgba {
    alpha(INK, a)
}

/// Stroke weights. Every line is 1px; only its strength varies, in three
/// steps, so the drawing reads as one system:
/// - [`hairline`]: structure that should recede (sheet outlines, table cells,
///   empty frames);
/// - [`line`]: things you read or touch (buttons, selection, dimensions, rules,
///   rulers);
/// - solid [`INK`]: the one thing on a surface that should lead.
pub fn hairline() -> Rgba {
    wash(0.16)
}

pub fn line() -> Rgba {
    wash(0.4)
}

/// Solid fill for things floating above the drawing (dialogs, toasts), so
/// nothing underneath shows through.
pub const OVERLAY: u32 = 0x0442c4;

/// A sheet's fill: a solid, slightly deeper blue that keeps the grid off
/// the content.
pub fn sheet_fill() -> Rgba {
    c(OVERLAY)
}

/// Register the bundled fonts and restyle gpui-component widgets (inputs).
pub fn install(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../../assets/fonts/Geist-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/Geist-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/Geist-SemiBold.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/Geist-Bold.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/GeistMono-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/GeistMono-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/GeistMono-SemiBold.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/GeistMono-Bold.ttf")),
    ];
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        eprintln!("blueprint: couldn't load bundled fonts: {e}");
    }

    Theme::change(ThemeMode::Dark, None, cx);
    let t = Theme::global_mut(cx);
    t.font_family = SANS.into();
    t.mono_font_family = MONO.into();
    t.font_size = px(13.5);
    t.radius = px(0.);
    t.foreground = Hsla::from(c(TEXT));
    t.background = Hsla::from(c(0x0340b8));
    t.caret = Hsla::from(c(INK));
    t.selection = Hsla::from(wash(0.3));
    t.muted_foreground = Hsla::from(c(FAINT));
    t.border = Hsla::from(wash(0.45));
    t.input = Hsla::from(wash(0.45));
    t.ring = Hsla::from(c(INK));
}

/// Log colors, lightened to read on the blue field.
pub fn ansi_color(color: ansi::Color) -> Rgba {
    const BASIC: [u32; 8] = [0x9db7f5, 0xff9d94, 0x8affc1, 0xffe08a, 0xc4d8ff, 0xf0b3ff, 0x8af3ff, 0xffffff];
    const BRIGHT: [u32; 8] = [0xb9cff7, 0xffb8b1, 0xb0ffd4, 0xffeab0, 0xdbe7ff, 0xf6ccff, 0xb5f8ff, 0xffffff];
    // Dark colors meant for light terminals would vanish on blue.
    let lighten = |r: u32, g: u32, b: u32| {
        let lum = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
        if lum >= 150. {
            return rgb((r << 16) | (g << 8) | b);
        }
        let t = (150. - lum) / 255.;
        let up = |v: u32| (v as f32 + (255. - v as f32) * t * 1.6).min(255.) as u32;
        rgb((up(r) << 16) | (up(g) << 8) | up(b))
    };
    match color {
        ansi::Color::Basic(i) => rgb(BASIC[i as usize % 8]),
        ansi::Color::Bright(i) => rgb(BRIGHT[i as usize % 8]),
        ansi::Color::Indexed(i) if i < 8 => rgb(BASIC[i as usize]),
        ansi::Color::Indexed(i) if i < 16 => rgb(BRIGHT[i as usize - 8]),
        ansi::Color::Indexed(i) if i >= 232 => {
            let v = 8 + (i as u32 - 232) * 10;
            lighten(v, v, v)
        }
        ansi::Color::Indexed(i) => {
            // 6x6x6 color cube.
            let i = i as u32 - 16;
            let level = |n: u32| if n == 0 { 0 } else { 55 + n * 40 };
            lighten(level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        ansi::Color::Rgb(r, g, b) => lighten(r as u32, g as u32, b as u32),
    }
}
