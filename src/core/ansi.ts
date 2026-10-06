//! Minimal ANSI handling for log lines: SGR colors become style runs, every
//! other escape sequence (cursor moves, screen clears, OSC links) is dropped.

export type Color =
  /** 0-7: black, red, green, yellow, blue, magenta, cyan, white. */
  | { kind: "basic"; index: number }
  | { kind: "bright"; index: number }
  | { kind: "indexed"; index: number }
  | { kind: "rgb"; r: number; g: number; b: number }

export interface Style {
  fg?: Color
  bold: boolean
  dim: boolean
  italic: boolean
  underline: boolean
}

/** A piece of a line in one style. */
export interface Run {
  text: string
  style: Style
}

const plain = (): Style => ({ bold: false, dim: false, italic: false, underline: false })

const isPlain = (s: Style) => !s.fg && !s.bold && !s.dim && !s.italic && !s.underline

type Token = { text: string } | { sgr: string }

/** Split a line into text and SGR parameter strings, discarding other escapes. */
function tokenize(s: string): Token[] {
  const out: Token[] = []
  let i = 0
  let textStart = 0
  while (i < s.length) {
    if (s.charCodeAt(i) !== 0x1b) {
      i++
      continue
    }
    if (textStart < i) out.push({ text: s.slice(textStart, i) })
    const next = s[i + 1]
    if (next === "[") {
      // CSI: params, then a final byte in 0x40..=0x7e.
      let j = i + 2
      while (j < s.length && !(s.charCodeAt(j) >= 0x40 && s.charCodeAt(j) <= 0x7e)) j++
      if (j < s.length && s[j] === "m") out.push({ sgr: s.slice(i + 2, j) })
      i = Math.min(j + 1, s.length)
    } else if (next === "]") {
      // OSC: terminated by BEL or ESC \.
      let j = i + 2
      while (j < s.length) {
        if (s.charCodeAt(j) === 0x07) {
          j++
          break
        }
        if (s.charCodeAt(j) === 0x1b && s[j + 1] === "\\") {
          j += 2
          break
        }
        j++
      }
      i = j
    } else if (next !== undefined) {
      // A two-byte escape like `ESC 7`. Skip the whole character after ESC.
      i += 1 + (s.codePointAt(i + 1)! > 0xffff ? 2 : 1)
    } else {
      i++
    }
    textStart = i
  }
  if (textStart < s.length) out.push({ text: s.slice(textStart) })
  return out
}

/** Replace tabs and drop remaining control characters. */
function clean(text: string): string {
  // eslint-disable-next-line no-control-regex
  return text.replaceAll("\t", "    ").replace(/[\u0000-\u001f\u007f-\u009f]/g, "")
}

/** The line as styled runs, escapes removed. Adjacent runs never share a style object. */
export function parse(s: string): Run[] {
  let style = plain()
  const runs: Run[] = []
  for (const tok of tokenize(s)) {
    if ("sgr" in tok) {
      style = applySgr(style, tok.sgr)
      continue
    }
    const text = clean(tok.text)
    if (!text) continue
    const last = runs[runs.length - 1]
    if (last && last.style === style) last.text += text
    else runs.push({ text, style })
  }
  return runs
}

/** The text of a line without its escapes. */
export function strip(s: string): string {
  return parse(s)
    .map((r) => r.text)
    .join("")
}

/** Whether any run carries a style. */
export function isStyled(runs: Run[]): boolean {
  return runs.some((r) => !isPlain(r.style))
}

function applySgr(prev: Style, params: string): Style {
  const style = { ...prev }
  const nums = params === "" ? [0] : params.split(/[;:]/).map((p) => Number.parseInt(p, 10) || 0)
  for (let k = 0; k < nums.length; k++) {
    const n = nums[k]!
    if (n === 0) Object.assign(style, plain(), { fg: undefined })
    else if (n === 1) style.bold = true
    else if (n === 2) style.dim = true
    else if (n === 3) style.italic = true
    else if (n === 4) style.underline = true
    else if (n === 22) style.bold = style.dim = false
    else if (n === 23) style.italic = false
    else if (n === 24) style.underline = false
    else if (n >= 30 && n <= 37) style.fg = { kind: "basic", index: n - 30 }
    else if (n === 39) style.fg = undefined
    else if (n >= 90 && n <= 97) style.fg = { kind: "bright", index: n - 90 }
    else if (n === 38) {
      const mode = nums[++k]
      if (mode === 5 && k + 1 < nums.length) style.fg = { kind: "indexed", index: nums[++k]! & 0xff }
      else if (mode === 2 && k + 3 < nums.length) {
        style.fg = { kind: "rgb", r: nums[k + 1]! & 0xff, g: nums[k + 2]! & 0xff, b: nums[k + 3]! & 0xff }
        k += 3
      }
    } else if (n === 48) {
      // Background colors are skipped: logs keep the console's own background.
      const mode = nums[++k]
      k += mode === 5 ? 1 : mode === 2 ? 3 : 0
    }
  }
  return style
}
