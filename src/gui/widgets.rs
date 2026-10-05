//! Small building blocks for the drawing.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::sketch;
use super::theme::{AMBER, FAINT, GREEN, INK, MONO, MUTED, ON_INK, RED, WELL, alpha, c, hairline, line, sheet_fill, wash};
use crate::manager::Status;

pub fn icon(name: IconName, size: f32, color: Rgba) -> Svg {
    svg().path(name.path()).size(px(size)).flex_none().text_color(color)
}

/// A line of interface text.
pub fn label(text: impl Into<SharedString>, size: f32, color: u32) -> Div {
    div().font_family(MONO).text_size(px(size)).text_color(c(color)).child(text.into())
}

/// A small white tag with blue text, like the labels on a drawing.
pub fn tag(text: impl Into<SharedString>) -> Div {
    div()
        .h(px(16.))
        .px_1p5()
        .flex()
        .flex_none()
        .items_center()
        .bg(c(INK))
        .child(mono(text, 10., ON_INK).font_weight(FontWeight::SEMIBOLD))
}

/// A [`tag`] pinned over a sheet's top edge, for the sheet that leads the
/// board. Pass uppercase text.
pub fn corner_tag(text: impl Into<SharedString>) -> Div {
    tag(text).absolute().top(px(-8.)).left(px(14.))
}

/// The quiet version of [`corner_tag`] for supporting sheets: outlined ink
/// instead of solid, so only one tag on the board shouts.
pub fn corner_label(text: impl Into<SharedString>) -> Div {
    quiet_tag(text).absolute().top(px(-8.)).left(px(14.))
}

/// An outlined [`tag`], for labels that annotate rather than lead.
pub fn quiet_tag(text: impl Into<SharedString>) -> Div {
    div()
        .h(px(16.))
        .px_1p5()
        .flex()
        .flex_none()
        .items_center()
        .bg(sheet_fill())
        .border_1()
        .border_color(line())
        .child(mono(text, 10., INK).font_weight(FontWeight::SEMIBOLD))
}

/// An empty frame: outlined, crossed corner to corner, with a tag saying why.
pub fn placeholder(text: impl Into<SharedString>) -> Div {
    div()
        .relative()
        .flex_1()
        .min_h(px(60.))
        .flex()
        .items_center()
        .justify_center()
        .child(sketch::border(hairline(), 1.))
        .child(sketch::cross(hairline()))
        .child(quiet_tag(text))
}

/// A section heading.
pub fn heading(text: impl Into<SharedString>, size: f32) -> Div {
    label(text, size, INK).font_weight(FontWeight::SEMIBOLD)
}

/// A small uppercase caption, like a field name on a drawing.
pub fn caption(text: &str) -> Div {
    mono(text.to_uppercase(), 10.5, FAINT).font_weight(FontWeight::MEDIUM)
}

/// A sheet with a faint hairline outline.
pub fn sheet() -> Div {
    div().relative().bg(sheet_fill()).child(sketch::border(hairline(), 1.))
}

/// The sheet the eye should land on first: same fill, a firmer outline.
pub fn lead_sheet() -> Div {
    div().relative().bg(sheet_fill()).child(sketch::border(line(), 1.))
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
        Kind::Primary => (c(ON_INK), c(INK), c(0xdbe7ff)),
        Kind::Plain => (c(INK), wash(0.), wash(0.1)),
        Kind::Ghost => (c(MUTED), wash(0.), wash(0.08)),
        Kind::Danger => (c(INK), c(0xe5483c), c(0xc93a2f)),
    };
    let hover_fg = if kind == Kind::Ghost { c(INK) } else { fg };
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
        .font_family(MONO)
        .font_weight(FontWeight::MEDIUM)
        .text_size(px(13.))
        .text_color(fg)
        .bg(bg)
        .cursor_pointer()
        .hover(move |el| el.bg(hover_bg).text_color(hover_fg))
        .on_click(on_click)
        .when(kind == Kind::Plain, |el| el.child(sketch::border(line(), 1.)))
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
        .hover(|el| el.bg(wash(0.1)))
        .on_click(on_click)
        .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tooltip).build(window, cx))
        .child(icon(name, 16., color))
}

pub fn status_color(s: Status) -> u32 {
    match s {
        Status::Running => GREEN,
        Status::Starting | Status::Stopping => AMBER,
        Status::Crashed => RED,
        Status::Stopped => FAINT,
    }
}

pub fn status_word(s: Status) -> &'static str {
    match s {
        Status::Running => "running",
        Status::Starting => "starting",
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

/// A blueprint-style field: mono caption over its value.
pub fn spec(label: &'static str, value: impl Into<SharedString>, value_color: u32) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .min_w_0()
        .child(caption(label))
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
        .child(caption(label))
        .child(mono(value, 13., value_color).overflow_hidden().whitespace_nowrap().text_ellipsis_start())
}

/// A text input drawn as a drawing's field: a recessed well with a 1px
/// outline that turns solid ink while it has focus. Values are set in mono.
pub fn text_field(state: &Entity<InputState>, window: &Window, cx: &App) -> Div {
    let focused = state.read(cx).focus_handle(cx).is_focused(window);
    div()
        .relative()
        // Same height as buttons, so a field and its button line up. The
        // input pads itself, so the well adds none.
        .h(px(34.))
        .flex()
        .items_center()
        .bg(alpha(WELL, 0.45))
        .font_family(MONO)
        .text_size(px(13.))
        .child(sketch::border(if focused { c(INK) } else { line() }, 1.))
        .child(Input::new(state).appearance(false))
}
