//! The main window, laid out like a drawing sheet: projects and a title block
//! on the left, details and logs on the right, plus the add/edit dialog.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::logs::LogView;
use super::sketch;
use super::theme::{self, FAINT, FIELD, INK, MONO, MUTED, RED, TEXT, alpha, c, hairline, line, wash};
use super::widgets::{Kind, button, corner_label, corner_tag, heading, lead_sheet, icon, placeholder, tag as ink_tag, icon_button, caption, label, lamp, text_field, mono, sheet, spec, spec_tail, status_color, status_word};
use crate::config::{display_path, suggest_name};
use crate::folders::{self, Folder};
use crate::stack::{self, Recipe};
use crate::manager::{Entry, Field, Id, Manager, MsgKind, Status};
use crate::process;

/// Space between sheets.
const GUTTER: f32 = 18.;
const SIDEBAR: f32 = 300.;
/// Thickness of the rulers along the top and left edges.
const RULER: f32 = 10.;


struct ProjectForm {
    editing: Option<Id>,
    name: Entity<InputState>,
    port: Entity<InputState>,
    command: Entity<InputState>,
    filter: Entity<InputState>,
    folder: Option<PathBuf>,
    /// How the picked folder would start, if recognized.
    recipe: Option<Recipe>,
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
    pub(super) logs: LogView,
    last_second: Instant,
    focus: FocusHandle,
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

