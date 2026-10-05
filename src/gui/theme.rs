//! portboard's look: a blueprint. White paper sheets inked in blue, laid on
//! a blue field with a fine white grid, set in Geist with Geist Mono for data.

use std::borrow::Cow;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, Rgba, px, rgb};

use crate::ansi;

/// The interface face.
pub const SANS: &str = "Geist";
/// The data face: URLs, ports, paths, logs, form values and tags.
pub const MONO: &str = "Geist Mono";

/// The blueprint field the sheets are laid on.
pub const FIELD: u32 = 0x0552e1;
/// What's drawn straight on the field (grid, rulers, sheet header).
pub const CHALK: u32 = 0xffffff;
/// Chalk at 72% over the field, kept opaque for crisp text.
pub const CHALK_MUTED: u32 = 0xb9cff7;
/// The ink on the sheets: every line, heading and link is drawn in it.
pub const INK: u32 = 0x0552e1;
/// White text on blue ink (tags, primary buttons).
pub const ON_INK: u32 = 0xffffff;
pub const TEXT: u32 = 0x0b2a75;
pub const MUTED: u32 = 0x4d6db3;
pub const FAINT: u32 = 0x8098cc;
pub const GREEN: u32 = 0x12924f;
pub const AMBER: u32 = 0xb86e00;
pub const RED: u32 = 0xd93a2b;

pub fn c(hex: u32) -> Rgba {
    rgb(hex)
}

pub fn alpha(hex: u32, a: f32) -> Rgba {
    let mut c = rgb(hex);
    c.a = a;
    c
}

/// Blue ink at low opacity, for hover and selection fills on a sheet.
pub fn wash(a: f32) -> Rgba {
    alpha(INK, a)
}

/// Chalk at low opacity, for lines drawn on the field.
pub fn chalk(a: f32) -> Rgba {
    alpha(CHALK, a)
}

/// Solid fill for things floating above the drawing (dialogs, toasts), so
/// nothing underneath shows through.
pub const OVERLAY: u32 = 0xffffff;

/// A sheet's fill: plain white paper.
pub fn sheet_fill() -> Rgba {
    c(0xffffff)
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
        eprintln!("portboard: couldn't load bundled fonts: {e}");
    }

    Theme::change(ThemeMode::Light, None, cx);
    let t = Theme::global_mut(cx);
    t.font_family = SANS.into();
    t.mono_font_family = MONO.into();
    t.font_size = px(13.5);
    t.radius = px(0.);
    t.foreground = Hsla::from(c(TEXT));
    t.background = Hsla::from(c(0xffffff));
    t.caret = Hsla::from(c(INK));
    t.selection = Hsla::from(wash(0.3));
    t.muted_foreground = Hsla::from(c(FAINT));
    t.border = Hsla::from(wash(0.45));
    t.input = Hsla::from(wash(0.45));
    t.ring = Hsla::from(c(INK));
}

/// Log colors, darkened to read on white paper.
pub fn ansi_color(color: ansi::Color) -> Rgba {
    const BASIC: [u32; 8] = [0x3a4a6b, 0xc62828, 0x1b873f, 0x9a6400, 0x0552e1, 0xa22bb8, 0x00838f, 0x4d6db3];
    const BRIGHT: [u32; 8] = [0x5a6b8c, 0xe0453a, 0x23a04e, 0xb57a00, 0x2f6ff0, 0xb848cc, 0x0097a7, 0x8098cc];
    // Light colors meant for dark terminals would vanish on white.
    let darken = |r: u32, g: u32, b: u32| {
        let lum = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
        if lum <= 110. {
            return rgb((r << 16) | (g << 8) | b);
        }
        let t = (lum - 110.) / 255.;
        let down = |v: u32| (v as f32 * (1. - t * 1.6)).max(0.) as u32;
        rgb((down(r) << 16) | (down(g) << 8) | down(b))
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
