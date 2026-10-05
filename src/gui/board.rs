//! The main window: the board of projects, the console for the selected one,
//! and the add/edit and proxy setup dialogs.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::theme::{
    self, AMBER, CONSOLE, FAINT, GREEN, INK, LINE, MONO, MUTED, PANEL, RAISED, RAISED_HI, RED, SANS, TEXT, alpha, c,
};
use super::widgets::{Kind, button, caps, icon, icon_button, lamp, status_color};
use crate::ansi;
use crate::config::{display_path, suggest_name};
use crate::folders::{self, Folder};
use crate::manager::{Entry, Field, Id, Manager, MsgKind, Setup, SetupKind, Status};

/// Rendering more lines than this makes long logs sluggish; older ones stay in memory.
const MAX_RENDERED_LINES: usize = 800;

// Column widths shared by the board header and rows.
const COL_STATUS: f32 = 120.;
const COL_ADDRESS: f32 = 300.;
const COL_PORT: f32 = 72.;
const COL_UP: f32 = 64.;
const COL_ACTION: f32 = 34.;

struct ProjectForm {
    editing: Option<Id>,
    name: Entity<InputState>,
    port: Entity<InputState>,
    command: Entity<InputState>,
    filter: Entity<InputState>,
    folder: Option<PathBuf>,
    /// Subfolders of the projects root, for one-click picking.
    folders: Vec<Folder>,
    error: Option<(Field, String)>,
    _subscriptions: Vec<Subscription>,
}

pub struct Board {
    m: Manager,
    selected: Option<Id>,
    form: Option<ProjectForm>,
    /// Remove needs a second click; this is the project awaiting it.
    confirm_remove: Option<Id>,
    console_scroll: ScrollHandle,
    /// Keep the console pinned to the newest line.
    follow: bool,
    /// (selected project, its log revision) when last rendered.
    seen_logs: (Option<Id>, u64),
    last_second: Instant,
    focus: FocusHandle,
}