    fn select(&mut self, id: Id, cx: &mut Context<Self>) {
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
            "detected from the folder",
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
            recipe: project.as_ref().and_then(|p| stack::detect(&p.path)),
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
        // Same for the command: fill in what the folder looks like it needs.
        let recipe = stack::detect(&path);
        let current = form.command.read(cx).value().to_string();
        let old_command = form.recipe.as_ref().map(|r| r.command.as_str()).unwrap_or_default();
        if current.trim().is_empty() || current == old_command {
            let command = recipe.as_ref().map(|r| r.command.clone()).unwrap_or_default();
            form.command.update(cx, |s, cx| s.set_value(command, window, cx));
        }
        form.recipe = recipe;
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
                match form.editing {
                    Some(id) => self.m.update(id, project),
                    None => self.selected = Some(self.m.add(project)),
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
            self.m.remove(id);
            self.confirm_remove = None;
            self.selected = self.m.entries.first().map(|e| e.id);
        }
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // Rendering

    fn render_projects(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.m.entries.len();
        let body: AnyElement = if count == 0 {
            div().flex_1().flex().p_4().child(placeholder("NO PROJECTS · ADD ONE BELOW")).into_any_element()
        } else {
            let rows: Vec<AnyElement> =
                self.m.entries.iter().map(|e| self.render_project_row(e, cx).into_any_element()).collect();
            div()
                .id("project-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px_2()
                .py_2()
                .flex()
                .flex_col()
                .gap_0p5()
                .children(rows)
                .into_any_element()
        };

        sheet()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .pt_3()
            .child(corner_label(format!("PROJECTS · {count:02}")))
            .child(body)
    }

    fn render_project_row(&self, e: &Entry, cx: &mut Context<Self>) -> impl IntoElement {
        let id = e.id;
        let status = e.status();
        let selected = self.selected == Some(id);
        let row_id = format!("row-{id}");
        let detail = match &e.run {
            Some(r) if status == Status::Running => format!("running · {}", fmt_duration(r.started.elapsed())),
            _ => status_word(status).to_string(),
        };

        div()
            .id(SharedString::from(row_id.clone()))
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap_3()
            .h(px(50.))
            .pl_2()
            .pr_1()
            .rounded(px(3.))
            .cursor_pointer()
            .map(|el| {
                if selected {
                    el.bg(wash(0.1)).child(sketch::border(line(), 1.))
                } else {
                    el.hover(|el| el.bg(wash(0.05)))
                }
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.focus, cx);
                this.select(id, cx);
            }))
            .child(lamp(status))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        mono(e.project.name.clone(), 14., if selected { INK } else { TEXT })
                            .font_weight(FontWeight::MEDIUM)
                            .truncate(),
                    )
                    .child(label(detail, 11.5, if status == Status::Crashed { RED } else { MUTED }).truncate()),
            )
            .child(if e.is_active() {
                icon_button(
                    SharedString::from(format!("stop-{id}")),
                    IconName::Square,
                    c(RED),
                    "Stop (Enter)",
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
                    c(INK),
                    "Start (Enter)",
                    cx.listener(move |this, _, _, cx| {
                        this.m.start(id);
                        this.select(id, cx);
                        cx.stop_propagation();
                    }),
                )
            })
    }

    /// The "add project / manage" box, drawn as a drawing's title block.
    fn render_manage(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let m = &self.m;
        let any_active = m.entries.iter().any(Entry::is_active);
        let any_idle = m.entries.iter().any(|e| !e.is_active());
        // Adding leads only on an empty board; otherwise the selected
        // project's start button is the main action.
        let add_kind = if m.entries.is_empty() { Kind::Primary } else { Kind::Plain };

        let cell_label = |text: &'static str| {
            div()
                .w(px(72.))
                .flex_none()
                .px_2p5()
                .py_1p5()
                .border_r_1()
                .border_color(hairline())
                .child(label(text, 11.5, MUTED))
        };

        // Values lose their start when too long: paths end in what matters.
        let row = |name: &'static str, value: String| {
            div().flex().items_center().child(cell_label(name)).child(
                div().px_2p5().flex_1().min_w_0().child(
                    mono(value, 12., TEXT).overflow_hidden().whitespace_nowrap().text_ellipsis_start(),
                ),
            )
        };
        let (lo, hi) = (process::AUTO_PORTS.start(), process::AUTO_PORTS.end());
        let title_block = div()
            .flex()
            .flex_col()
            .border_1()
            .border_color(hairline())
            .child(row("ports", format!("auto · {lo}–{hi}")))
            .child(row("config", display_path(m.config_path())).border_t_1().border_color(hairline()));

        sheet()
            .child(corner_label("MANAGE"))
            .flex_none()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                button(
                    "add",
                    Some(IconName::Plus),
                    "add project",
                    add_kind,
                    cx.listener(|this, _, window, cx| this.open_form(None, window, cx)),
                )
                .w_full(),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        button(
                            "start-all",
                            Some(IconName::Play),
                            "start all",
                            if any_idle { Kind::Plain } else { Kind::Ghost },
                            cx.listener(|this, _, _, cx| {
                                this.m.start_all();
                                cx.notify();
                            }),
                        )
                        .flex_1(),
                    )
                    .child(
                        button(
                            "stop-all",
                            Some(IconName::Square),
                            "stop all",
                            if any_active { Kind::Plain } else { Kind::Ghost },
                            cx.listener(|this, _, _, cx| {
                                this.m.stop_all();
                                cx.notify();
                            }),
                        )
                        .flex_1(),
                    ),
            )
            .child(title_block)
    }

    fn render_details(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(e) = self.selected.and_then(|id| self.m.get(id)) else {
            return lead_sheet()
                .child(corner_tag("DETAILS"))
                .flex_none()
                .h(px(190.))
                .flex()
                .p_5()
                .child(placeholder("PICK A PROJECT"));
        };
        let id = e.id;
        let status = e.status();
        let live = status == Status::Running;
        let url = e.url();
        let confirming = self.confirm_remove == Some(id);
        let (open_url, copy_url) = (url.clone().unwrap_or_default(), url.clone().unwrap_or_default());
        // While it runs, show what it runs with; edits wait for a restart.
        let shown = e.run.as_ref().map_or(&e.project, |r| &r.project);
        let port = match (&e.run, shown.port) {
            (Some(r), None) => format!("{} auto", r.port),
            (_, Some(p)) => format!("{p} fixed"),
            (None, None) => "auto".into(),
        };
        // An empty command shows what it resolves to.
        let command = match (&e.run, shown.command.trim()) {
            (Some(r), _) => r.command.clone(),
            (None, "") => stack::dev_script_command(&shown.path).unwrap_or_else(|| "package.json dev".into()),
            (None, c) => c.to_string(),
        };
        let route = match e.port() {
            Some(p) => format!("PORT={p}"),
            None => "PORT picked on start".into(),
        };
        let pid = e.run.as_ref().map(|r| r.pid().to_string()).unwrap_or_else(|| "—".into());
        let up = e.run.as_ref().map(|r| fmt_duration(r.started.elapsed())).unwrap_or_else(|| "—".into());
        let mut status_text = status_word(status).to_string();
        if let Some(code) = e.last_exit.filter(|c| *c != 0 && e.run.is_none()) {
            status_text = format!("{status_text} · exit {code}");
        }
        if e.edited_while_running() {
            status_text = format!("{status_text} · edited, restart to apply");
        }

        let toggle = if e.is_active() {
            button(
                "toggle",
                Some(IconName::Square),
                "stop",
                Kind::Danger,
                cx.listener(move |this, _, _, cx| {
                    this.m.stop(id);
                    cx.notify();
                }),
            )
        } else {
            button(
                "toggle",
                Some(IconName::Play),
                "start",
                Kind::Primary,
                cx.listener(move |this, _, _, cx| {
                    this.m.start(id);
                    cx.notify();
                }),
            )
        };

        lead_sheet()
            .child(corner_tag("DETAILS"))
            .flex_none()
            .px_6()
            .pt_5()
            .pb_5()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(lamp(status))
                    .child(
                        mono(e.project.name.clone(), 24., TEXT)
                            .font_weight(FontWeight::SEMIBOLD)
                            .min_w_0()
                            .truncate(),
                    )
                    .child(label(status_text, 13., status_color(status)).flex_none())
                    .child(div().flex_1())
                    .when(url.is_some(), |el| {
                        el.child(icon_button(
                            "open",
                            IconName::ExternalLink,
                            c(INK),
                            "Open in browser (O)",
                            cx.listener(move |this, _, _, cx| {
                                if let Some(url) = this.m.get(id).and_then(Entry::url) {
                                    cx.open_url(&url);
                                }
                            }),
                        ))
                        .child(icon_button(
                            "copy",
                            IconName::Copy,
                            c(INK),
                            "Copy URL",
                            cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy_url.clone()));
                                this.m.info(format!("copied {copy_url}"));
                                cx.notify();
                            }),
                        ))
                    })
                    .child(icon_button(
                        "restart",
                        IconName::RotateCw,
                        c(INK),
                        "Restart (R)",
                        cx.listener(move |this, _, _, cx| {
                            this.m.restart(id);
                            cx.notify();
                        }),
                    ))
                    .child(icon_button(
                        "edit",
                        IconName::Pencil,
                        c(INK),
                        "Edit (E)",
                        cx.listener(move |this, _, window, cx| this.open_form(Some(id), window, cx)),
                    ))
                    .child(if confirming {
                        button(
                            "remove-confirm",
                            Some(IconName::Trash),
                            "remove?",
                            Kind::Danger,
                            cx.listener(move |this, _, _, cx| this.remove(id, cx)),
                        )
                        .into_any_element()
                    } else {
                        icon_button(
                            "remove",
                            IconName::Trash,
                            c(MUTED),
                            "Remove (Delete)",
                            cx.listener(move |this, _, _, cx| this.remove(id, cx)),
                        )
                        .into_any_element()
                    })
                    .child(div().w(px(6.)))
                    .child(toggle),
            )
            .child(
                div()
                    .id("details-url")
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .font_family(MONO)
                    .text_size(px(15.))
                    .text_color(c(if live { INK } else { FAINT }))
                    .when(live, |el| {
                        el.cursor_pointer()
                            .hover(|el| el.underline())
                            .on_click(move |_, _, cx| cx.open_url(&open_url))
                    })
                    .child(url.unwrap_or_else(|| "http://localhost:—".into()))
                    .when(live, |el| el.child(icon(IconName::ArrowUpRight, 15., c(INK)))),
            )
            .child(
                div()
                    .relative()
                    .h(px(16.))
                    // A pause between who the project is and how it's wired.
                    .mt_4()
                    .mb_2()
                    .child(sketch::dimension(line()))
                    .child(div().absolute().inset_0().flex().items_center().justify_center().child(ink_tag(route))),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_x_8()
                    .gap_y_3()
                    // Keep the folder readable on narrow or scaled windows;
                    // the other specs wrap below it instead of crushing it.
                    .child(spec_tail("folder", display_path(&shown.path), TEXT).flex_1().min_w(px(160.)))
                    .child(spec("port", port, TEXT).w(px(120.)))
                    .child(spec("command", command, if shown.command.is_empty() { MUTED } else { TEXT }).w(px(200.)))
                    .child(spec("pid", pid, TEXT).w(px(80.)))
                    .child(spec("up", up, TEXT).w(px(64.))),
            )
    }

    fn selected_entry(&self) -> Option<&Entry> {
        self.selected.and_then(|id| self.m.get(id))
    }

    pub(super) fn render_log_line(&self, ix: usize) -> AnyElement {
        self.logs.render_line(self.selected_entry(), ix)
    }

    pub(super) fn copy_logs(&mut self, cx: &mut Context<Self>) {
        let Some(e) = self.selected_entry() else { return };
        let text = self.logs.index.plain_text(e);
        let n = self.logs.index.lines.len();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.m.info(format!("Copied {n} log lines"));
        cx.notify();
    }

    /// Save the lines shown to a file the user picks.
    pub(super) fn save_logs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(e) = self.selected_entry() else { return };
        let text = self.logs.index.plain_text(e);
        let dir = e.project.path.clone();
        let rx = cx.prompt_for_new_path(&dir, Some(&format!("{}-logs.txt", e.project.name)));
        cx.spawn_in(window, async move |this, cx| {
            let result = match rx.await {
                Ok(Ok(Some(path))) => Some(std::fs::write(&path, text).map(|_| path).map_err(|e| e.to_string())),
                Ok(Err(e)) => Some(Err(format!("couldn't open the save dialog: {e}"))),
                _ => None,
            };
            if let Some(result) = result {
                let _ = this.update(cx, |this, cx| {
                    match result {
                        Ok(path) => this.m.info(format!("Saved logs to {}", display_path(&path))),
                        Err(e) => this.m.error(format!("Couldn't save the logs: {e}")),
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(super) fn clear_logs(&mut self, cx: &mut Context<Self>) {
        if let Some(i) = self.selected.and_then(|id| self.m.index(id)) {
            self.m.entries[i].clear_logs();
        }
        cx.notify();
    }

    fn render_form(&self, form: &ProjectForm, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = if form.editing.is_some() { "edit project" } else { "new project" };
        let err = |field: Field| form.error.as_ref().filter(|(f, _)| *f == field).map(|(_, m)| m.clone());
        let port_value = form.port.read(cx).value().trim().to_string();
        let (lo, hi) = (process::AUTO_PORTS.start(), process::AUTO_PORTS.end());
        let port_note = match port_value.parse::<u16>() {
            Ok(p) if p > 0 => format!("http://localhost:{p}"),
            _ => format!("auto: {lo}–{hi}"),
        };

        let folder_section: AnyElement = match &form.folder {
            Some(path) => div()
                .flex()
                .items_center()
                .gap_3()
                .h(px(42.))
                .px_3()
                .border_1()
                .border_color(hairline())
                .bg(wash(0.04))
                .child(icon(IconName::FolderOpen, 16., c(INK)))
                .child(
                    mono(display_path(path), 12.5, TEXT)
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis_start(),
                )
                .children(folders::tags_for(path).into_iter().map(tag))
                .child(button(
                    "change-folder",
                    None,
                    "change",
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
                let rows: Vec<AnyElement> = form
                    .folders
                    .iter()
                    .filter(|f| folders::fuzzy(query.trim(), &f.name))
                    .enumerate()
                    .map(|(ix, f)| {
                        let path = f.path.clone();
                        div()
                            .id(("folder", ix))
                            .flex()
                            .items_center()
                            .gap_2p5()
                            .h(px(32.))
                            .px_3()
                            .cursor_pointer()
                            .hover(|el| el.bg(wash(0.07)))
                            .on_click(cx.listener(move |this, _, window, cx| this.pick_folder(path.clone(), window, cx)))
                            .child(icon(IconName::Folder, 14., c(INK)))
                            .child(mono(f.name.clone(), 12.5, TEXT).flex_1().truncate())
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
                            .child(text_field(&form.filter, window, cx).flex_1())
                            .child(button(
                                "browse",
                                Some(IconName::FolderSearch),
                                "browse…",
                                Kind::Plain,
                                cx.listener(|this, _, window, cx| this.browse_folder(window, cx)),
                            )),
                    )
                    .child(
                        div()
                            .id("folder-list")
                            .h(px(196.))
                            .overflow_y_scroll()
                            .py_1()
                            .border_1()
                            .border_color(hairline())
                            .children(rows)
                            .when(empty, |el| {
                                el.child(div().p_3().child(label(
                                    format!(
                                        "nothing matches in {} · use browse… for any folder",
                                        display_path(&self.m.config.projects_root)
                                    ),
                                    12.,
                                    FAINT,
                                )))
                            }),
                    )
                    .into_any_element()
            }
        };

        let command_value = form.command.read(cx).value().trim().to_string();
        let command_note = form.folder.as_ref().map(|dir| match &form.recipe {
            _ if command_value.is_empty() && stack::has_dev_script(dir) => {
                format!("runs {}", stack::dev_script_command(dir).unwrap_or_default())
            }
            Some(r) if r.command == command_value && r.reads_env => {
                format!("detected {} · the server must listen on $PORT", r.name)
            }
            Some(r) if r.command == command_value => format!("detected {} · gets its port from $PORT", r.name),
            _ if command_value.is_empty() => "set the command that starts the server".to_string(),
            _ => "the server gets its port in $PORT".to_string(),
        });

        let field = |name: &'static str, input: AnyElement, note: Option<(String, u32)>, error: Option<String>| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(caption(name))
                .child(input)
                .map(|el| match (error, note) {
                    (Some(e), _) => el.child(label(e, 12., RED)),
                    (None, Some((n, color))) => el.child(mono(n, 12., color)),
                    (None, None) => el,
                })
        };

        modal(
            sheet()
                .bg(c(theme::OVERLAY))
                .w(px(660.))
                .p_6()
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .child(heading(title, 20.).flex_1())
                        .child(icon_button(
                            "close-form",
                            IconName::X,
                            c(MUTED),
                            "Cancel (Esc)",
                            cx.listener(|this, _, window, cx| this.close_form(window, cx)),
                        )),
                )
                .child(sketch::rule(line()))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(caption("folder"))
                        .child(folder_section)
                        .when_some(err(Field::Folder), |el, e| el.child(label(e, 12., RED))),
                )
                .child(field("name", text_field(&form.name, window, cx).into_any_element(), None, err(Field::Name)))
                .child(
                    div()
                        .flex()
                        .gap_4()
                        .child(div().w(px(150.)).child(field(
                            "port",
                            text_field(&form.port, window, cx).into_any_element(),
                            Some((port_note, INK)),
                            err(Field::Port),
                        )))
                        .child(div().flex_1().child(field(
                            "command",
                            text_field(&form.command, window, cx).into_any_element(),
                            command_note.map(|n| (n, MUTED)),
                            err(Field::Command),
                        ))),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .pt_1()
                        .child(button(
                            "cancel",
                            None,
                            "cancel",
                            Kind::Ghost,
                            cx.listener(|this, _, window, cx| this.close_form(window, cx)),
                        ))
                        .child(button(
                            "save",
                            Some(IconName::Check),
                            if form.editing.is_some() { "save" } else { "add project" },
                            Kind::Primary,
                            cx.listener(|this, _, window, cx| this.save_form(window, cx)),
                        )),
                ),
        )
    }

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

/// Dim the drawing behind a dialog with a paper-colored veil.
fn modal(content: impl IntoElement) -> Div {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .p_4()
        .bg(alpha(0x02296f, 0.62))
        .occlude()
        // Short windows scroll the dialog instead of cutting it off.
        .child(div().id("modal").max_h_full().overflow_y_scroll().child(content))
}

fn tag(name: &'static str) -> Div {
    mono(name, 10., INK).flex_none().px_1p5().border_1().border_color(line())
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
    // Not `super::*`: that would pull in gpui's `test` macro and shadow `#[test]`.
    use super::fmt_duration;
    use std::time::Duration;

    #[test]
    fn durations() {
        assert_eq!(fmt_duration(Duration::from_secs(5)), "5s");
        assert_eq!(fmt_duration(Duration::from_secs(125)), "2m");
        assert_eq!(fmt_duration(Duration::from_secs(3720)), "1h02");
    }
}
