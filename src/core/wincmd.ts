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

type Part =
  /** Unquoted text. */
  | { plain: string }
  /** Text from inside quotes, without them. */
  | { quoted: string }
  /** A variable reference. */
  | { variable: string }

type Token =
  /** `spaced` records whether whitespace came before it, so `2>&1` stays in one piece. */
  | { parts: Part[]; spaced: boolean }
  | { op: string; spaced: boolean }

/** Operators, longest first so `&&` isn't read as two `&`. */
const OPS = ["&&", "||", ">>", ">&", "<<", "|", "&", ";", ">", "<", "("]

const isName = (s: string) => /^[A-Za-z_][A-Za-z0-9_]*$/.test(s)

class Unsupported extends Error {}

const unsupported = (what: string) => new Unsupported(`cmd.exe has no equivalent of ${what}`)

function pushText(parts: Part[], text: string, quoted: boolean) {
  const last = parts[parts.length - 1]
  if (!quoted && last && "plain" in last) last.plain += text
  else if (quoted && last && "quoted" in last) last.quoted += text
  else parts.push(quoted ? { quoted: text } : { plain: text })
}

/** Reads `$...` at the start of `s`: a variable and what follows it, or `undefined` for a lone `$`. */
function variable(s: string): [string, string] | undefined {
  const after = s.slice(1)
  if (after.startsWith("(")) throw unsupported("command substitution, $(...)")
  if (after.startsWith("{")) {
    const end = after.indexOf("}")
    if (end < 0) throw new Unsupported("the command has an unclosed ${")
    const name = after.slice(1, end)
    if (!isName(name)) throw unsupported(`\${${name}}`)
    return [name, after.slice(end + 1)]
  }
  const name = /^[A-Za-z0-9_]*/.exec(after)![0]
  return isName(name) ? [name, after.slice(name.length)] : undefined
}

function tokenize(command: string): Token[] {
  const tokens: Token[] = []
  let parts: Part[] = []
  let spaced = false
  let wordSpaced = false
  let rest = command
  const flush = () => {
    if (parts.length) tokens.push({ parts, spaced: wordSpaced })
    parts = []
  }

  while (rest.length) {
    const c = rest[0]!
    if (/\s/.test(c)) {
      flush()
      spaced = true
      rest = rest.slice(1)
      continue
    }
    if (!parts.length) wordSpaced = spaced
    if (c === "`") throw unsupported("command substitution, `...`")
    const op = OPS.find((o) => rest.startsWith(o))
    if (op) {
      if (op === "<<") throw unsupported("here-documents, <<")
      flush()
      tokens.push({ op, spaced })
      spaced = false
      rest = rest.slice(op.length)
      continue
    }
    if (c === ")") {
      flush()
      tokens.push({ op: ")", spaced })
      spaced = false
      rest = rest.slice(1)
      continue
    }
    spaced = false
    if (c === "'") {
      const end = rest.indexOf("'", 1)
      if (end < 0) throw new Unsupported("the command has an unclosed '")
      pushText(parts, rest.slice(1, end), true)
      rest = rest.slice(end + 1)
    } else if (c === '"') {
      // Variables expand inside double quotes; nothing else does.
      let inner = rest.slice(1)
      // Even an empty "" is an argument.
      pushText(parts, "", true)
      for (;;) {
        const stop = inner.search(/["$]/)
        if (stop < 0) throw new Unsupported('the command has an unclosed "')
        pushText(parts, inner.slice(0, stop), true)
        inner = inner.slice(stop)
        if (inner.startsWith('"')) {
          rest = inner.slice(1)
          break
        }
        const v = variable(inner)
        if (v) {
          parts.push({ variable: v[0] })
          inner = v[1]
        } else {
          pushText(parts, "$", true)
          inner = inner.slice(1)
        }
      }
    } else if (c === "$") {
      const v = variable(rest)
      if (v) {
        parts.push({ variable: v[0] })
        rest = v[1]
      } else {
        pushText(parts, "$", false)
        rest = rest.slice(1)
      }
    } else {
      pushText(parts, c, false)
      rest = rest.slice(1)
    }
  }
  flush()
  return tokens
}

/**
 * A word as `cmd.exe` should see it. Quoted parts keep the word quoted, so
 * spaces and `cmd.exe` operators inside stay part of it.
 */
function render(parts: Part[]): string {
  const quoted = parts.some((p) => "quoted" in p)
  let out = ""
  for (const p of parts) {
    if ("variable" in p) out += `%${p.variable}%`
    else {
      const s = "plain" in p ? p.plain : p.quoted
      out += quoted ? s.replaceAll('"', '\\"') : s
    }
  }
  return quoted ? `"${out}"` : out
}

/** `NAME=value` at the start of a command, as the name and the value rendered for `set "..."`. */
function assignment(parts: Part[]): [string, string] | undefined {
  const first = parts[0]
  if (!first || !("plain" in first)) return undefined
  const eq = first.plain.indexOf("=")
  if (eq < 0) return undefined
  const name = first.plain.slice(0, eq)
  if (!isName(name)) return undefined
  let value = first.plain.slice(eq + 1)
  for (const p of parts.slice(1)) value += "variable" in p ? `%${p.variable}%` : "plain" in p ? p.plain : p.quoted
  return [name, value]
}

export type Translation = { ok: true; command: string } | { ok: false; error: string }

/** Translate `command` for `cmd.exe`, or say what it uses that can't be. */
export function translate(command: string): Translation {
  let tokens: Token[]
  try {
    tokens = tokenize(command.trim())
  } catch (e) {
    if (e instanceof Unsupported) return { ok: false, error: e.message }
    throw e
  }
  let out = ""
  let commandStart = true
  let redirect = false
  for (const token of tokens) {
    if ("op" in token) {
      if (token.spaced) out += " "
      out += token.op === ";" ? "&" : token.op
      commandStart = ["&&", "||", "|", "&", ";", "("].includes(token.op)
      redirect = [">", ">>", "<"].includes(token.op)
      continue
    }
    if (token.spaced && out) out += " "
    const set = commandStart ? assignment(token.parts) : undefined
    if (set) {
      const [name, value] = set
      if (value.includes('"')) return { ok: false, error: `can't set ${name} to a value containing " in cmd.exe` }
      out += `set "${name}=${value}" &&`
      continue
    }
    const word = render(token.parts)
    out += redirect && word === "/dev/null" ? "NUL" : word
    commandStart = false
    redirect = false
  }
  return { ok: true, command: out }
}