impl Board {
    pub fn new(m: Manager, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let selected = m.entries.first().map(|e| e.id);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            m,
            selected,
            form: None,
            confirm_remove: None,
            console_scroll: ScrollHandle::new(),
            follow: true,
            seen_logs: (None, 0),
            last_second: Instant::now(),
            focus,
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
        let rev = self.selected.and_then(|id| self.m.get(id)).map_or(0, |e| e.log_rev);
        if (self.selected, rev) != self.seen_logs {
            if self.seen_logs.0 != self.selected {
                self.follow = true;
            } else {
                self.follow = self.console_at_bottom();
            }
            self.seen_logs = (self.selected, rev);
            if self.follow {
                self.console_scroll.scroll_to_bottom();
            }
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    pub fn shutdown(&mut self) {
        self.m.shutdown();
    }

    fn console_at_bottom(&self) -> bool {
        let max = self.console_scroll.max_offset().y;
        let offset = -self.console_scroll.offset().y;
        offset >= max - px(24.)
    }

    fn select(&mut self, id: Id, cx: &mut Context<Self>) {
        if self.selected != Some(id) {
            self.selected = Some(id);
            self.confirm_remove = None;
            self.follow = true;
            self.console_scroll.scroll_to_bottom();
        }
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.form.is_some() || self.m.setup != Setup::None {
            if event.keystroke.key == "escape" {
                self.form = None;
                if self.m.setup == Setup::Needed {
                    self.m.cancel_setup();
                }
                cx.notify();
            }
            return;
        }
        let ids: Vec<Id> = self.m.entries.iter().map(|e| e.id).collect();
        let pos = self.selected.and_then(|id| ids.iter().position(|&i| i == id));
        match event.keystroke.key.as_str() {
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

    fn open_form(&mut self, editing: Option<Id>, window: &mut Window, cx: &mut Context<Self>) {
        let project = editing.and_then(|id| self.m.get(id)).map(|e| e.project.clone());
        let input = |placeholder: &'static str, value: String, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).default_value(value))
        };
        let name = input("my-app", project.as_ref().map(|p| p.name.clone()).unwrap_or_default(), window, cx);
        let port = input(
            "auto",
            project.as_ref().and_then(|p| p.port).map(|p| p.to_string()).unwrap_or_default(),
            window,
            cx,
        );
        let command = input(
            "package.json \u{201c}dev\u{201d} script",
            project.as_ref().map(|p| p.command.clone()).unwrap_or_default(),
            window,
            cx,
        );
        let filter = input("Filter folders", String::new(), window, cx);

        let mut subscriptions = Vec::new();
        for state in [&name, &port, &command] {
            subscriptions.push(cx.subscribe_in(state, window, |this: &mut Self, _, ev: &InputEvent, window, cx| {
                match ev {
                    InputEvent::PressEnter { .. } => this.save_form(window, cx),
                    InputEvent::Change => {
                        if let Some(f) = &mut this.form {
                            f.error = None;
                        }
                        cx.notify();
                    }
                    _ => {}
                }
            }));
        }
        subscriptions.push(cx.subscribe_in(&filter, window, |this: &mut Self, _, ev: &InputEvent, window, cx| {
            match ev {
                // Enter picks the first match.
                InputEvent::PressEnter { .. } => {
                    let first = this.form.as_ref().and_then(|f| {
                        let q = f.filter.read(cx).value().to_string();
                        f.folders.iter().find(|d| folders::fuzzy(q.trim(), &d.name)).map(|d| d.path.clone())
                    });
                    if let Some(path) = first {
                        this.pick_folder(path, window, cx);
                    }
                }
                InputEvent::Change => cx.notify(),
                _ => {}
            }
        }));

        let has_folder = project.is_some();
        self.form = Some(ProjectForm {
            editing,
            name: name.clone(),
            port,
            command,
            filter: filter.clone(),
            folder: project.map(|p| p.path),
            folders: folders::list(&self.m.config.projects_root),
            error: None,
            _subscriptions: subscriptions,
        });
        let focus_on = if has_folder { name } else { filter };
        focus_on.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    fn pick_folder(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &mut self.form else { return };
        // Refresh the suggested name unless the user typed their own.
        let current = form.name.read(cx).value().to_string();
        let old_suggestion = form.folder.as_deref().map(suggest_name).unwrap_or_default();
        if current.trim().is_empty() || current == old_suggestion {
            let name = suggest_name(&path);
            form.name.update(cx, |s, cx| s.set_value(name, window, cx));
        }
        form.folder = Some(path);
        form.error = None;
        let name = form.name.clone();
        name.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    fn browse_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose project folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let path = match rx.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Err(e)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.m.error(format!("Couldn't open the folder picker: {e}"));
                        cx.notify();
                    });
                    None
                }
                _ => None,
            };
            if let Some(path) = path {
                let _ = this.update_in(cx, |this, window, cx| this.pick_folder(path, window, cx));
            }
        })
        .detach();
    }

    fn save_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.form else { return };
        let value = |s: &Entity<InputState>| s.read(cx).value().to_string();
        let result = self.m.validate(
            form.editing,
            &value(&form.name),
            form.folder.as_ref(),
            &value(&form.port),
            &value(&form.command),
        );
        match result {
            Ok(project) => {
                let name = project.name.clone();
                match form.editing {
                    Some(id) => {
                        if self.m.update(id, project) {
                            self.m.info(format!("Saved. Restart {name} to apply the changes."));
                        } else {
                            self.m.info("Saved");
                        }
                    }
                    None => {
                        let id = self.m.add(project);
                        self.selected = Some(id);
                        self.m.info(format!("Added {name}. Press start when you're ready."));
                    }
                }
                self.form = None;
                window.focus(&self.focus, cx);
            }
            Err(err) => {
                if let Some(f) = &mut self.form {
                    f.error = Some(err);
                }
            }
        }
        cx.notify();
    }

    fn close_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn remove(&mut self, id: Id, cx: &mut Context<Self>) {
        if self.confirm_remove != Some(id) {
            self.confirm_remove = Some(id);
        } else {
            let name = self.m.get(id).map(|e| e.project.name.clone()).unwrap_or_default();
            self.m.remove(id);
            self.confirm_remove = None;
            self.selected = self.m.entries.first().map(|e| e.id);
            self.m.info(format!("Removed {name}"));
        }
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // Rendering

    fn render_top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let m = &self.m;
        let (dot, label) = if !m.portless_ok {
            (RED, "PORTLESS MISSING".to_string())
        } else if m.proxy_ready() {
            let port = m.effective_proxy_port();
            (GREEN, format!("PROXY :{port}{}", if m.proxy.tls { " · HTTPS" } else { "" }))
        } else if matches!(m.setup, Setup::Waiting(_)) {
            (AMBER, "PROXY SETTING UP".to_string())
        } else {
            (FAINT, format!("PROXY OFF · :{}", m.config.proxy_port))
        };
        let any_active = m.entries.iter().any(Entry::is_active);
        let tooltip = if m.proxy_ready() { "Stop the portless proxy" } else { "Start the portless proxy" };

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_3()
            .h(px(64.))
            .px_5()
            .border_b_1()
            .border_color(c(LINE))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .size(px(30.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .bg(c(AMBER))
                            .child(icon(IconName::Anchor, 16., c(theme::ON_AMBER))),
                    )
                    .child(
                        div()
                            .font_family(MONO)
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(15.))
                            .child("PORTBOARD"),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("proxy-pill")
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(32.))
                    .px_3()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(c(LINE))
                    .font_family(MONO)
                    .text_size(px(11.5))
                    .text_color(c(MUTED))
                    .cursor_pointer()
                    .hover(|el| el.bg(c(RAISED)).text_color(c(TEXT)))
                    .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tooltip).build(window, cx))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.m.toggle_proxy();
                        cx.notify();
                    }))
                    .child(div().size(px(7.)).rounded_full().bg(c(dot)))
                    .child(label),
            )
            .when(!m.proxy_needs_root(), |el| {
                el.child(button(
                    "clean-urls",
                    None,
                    "Use clean URLs",
                    Kind::Ghost,
                    cx.listener(|this, _, _, cx| {
                        this.m.use_clean_urls();
                        this.m.info("URLs will drop the port. The next start sets up port 443 once.");
                        cx.notify();
                    }),
                ))
            })
            .when(!m.entries.is_empty(), |el| {
                el.child(if any_active {
                    button(
                        "stop-all",
                        Some(IconName::Square),
                        "Stop all",
                        Kind::Ghost,
                        cx.listener(|this, _, _, cx| {
                            this.m.stop_all();
                            cx.notify();
                        }),
                    )
                } else {
                    button(
                        "start-all",
                        Some(IconName::Play),
                        "Start all",
                        Kind::Ghost,
                        cx.listener(|this, _, _, cx| {
                            this.m.start_all();
                            cx.notify();
                        }),
                    )
                })
            })
            .child(button(
                "add",
                Some(IconName::Plus),
                "Add project",
                Kind::Primary,
                cx.listener(|this, _, window, cx| this.open_form(None, window, cx)),
            ))
    }

    fn render_board(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.m.entries.is_empty() {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .child(
                    div()
                        .font_family(MONO)
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(22.))
                        .text_color(c(AMBER))
                        .child("NO DEPARTURES YET"),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(c(MUTED))
                        .child("Add a project folder and run it at https://name.localhost"),
                )
                .child(div().h(px(8.)))
                .child(button(
                    "add-empty",
                    Some(IconName::Plus),
                    "Add project",
                    Kind::Primary,
                    cx.listener(|this, _, window, cx| this.open_form(None, window, cx)),
                ))
                .into_any_element();
        }

        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap_4()
            .h(px(34.))
            .px_5()
            .border_b_1()
            .border_color(c(LINE))
            .child(caps("STATUS").w(px(COL_STATUS)))
            .child(caps("PROJECT").flex_1())
            .child(caps("ADDRESS").w(px(COL_ADDRESS)))
            .child(caps("PORT").w(px(COL_PORT)))
            .child(caps("UP").w(px(COL_UP)))
            .child(div().w(px(COL_ACTION)));

        let rows: Vec<AnyElement> = self.m.entries.iter().map(|e| self.render_row(e, cx).into_any_element()).collect();

        div()
            .flex()
            .flex_col()
            .flex_none()
            .max_h(relative(0.5))
            .child(header)
            .child(div().id("board").flex().flex_col().min_h_0().overflow_y_scroll().children(rows))
            .into_any_element()
    }

    fn render_row(&self, e: &Entry, cx: &mut Context<Self>) -> impl IntoElement {
        let id = e.id;
        let status = e.status();
        let selected = self.selected == Some(id);
        let url = self.m.expected_url(e);
        let live = status == Status::Running;
        let uptime = e.run.as_ref().map(|r| fmt_duration(r.started.elapsed())).unwrap_or_default();
        let (port, port_color) = match e.project.port {
            Some(p) => (format!(":{p}"), TEXT),
            None => match e.app_port.filter(|_| e.run.is_some()) {
                Some(p) => (format!(":{p}"), MUTED),
                None => ("auto".to_string(), FAINT),
            },
        };
        let open_url = url.clone();

        div()
            .id(SharedString::from(format!("row-{id}")))
            .flex()
            .flex_none()
            .items_center()
            .gap_4()
            .h(px(58.))
            .relative()
            .px_5()
            .border_b_1()
            .border_color(c(LINE))
            .cursor_pointer()
            .map(|el| {
                if selected {
                    el.bg(c(RAISED))
                        .child(div().absolute().left_0().top_0().bottom_0().w(px(3.)).bg(c(AMBER)))
                } else {
                    el.hover(|el| el.bg(c(PANEL)))
                }
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.focus, cx);
                this.select(id, cx);
            }))
            .child(
                div()
                    .w(px(COL_STATUS))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(lamp(status))
                    .child(
                        div()
                            .font_family(MONO)
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(11.))
                            .text_color(c(status_color(status)))
                            .child(status.label()),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .font_family(MONO)
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(14.))
                            .truncate()
                            .child(e.project.name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(c(MUTED))
                            .truncate()
                            .child(display_path(&e.project.path)),
                    ),
            )
            .child(
                div()
                    .id(SharedString::from(format!("url-{id}")))
                    .w(px(COL_ADDRESS))
                    .font_family(MONO)
                    .text_size(px(12.5))
                    .truncate()
                    .text_color(c(if live { AMBER } else { FAINT }))
                    .when(live, |el| {
                        el.hover(|el| el.underline()).on_click(move |_, _, cx| {
                            cx.open_url(&open_url);
                            cx.stop_propagation();
                        })
                    })
                    .child(url),
            )
            .child(
                div()
                    .w(px(COL_PORT))
                    .font_family(MONO)
                    .text_size(px(12.5))
                    .text_color(c(port_color))
                    .child(port),
            )
            .child(
                div()
                    .w(px(COL_UP))
                    .font_family(MONO)
                    .text_size(px(12.))
                    .text_color(c(MUTED))
                    .child(uptime),
            )
            .child(div().w(px(COL_ACTION)).child(if e.is_active() {
                icon_button(
                    SharedString::from(format!("stop-{id}")),
                    IconName::Square,
                    c(RED),
                    "Stop",
                    cx.listener(move |this, _, _, cx| {
                        this.m.stop(id);
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
            } else {
                icon_button(
                    SharedString::from(format!("start-{id}")),
                    IconName::Play,
                    c(AMBER),
                    "Start",
                    cx.listener(move |this, _, _, cx| {
                        this.m.start(id);
                        this.select(id, cx);
                        cx.stop_propagation();
                    }),
                )
            }))
    }

    fn render_console(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(e) = self.selected.and_then(|id| self.m.get(id)) else {
            return div().flex_1().into_any_element();
        };
        let id = e.id;
        let url = self.m.expected_url(e);
        let live = e.status() == Status::Running;
        let confirming = self.confirm_remove == Some(id);
        let (open_url, copy_url) = (url.clone(), url.clone());

        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap_3()
            .h(px(48.))
            .px_5()
            .bg(c(PANEL))
            .border_b_1()
            .border_color(c(LINE))
            .child(caps("CONSOLE"))
            .child(
                div()
                    .font_family(MONO)
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(13.))
                    .child(e.project.name.clone()),
            )
            .child(
                div()
                    .id("console-url")
                    .font_family(MONO)
                    .text_size(px(12.))
                    .text_color(c(if live { AMBER } else { FAINT }))
                    .truncate()
                    .min_w_0()
                    .when(live, |el| {
                        el.cursor_pointer()
                            .hover(|el| el.underline())
                            .on_click(move |_, _, cx| cx.open_url(&open_url))
                    })
                    .child(url),
            )
            .child(div().flex_1())
            .child(icon_button(
                "open",
                IconName::ExternalLink,
                c(MUTED),
                "Open in browser",
                cx.listener(move |this, _, _, cx| {
                    if let Some(e) = this.m.get(id) {
                        cx.open_url(&this.m.expected_url(e));
                    }
                }),
            ))
            .child(icon_button(
                "copy",
                IconName::Copy,
                c(MUTED),
                "Copy URL",
                cx.listener(move |this, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy_url.clone()));
                    this.m.info(format!("Copied {copy_url}"));
                    cx.notify();
                }),
            ))
            .child(icon_button(
                "restart",
                IconName::RotateCw,
                c(MUTED),
                "Restart",
                cx.listener(move |this, _, _, cx| {
                    this.m.restart(id);
                    cx.notify();
                }),
            ))
            .child(icon_button(
                "edit",
                IconName::Pencil,
                c(MUTED),
                "Edit",
                cx.listener(move |this, _, window, cx| this.open_form(Some(id), window, cx)),
            ))
            .child(icon_button(
                "clear",
                IconName::Eraser,
                c(MUTED),
                "Clear console",
                cx.listener(move |this, _, _, cx| {
                    if let Some(i) = this.m.index(id) {
                        this.m.entries[i].clear_logs();
                    }
                    cx.notify();
                }),
            ))
            .child(if confirming {
                button(
                    "remove-confirm",
                    Some(IconName::Trash),
                    "Remove?",
                    Kind::Danger,
                    cx.listener(move |this, _, _, cx| this.remove(id, cx)),
                )
                .into_any_element()
            } else {
                icon_button(
                    "remove",
                    IconName::Trash,
                    c(MUTED),
                    "Remove from portboard",
                    cx.listener(move |this, _, _, cx| this.remove(id, cx)),
                )
                .into_any_element()
            });

        let skip = e.logs.len().saturating_sub(MAX_RENDERED_LINES);
        let lines: Vec<AnyElement> = e.logs.iter().skip(skip).map(|l| log_line(l)).collect();
        let body = if lines.is_empty() {
            let hint = if e.run.is_some() { "Waiting for output…" } else { "Not running. Press start to launch it." };
            div()
                .flex_1()
                .p_5()
                .font_family(MONO)
                .text_size(px(12.5))
                .text_color(c(FAINT))
                .child(hint)
                .into_any_element()
        } else {
            div()
                .id("console")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.console_scroll)
                .px_5()
                .py_3()
                .font_family(MONO)
                .text_size(px(12.5))
                .line_height(px(19.))
                .text_color(c(0xd8d4ca))
                .children(lines)
                .into_any_element()
        };

        let jump = (!self.follow && !e.logs.is_empty()).then(|| {
            div().absolute().bottom_4().right_5().child(button(
                "jump",
                Some(IconName::ArrowDown),
                "Latest",
                Kind::Plain,
                cx.listener(|this, _, _, cx| {
                    this.follow = true;
                    this.console_scroll.scroll_to_bottom();
                    cx.notify();
                }),
            ))
        });

        div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(c(CONSOLE))
            .border_t_1()
            .border_color(c(LINE))
            .child(header)
            .child(body)
            .children(jump)
            .into_any_element()
    }

    fn render_form(&self, form: &ProjectForm, cx: &mut Context<Self>) -> impl IntoElement {
        let title = if form.editing.is_some() { "Edit project" } else { "New project" };
        let err = |field: Field| form.error.as_ref().filter(|(f, _)| *f == field).map(|(_, m)| m.clone());
        let name_value = form.name.read(cx).value().to_string();
        let preview = if name_value.trim().is_empty() {
            self.m.url_for("name")
        } else {
            self.m.url_for(name_value.trim())
        };

        let folder_section: AnyElement = match &form.folder {
            Some(path) => div()
                .flex()
                .items_center()
                .gap_3()
                .h(px(44.))
                .px_3()
                .rounded(px(6.))
                .bg(c(RAISED))
                .child(icon(IconName::FolderOpen, 16., c(AMBER)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_family(MONO)
                        .text_size(px(12.5))
                        .truncate()
                        .child(display_path(path)),
                )
                .children(folders::tags_for(path).into_iter().map(tag))
                .child(button(
                    "change-folder",
                    None,
                    "Change",
                    Kind::Ghost,
                    cx.listener(|this, _, window, cx| {
                        if let Some(f) = &mut this.form {
                            f.folder = None;
                            let filter = f.filter.clone();
                            filter.update(cx, |s, cx| s.focus(window, cx));
                        }
                        cx.notify();
                    }),
                ))
                .into_any_element(),
            None => {
                let query = form.filter.read(cx).value().to_string();
                let matches: Vec<&Folder> =
                    form.folders.iter().filter(|f| folders::fuzzy(query.trim(), &f.name)).collect();
                let rows: Vec<AnyElement> = matches
                    .iter()
                    .enumerate()
                    .map(|(ix, f)| {
                        let path = f.path.clone();
                        div()
                            .id(("folder", ix))
                            .flex()
                            .items_center()
                            .gap_2p5()
                            .h(px(34.))
                            .px_3()
                            .rounded(px(5.))
                            .cursor_pointer()
                            .hover(|el| el.bg(c(RAISED_HI)))
                            .on_click(cx.listener(move |this, _, window, cx| this.pick_folder(path.clone(), window, cx)))
                            .child(icon(IconName::Folder, 14., c(MUTED)))
                            .child(div().flex_1().font_family(MONO).text_size(px(12.5)).truncate().child(f.name.clone()))
                            .children(f.tags.iter().copied().map(tag))
                            .into_any_element()
                    })
                    .collect();
                let empty = rows.is_empty();
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(div().flex_1().child(Input::new(&form.filter)))
                            .child(button(
                                "browse",
                                Some(IconName::FolderSearch),
                                "Browse…",
                                Kind::Plain,
                                cx.listener(|this, _, window, cx| this.browse_folder(window, cx)),
                            )),
                    )
                    .child(
                        div()
                            .id("folder-list")
                            .h(px(200.))
                            .overflow_y_scroll()
                            .p_1()
                            .rounded(px(6.))
                            .bg(c(INK))
                            .border_1()
                            .border_color(c(LINE))
                            .children(rows)
                            .when(empty, |el| {
                                el.child(
                                    div()
                                        .p_3()
                                        .text_size(px(12.5))
                                        .text_color(c(FAINT))
                                        .child(format!(
                                            "No folders match in {}. Use Browse… to pick any folder.",
                                            display_path(&self.m.config.projects_root)
                                        )),
                                )
                            }),
                    )
                    .into_any_element()
            }
        };

        let field = |label: &'static str, input: AnyElement, note: Option<String>, error: Option<String>| {
            div()
                .flex()
                .flex_col()
                .gap_1p5()
                .child(caps(label))
                .child(input)
                .when_some(error.or(note.clone()).filter(|_| true), |el, text| {
                    let is_error = note.as_deref() != Some(text.as_str());
                    el.child(
                        div()
                            .font_family(if is_error { SANS } else { MONO })
                            .text_size(px(12.))
                            .text_color(c(if is_error { RED } else { AMBER }))
                            .child(text),
                    )
                })
        };

        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(alpha(0x000000, 0.62))
            .occlude()
            .child(
                div()
                    .w(px(640.))
                    .flex()
                    .flex_col()
                    .gap_5()
                    .p_6()
                    .rounded(px(10.))
                    .bg(c(PANEL))
                    .border_1()
                    .border_color(c(LINE))
                    .shadow_lg()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .font_family(SANS)
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(px(20.))
                                    .child(title),
                            )
                            .child(icon_button(
                                "close-form",
                                IconName::X,
                                c(MUTED),
                                "Cancel",
                                cx.listener(|this, _, window, cx| this.close_form(window, cx)),
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .child(caps("FOLDER"))
                            .child(folder_section)
                            .when_some(err(Field::Folder), |el, e| {
                                el.child(div().text_size(px(12.)).text_color(c(RED)).child(e))
                            }),
                    )
                    .child(field(
                        "NAME",
                        Input::new(&form.name).into_any_element(),
                        Some(preview),
                        err(Field::Name),
                    ))
                    .child(
                        div()
                            .flex()
                            .gap_4()
                            .child(div().w(px(150.)).child(field(
                                "PORT",
                                Input::new(&form.port).into_any_element(),
                                None,
                                err(Field::Port),
                            )))
                            .child(div().flex_1().child(field(
                                "COMMAND",
                                Input::new(&form.command).into_any_element(),
                                None,
                                err(Field::Command),
                            ))),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(button(
                                "cancel",
                                None,
                                "Cancel",
                                Kind::Ghost,
                                cx.listener(|this, _, window, cx| this.close_form(window, cx)),
                            ))
                            .child(button(
                                "save",
                                Some(IconName::Check),
                                if form.editing.is_some() { "Save" } else { "Add project" },
                                Kind::Primary,
                                cx.listener(|this, _, window, cx| this.save_form(window, cx)),
                            )),
                    ),
            )
    }

    fn render_setup(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let port = self.m.config.proxy_port;
        let waiting = matches!(self.m.setup, Setup::Waiting(_));
        let stray = self.m.stray_proxy_port();

        let option = |id: &'static str,
                      name: IconName,
                      title: &'static str,
                      body: String,
                      recommended: bool,
                      on_click: Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>| {
            div()
                .id(id)
                .flex()
                .items_start()
                .gap_3()
                .p_4()
                .rounded(px(8.))
                .border_1()
                .border_color(c(if recommended { AMBER } else { LINE }))
                .when(recommended, |el| el.bg(alpha(AMBER, 0.06)))
                .cursor_pointer()
                .hover(|el| el.bg(c(RAISED)))
                .on_click(move |ev, window, cx| on_click(ev, window, cx))
                .child(icon(name, 18., c(if recommended { AMBER } else { MUTED })))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().font_weight(FontWeight::BOLD).text_size(px(14.)).child(title))
                                .when(recommended, |el| {
                                    el.child(
                                        div()
                                            .px_1p5()
                                            .rounded(px(3.))
                                            .bg(c(AMBER))
                                            .font_family(MONO)
                                            .font_weight(FontWeight::BOLD)
                                            .text_size(px(9.5))
                                            .text_color(c(theme::ON_AMBER))
                                            .child("RECOMMENDED"),
                                    )
                                }),
                        )
                        .child(div().text_size(px(12.5)).text_color(c(MUTED)).child(body)),
                )
        };

        let content: AnyElement = if waiting {
            div()
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(icon(IconName::SquareTerminal, 20., c(AMBER)))
                        .child(
                            div()
                                .text_size(px(14.))
                                .child("Finish in the terminal window. Enter your password there."),
                        ),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(c(MUTED))
                        .child("portboard starts your projects as soon as the proxy is up."),
                )
                .child(div().flex().justify_end().child(button(
                    "cancel-setup",
                    None,
                    "Cancel",
                    Kind::Ghost,
                    cx.listener(|this, _, _, cx| {
                        this.m.cancel_setup();
                        cx.notify();
                    }),
                )))
                .into_any_element()
        } else {
            let fallback = crate::config::FALLBACK_PROXY_PORT;
            let svc = cx.listener(|this, _, _, cx| {
                this.m.run_setup(SetupKind::Service);
                cx.notify();
            });
            let once = cx.listener(|this, _, _, cx| {
                this.m.run_setup(SetupKind::Once);
                cx.notify();
            });
            let fb = cx.listener(|this, _, _, cx| {
                this.m.use_fallback_port();
                cx.notify();
            });
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div().text_size(px(13.5)).text_color(c(MUTED)).child(format!(
                        "To serve https://name.localhost with no port, the portless proxy listens on port {port}, \
                         which needs your password once.{}",
                        stray
                            .map(|p| format!(" The proxy on :{p} will be stopped first."))
                            .unwrap_or_default()
                    )),
                )
                .child(option(
                    "setup-service",
                    IconName::ShieldCheck,
                    "Install as a service",
                    "Opens a terminal for your password. Starts on boot and trusts the HTTPS certificate.".into(),
                    true,
                    Box::new(svc),
                ))
                .child(option(
                    "setup-once",
                    IconName::Power,
                    "Start once",
                    "Opens a terminal for your password. Runs until you reboot.".into(),
                    false,
                    Box::new(once),
                ))
                .child(option(
                    "setup-fallback",
                    IconName::Globe,
                    "Skip, use port 1355",
                    format!("No password. URLs end in :{fallback}."),
                    false,
                    Box::new(fb),
                ))
                .child(div().flex().justify_end().child(button(
                    "cancel-setup",
                    None,
                    "Cancel",
                    Kind::Ghost,
                    cx.listener(|this, _, _, cx| {
                        this.m.cancel_setup();
                        cx.notify();
                    }),
                )))
                .into_any_element()
        };

        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(alpha(0x000000, 0.62))
            .occlude()
            .child(
                div()
                    .w(px(560.))
                    .flex()
                    .flex_col()
                    .gap_4()
                    .p_6()
                    .rounded(px(10.))
                    .bg(c(PANEL))
                    .border_1()
                    .border_color(c(LINE))
                    .shadow_lg()
                    .child(caps(if waiting { "WAITING FOR THE PROXY" } else { "ONE-TIME SETUP" }).text_color(c(AMBER)))
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(20.))
                            .child("Clean URLs need the proxy on port 443"),
                    )
                    .child(content),
            )
    }

    fn render_toast(&self) -> Option<impl IntoElement> {
        let (text, kind, _) = self.m.message.as_ref()?;
        let color = if *kind == MsgKind::Error { RED } else { AMBER };
        Some(
            div().absolute().bottom_5().left_0().right_0().flex().justify_center().child(
                div()
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .max_w(px(640.))
                    .px_4()
                    .py_2p5()
                    .rounded(px(8.))
                    .bg(c(RAISED_HI))
                    .border_1()
                    .border_color(alpha(color, 0.5))
                    .shadow_lg()
                    .child(div().size(px(7.)).flex_none().rounded_full().bg(c(color)))
                    .child(div().text_size(px(13.)).child(text.clone())),
            ),
        )
    }
}

