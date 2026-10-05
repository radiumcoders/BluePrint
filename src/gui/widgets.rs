//! Small building blocks in portboard's style.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::theme::{self, AMBER, FAINT, GREEN, MONO, MUTED, ON_AMBER, RAISED, RAISED_HI, RED, TEXT, alpha, c};
use crate::manager::Status;

pub fn icon(name: IconName, size: f32, color: Rgba) -> Svg {
    svg().path(name.path()).size(px(size)).flex_none().text_color(color)
}

/// Tiny letter-spaced caps, like the column headings on a departure board.
pub fn caps(text: impl Into<SharedString>) -> Div {
    div()
        .font_family(MONO)
        .text_size(px(10.5))
        .text_color(c(FAINT))
        .font_weight(FontWeight::BOLD)
        .child(text.into())
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// Amber, for the main action.
    Primary,
    Plain,
    /// Text only until hovered.
    Ghost,
    Danger,
}

pub fn button(
    id: impl Into<ElementId>,
    name: Option<IconName>,
    label: impl Into<SharedString>,
    kind: Kind,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (fg, bg, hover_bg) = match kind {
        Kind::Primary => (c(ON_AMBER), c(AMBER), c(0xffc45a)),
        Kind::Plain => (c(TEXT), c(RAISED), c(RAISED_HI)),
        Kind::Ghost => (c(MUTED), alpha(RAISED, 0.), c(RAISED)),
        Kind::Danger => (c(0x1d0503), c(RED), c(0xff7a70)),
    };
    let hover_fg = if kind == Kind::Ghost { c(TEXT) } else { fg };
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .h(px(32.))
        .px_3()
        .rounded(px(6.))
        .font_family(theme::SANS)
        .font_weight(FontWeight::MEDIUM)
        .text_size(px(13.))
        .text_color(fg)
        .bg(bg)
        .cursor_pointer()
        .hover(move |el| el.bg(hover_bg).text_color(hover_fg))
        .on_click(on_click)
        .when_some(name, |el, name| el.child(icon(name, 14., fg)))
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
        .size(px(30.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|el| el.bg(c(RAISED_HI)))
        .on_click(on_click)
        .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tooltip).build(window, cx))
        .child(icon(name, 15., color))
}

pub fn status_color(s: Status) -> u32 {
    match s {
        Status::Running => GREEN,
        Status::Starting | Status::Waiting | Status::Stopping => AMBER,
        Status::Crashed => RED,
        Status::Stopped => FAINT,
    }
}

/// A status lamp: a solid dot inside a soft halo when lit.
pub fn lamp(s: Status) -> Div {
    let color = status_color(s);
    let lit = !matches!(s, Status::Stopped);
    div()
        .flex_none()
        .size(px(16.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .when(lit, |el| el.bg(alpha(color, 0.16)))
        .child(div().size(px(8.)).rounded_full().bg(c(color)))
}
