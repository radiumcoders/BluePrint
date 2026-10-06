//! The details sheet: the selected project's state, address and wiring.

import { TextAttributes } from "@opentui/core"
import { displayPath } from "../core/config"
import { fmtDuration } from "../core/logindex"
import { devScriptCommand } from "../core/stack"
import type { Board } from "./board"
import { statusColor, statusGlyph, statusWord, theme, truncStart } from "./theme"

/** Rows the sheet takes, borders included. */
export const DETAILS_HEIGHT = 6

function Spec({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <box height={1} flexDirection="row">
      <text wrapMode="none">
        <span fg={theme.faint}>{label.padEnd(9)}</span>
        {children}
      </text>
    </box>
  )
}

export function Details({ board, width }: { board: Board; width: number }) {
  const e = board.entry
  if (!e) {
    return (
      <box
        height={DETAILS_HEIGHT}
        border
        borderStyle="rounded"
        borderColor={theme.border}
        title=" Details "
        paddingLeft={2}
        paddingTop={1}
      >
        <text fg={theme.muted}>Pick a project, or add one with n.</text>
      </box>
    )
  }
  const status = e.status()
  const run = e.run
  // While it runs, show what it runs with; edits wait for a restart.
  const shown = run?.project ?? e.project
  const port =
    run && shown.port === undefined
      ? `${run.port} (auto)`
      : shown.port !== undefined
        ? `${shown.port} (fixed)`
        : "auto, picked on start"
  // An empty command shows what it resolves to.
  const command = run ? run.command : shown.command.trim() || devScriptCommand(shown.path) || "package.json dev script"
  const url = e.url()
  const live = status === "running"
  let note = ""
  if (e.lastExit !== undefined && e.lastExit !== 0 && !run) note = `exit ${e.lastExit}`
  if (run) note = `up ${fmtDuration(performance.now() - run.started)}`

  return (
    <box
      height={DETAILS_HEIGHT}
      border
      borderStyle="rounded"
      borderColor={theme.borderStrong}
      title={` ${e.project.name} `}
      titleAlignment="left"
      flexDirection="column"
      paddingLeft={2}
      paddingRight={2}
    >
      <box height={1} flexDirection="row" justifyContent="space-between">
        <text wrapMode="none">
          <span fg={statusColor(status)}>{`${statusGlyph(status)} ${statusWord(status)}`}</span>
          {note && <span fg={theme.muted}>{`  ·  ${note}`}</span>}
          {e.editedWhileRunning() && <span fg={theme.amber}>{"  ·  edited, restart to apply"}</span>}
        </text>
        {url && (
          <text
            onMouseDown={() => live && board.openUrl(url)}
            fg={live ? theme.accent : theme.faint}
            attributes={live ? TextAttributes.UNDERLINE : TextAttributes.NONE}
          >
            {url}
          </text>
        )}
      </box>
      <Spec label="folder">
        <span fg={theme.text}>{truncStart(displayPath(shown.path), width - 2 - 4 - 9)}</span>
      </Spec>
      <Spec label="command">
        <span fg={shown.command || run ? theme.text : theme.muted}>{command}</span>
      </Spec>
      <Spec label="port">
        <span fg={theme.text}>{port}</span>
        {run && <span fg={theme.faint}>{"     pid "}</span>}
        {run && <span fg={theme.text}>{String(run.pid)}</span>}
      </Spec>
    </box>
  )
}
