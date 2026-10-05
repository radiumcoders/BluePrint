//! The left column: the project list and the "manage" title block.


use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::board::Board;
use super::sketch;
use super::theme::{INK, MUTED, RED, TEXT, c, hairline, line, wash};
use super::widgets::{
    Kind, button, corner_label, fmt_duration, icon_button, label, lamp, mono, placeholder, sheet, status_word,
};
use crate::config::display_path;
use crate::manager::{Entry, Status};
use crate::process;

impl Board {
    pub(super) fn render_projects(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

    pub(super) fn render_project_row(&self, e: &Entry, cx: &mut Context<Self>) -> impl IntoElement {
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
    pub(super) fn render_manage(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
}
