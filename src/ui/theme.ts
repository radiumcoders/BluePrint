//! The look: a calm dark sheet with one blueprint-blue accent. Color is kept
//! for meaning (status, the focused thing, the main action), so the eye
//! always has one place to land.

import type { Color } from "../core/ansi"
import type { Status } from "../core/manager"

export const theme = {
  bg: "#0d1017",
  /** Laid over the board behind a dialog. */
  backdrop: "#05070acc",
  /** Dialogs float above the board on this. */
  overlay: "#151a26",
  /** Text fields sink into this. */
  well: "#1c2333",
  wellFocus: "#232c41",
  /** The selected project row. */
  selection: "#18202f",
  border: "#262d3d",
  borderStrong: "#3a4358",
  accent: "#5b93ff",
  onAccent: "#0d1017",
  text: "#e4e8f1",
  muted: "#8d96a8",
  faint: "#5d6679",
  green: "#7ee2a8",
  amber: "#f2c46d",
  red: "#f38b8b",
} as const

export function statusColor(s: Status): string {
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

const hex = (r: number, g: number, b: number) =>
  "#" + [r, g, b].map((v) => Math.round(Math.max(0, Math.min(255, v))).toString(16).padStart(2, "0")).join("")

/** Log colors, tuned to read on the dark sheet. */
const BASIC = ["#7f8796", "#f38b8b", "#7ee2a8", "#f2c46d", "#7aa7ff", "#d4a5ff", "#7fd8e8", "#e4e8f1"]
const BRIGHT = ["#a3abba", "#ffadad", "#a6f0c6", "#ffdc95", "#a5c3ff", "#e5c6ff", "#a8ecf5", "#ffffff"]

/** Dark colors meant for light terminals would vanish here; lift them. */
function lift(r: number, g: number, b: number): string {
  const lum = 0.2126 * r + 0.7152 * g + 0.0722 * b
  if (lum >= 90) return hex(r, g, b)
  const t = ((90 - lum) / 255) * 1.8
  return hex(r + (255 - r) * t, g + (255 - g) * t, b + (255 - b) * t)
}

export function ansiColor(c: Color): string {
  switch (c.kind) {
    case "basic":
      return BASIC[c.index % 8]!
    case "bright":
      return BRIGHT[c.index % 8]!
    case "rgb":
      return lift(c.r, c.g, c.b)
    case "indexed": {
      const i = c.index
      if (i < 8) return BASIC[i]!
      if (i < 16) return BRIGHT[i - 8]!
      if (i >= 232) {
        const v = 8 + (i - 232) * 10
        return lift(v, v, v)
      }
      // 6x6x6 color cube.
      const n = i - 16
      const level = (x: number) => (x === 0 ? 0 : 55 + x * 40)
      return lift(level(Math.floor(n / 36)), level(Math.floor(n / 6) % 6), level(n % 6))
    }
  }
}

/** `text` cut to `width` columns from the start, so paths keep their last folders. */
export function truncStart(text: string, width: number): string {
  if (text.length <= width) return text
  return width <= 1 ? "…" : "…" + text.slice(text.length - width + 1)
}
