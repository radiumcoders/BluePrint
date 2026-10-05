//! The main window, laid out like a drawing sheet: projects and a title block
//! on the left (projects.rs), details (details.rs) and logs (logs.rs) on the
//! right, and the add/edit dialog over it all (form.rs). The board holds the
//! state and handles keys; each part renders itself as `impl Board` there.

use std::time::{Duration, Instant};

use gpui_kit::*;

use super::form::ProjectForm;
use super::logs::LogView;
use super::sketch;
use super::theme::{self, FIELD, INK, MONO, MUTED, RED, TEXT, c, line, wash};
use super::widgets::{label, sheet};
use crate::manager::{Entry, Id, Manager, MsgKind};

/// Space between sheets.
const GUTTER: f32 = 18.;
const SIDEBAR: f32 = 300.;
/// Thickness of the rulers along the top and left edges.
const RULER: f32 = 10.;



pub struct Board {
    pub(super) m: Manager,
    pub(super) selected: Option<Id>,
    pub(super) form: Option<ProjectForm>,
    /// Remove needs a second click; this is the project awaiting it.
    pub(super) confirm_remove: Option<Id>,
    pub(super) logs: LogView,
    pub(super) last_second: Instant,
    pub(super) focus: FocusHandle,
}

impl Board {
    pub fn new(m: Manager, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let selected = m.entries.first().map(|e| e.id);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let logs = LogView::new(window, cx);
        Self {
            m,
            selected,
            form: None,
            confirm_remove: None,
            logs,
            last_second: Instant::now(),
            focus,
        }
    }

    /// Open a dialog or start every project at startup
    /// (`BLUEPRINT_OPEN=add|edit|start`), so screenshots of a window that
    /// can't take keyboard focus can still show them.
    pub fn open_from_env(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match std::env::var("BLUEPRINT_OPEN").as_deref() {
            Ok("add") => self.open_form(None, window, cx),
            Ok("edit") => self.open_form(self.selected, window, cx),
            Ok("start") => self.m.start_all(),
            _ => {}
        }
    }

    /// Called ~10 times a second.
    pub fn tick(&mut self, cx: &mut Context<Self>) {
        let mut changed = self.m.tick();
        if self.last_second.elapsed() >= Duration::from_secs(1) {
            self.last_second = Instant::now();
            // Uptimes tick over.
            changed |= self.m.running_count() > 0;
        }
        if self.selected.is_some_and(|id| self.m.get(id).is_none()) {
            self.selected = self.m.entries.first().map(|e| e.id);
        }
        if changed {
            cx.notify();
        }
    }

    pub fn shutdown(&mut self) {
        self.m.shutdown();
    }

