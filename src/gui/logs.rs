//! The log console: every retained line of the selected project, laid out
//! lazily so thousands of lines stay smooth, with a filter, copy and save.

use std::collections::VecDeque;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::board::Board;
use super::theme::{self, FAINT, INK, MONO, MUTED, TEXT, c};
use super::widgets::{Kind, button, corner_label, icon_button, label, placeholder, sheet, text_field};
use crate::ansi;
use crate::manager::{Entry, Id, MAX_LOG_LINES};

/// Which retained lines the console shows, by line number (see
/// [`Entry::log_start`]), kept in step with the log a change at a time.
#[derive(Default)]
pub struct LogIndex {
    /// The project and (lowercased) filter the lines were picked for.
    key: Option<(Id, String)>,
    /// Numbers of the lines shown, oldest first.
    pub lines: VecDeque<u64>,
    /// Number of the next line not yet looked at.
    next: u64,
}

#[derive(Debug, PartialEq)]
pub enum Change {
    None,
    /// Different lines altogether.
    Reset,
    /// `dropped` lines left the front and `added` arrived at the back.
    Splice { dropped: usize, added: usize },
}

impl LogIndex {
    pub fn sync(&mut self, entry: Option<&Entry>, filter: &str) -> Change {
        let Some(e) = entry else {
            let had = self.key.take().is_some() || !self.lines.is_empty();
            self.lines.clear();
            return if had { Change::Reset } else { Change::None };
        };
        let query = filter.trim().to_lowercase();
        let end = e.log_start + e.logs.len() as u64;
        let line = |n: u64| &e.logs[(n - e.log_start) as usize];
        let key = Some((e.id, query));
        if self.key != key {
            self.key = key;
            let query = &self.key.as_ref().unwrap().1;
            self.lines = (e.log_start..end).filter(|&n| matches(line(n), query)).collect();
            self.next = end;
            return Change::Reset;
        }
        let query = &self.key.as_ref().unwrap().1;
        let mut dropped = 0;
        while self.lines.front().is_some_and(|&n| n < e.log_start) {
            self.lines.pop_front();
            dropped += 1;
        }
        let before = self.lines.len();
        self.lines.extend((self.next.max(e.log_start)..end).filter(|&n| matches(line(n), query)));
        self.next = end;
        match (dropped, self.lines.len() - before) {
            (0, 0) => Change::None,
            (dropped, added) => Change::Splice { dropped, added },
        }
    }

    pub fn filtering(&self) -> bool {
        self.key.as_ref().is_some_and(|(_, q)| !q.is_empty())
    }

    /// The text of the lines shown, without escapes, for copying or saving.
    pub fn plain_text(&self, e: &Entry) -> String {
        let mut out = String::new();
        for &n in &self.lines {
            if let Some(line) = n.checked_sub(e.log_start).and_then(|i| e.logs.get(i as usize)) {
                out.push_str(&ansi::strip(line));
                out.push('\n');
            }
        }
        out
    }
}

fn matches(line: &str, query: &str) -> bool {
    query.is_empty() || ansi::strip(line).to_lowercase().contains(query)
}

/// The console's state, held by the board.
pub struct LogView {
    pub list: ListState,
    pub filter: Entity<InputState>,
    pub index: LogIndex,
    _subscription: Subscription,
}

