//! The left column: every project, one line each.

import { TextAttributes } from "@opentui/core"
import type { Board } from "./board"
import { statusColor, statusGlyph, theme } from "./theme"

export function Projects({ board, width, rows }: { board: Board; width: number; rows: number }) {
  // More projects than rows: keep the selected one in view.
  const all = board.m.entries
  const at = Math.max(0, all.findIndex((e) => e.id === board.selected))
  const first = Math.max(0, Math.min(at - Math.floor(rows / 2), all.length - rows))
  const entries = all.slice(first, first + rows)
  // Inside the border and padding.
  const inner = width - 4
  return (
    <box
      width={width}
      border
      borderStyle="rounded"
      borderColor={theme.border}
      title=" Projects "
      bottomTitle={all.length > rows ? ` ${at + 1} of ${all.length} ` : undefined}
      bottomTitleAlignment="right"
      titleAlignment="left"
      flexDirection="column"
      paddingLeft={1}
      paddingRight={1}
    >
      {all.length === 0 ? (
        <box flexDirection="column" paddingTop={1}>
          <text fg={theme.muted}>No projects yet.</text>
          <text>
            <span fg={theme.muted}>Press </span>
            <span fg={theme.accent} attributes={TextAttributes.BOLD}>
              n
            </span>
            <span fg={theme.muted}> to add one.</span>
          </text>
        </box>
      ) : (
        entries.map((e) => {
          const selected = e.id === board.selected
          const status = e.status()
          const port = e.port()
          const right = port === undefined ? "auto" : String(port)
          const nameWidth = Math.max(1, inner - 4 - right.length)
          const name = e.project.name.length > nameWidth ? e.project.name.slice(0, nameWidth - 1) + "…" : e.project.name
          return (
            <box
              key={e.id}
              height={1}
              flexDirection="row"
              backgroundColor={selected ? theme.selection : undefined}
              onMouseDown={() => board.select(e.id)}
            >
              <text wrapMode="none">
                <span fg={theme.accent}>{selected ? "▌" : " "}</span>
                <span fg={statusColor(status)}>{statusGlyph(status)}</span>
                <span> </span>
                <span
                  fg={selected ? theme.text : status === "stopped" ? theme.muted : theme.text}
                  attributes={selected ? TextAttributes.BOLD : TextAttributes.NONE}
                >
                  {name.padEnd(nameWidth)}
                </span>
                <span fg={e.isActive() ? theme.muted : theme.faint}>{` ${right}`}</span>
              </text>
            </box>
          )
        })
      )}
    </box>
  )
}
