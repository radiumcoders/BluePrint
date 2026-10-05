//! portboard's own look: a departure board. Ink background, signal amber,
//! warm off-white text, and status lamps.

use std::borrow::Cow;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, Rgba, px, rgb};

use crate::ansi;

/// Board text: names, URLs, ports, status words.
pub const MONO: &str = "Space Mono";
/// Everything else.
pub const SANS: &str = "Space Grotesk";

pub const INK: u32 = 0x0b0c0e;
pub const PANEL: u32 = 0x111318;
pub const RAISED: u32 = 0x181b21;
pub const RAISED_HI: u32 = 0x20242c;
pub const LINE: u32 = 0x23272f;
pub const CONSOLE: u32 = 0x07080a;
pub const TEXT: u32 = 0xece6d8;
pub const MUTED: u32 = 0x8b909a;
pub const FAINT: u32 = 0x565b65;
pub const AMBER: u32 = 0xffb224;
pub const ON_AMBER: u32 = 0x1d1300;
pub const GREEN: u32 = 0x3ddc84;
pub const RED: u32 = 0xff5a4e;

pub fn c(hex: u32) -> Rgba {
    rgb(hex)
}

pub fn alpha(hex: u32, a: f32) -> Rgba {
    let mut c = rgb(hex);
    c.a = a;
    c
}

/// Register the bundled fonts and restyle gpui-component widgets (inputs).
pub fn install(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../../assets/fonts/SpaceMono-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/SpaceMono-Bold.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/SpaceGrotesk-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/SpaceGrotesk-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/SpaceGrotesk-Bold.ttf")),
    ];
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        eprintln!("portboard: couldn't load bundled fonts: {e}");
    }

    Theme::change(ThemeMode::Dark, None, cx);
    let t = Theme::global_mut(cx);
    t.font_family = MONO.into();
    t.mono_font_family = MONO.into();
    t.font_size = px(14.);
    t.radius = px(6.);
    t.foreground = Hsla::from(c(TEXT));
    t.background = Hsla::from(c(INK));
    t.caret = Hsla::from(c(AMBER));
    t.selection = Hsla::from(alpha(AMBER, 0.28));
    t.muted_foreground = Hsla::from(c(FAINT));
    t.border = Hsla::from(c(LINE));
    t.input = Hsla::from(c(LINE));
    t.ring = Hsla::from(c(AMBER));
}

/// Log colors, tuned to read on the console background.
pub fn ansi_color(color: ansi::Color) -> Rgba {
    const BASIC: [u32; 8] = [0x6b7079, 0xff6b5e, 0x5fdc8f, 0xffc04d, 0x6fa8ff, 0xd38cff, 0x5ad7d0, 0xd8d4ca];
    const BRIGHT: [u32; 8] = [0x8b909a, 0xff8a7f, 0x7ff0a8, 0xffd47a, 0x93bfff, 0xe2abff, 0x86ebe4, 0xf4f0e6];
    match color {
        ansi::Color::Basic(i) => rgb(BASIC[i as usize % 8]),
        ansi::Color::Bright(i) => rgb(BRIGHT[i as usize % 8]),
        ansi::Color::Indexed(i) if i < 8 => rgb(BASIC[i as usize]),
        ansi::Color::Indexed(i) if i < 16 => rgb(BRIGHT[i as usize - 8]),
        ansi::Color::Indexed(i) if i >= 232 => {
            let v = 8 + (i as u32 - 232) * 10;
            rgb((v << 16) | (v << 8) | v)
        }
        ansi::Color::Indexed(i) => {
            // 6x6x6 color cube.
            let i = i as u32 - 16;
            let level = |n: u32| if n == 0 { 0 } else { 55 + n * 40 };
            rgb((level(i / 36) << 16) | (level((i / 6) % 6) << 8) | level(i % 6))
        }
        ansi::Color::Rgb(r, g, b) => rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32),
    }
}
