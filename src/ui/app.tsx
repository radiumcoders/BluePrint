//! The screen, top to bottom: the name and how many run; the board, one line
//! per project; a rule with the selected project's details; its logs; and a
//! key line. No boxes: rules and alignment do the separating. Forms, help and
//! prompts open in place of the logs.

import { useKeyboard, useSelectionHandler, useTerminalDimensions } from "@opentui/react"
import { useEffect, useReducer } from "react"
import type { Board } from "./board"
import { Strip, Welcome } from "./details"
import { HelpDialog, ProjectForm, QuittingDialog, SaveDialog, sheetRows } from "./dialogs"
import { Logs } from "./logs"
import { boardRows, GUTTER, Projects } from "./projects"
import { BOLD, DIM, theme } from "./theme"
import pkg from "../../package.json"

export function App({ board, onCopy }: { board: Board; onCopy: (text: string) => boolean }) {
  const [, redraw] = useReducer((n: number) => n + 1, 0)
  useEffect(() => board.subscribe(redraw), [board])
  useKeyboard((k) => board.key(k))
  // Text selected with the mouse goes to the clipboard, as terminals do.
  useSelectionHandler((selection) => {
    if (selection.isDragging) return
    const text = selection.getSelectedText()
    if (text.trim() && onCopy(text)) {
      board.m.info("Copied the selection")
      board.changed()
    }
  })
  const { width, height } = useTerminalDimensions()

  const count = board.m.entries.length
  // An open section takes the place of the logs; the board gives up rows if it needs them.
  const sheet = sheetRows(board, width, height)
  const rows = sheet ? Math.max(0, Math.min(count, height - 3 - sheet - 2)) : boardRows(count, height)
  const filterRow = board.mode === "filter" || board.filter !== "" ? 1 : 0
  // The top line and a gap, the column heads, a gap, the rule and the key line.
  board.viewRows = Math.max(1, height - 6 - rows - filterRow)
  board.syncLogs()

  return (
    <box width={width} height={height} flexDirection="column" backgroundColor={theme.bg}>
      <TopLine board={board} />
      <box height={1} />
      {count === 0 && !sheet && <Welcome board={board} />}
      {rows > 0 && <Projects board={board} width={width} rows={rows} />}
      {rows > 0 && <box height={1} />}
      {sheet ? (
        <Sheet board={board} width={width} height={height} />
      ) : (
        board.entry && (
          <>
            <Strip board={board} width={width} />
            <Logs board={board} />
          </>
        )
      )}
      <box flexGrow={1} />
      <KeyLine board={board} width={width} />
    </box>
  )
}

function Sheet({ board, width, height }: { board: Board; width: number; height: number }) {
  switch (board.mode) {
    case "form":
      return board.form ? <ProjectForm board={board} form={board.form} width={width} height={height} /> : null
    case "save":
      return <SaveDialog board={board} width={width} height={height} />
    case "help":
      return <HelpDialog width={width} height={height} />
    case "quitting":
      return <QuittingDialog board={board} width={width} height={height} />
    default:
      return null
  }
}

/** The name on the left; how many run on the right. */
function TopLine({ board }: { board: Board }) {
  const total = board.m.entries.length
  const running = board.m.runningCount()
  return (
    <box height={1} flexDirection="row" justifyContent="space-between" paddingLeft={GUTTER} paddingRight={1}>
      <text wrapMode="none" fg={theme.text}>
        <span fg={theme.accent} attributes={BOLD}>
          ◆ blueprint
        </span>
        <span fg={theme.faint}>{`  v${pkg.version}`}</span>
      </text>
      {total > 0 && (
        <text wrapMode="none" fg={theme.text}>
          {running > 0 && <span fg={theme.green} attributes={BOLD}>{`● ${running} running`}</span>}
          {running > 0 && <span fg={theme.faint}>{"  ·  "}</span>}
          <span fg={theme.text} attributes={DIM}>{`${total} project${total === 1 ? "" : "s"}`}</span>
        </text>
      )}
    </box>
  )
}

type Hint = [key: string, action: string]

function hints(board: Board): Hint[] {
  switch (board.mode) {
    case "filter":
      return [
        ["type", "filter"],
        ["enter", "done"],
        ["esc", "clear"],
      ]
    case "form":
      return board.form?.focus === "folder" && board.form.picking
        ? [
            ["↑↓", "choose"],
            ["enter", "pick"],
            ["tab", "next field"],
            ["esc", board.form.folder ? "keep folder" : "cancel"],
          ]
        : [
            ["tab", "next field"],
            ["enter", "save"],
            ["esc", "cancel"],
          ]
    case "save":
      return [
        ["enter", "save"],
        ["esc", "cancel"],
      ]
    case "help":
    case "quitting":
      return []
    case "normal": {
      const e = board.entry
      if (!e) {
        return [
          ["n", "new project"],
          ["?", "help"],
          ["q", "quit"],
        ]
      }
      return [
        ["enter", e.isActive() ? "stop" : "start"],
        ["r", "restart"],
        ["n", "new"],
        ["e", "edit"],
        ["o", "open"],
        ["/", "filter"],
        ["?", "help"],
        ["q", "quit"],
      ]
    }
  }
}

/** Keys for what can be done now, or a message when there is one. */
function KeyLine({ board, width }: { board: Board; width: number }) {
  const msg = board.m.message
  const confirming = board.mode === "normal" ? board.m.get(board.confirmRemove) : undefined
  let left: React.ReactNode
  if (confirming) {
    left = (
      <text wrapMode="none" fg={theme.text}>
        <span fg={theme.red} attributes={BOLD}>{`Remove ${confirming.project.name}?  `}</span>
        <span fg={theme.text} attributes={BOLD}>d</span>
        <span fg={theme.text} attributes={DIM}> again to remove   </span>
        <span fg={theme.text} attributes={BOLD}>esc</span>
        <span fg={theme.text} attributes={DIM}> to keep it</span>
      </text>
    )
  } else if (msg && board.mode !== "quitting") {
    const error = msg.kind === "error"
    left = (
      <text wrapMode="none" fg={theme.text}>
        <span fg={error ? theme.red : theme.accent}>{error ? "✕ " : "● "}</span>
        <span fg={error ? theme.red : theme.text}>{msg.text}</span>
      </text>
    )
  } else {
    // As many hints as fit, in order of importance.
    const shown: Hint[] = []
    let used = GUTTER
    for (const h of hints(board)) {
      const w = h[0].length + h[1].length + 4
      if (used + w > width) break
      shown.push(h)
      used += w
    }
    left = (
      <text wrapMode="none" fg={theme.text}>
        {shown.map(([key, action], i) => (
          <span key={i}>
            <span fg={theme.accent} attributes={BOLD}>
              {key}
            </span>
            <span fg={theme.text} attributes={DIM}>{` ${action}   `}</span>
          </span>
        ))}
      </text>
    )
  }
  return (
    <box height={1} flexDirection="row" paddingLeft={GUTTER} paddingRight={1} overflow="hidden">
      {left}
    </box>
  )
}
