//! The board: projects on the left, the selected project's details and logs
//! on the right, a key line along the bottom, and dialogs over it all.

import { TextAttributes } from "@opentui/core"
import { useKeyboard, useSelectionHandler, useTerminalDimensions } from "@opentui/react"
import { useEffect, useReducer } from "react"
import type { Board } from "./board"
import { Details, DETAILS_HEIGHT } from "./details"
import { HelpDialog, ProjectForm, QuittingDialog, SaveDialog } from "./dialogs"
import { Logs } from "./logs"
import { Projects } from "./projects"
import { theme } from "./theme"
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

  const sidebar = Math.max(24, Math.min(36, Math.floor(width * 0.28)))
  const filterRow = board.mode === "filter" || board.filter !== "" ? 1 : 0
  // Header and key line, the details sheet, and the console's own borders.
  board.viewRows = Math.max(1, height - 2 - DETAILS_HEIGHT - 2 - filterRow)
  board.syncLogs()

  return (
    <box width={width} height={height} flexDirection="column" backgroundColor={theme.bg}>
      <Header board={board} />
      <box flexGrow={1} flexDirection="row" minHeight={0}>
        <Projects board={board} width={sidebar} rows={Math.max(1, height - 4)} />
        <box flexGrow={1} flexDirection="column" minWidth={0}>
          <Details board={board} width={width - sidebar} />
          <Logs board={board} />
        </box>
      </box>
      <KeyLine board={board} width={width} />
      {board.mode === "form" && board.form && <ProjectForm board={board} form={board.form} width={width} height={height} />}
      {board.mode === "save" && <SaveDialog board={board} width={width} height={height} />}
      {board.mode === "help" && <HelpDialog width={width} height={height} />}
      {board.mode === "quitting" && <QuittingDialog board={board} width={width} height={height} />}
    </box>
  )
}

function Header({ board }: { board: Board }) {
  const total = board.m.entries.length
  const running = board.m.runningCount()
  return (
    <box height={1} flexDirection="row" justifyContent="space-between" paddingLeft={1} paddingRight={1}>
      <text>
        <span fg={theme.accent} attributes={TextAttributes.BOLD}>
          ◆ blueprint
        </span>
        <span fg={theme.faint}>{`  v${pkg.version}`}</span>
      </text>
      <text>
        {running > 0 ? (
          <span fg={theme.green}>{`● ${running} running`}</span>
        ) : (
          <span fg={theme.faint}>nothing running</span>
        )}
        <span fg={theme.faint}>{`  ·  ${total} project${total === 1 ? "" : "s"}`}</span>
      </text>
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
  if (confirming) {
    return (
      <box height={1} paddingLeft={1}>
        <text>
          <span fg={theme.red}>{`Remove ${confirming.project.name}? `}</span>
          <span fg={theme.text}>d</span>
          <span fg={theme.muted}> again to remove · </span>
          <span fg={theme.text}>esc</span>
          <span fg={theme.muted}> to keep it</span>
        </text>
      </box>
    )
  }
  if (msg && board.mode !== "quitting") {
    return (
      <box height={1} paddingLeft={1}>
        <text wrapMode="none">
          <span fg={msg.kind === "error" ? theme.red : theme.accent}>{"● "}</span>
          <span fg={msg.kind === "error" ? theme.red : theme.text}>{msg.text}</span>
        </text>
      </box>
    )
  }
  // As many hints as fit, in order of importance.
  const shown: Hint[] = []
  let used = 1
  for (const h of hints(board)) {
    const w = h[0].length + h[1].length + 4
    if (used + w > width) break
    shown.push(h)
    used += w
  }
  return (
    <box height={1} paddingLeft={1} flexDirection="row">
      <text wrapMode="none">
        {shown.map(([key, action], i) => (
          <span key={i}>
            <span fg={theme.text} attributes={TextAttributes.BOLD}>
              {key}
            </span>
            <span fg={theme.muted}>{` ${action}   `}</span>
          </span>
        ))}
      </text>
    </box>
  )
}
