//! Small building blocks in portboard's blueprint style.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::sketch;
use super::theme::{AMBER, BLUE, BLUE_DEEP, FAINT, GREEN, HAND, MONO, MUTED, RED, SHEET, alpha, c, wash};
use crate::manager::Status;

pub fn icon(name: IconName, size: f32, color: Rgba) -> Svg {
    svg().path(name.path()).size(px(size)).flex_none().text_color(color)
}

/// A hand-lettered label, like the annotations on a drawing.
pub fn hand(text: impl Into<SharedString>, size: f32, color: u32) -> Div {
    div().font_family(HAND).text_size(px(size)).text_color(c(color)).child(text.into())
}

/// A white sheet outlined in ink, its edges running past the corners.
pub fn sheet() -> Div {
    div().relative().bg(c(SHEET)).child(sketch::border(c(BLUE), 1., 10.))
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// Solid ink, for the main action.
    Primary,
    /// Outlined in sketched ink.
    Plain,
    /// Text only until hovered.
    Ghost,
    Danger,
}

pub fn button(
    id: &'static str,
    name: Option<IconName>,
    label: impl Into<SharedString>,
    kind: Kind,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (fg, bg, hover_bg) = match kind {
        Kind::Primary => (c(SHEET), c(BLUE), c(BLUE_DEEP)),
        Kind::Plain => (c(BLUE), c(SHEET), wash(0.08)),
        Kind::Ghost => (c(MUTED), alpha(SHEET, 0.), wash(0.07)),
        Kind::Danger => (c(SHEET), c(RED), c(0xb42318)),
    };
    let hover_fg = if kind == Kind::Ghost { c(BLUE) } else { fg };
    div()
        .id(id)
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap_2()
        .h(px(34.))
        .px_3p5()
        .rounded(px(3.))
        .font_family(HAND)
        .text_size(px(15.))
        .text_color(fg)
        .bg(bg)
        .cursor_pointer()
        .hover(move |el| el.bg(hover_bg).text_color(hover_fg))
        .on_click(on_click)
        .when(kind == Kind::Plain, |el| el.child(sketch::border(c(BLUE), 1., 4.)))
        .when_some(name, |el, name| el.child(icon(name, 15., fg)))
        .child(label.into())
}

pub fn icon_button(
    id: impl Into<ElementId>,
    name: IconName,
    color: Rgba,
    tooltip: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(px(32.))
        .rounded(px(3.))
        .cursor_pointer()
        .hover(|el| el.bg(wash(0.08)))
        .on_click(on_click)
        .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tooltip).build(window, cx))
        .child(icon(name, 16., color))
}

pub fn status_color(s: Status) -> u32 {
    match s {
        Status::Running => GREEN,
        Status::Starting | Status::Waiting | Status::Stopping => AMBER,
        Status::Crashed => RED,
        Status::Stopped => FAINT,
    }
}

pub fn status_word(s: Status) -> &'static str {
    match s {
        Status::Running => "running",
        Status::Starting => "starting",
        Status::Waiting => "waiting for proxy",
        Status::Stopping => "stopping",
        Status::Crashed => "crashed",
        Status::Stopped => "stopped",
    }
}

/// A status dot; lit states get a thin ring around them.
pub fn lamp(s: Status) -> Div {
    let color = status_color(s);
    let lit = !matches!(s, Status::Stopped);
    div()
        .relative()
        .flex_none()
        .size(px(14.))
        .flex()
        .items_center()
        .justify_center()
        .child(div().size(px(6.)).rounded_full().bg(c(color)))
        .when(lit, |el| {
            el.child(div().absolute().inset_0().rounded_full().border_1().border_color(alpha(color, 0.55)))
        })
}

/// Monospace text in the data face.
pub fn mono(text: impl Into<SharedString>, size: f32, color: u32) -> Div {
    div().font_family(MONO).text_size(px(size)).text_color(c(color)).child(text.into())
}

/// A blueprint-style field: hand-lettered caption over a mono value.
pub fn spec(label: &'static str, value: impl Into<SharedString>, value_color: u32) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .min_w_0()
        .child(hand(label, 13., MUTED))
        .child(mono(value, 13., value_color).truncate())
}

/// Like [`spec`], but long values lose their start instead of their end
/// (paths are most recognizable by their last folders).
pub fn spec_tail(label: &'static str, value: impl Into<SharedString>, value_color: u32) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .min_w_0()
        .child(hand(label, 13., MUTED))
        .child(mono(value, 13., value_color).overflow_hidden().whitespace_nowrap().text_ellipsis_start())
}