    pub(super) fn select(&mut self, id: Id, cx: &mut Context<Self>) {
        if self.selected != Some(id) {
            self.selected = Some(id);
            self.confirm_remove = None;
        }
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.form.is_some() {
            if event.keystroke.key == "escape" {
                self.form = None;
                cx.notify();
            }
            return;
        }
        let mods = &event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();
        // Typing in the log filter isn't a shortcut; Esc clears it and leaves.
        if self.logs.filter_focused(window, cx) {
            if key == "escape" {
                self.logs.clear_filter(window, cx);
                window.focus(&self.focus, cx);
                cx.notify();
                cx.stop_propagation();
            }
            return;
        }
        if key == "/" || ((mods.control || mods.platform) && key == "f") {
            self.logs.focus_filter(window, cx);
            cx.stop_propagation();
            return;
        }
        let ids: Vec<Id> = self.m.entries.iter().map(|e| e.id).collect();
        let pos = self.selected.and_then(|id| ids.iter().position(|&i| i == id));
        if mods.control || mods.platform {
            return;
        }
        if mods.alt {
            let delta = match event.keystroke.key.as_str() {
                "up" => -1,
                "down" => 1,
                _ => return,
            };
            if let Some(id) = self.selected {
                self.m.move_by(id, delta);
                cx.notify();
            }
            cx.stop_propagation();
            return;
        }
        let sel = self.selected;
        match event.keystroke.key.as_str() {
            "n" => self.open_form(None, window, cx),
            "e" if sel.is_some() => self.open_form(sel, window, cx),
            "r" if sel.is_some() => {
                self.m.restart(sel.unwrap_or_default());
                cx.notify();
            }
            "o" => {
                if let Some(url) = sel.and_then(|id| self.m.get(id)).and_then(Entry::url) {
                    cx.open_url(&url);
                }
            }
            "delete" if sel.is_some() => self.remove(sel.unwrap_or_default(), cx),
            "down" if !ids.is_empty() => {
                let next = pos.map_or(0, |p| (p + 1).min(ids.len() - 1));
                self.select(ids[next], cx);
            }
            "up" if !ids.is_empty() => {
                let prev = pos.map_or(0, |p| p.saturating_sub(1));
                self.select(ids[prev], cx);
            }
            "enter" | "space" => {
                if let Some(id) = self.selected {
                    self.m.toggle(id);
                    cx.notify();
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    // -----------------------------------------------------------------------
    // Add / edit

    pub(super) fn remove(&mut self, id: Id, cx: &mut Context<Self>) {
        if self.confirm_remove != Some(id) {
            self.confirm_remove = Some(id);
        } else {
            self.m.remove(id);
            self.confirm_remove = None;
            self.selected = self.m.entries.first().map(|e| e.id);
        }
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // Rendering

    fn render_toast(&self) -> Option<impl IntoElement> {
        let (text, kind, _) = self.m.message.as_ref()?;
        let color = if *kind == MsgKind::Error { RED } else { INK };
        Some(
            div().absolute().bottom_6().left_0().right_0().flex().justify_center().child(
                sheet()
                .bg(c(theme::OVERLAY))
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .max_w(px(680.))
                    .px_4()
                    .py_2()
                    .shadow_md()
                    .child(div().size(px(7.)).flex_none().rounded_full().bg(c(color)))
                    .child(label(text.clone(), 13., TEXT)),
            ),
        )
    }
}

impl Render for Board {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Keyboard shortcuts need the board focused. Without a dialog open,
        // reclaim focus if it's nowhere or stuck on an input that was just
        // closed (it isn't inside the board anymore).
        if self.form.is_none() && !self.focus.contains_focused(window, cx) {
            window.focus(&self.focus, cx);
        }
        let entry = self.selected.and_then(|id| self.m.get(id));
        self.logs.sync(entry, cx);
        let form = self.form.as_ref().map(|f| self.render_form(f, window, cx).into_any_element());
        div()
            .relative()
            .size_full()
            .bg(c(FIELD))
            .text_color(c(TEXT))
            .font_family(MONO)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .child(sketch::grid(10., 5, wash(0.07), wash(0.15)))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .child(self.render_sheet_header())
                    .child(div().h(px(RULER)).pl(px(RULER)).child(sketch::ruler(false, line())))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .child(div().w(px(RULER)).h_full().child(sketch::ruler(true, line())))
                            .child(self.render_sheets(window, cx)),
                    ),
            )
            .children(self.render_toast())
            .children(form)
    }
}

impl Board {
    /// The strip above the rulers: what the drawing is, and its revision.
    fn render_sheet_header(&self) -> impl IntoElement {
        let m = &self.m;
        let running = m.running_count();
        div()
            .h(px(30.))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .px(px(RULER + GUTTER))
            .child(label("sheet 01 · blueprint · dev servers", 11.5, MUTED))
            .child(label(
                format!(
                    "rev {} · {:02} projects · {} running",
                    env!("CARGO_PKG_VERSION"),
                    m.entries.len(),
                    running
                ),
                11.5,
                MUTED,
            ))
    }

    fn render_sheets(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
                div()
                    .flex_1()
                    .min_w_0()
                    .p(px(GUTTER))
                    .flex()
                    .gap(px(GUTTER))
                    .child(
                        div()
                            .w(px(SIDEBAR))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .gap(px(GUTTER))
                            .child(self.render_projects(cx))
                            .child(self.render_manage(cx)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(GUTTER))
                            .child(self.render_details(cx))
                            .child(self.logs.render(self.selected_entry(), window, cx)),
                    )
    }
}

