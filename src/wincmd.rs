//! Running POSIX-style commands with `cmd.exe`, the shell on Windows.
//!
//! Commands are written once and run on every platform, so they're written
//! the way `sh` reads them. This translates the parts of that syntax that
//! `cmd.exe` lacks or reads differently:
//!
//! - `$NAME` and `${NAME}` become `%NAME%`;
//! - `NAME=value` before a command becomes `set "NAME=value" &&`;
//! - single quotes become double quotes (`cmd.exe` and the programs it runs
//!   only know those);
//! - `;` becomes `&`, and redirecting to `/dev/null` redirects to `NUL`.
//!
//! What both shells read alike passes through as written: `&&`, `||`, `|`,
//! redirections like `2>&1`, double quotes, and backslashes, which are path
//! separators on Windows rather than escapes. Syntax with no `cmd.exe`
//! equivalent (`$(...)`, backticks, `${NAME:-default}`, here-documents) is
//! refused with an error rather than run wrong.
//!
//! Known gaps: `%` is passed through, so `%NAME%` in a command still expands
//! as `cmd.exe` would, and a `NAME=value` set before one command in a chain
//! stays set for the rest of it.

#[derive(Debug)]
enum Part {
    /// Unquoted text.
    Plain(String),
    /// Text from inside quotes, without them.
    Quoted(String),
    /// A variable reference.
    Var(String),
}

