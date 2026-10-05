//! The details sheet: the selected project's state, address and wiring.


use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::board::Board;
use super::sketch;
use super::theme::{FAINT, INK, MONO, MUTED, TEXT, c, line};
use super::widgets::{
    Kind, button, corner_tag, fmt_duration, icon, icon_button, label, lamp, lead_sheet, mono, placeholder, spec, spec_tail,
    status_color, status_word, tag as ink_tag,
};
use crate::config::display_path;
use crate::stack;
use crate::manager::{Entry, Status};

impl Board {
    pub(super) fn render_details(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
}
