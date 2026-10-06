//! The rule under the board: the selected project's name, address, folder and
//! process drawn into one line that heads its logs. Or, with no projects, a
//! few words on getting started.

import type { ReactNode } from "react"
import { displayPath } from "../core/config"
import { MAX_LOG_LINES } from "../core/manager"
import type { Board } from "./board"
import { BOLD, DIM, theme, truncStart, UNDERLINE } from "./theme"

const JOIN = " ── "

interface Piece {
  text: string
  node: (text: string) => ReactNode
}

export function Strip({ board, width }: { board: Board; width: number }) {
  const e = board.entry!
  const run = e.run
  const filtering = board.mode === "filter"
  const rule = filtering ? theme.accent : theme.border
  const url = e.url()
  const live = e.status() === "running"

  const index = board.index
  const shown = index.lines.length
  let count = ""
  if (e.logs.length > 0) {
    count = index.filtering() ? `${shown} of ${e.logs.length} lines` : `${e.logs.length} lines`
    if (e.logStart > 0) count += ` · keeps the last ${MAX_LOG_LINES}`
  }
  const behind = !board.follow && shown > 0

  const pieces: Piece[] = [
    {
      text: e.project.name,
      node: (t) => (
        <text fg={theme.text} attributes={BOLD}>
          {t}
        </text>
      ),
    },
  ]
  if (url) {
    pieces.push({
      text: url,
      node: (t) => (
        <text
          fg={live ? theme.accent : theme.faint}
          attributes={live ? UNDERLINE : 0}
          onMouseDown={() => live && board.openUrl(url)}
        >
          {t}
        </text>
      ),
    })
  }
  if (run) pieces.push({ text: `pid ${run.pid}`, node: (t) => <text fg={theme.faint}>{t}</text> })
  if (e.editedWhileRunning()) {
    pieces.push({ text: "edited, restart to apply", node: (t) => <text fg={theme.amber}>{t}</text> })
  }

  // Right of the rule: whether the console is behind, and the line count.
  const right: Piece[] = []
  if (behind) right.push({ text: "end ↓ latest", node: (t) => <text fg={theme.amber}>{t}</text> })
  if (count) right.push({ text: count, node: (t) => <text fg={theme.faint}>{t}</text> })

  const len = (ps: Piece[]) => ps.reduce((n, p) => n + p.text.length, 0) + Math.max(0, ps.length - 1) * JOIN.length
  // "── " before, " " and the fill, then " ──" after anything on the right.
  const fixed = 3 + len(pieces) + 1 + (right.length ? 1 + len(right) + 3 : 0)
  // The folder goes after the name and address, cut to what's left.
  const room = width - fixed - JOIN.length - 4
  if (room >= 8) {
    pieces.splice(url ? 2 : 1, 0, {
      text: truncStart(displayPath((run?.project ?? e.project).path), room),
      node: (t) => (
        <text fg={theme.text} attributes={DIM}>
          {t}
        </text>
      ),
    })
  }
  const fill = Math.max(1, width - 3 - len(pieces) - 1 - (right.length ? 1 + len(right) + 3 : 0))

  const line = (t: string) => <text fg={rule}>{t}</text>
  const joined = (ps: Piece[]) =>
    ps.flatMap((p, i) => (i === 0 ? [p.node(p.text)] : [line(JOIN), p.node(p.text)]))
  return (
    <box height={1} flexDirection="row" overflow="hidden">
      {[line("── "), ...joined(pieces), line(" " + "─".repeat(fill)), ...(right.length ? [line(" "), ...joined(right), line(" ──")] : [])].map(
        (node, i) => (
          <box key={i} flexShrink={0}>
            {node}
          </box>
        ),
      )}
    </box>
  )
}

const STEPS: [string, string][] = [
  ["n", "add a project: pick its folder, the command is detected"],
  ["enter", "start it; it gets its own port in $PORT"],
  ["o", "open it in the browser"],
  ["?", "every key"],
]

export function Welcome({ board }: { board: Board }) {
  return (
    <box flexDirection="column" paddingLeft={2} gap={1}>
      <box flexDirection="column">
        <text fg={theme.text} attributes={BOLD}>
          No projects yet.
        </text>
        <text fg={theme.text}>
          <span fg={theme.text} attributes={DIM}>
            Press{" "}
          </span>
          <span fg={theme.accent} attributes={BOLD}>
            n
          </span>
          <span fg={theme.text} attributes={DIM}>
            {" "}to add one.
          </span>
        </text>
      </box>
      <box flexDirection="column">
        {STEPS.map(([key, what]) => (
          <text key={key} wrapMode="none" fg={theme.text}>
            <span fg={theme.accent} attributes={BOLD}>
              {key.padEnd(8)}
            </span>
            <span fg={theme.text} attributes={DIM}>
              {what}
            </span>
          </text>
        ))}
      </box>
      <text wrapMode="none" fg={theme.faint}>
        {`Folders come from ${displayPath(board.m.config.projectsRoot)}`}
      </text>
    </box>
  )
}
