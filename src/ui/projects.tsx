//! The board: every project on one line, port first, like a departures board.

import { fmtDuration } from "../core/logindex"
import type { Entry } from "../core/manager"
import { devScriptCommand } from "../core/stack"
import type { Board } from "./board"
import { BOLD, DIM, statusColor, statusGlyph, statusWord, theme, truncEnd } from "./theme"

/** The selection bar and a space before the first column. */
export const GUTTER = 2

interface Columns {
  port: number
  name: number
  status: number
  up: number
  /** 0 when there's no room for it. */
  command: number
}

/**
 * Column widths for `width`. When it's narrow the command goes first, then
 * long names get cut, then the uptime goes.
 */
function columns(width: number, names: string[]): Columns {
  const avail = width - GUTTER - 1
  const port = 8
  const status = 18
  let up = 10
  const wanted = Math.max(10, Math.min(30, Math.max(7, ...names.map((n) => n.length)) + 3))
  let name = wanted
  let command = avail - port - name - status - up
  if (command < 12) {
    command = 0
    name = Math.min(wanted, avail - port - status - up)
    if (name < 12) {
      up = 0
      name = Math.min(wanted, avail - port - status)
    }
  }
  return { port, name: Math.max(6, name), status, up, command }
}

/** Rows the board's projects get, so the logs below keep most of the screen. */
export function boardRows(count: number, height: number): number {
  // The top line, gaps, column heads, the details rule and the key line.
  const avail = height - 6
  return Math.min(count, Math.max(1, avail - Math.max(3, Math.ceil(avail * 0.45))))
}

function portCell(e: Entry): { text: string; auto: boolean } {
  const port = e.port()
  if (port === undefined) return { text: "auto", auto: true }
  return { text: String(port), auto: e.project.port === undefined }
}

function upCell(e: Entry): string {
  if (e.run) return fmtDuration(performance.now() - e.run.started)
  if (e.lastExit !== undefined && e.lastExit !== 0) return `exit ${e.lastExit}`
  return ""
}

/** What it runs with, or, for an empty command, what that resolves to. */
function commandCell(e: Entry): { text: string; derived: boolean } {
  if (e.run) return { text: e.run.command, derived: false }
  const own = e.project.command.trim()
  if (own) return { text: own, derived: false }
  return { text: devScriptCommand(e.project.path) ?? "package.json dev script", derived: true }
}

export function Projects({ board, width, rows }: { board: Board; width: number; rows: number }) {
  const all = board.m.entries
  // More projects than rows: keep the selected one in view.
  const at = Math.max(0, all.findIndex((e) => e.id === board.selected))
  const first = Math.max(0, Math.min(at - Math.floor(rows / 2), all.length - rows))
  const entries = all.slice(first, first + rows)
  const c = columns(
    width,
    all.map((e) => e.project.name),
  )
  const scrolled = all.length > rows ? `${at + 1} of ${all.length}` : ""

  return (
    <box flexDirection="column" flexShrink={0}>
      <box height={1} flexDirection="row" justifyContent="space-between" paddingRight={1}>
        <text wrapMode="none" fg={theme.faint}>
          {" ".repeat(GUTTER) +
            "PORT".padEnd(c.port) +
            "PROJECT".padEnd(c.name) +
            "STATUS".padEnd(c.status) +
            (c.up ? "UPTIME".padEnd(c.up) : "") +
            (c.command ? "COMMAND" : "")}
        </text>
        {scrolled && <text fg={theme.faint}>{scrolled}</text>}
      </box>
      {entries.map((e) => {
        const selected = e.id === board.selected
        const status = e.status()
        const active = e.isActive()
        const port = portCell(e)
        const command = commandCell(e)
        return (
          <box
            key={e.id}
            height={1}
            flexDirection="row"
            backgroundColor={selected ? theme.selection : undefined}
            onMouseDown={() => board.select(e.id)}
          >
            <text wrapMode="none" fg={theme.text}>
              <span fg={theme.accent}>{(selected ? "▌" : " ").padEnd(GUTTER)}</span>
              <span fg={port.auto && !active ? theme.faint : theme.text} attributes={port.auto ? DIM : BOLD}>
                {port.text.padEnd(c.port)}
              </span>
              <span fg={theme.text} attributes={selected ? BOLD : active ? 0 : DIM}>
                {truncEnd(e.project.name, c.name - 2).padEnd(c.name)}
              </span>
              <span fg={statusColor(status)} attributes={active || status === "crashed" ? BOLD : 0}>
                {`${statusGlyph(status)} ${statusWord(status).toUpperCase()}`.padEnd(c.status)}
              </span>
              {c.up > 0 && (
                <span fg={status === "crashed" ? theme.red : theme.text} attributes={DIM}>
                  {upCell(e).padEnd(c.up)}
                </span>
              )}
              {c.command > 0 && (
                <span fg={command.derived ? theme.faint : theme.text} attributes={command.derived || !active ? DIM : 0}>
                  {truncEnd(command.text, c.command)}
                </span>
              )}
            </text>
          </box>
        )
      })}
    </box>
  )
}
