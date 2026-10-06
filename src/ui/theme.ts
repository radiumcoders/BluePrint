//! The look comes from the terminal. Text and background are the terminal's
//! own defaults and every color is a slot in its palette, so blueprint wears
//! whatever theme the terminal has, and follows it when it changes. Color is
//! kept for meaning (status, the focused thing, the main action), so the eye
//! always has one place to land.
//!
//! A few quiet fills (the selected row, text fields) have no palette
//! slot; they are mixed from the terminal's background and foreground once it
//! reports them, and left out where it doesn't.

import { RGBA, TextAttributes, type TerminalColors } from "@opentui/core"
import type { Color } from "../core/ansi"
import type { Status } from "../core/manager"

const slot = (i: number) => RGBA.fromIndex(i)

/** Muted text: the terminal's own dim rendering of the foreground. */
export const DIM = TextAttributes.DIM
export const BOLD = TextAttributes.BOLD
export const UNDERLINE = TextAttributes.UNDERLINE

export const theme = {
  text: RGBA.defaultForeground(),
  bg: RGBA.defaultBackground(),
  /** Bright black, which themes use for comments and other quiet text. */
  faint: slot(8),
  border: slot(8),
  accent: slot(4),
  green: slot(2),
  amber: slot(3),
  red: slot(1),
  /** Text drawn on an accent fill. */
  onAccent: slot(0),
  /** The selected project row. */
  selection: undefined as RGBA | undefined,
  /** Text fields sink into this. */
  well: RGBA.defaultBackground(),
  wellFocus: RGBA.defaultBackground(),
  /**
   * The text cursor. OpenTUI always paints the cursor a fixed color, so this
   * is the terminal's own cursor color once it reports it.
   */
  cursor: RGBA.fromInts(255, 255, 255),
}

function parseHex(hex: string | null): [number, number, number] | undefined {
  const m = hex && /^#?([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})/i.exec(hex)
  return m ? [parseInt(m[1]!, 16), parseInt(m[2]!, 16), parseInt(m[3]!, 16)] : undefined
}

/** `bg` moved `t` of the way toward `fg`. */
function mix(bg: [number, number, number], fg: [number, number, number], t: number): RGBA {
  return RGBA.fromInts(...(bg.map((b, i) => Math.round(b + (fg[i]! - b) * t)) as [number, number, number]))
}

/** Mix the fills from the colors the terminal reported, or drop them if it didn't. */
export function applyTerminalColors(colors: TerminalColors) {
  const bg = parseHex(colors.defaultBackground)
  const fg = parseHex(colors.defaultForeground)
  if (!bg || !fg) {
    theme.selection = undefined
    theme.well = theme.wellFocus = theme.bg
    theme.onAccent = slot(0)
    return
  }
  theme.cursor = RGBA.fromInts(...(parseHex(colors.cursorColor) ?? fg))
  theme.selection = mix(bg, fg, 0.12)
  theme.well = mix(bg, fg, 0.09)
  theme.wellFocus = mix(bg, fg, 0.16)
  theme.onAccent = RGBA.fromInts(...bg)
}

export function statusColor(s: Status): RGBA {
  switch (s) {
    case "running":
      return theme.green
    case "starting":
    case "stopping":
    case "unresponsive":
      return theme.amber
    case "crashed":
      return theme.red
    case "stopped":
      return theme.faint
  }
}

export function statusWord(s: Status): string {
  return s === "unresponsive" ? "not responding" : s
}

/** A status dot: filled while something runs, hollow when stopped, a cross after a crash. */
export function statusGlyph(s: Status): string {
  return s === "stopped" ? "○" : s === "crashed" ? "✕" : "●"
}

/** A log color as the terminal itself would show it. */
export function ansiColor(c: Color): RGBA {
  switch (c.kind) {
    case "basic":
      return slot(c.index % 8)
    case "bright":
      return slot(8 + (c.index % 8))
    case "indexed":
      return slot(c.index & 0xff)
    case "rgb":
      return RGBA.fromInts(c.r, c.g, c.b)
  }
}

/** `text` cut to `width` columns from the start, so paths keep their last folders. */
export function truncStart(text: string, width: number): string {
  if (text.length <= width) return text
  return width <= 1 ? "…" : "…" + text.slice(text.length - width + 1)
}

/** `text` cut to `width` columns from the end. */
export function truncEnd(text: string, width: number): string {
  if (text.length <= width) return text
  return width <= 1 ? "…" : text.slice(0, width - 1) + "…"
}
