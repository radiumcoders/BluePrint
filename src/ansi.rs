//! Minimal ANSI handling for log lines: SGR colors become style runs, every
//! other escape sequence (cursor moves, screen clears, OSC links) is dropped.

use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// 0-7: black, red, green, yellow, blue, magenta, cyan, white.
    Basic(u8),
    Bright(u8),
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Style {
    pub fg: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
}

/// A log line with escapes removed, plus styled byte ranges into `text`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Styled {
    pub text: String,
    pub runs: Vec<(Range<usize>, Style)>,
}

enum Token<'a> {
    Text(&'a str),
    Sgr(&'a str),
}

/// Split a line into text runs and SGR parameter strings, discarding other escapes.
fn tokenize(s: &str) -> Vec<Token<'_>> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut text_start = 0;
    while i < bytes.len() {
        if bytes[i] != 0x1b {
            i += 1;
            continue;
        }
        if text_start < i {
            out.push(Token::Text(&s[text_start..i]));
        }
        match bytes.get(i + 1) {
            Some(b'[') => {
                // CSI: params, then a final byte in 0x40..=0x7e.
                let mut j = i + 2;
                while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'm' {
                    out.push(Token::Sgr(&s[i + 2..j]));
                }
                i = (j + 1).min(bytes.len());
            }
            Some(b']') => {
                // OSC: terminated by BEL or ESC \.
                let mut j = i + 2;
                while j < bytes.len() {
                    if bytes[j] == 0x07 {
                        j += 1;
                        break;
                    }
                    if bytes[j] == 0x1b && bytes.get(j + 1) == Some(&b'\\') {
                        j += 2;
                        break;
                    }
                    j += 1;
                }
                i = j;
            }
            Some(_) => i += 2,
            None => i += 1,
        }
        text_start = i;
    }
    if text_start < bytes.len() {
        out.push(Token::Text(&s[text_start..]));
    }
    out
}

/// Replace tabs and drop remaining control characters.
fn clean_into(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '\t' => out.push_str("    "),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
}

pub fn strip(s: &str) -> String {
    parse(s).text
}

pub fn parse(s: &str) -> Styled {
    let mut style = Style::default();
    let mut out = Styled::default();
    for tok in tokenize(s) {
        match tok {
            Token::Text(t) => {
                let start = out.text.len();
                clean_into(&mut out.text, t);
                let end = out.text.len();
                if end > start && style != Style::default() {
                    out.runs.push((start..end, style));
                }
            }
            Token::Sgr(params) => style = apply_sgr(style, params),
        }
    }
    out
}

fn apply_sgr(mut style: Style, params: &str) -> Style {
    let nums: Vec<u16> = if params.is_empty() {
        vec![0]
    } else {
        params.split([';', ':']).map(|p| p.parse().unwrap_or(0)).collect()
    };
    let mut it = nums.into_iter();
    while let Some(n) = it.next() {
        match n {
            0 => style = Style::default(),
            1 => style.bold = true,
            2 => style.dim = true,
            3 => style.italic = true,
            4 => style.underline = true,
            22 => {
                style.bold = false;
                style.dim = false;
            }
            23 => style.italic = false,
            24 => style.underline = false,
            30..=37 => style.fg = Some(Color::Basic((n - 30) as u8)),
            39 => style.fg = None,
            90..=97 => style.fg = Some(Color::Bright((n - 90) as u8)),
            38 => {
                let color = match it.next() {
                    Some(5) => it.next().map(|i| Color::Indexed(i as u8)),
                    Some(2) => match (it.next(), it.next(), it.next()) {
                        (Some(r), Some(g), Some(b)) => Some(Color::Rgb(r as u8, g as u8, b as u8)),
                        _ => None,
                    },
                    _ => None,
                };
                if color.is_some() {
                    style.fg = color;
                }
            }
            // Background colors and anything else are ignored: logs keep the
            // console's own background.
            48 => {
                match it.next() {
                    Some(5) => {
                        it.next();
                    }
                    Some(2) => {
                        it.next();
                        it.next();
                        it.next();
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_non_sgr() {
        assert_eq!(strip("\x1b[2J\x1b[Hhello\x1b]8;;http://x\x07link\x1b]8;;\x07"), "hellolink");
        assert_eq!(strip("a\tb"), "a    b");
    }

    #[test]
    fn styled_runs() {
        let s = parse("\x1b[1;32mok\x1b[0m done \x1b[38;2;1;2;3mrgb");
        assert_eq!(s.text, "ok done rgb");
        assert_eq!(s.runs.len(), 2);
        assert_eq!(s.runs[0].0, 0..2);
        assert_eq!(s.runs[0].1.fg, Some(Color::Basic(2)));
        assert!(s.runs[0].1.bold);
        assert_eq!(s.runs[1].0, 8..11);
        assert_eq!(s.runs[1].1.fg, Some(Color::Rgb(1, 2, 3)));
    }

    #[test]
    fn background_params_are_skipped() {
        let s = parse("\x1b[48;5;12;31mx");
        assert_eq!(s.runs[0].1.fg, Some(Color::Basic(1)));
    }

    #[test]
    fn truncated_escape() {
        assert_eq!(strip("abc\x1b["), "abc");
        assert_eq!(strip("abc\x1b"), "abc");
    }
}
