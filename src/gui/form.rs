//! The add/edit project dialog.

use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::board::Board;
use super::sketch;
use super::theme::{self, FAINT, INK, MUTED, RED, TEXT, alpha, c, hairline, line, wash};
use super::widgets::{Kind, button, caption, heading, icon, icon_button, label, mono, sheet, text_field};
use crate::config::{display_path, suggest_name};
use crate::folders::{self, Folder};
use crate::stack::{self, Recipe};
use crate::manager::{Field, Id};
use crate::process;

pub(super) struct ProjectForm {
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

impl Board {
    pub(super) fn open_form(&mut self, editing: Option<Id>, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(super) fn pick_folder(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(super) fn browse_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(super) fn save_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(super) fn close_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(super) fn render_form(&self, form: &ProjectForm, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
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

}

/// Dim the drawing behind a dialog with a paper-colored veil.
pub(super) fn modal(content: impl IntoElement) -> Div {
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
