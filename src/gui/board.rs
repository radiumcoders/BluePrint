//! The main window, laid out like a drawing sheet: projects and a title block
//! on the left, details and logs on the right, plus the add/edit and proxy
//! setup dialogs.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::sketch;
use super::theme::{self, FAINT, FIELD, INK, MONO, MUTED, RED, SANS, TEXT, alpha, c, wash};
use super::widgets::{Kind, button, corner_tag, heading, icon, placeholder, tag as ink_tag, icon_button, label, lamp, mono, sheet, spec, spec_tail, status_color, status_word};
use crate::ansi;
use crate::config::{display_path, suggest_name};
use crate::folders::{self, Folder};
use crate::manager::{Entry, Field, Id, Manager, MsgKind, Setup, SetupKind, Status};

/// Rendering more lines than this makes long logs sluggish; older ones stay in memory.
const MAX_RENDERED_LINES: usize = 800;
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

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
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
        let mods = &event.keystroke.modifiers;
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
                if let Some(e) = sel.and_then(|id| self.m.get(id)) {
                    cx.open_url(&self.m.expected_url(e));
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
            .child(corner_tag(format!("PROJECTS · {count:02}")))
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
                    el.child(sketch::hatch(wash(0.2), 5.))
                        .child(sketch::border(c(INK), 1., 6.))
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
        let (dot, proxy) = if !m.portless_ok {
            (RED, "portless missing".to_string())
        } else if m.proxy_ready() {
            let scheme = if m.proxy.tls { "https" } else { "http" };
            (theme::GREEN, format!("on · {scheme} :{}", m.effective_proxy_port()))
        } else if matches!(m.setup, Setup::Waiting(_)) {
            (theme::AMBER, "setting up…".to_string())
        } else {
            (FAINT, format!("off · :{}", m.config.proxy_port))
        };
        let clean = m.proxy_needs_root();
        let any_active = m.entries.iter().any(Entry::is_active);
        let line = alpha(INK, 0.3);

        let cell_label = |text: &'static str| {
            div()
                .w(px(72.))
                .flex_none()
                .px_2p5()
                .py_1p5()
                .border_r_1()
                .border_color(line)
                .child(label(text, 11.5, MUTED))
        };

        let title_block = div()
            .flex()
            .flex_col()
            .border_1()
            .border_color(line)
            .child(
                div()
                    .id("tb-proxy")
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .hover(|el| el.bg(wash(0.05)))
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new("Start or stop the portless proxy").build(window, cx)
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.m.toggle_proxy();
                        cx.notify();
                    }))
                    .child(cell_label("proxy"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_2p5()
                            .child(div().size(px(7.)).rounded_full().bg(c(dot)))
                            .child(mono(proxy, 12., TEXT)),
                    ),
            )
            .child(
                div()
                    .id("tb-urls")
                    .flex()
                    .items_center()
                    .border_t_1()
                    .border_color(line)
                    .when(!clean, |el| {
                        el.cursor_pointer()
                            .hover(|el| el.bg(wash(0.05)))
                            .tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new("Switch to https://name.localhost (port 443)")
                                    .build(window, cx)
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.m.use_clean_urls();
                                this.m.info("URLs will drop the port. The next start sets up port 443 once.");
                                cx.notify();
                            }))
                    })
                    .child(cell_label("urls"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_2p5()
                            .flex_1()
                            .child(mono(if clean { "clean · no port" } else { "with :1355" }, 12., TEXT))
                            .when(!clean, |el| el.child(div().flex_1()).child(label("make clean", 11.5, INK))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .border_t_1()
                    .border_color(line)
                    .child(cell_label("sheet"))
                    .child(div().px_2p5().child(mono(
                        format!("portboard v{} · 1/1", env!("CARGO_PKG_VERSION")),
                        12.,
                        MUTED,
                    ))),
            );

        sheet()
            .child(corner_tag("MANAGE"))
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
                    Kind::Primary,
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
                            Kind::Plain,
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
            return sheet()
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
        let url = self.m.expected_url(e);
        let confirming = self.confirm_remove == Some(id);
        let (open_url, copy_url) = (url.clone(), url.clone());
        let port = match (e.project.port, e.app_port.filter(|_| e.run.is_some())) {
            (Some(p), _) => format!("{p} fixed"),
            (None, Some(p)) => format!("{p} auto"),
            (None, None) => "auto".into(),
        };
        let command =
            if e.project.command.is_empty() { "package.json dev".to_string() } else { e.project.command.clone() };
        let host = url.split("://").nth(1).unwrap_or(&url).to_string();
        let route = match e.project.port.or(e.app_port.filter(|_| e.run.is_some())) {
            Some(p) => format!("127.0.0.1:{p} → {host}"),
            None => format!("port assigned on start → {host}"),
        };
        let pid = e.run.as_ref().map(|r| r.pid().to_string()).unwrap_or_else(|| "—".into());
        let up = e.run.as_ref().map(|r| fmt_duration(r.started.elapsed())).unwrap_or_else(|| "—".into());
        let mut status_text = status_word(status).to_string();
        if let Some(code) = e.last_exit.filter(|c| *c != 0 && e.run.is_none()) {
            status_text = format!("{status_text} · exit {code}");
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

        sheet()
            .child(corner_tag("DETAILS"))
            .flex_none()
            .px_6()
            .pt_5()
            .pb_5()
            .flex()
            .flex_col()
            .gap_3()
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
                    .child(icon_button(
                        "open",
                        IconName::ExternalLink,
                        c(INK),
                        "Open in browser (O)",
                        cx.listener(move |this, _, _, cx| {
                            if let Some(e) = this.m.get(id) {
                                cx.open_url(&this.m.expected_url(e));
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
                    .child(url)
                    .when(live, |el| el.child(icon(IconName::ArrowUpRight, 15., c(INK)))),
            )
            .child(
                div()
                    .relative()
                    .h(px(16.))
                    .my_1()
                    .child(sketch::dimension(wash(0.55)))
                    .child(div().absolute().inset_0().flex().items_center().justify_center().child(ink_tag(route))),
            )
            .child(
                div()
                    .flex()
                    .gap_8()
                    .child(spec_tail("folder", display_path(&e.project.path), TEXT).flex_1())
                    .child(spec("port", port, TEXT).w(px(120.)))
                    .child(spec("command", command, if e.project.command.is_empty() { MUTED } else { TEXT }).w(px(200.)))
                    .child(spec("pid", pid, TEXT).w(px(80.)))
                    .child(spec("up", up, TEXT).w(px(64.))),
            )
    }

    fn render_logs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entry = self.selected.and_then(|id| self.m.get(id));
        let tag = match entry {
            Some(e) => format!("LOGS · {}", e.project.name.to_uppercase()),
            None => "LOGS".to_string(),
        };
        let tools = div()
            .absolute()
            .top_2()
            .right_3()
            .flex()
            .items_center()
            .gap_1()
            .when(!self.follow && entry.is_some_and(|e| !e.logs.is_empty()), |el| {
                el.child(button(
                    "jump",
                    Some(IconName::ArrowDown),
                    "latest",
                    Kind::Plain,
                    cx.listener(|this, _, _, cx| {
                        this.follow = true;
                        this.console_scroll.scroll_to_bottom();
                        cx.notify();
                    }),
                ))
            })
            .when(entry.is_some(), |el| {
                el.child(icon_button(
                    "clear",
                    IconName::Eraser,
                    c(MUTED),
                    "Clear logs",
                    cx.listener(|this, _, _, cx| {
                        if let Some(i) = this.selected.and_then(|id| this.m.index(id)) {
                            this.m.entries[i].clear_logs();
                        }
                        cx.notify();
                    }),
                ))
            });

        let body: AnyElement = match entry {
            Some(e) if !e.logs.is_empty() => {
                let skip = e.logs.len().saturating_sub(MAX_RENDERED_LINES);
                div()
                    .id("logs")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.console_scroll)
                    .px_5()
                    .py_2()
                    .font_family(MONO)
                    .text_size(px(12.5))
                    .line_height(px(19.))
                    .text_color(c(TEXT))
                    .children(e.logs.iter().skip(skip).map(|l| log_line(l)))
                    .into_any_element()
            }
            Some(e) => {
                let hint = if e.run.is_some() { "WAITING FOR OUTPUT" } else { "NOT RUNNING · PRESS START" };
                div().flex_1().flex().p_5().pt(px(46.)).child(placeholder(hint)).into_any_element()
            }
            None => div().flex_1().into_any_element(),
        };

        sheet()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .pt_3()
            .child(corner_tag(tag))
            .child(body)
            .child(tools)
    }

    fn render_form(&self, form: &ProjectForm, cx: &mut Context<Self>) -> impl IntoElement {
        let title = if form.editing.is_some() { "edit project" } else { "new project" };
        let err = |field: Field| form.error.as_ref().filter(|(f, _)| *f == field).map(|(_, m)| m.clone());
        let name_value = form.name.read(cx).value().to_string();
        let preview = self.m.url_for(if name_value.trim().is_empty() { "name" } else { name_value.trim() });
        let line = alpha(INK, 0.3);

        let folder_section: AnyElement = match &form.folder {
            Some(path) => div()
                .flex()
                .items_center()
                .gap_3()
                .h(px(42.))
                .px_3()
                .border_1()
                .border_color(line)
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
                            .child(div().flex_1().font_family(MONO).child(Input::new(&form.filter)))
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
                            .border_color(line)
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

        let field = |name: &'static str, input: AnyElement, note: Option<String>, error: Option<String>| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(label(name, 12.5, MUTED))
                .child(input)
                .map(|el| match (error, note) {
                    (Some(e), _) => el.child(label(e, 12., RED)),
                    (None, Some(n)) => el.child(mono(n, 12., INK)),
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
                .child(sketch::rule(alpha(INK, 0.45)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(label("folder", 12.5, MUTED))
                        .child(folder_section)
                        .when_some(err(Field::Folder), |el, e| el.child(label(e, 12., RED))),
                )
                .child(field("name", div().font_family(MONO).child(Input::new(&form.name)).into_any_element(), Some(preview), err(Field::Name)))
                .child(
                    div()
                        .flex()
                        .gap_4()
                        .child(div().w(px(150.)).child(field(
                            "port",
                            div().font_family(MONO).child(Input::new(&form.port)).into_any_element(),
                            None,
                            err(Field::Port),
                        )))
                        .child(div().flex_1().child(field(
                            "command",
                            div().font_family(MONO).child(Input::new(&form.command)).into_any_element(),
                            None,
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
                .relative()
                .flex()
                .items_start()
                .gap_3()
                .p_4()
                .cursor_pointer()
                .hover(|el| el.bg(wash(0.05)))
                .on_click(move |ev, window, cx| on_click(ev, window, cx))
                .child(sketch::border(alpha(INK, if recommended { 1. } else { 0.35 }), 1., 6.))
                .child(icon(name, 18., c(if recommended { INK } else { MUTED })))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(label(title, 14., TEXT).font_weight(FontWeight::SEMIBOLD))
                                .when(recommended, |el| {
                                    el.child(
                                        mono("RECOMMENDED", 9.5, theme::ON_INK)
                                            .font_weight(FontWeight::BOLD)
                                            .px_1p5()
                                            .bg(c(INK)),
                                    )
                                }),
                        )
                        .child(label(body, 12., MUTED)),
                )
        };

        let content: AnyElement = if waiting {
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(icon(IconName::SquareTerminal, 22., c(INK)))
                        .child(label("finish in the terminal window, it asks for your password", 13., TEXT)),
                )
                .child(label("portboard starts your projects as soon as the proxy is up.", 12.5, MUTED))
                .child(div().flex().justify_end().child(button(
                    "cancel-setup",
                    None,
                    "cancel",
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
                .child(label(
                    format!(
                        "to serve https://name.localhost with no port, the portless proxy listens on port {port}. \
                         that needs your password once.{}",
                        stray.map(|p| format!(" the proxy on :{p} will be stopped first.")).unwrap_or_default()
                    ),
                    12.5,
                    MUTED,
                ).line_height(px(20.)))
                .child(option(
                    "setup-service",
                    IconName::ShieldCheck,
                    "install as a service",
                    "opens a terminal for your password · starts on boot · trusts the https certificate".into(),
                    true,
                    Box::new(svc),
                ))
                .child(option(
                    "setup-once",
                    IconName::Power,
                    "start once",
                    "opens a terminal for your password · runs until you reboot".into(),
                    false,
                    Box::new(once),
                ))
                .child(option(
                    "setup-fallback",
                    IconName::Globe,
                    "skip, use port 1355",
                    format!("no password · urls end in :{fallback}"),
                    false,
                    Box::new(fb),
                ))
                .child(div().flex().justify_end().child(button(
                    "cancel-setup",
                    None,
                    "cancel",
                    Kind::Ghost,
                    cx.listener(|this, _, _, cx| {
                        this.m.cancel_setup();
                        cx.notify();
                    }),
                )))
                .into_any_element()
        };

        modal(
            sheet()
                .bg(c(theme::OVERLAY))
                .w(px(580.))
                .p_6()
                .flex()
                .flex_col()
                .gap_3()
                .child(label(if waiting { "waiting for the proxy" } else { "one-time setup" }, 12.5, MUTED))
                .child(heading("clean urls need port 443", 20.))
                .child(sketch::rule(alpha(INK, 0.45)))
                .child(content),
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
        let form = self.form.as_ref().map(|f| self.render_form(f, cx).into_any_element());
        let setup = (self.m.setup != Setup::None).then(|| self.render_setup(cx).into_any_element());
        div()
            .relative()
            .size_full()
            .bg(c(FIELD))
            .text_color(c(TEXT))
            .font_family(SANS)
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
                    .child(div().h(px(RULER)).pl(px(RULER)).child(sketch::ruler(false, wash(0.5))))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .child(div().w(px(RULER)).h_full().child(sketch::ruler(true, wash(0.5))))
                            .child(self.render_sheets(cx)),
                    ),
            )
            .children(self.render_toast())
            .children(form)
            .children(setup)
    }
}

impl Board {
    /// The strip above the rulers: what the drawing is, and its revision.
    fn render_sheet_header(&self) -> impl IntoElement {
        let m = &self.m;
        let running = m.running_count();
        let proxy = if m.proxy_ready() { format!("proxy :{}", m.effective_proxy_port()) } else { "proxy off".into() };
        div()
            .h(px(30.))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .px(px(RULER + GUTTER))
            .child(label("sheet 01 · portboard · dev servers", 11.5, MUTED))
            .child(label(
                format!(
                    "rev {} · {:02} projects · {} running · {proxy}",
                    env!("CARGO_PKG_VERSION"),
                    m.entries.len(),
                    running
                ),
                11.5,
                MUTED,
            ))
    }

    fn render_sheets(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                            .child(self.render_logs(cx)),
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
        .bg(alpha(0x02296f, 0.62))
        .occlude()
        .child(content)
}

fn tag(name: &'static str) -> Div {
    mono(name, 10., INK).flex_none().px_1p5().border_1().border_color(alpha(INK, 0.4))
}

fn log_line(raw: &str) -> AnyElement {
    if raw.is_empty() {
        return div().h(px(19.)).into_any_element();
    }
    if raw.starts_with("── ") {
        return div().text_color(c(INK)).child(raw.to_string()).into_any_element();
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