impl LogView {
    pub fn new(window: &mut Window, cx: &mut Context<Board>) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("filter logs"));
        let subscription = cx.subscribe(&filter, |_, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                cx.notify();
            }
        });
        let list = ListState::new(0, ListAlignment::Top, px(400.));
        list.set_follow_mode(FollowMode::Tail);
        Self { list, filter, index: LogIndex::default(), _subscription: subscription }
    }

    /// Bring the shown lines in step with `entry`'s log.
    pub fn sync(&mut self, entry: Option<&Entry>, cx: &App) {
        let filter = self.filter.read(cx).value().to_string();
        match self.index.sync(entry, &filter) {
            Change::None => {}
            Change::Reset => {
                self.list.reset(self.index.lines.len());
                self.list.set_follow_mode(FollowMode::Tail);
            }
            Change::Splice { dropped, added } => {
                if dropped > 0 {
                    self.list.splice(0..dropped, 0);
                }
                let at = self.index.lines.len() - added;
                self.list.splice(at..at, added);
            }
        }
    }

    pub fn follow(&self) {
        self.list.set_follow_mode(FollowMode::Tail);
    }

    pub fn filter_focused(&self, window: &Window, cx: &App) -> bool {
        self.filter.read(cx).focus_handle(cx).is_focused(window)
    }

    pub fn focus_filter(&self, window: &mut Window, cx: &mut App) {
        self.filter.update(cx, |s, cx| s.focus(window, cx));
    }

    pub fn clear_filter(&self, window: &mut Window, cx: &mut App) {
        self.filter.update(cx, |s, cx| s.set_value("", window, cx));
    }

    /// Line `ix` of the console.
    pub fn render_line(&self, entry: Option<&Entry>, ix: usize) -> AnyElement {
        let line = entry.zip(self.index.lines.get(ix)).and_then(|(e, &n)| {
            n.checked_sub(e.log_start).and_then(|i| e.logs.get(i as usize))
        });
        let query = self.index.key.as_ref().map(|(_, q)| q.as_str()).unwrap_or_default();
        let body = match line {
            Some(raw) => log_line(raw, query),
            None => div().into_any_element(),
        };
        div().px_5().child(body).into_any_element()
    }

    pub fn render(&self, entry: Option<&Entry>, window: &Window, cx: &mut Context<Board>) -> Div {
        let tag = match entry {
            Some(e) => format!("LOGS · {}", e.project.name.to_uppercase()),
            None => "LOGS".to_string(),
        };
        let shown = self.index.lines.len();
        let count = entry.map(|e| {
            let total = e.logs.len();
            let mut text = if self.index.filtering() {
                format!("{shown} of {total} lines")
            } else {
                format!("{total} lines")
            };
            if e.log_start > 0 {
                text.push_str(&format!(" · older dropped, keeps {MAX_LOG_LINES}"));
            }
            text
        });
        let has_lines = entry.is_some_and(|e| !e.logs.is_empty());

        let toolbar = div()
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .px_3()
            .pt_4()
            .pb_1()
            .when(entry.is_some(), |el| {
                el.child(div().w(px(220.)).child(text_field(&self.filter, window, cx)))
                    .child(icon_button(
                        "logs-filter-clear",
                        IconName::X,
                        c(MUTED),
                        "Clear filter (Esc)",
                        cx.listener(|this, _, window, cx| {
                            this.logs.clear_filter(window, cx);
                            cx.notify();
                        }),
                    ))
            })
            .child(label(count.unwrap_or_default(), 11.5, MUTED).flex_1().min_w_0().truncate().pl_1())
            .when(has_lines && !self.list.is_following_tail(), |el| {
                el.child(button(
                    "jump",
                    Some(IconName::ArrowDown),
                    "latest",
                    Kind::Plain,
                    cx.listener(|this, _, _, cx| {
                        this.logs.follow();
                        cx.notify();
                    }),
                ))
            })
            .when(has_lines, |el| {
                el.child(icon_button(
                    "logs-copy",
                    IconName::Copy,
                    c(INK),
                    "Copy the lines shown",
                    cx.listener(|this, _, _, cx| this.copy_logs(cx)),
                ))
                .child(icon_button(
                    "logs-save",
                    IconName::Download,
                    c(INK),
                    "Save the lines shown to a file",
                    cx.listener(|this, _, window, cx| this.save_logs(window, cx)),
                ))
            })
            .when(entry.is_some(), |el| {
                el.child(icon_button(
                    "clear",
                    IconName::Eraser,
                    c(MUTED),
                    "Clear logs",
                    cx.listener(|this, _, _, cx| this.clear_logs(cx)),
                ))
            });

        let body: AnyElement = match entry {
            Some(_) if shown > 0 => div()
                .flex_1()
                .min_h_0()
                .py_1()
                .font_family(MONO)
                .text_size(px(12.5))
                .line_height(px(19.))
                .text_color(c(TEXT))
                .child(
                    list(self.list.clone(), cx.processor(|this: &mut Board, ix, _, _| this.render_log_line(ix)))
                        .size_full(),
                )
                .into_any_element(),
            Some(e) => {
                let hint = if self.index.filtering() {
                    "NO LINES MATCH"
                } else if e.run.is_some() {
                    "WAITING FOR OUTPUT"
                } else {
                    "NOT RUNNING · PRESS START"
                };
                div().flex_1().flex().p_5().pt_2().child(placeholder(hint)).into_any_element()
            }
            None => div().flex_1().into_any_element(),
        };

        sheet()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(corner_label(tag))
            .child(toolbar)
            .child(body)
    }
}