#[derive(Debug)]
enum Token {
    /// `spaced` records whether whitespace came before it, so `2>&1` stays
    /// in one piece.
    Word { parts: Vec<Part>, spaced: bool },
    Op { op: &'static str, spaced: bool },
}

/// Operators, longest first so `&&` isn't read as two `&`.
const OPS: [&str; 11] = ["&&", "||", ">>", ">&", "<<", "|", "&", ";", ">", "<", "("];

fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn unsupported(what: &str) -> String {
    format!("cmd.exe has no equivalent of {what}")
}

fn tokenize(command: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut parts: Vec<Part> = Vec::new();
    let mut spaced = false;
    let mut word_spaced = false;
    let mut rest = command;

    // Appends literal text to the word being built.
    fn push_text(parts: &mut Vec<Part>, text: &str, quoted: bool) {
        match (parts.last_mut(), quoted) {
            (Some(Part::Plain(s)), false) | (Some(Part::Quoted(s)), true) => s.push_str(text),
            (_, false) => parts.push(Part::Plain(text.into())),
            (_, true) => parts.push(Part::Quoted(text.into())),
        }
    }

    // Reads `$...` at the start of `s`: a variable and what follows it, or
    // `None` for a `$` that's just a character.
    fn variable(s: &str) -> Result<Option<(String, &str)>, String> {
        let after = &s[1..];
        if after.starts_with('(') {
            return Err(unsupported("command substitution, $(...)"));
        }
        if let Some(inner) = after.strip_prefix('{') {
            let end = inner.find('}').ok_or("the command has an unclosed ${")?;
            let name = &inner[..end];
            if !is_name(name) {
                return Err(unsupported(&format!("${{{name}}}")));
            }
            return Ok(Some((name.into(), &inner[end + 1..])));
        }
        let len = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(after.len());
        let name = &after[..len];
        Ok(is_name(name).then(|| (name.into(), &after[len..])))
    }

    let flush = |parts: &mut Vec<Part>, tokens: &mut Vec<Token>, spaced: bool| {
        if !parts.is_empty() {
            tokens.push(Token::Word { parts: std::mem::take(parts), spaced });
        }
    };

    while let Some(c) = rest.chars().next() {
        if c.is_whitespace() {
            flush(&mut parts, &mut tokens, word_spaced);
            spaced = true;
            rest = &rest[c.len_utf8()..];
            continue;
        }
        if parts.is_empty() {
            word_spaced = spaced;
        }
        if c == '`' {
            return Err(unsupported("command substitution, `...`"));
        }
        if let Some(op) = OPS.iter().find(|op| rest.starts_with(**op)) {
            if *op == "<<" {
                return Err(unsupported("here-documents, <<"));
            }
            flush(&mut parts, &mut tokens, word_spaced);
            tokens.push(Token::Op { op, spaced });
            spaced = false;
            rest = &rest[op.len()..];
            continue;
        }
        if c == ')' {
            flush(&mut parts, &mut tokens, word_spaced);
            tokens.push(Token::Op { op: ")", spaced });
            spaced = false;
            rest = &rest[1..];
            continue;
        }
        spaced = false;
        match c {
            '\'' => {
                let end = rest[1..].find('\'').ok_or("the command has an unclosed '")?;
                push_text(&mut parts, &rest[1..=end], true);
                rest = &rest[end + 2..];
            }
            '"' => {
                // Variables expand inside double quotes; nothing else does.
                let mut inner = &rest[1..];
                // Even an empty "" is an argument.
                push_text(&mut parts, "", true);
                loop {
                    let stop = inner.find(['"', '$']).ok_or("the command has an unclosed \"")?;
                    push_text(&mut parts, &inner[..stop], true);
                    inner = &inner[stop..];
                    if let Some(after) = inner.strip_prefix('"') {
                        rest = after;
                        break;
                    }
                    match variable(inner)? {
                        Some((name, after)) => {
                            parts.push(Part::Var(name));
                            inner = after;
                        }
                        None => {
                            push_text(&mut parts, "$", true);
                            inner = &inner[1..];
                        }
                    }
                }
            }
            '$' => match variable(rest)? {
                Some((name, after)) => {
                    parts.push(Part::Var(name));
                    rest = after;
                }
                None => {
                    push_text(&mut parts, "$", false);
                    rest = &rest[1..];
                }
            },
            c => {
                push_text(&mut parts, &rest[..c.len_utf8()], false);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    flush(&mut parts, &mut tokens, word_spaced);
    Ok(tokens)
}

/// A word as `cmd.exe` should see it. Quoted parts keep the word quoted, so
/// spaces and `cmd.exe` operators inside stay part of it.
fn render(parts: &[Part]) -> String {
    let quoted = parts.iter().any(|p| matches!(p, Part::Quoted(_)));
    let mut out = String::new();
    for part in parts {
        match part {
            Part::Plain(s) | Part::Quoted(s) if quoted => out.push_str(&s.replace('"', "\\\"")),
            Part::Plain(s) | Part::Quoted(s) => out.push_str(s),
            Part::Var(name) => out.push_str(&format!("%{name}%")),
        }
    }
    if quoted { format!("\"{out}\"") } else { out }
}

/// Translate `command` for `cmd.exe`, or say what it uses that can't be.
pub fn translate(command: &str) -> Result<String, String> {
    let tokens = tokenize(command.trim())?;
    let mut out = String::new();
    let mut command_start = true;
    let mut redirect = false;
    for token in &tokens {
        match token {
            Token::Op { op, spaced } => {
                if *spaced {
                    out.push(' ');
                }
                out.push_str(if *op == ";" { "&" } else { op });
                command_start = matches!(*op, "&&" | "||" | "|" | "&" | ";" | "(");
                redirect = matches!(*op, ">" | ">>" | "<");
            }
            Token::Word { parts, spaced } => {
                if *spaced && !out.is_empty() {
                    out.push(' ');
                }
                if command_start && let Some((name, value)) = assignment_value(parts) {
                    if value.contains('"') {
                        return Err(format!("can't set {name} to a value containing \" in cmd.exe"));
                    }
                    out.push_str(&format!("set \"{name}={value}\" &&"));
                    continue;
                }
                let word = render(parts);
                out.push_str(if redirect && word == "/dev/null" { "NUL" } else { &word });
                command_start = false;
                redirect = false;
            }
        }
    }
    Ok(out)
}

/// `NAME=value` at the start of a command, as the name and the value
/// rendered for the inside of `set "..."`.
fn assignment_value(parts: &[Part]) -> Option<(&str, String)> {
    let Some(Part::Plain(first)) = parts.first() else { return None };
    let (name, head) = first.split_once('=')?;
    if !is_name(name) {
        return None;
    }
    let mut value = head.to_string();
    for part in &parts[1..] {
        match part {
            Part::Plain(s) | Part::Quoted(s) => value.push_str(s),
            Part::Var(v) => value.push_str(&format!("%{v}%")),
        }
    }
    Some((name, value))
}

#[cfg(test)]
mod tests {
    use super::translate;

    fn ok(command: &str) -> String {
        translate(command).unwrap_or_else(|e| panic!("{command:?}: {e}"))
    }

    #[test]
    fn variables() {
        assert_eq!(ok("trunk serve --port $PORT"), "trunk serve --port %PORT%");
        assert_eq!(ok("serve --port=${PORT}"), "serve --port=%PORT%");
        assert_eq!(ok("echo \"on $PORT\""), "echo \"on %PORT%\"");
        // Not variables: positional parameters and a bare $.
        assert_eq!(ok("echo $5 a=b $"), "echo $5 a=b $");
        // Single quotes keep $ literal.
        assert_eq!(ok("echo '$PORT'"), "echo \"$PORT\"");
    }

    #[test]
    fn assignments() {
        assert_eq!(
            ok("LEPTOS_SITE_ADDR=127.0.0.1:$PORT cargo leptos watch"),
            "set \"LEPTOS_SITE_ADDR=127.0.0.1:%PORT%\" && cargo leptos watch"
        );
        assert_eq!(ok("A=\"x y\" B='$z' run"), "set \"A=x y\" && set \"B=$z\" && run");
        // At the start of every command in a chain, and nowhere else.
        assert_eq!(ok("cd web && NODE_ENV=dev npm start a=b"), "cd web && set \"NODE_ENV=dev\" && npm start a=b");
        assert!(translate("A='say \"hi\"' run").is_err());
        assert_eq!(translate("echo $(date)").unwrap_err(), "cmd.exe has no equivalent of command substitution, $(...)");
    }

    #[test]
    fn quoting_and_operators() {
        assert_eq!(ok("pnpm dev"), "pnpm dev");
        assert_eq!(ok("echo 'a b' \"c\"d ''"), "echo \"a b\" \"cd\" \"\"");
        assert_eq!(ok("echo 'a&b' a|b"), "echo \"a&b\" a|b");
        assert_eq!(ok("serve 2>&1 >/dev/null; next || x"), "serve 2>&1 >NUL& next || x");
        assert_eq!(ok("(a && b) > log.txt"), "(a && b) > log.txt");
        // Windows paths keep their backslashes, quoted or not.
        assert_eq!(
            ok("X=1 \"C:\\Program Files\\a.exe\" C:\\x\\y"),
            "set \"X=1\" && \"C:\\Program Files\\a.exe\" C:\\x\\y"
        );
    }

    #[test]
    fn refuses_what_cmd_cannot_do() {
        for bad in ["echo $(date)", "echo `date`", "echo ${A:-x}", "echo ${A", "echo 'open", "echo \"open", "cat <<EOF"] {
            assert!(translate(bad).is_err(), "{bad}");
        }
    }
}