impl Render for Board {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Keyboard shortcuts need the board focused; claim focus whenever
        // nothing else (like a dialog input) has it.
        if window.focused(cx).is_none() {
            window.focus(&self.focus, cx);
        }
        let form = self.form.as_ref().map(|f| self.render_form(f, cx).into_any_element());
        let setup = (self.m.setup != Setup::None).then(|| self.render_setup(cx).into_any_element());
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(c(INK))
            .text_color(c(TEXT))
            .font_family(SANS)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .child(self.render_top_bar(cx))
            .child(self.render_board(cx))
            .child(self.render_console(cx))
            .children(self.render_toast())
            .children(form)
            .children(setup)
    }
}

fn tag(name: &'static str) -> Div {
    div()
        .flex_none()
        .px_1p5()
        .rounded(px(3.))
        .border_1()
        .border_color(c(LINE))
        .font_family(MONO)
        .text_size(px(10.))
        .text_color(c(MUTED))
        .child(name)
}

fn log_line(raw: &str) -> AnyElement {
    if raw.is_empty() {
        return div().h(px(19.)).into_any_element();
    }
    if raw.starts_with("── ") {
        return div().text_color(c(AMBER)).child(raw.to_string()).into_any_element();
    }
    if raw.starts_with("$ portless") {
        return div().text_color(c(FAINT)).child(raw.to_string()).into_any_element();
    }
    let styled = ansi::parse(raw);
    if styled.runs.is_empty() {
        return div().child(styled.text).into_any_element();
    }
    let highlights: Vec<_> = styled
        .runs
        .iter()
        .map(|(range, s)| {
            (
                range.clone(),
                HighlightStyle {
                    color: s.fg.map(|col| Hsla::from(theme::ansi_color(col))),
                    font_weight: s.bold.then_some(FontWeight::BOLD),
                    font_style: s.italic.then_some(FontStyle::Italic),
                    underline: s.underline.then_some(UnderlineStyle { thickness: px(1.), ..Default::default() }),
                    fade_out: s.dim.then_some(0.45),
                    ..Default::default()
                },
            )
        })
        .collect();
    div().child(StyledText::new(styled.text).with_highlights(highlights)).into_any_element()
}

fn fmt_duration(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        _ => format!("{}h{:02}", s / 3600, (s % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(fmt_duration(Duration::from_secs(5)), "5s");
        assert_eq!(fmt_duration(Duration::from_secs(125)), "2m");
        assert_eq!(fmt_duration(Duration::from_secs(3720)), "1h02");
    }
}