/// One log line. Status lines (`── ... ──`) are drawn in ink and the command
/// line faint; others keep their colors, or, while filtering, show plain
/// with the matches marked.
fn log_line(raw: &str, query: &str) -> AnyElement {
    if raw.is_empty() {
        return div().h(px(19.)).into_any_element();
    }
    if !query.is_empty() {
        let text = ansi::strip(raw);
        let lower = text.to_lowercase();
        // Lowercasing can change byte lengths outside ASCII; only mark
        // matches when offsets line up.
        let marks: Vec<_> = if lower.len() == text.len() {
            lower
                .match_indices(query)
                .map(|(i, m)| {
                    (
                        i..i + m.len(),
                        HighlightStyle {
                            color: Some(Hsla::from(c(theme::ON_INK))),
                            background_color: Some(Hsla::from(c(INK))),
                            ..Default::default()
                        },
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        return div().child(StyledText::new(text).with_highlights(marks)).into_any_element();
    }
    if raw.starts_with("── ") {
        return div().text_color(c(INK)).child(raw.to_string()).into_any_element();
    }
    if raw.starts_with("$ PORT=") {
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

#[cfg(test)]
mod tests {
    // Not `super::*`: that would pull in gpui's `test` macro and shadow `#[test]`.
    use super::{Change, LogIndex};
    use crate::config::Project;
    use crate::manager::Manager;

    fn manager_with(lines: &[&str]) -> Manager {
        let mut m = crate::testkit::manager("logindex");
        let tmp = std::env::temp_dir();
        m.add(Project { name: "a".into(), path: tmp, port: None, command: "x".into() });
        for l in lines {
            m.entries[0].log(*l);
        }
        m
    }

    #[test]
    fn follows_the_log() {
        let mut m = manager_with(&["one", "two", "three"]);
        let mut ix = LogIndex::default();
        assert_eq!(ix.sync(Some(&m.entries[0]), ""), Change::Reset);
        assert_eq!(ix.lines, [0, 1, 2]);
        assert_eq!(ix.sync(Some(&m.entries[0]), ""), Change::None);
        m.entries[0].log("four");
        assert_eq!(ix.sync(Some(&m.entries[0]), ""), Change::Splice { dropped: 0, added: 1 });
        // Clearing drops everything shown, and new lines keep counting up.
        m.entries[0].clear_logs();
        m.entries[0].log("five");
        assert_eq!(ix.sync(Some(&m.entries[0]), ""), Change::Splice { dropped: 4, added: 1 });
        assert_eq!(ix.lines, [4]);
        assert_eq!(ix.plain_text(&m.entries[0]), "five\n");
        assert_eq!(ix.sync(None, ""), Change::Reset);
    }

    #[test]
    fn filters() {
        let mut m = manager_with(&["GET /a", "\x1b[31merror\x1b[0m: boom", "GET /b"]);
        let mut ix = LogIndex::default();
        assert_eq!(ix.sync(Some(&m.entries[0]), " get "), Change::Reset);
        assert_eq!(ix.lines, [0, 2]);
        assert!(ix.filtering());
        m.entries[0].log("POST /c");
        assert_eq!(ix.sync(Some(&m.entries[0]), "get"), Change::None);
        m.entries[0].log("get /d");
        assert_eq!(ix.sync(Some(&m.entries[0]), "get"), Change::Splice { dropped: 0, added: 1 });
        // Escapes don't count toward matches, and copies come out plain.
        assert_eq!(ix.sync(Some(&m.entries[0]), "error:"), Change::Reset);
        assert_eq!(ix.plain_text(&m.entries[0]), "error: boom\n");
    }
}
