//! What opens under the board in place of the logs: the add/edit form, the
//! save-logs prompt, the "open in" picker, help, and the notice shown while servers stop on the way
//! out. Each is a section headed by a rule, like the details, so the board
//! stays in view above it.

import type { ReactNode } from "react"
import { displayPath } from "../core/config"
import { tagsFor } from "../core/folders"
import { AUTO_PORTS } from "../core/process"
import { devScriptCommand, hasDevScript } from "../core/stack"
import type { Board, Form, FormField } from "./board"
import pkg from "../../package.json"
import { GUTTER } from "./projects"
import { BOLD, DIM, theme, truncStart } from "./theme"

const LABEL = 10

/** A section under a rule with its title; `rows` counts the gap under the rule. */
function Dialog(props: { title: string; width: number; rows: number; screen: [number, number]; children: ReactNode }) {
  const [w] = props.screen
  const fill = Math.max(1, w - 3 - props.title.length - 1)
  return (
    <box flexDirection="column" flexShrink={0}>
      <text wrapMode="none" fg={theme.accent}>
        <span fg={theme.accent}>{"── "}</span>
        <span fg={theme.accent} attributes={BOLD}>
          {props.title}
        </span>
        <span fg={theme.accent}>{" " + "─".repeat(fill)}</span>
      </text>
      <box
        height={props.rows}
        width={Math.min(props.width, w - GUTTER - 1) + GUTTER}
        flexDirection="column"
        paddingLeft={GUTTER}
        paddingTop={1}
      >
        {props.children}
      </box>
    </box>
  )
}

/** Rows the open section takes, its rule included, or 0 when none is open. */
export function sheetRows(board: Board, width: number, height: number): number {
  switch (board.mode) {
    case "form":
      return board.form ? 1 + formRows(board.form, height) : 0
    case "save":
      return 1 + SAVE_ROWS
    case "help":
      return 1 + helpRows(width)
    case "agent":
      return 1 + agentRows(board, height)
    case "quitting":
      return 1 + QUITTING_ROWS
    default:
      return 0
  }
}

function Label({ text, focused }: { text: string; focused: boolean }) {
  return (
    <text fg={focused ? theme.accent : theme.text} attributes={focused ? BOLD : DIM}>
      {text.padEnd(LABEL)}
    </text>
  )
}

/** A line under a field: its error if it has one, else a note. */
function Note({ error, children }: { error?: string; children?: ReactNode }) {
  return (
    <box height={1} paddingLeft={LABEL}>
      {error ? (
        <text wrapMode="none" fg={theme.red}>
          {error}
        </text>
      ) : (
        <text wrapMode="none" fg={theme.faint}>
          {children ?? ""}
        </text>
      )}
    </box>
  )
}

function Field(props: {
  board: Board
  form: Form
  field: "name" | "port" | "command"
  label: string
  placeholder: string
  width: number
}) {
  const { board, form, field } = props
  const focused = form.focus === field
  return (
    <box
      height={1}
      flexDirection="row"
      onMouseDown={() => {
        form.focus = field
        board.changed()
      }}
    >
      <Label text={props.label} focused={focused} />
      <input
        width={props.width}
        value={form[field]}
        focused={focused}
        placeholder={props.placeholder}
        textColor={theme.text}
        focusedTextColor={theme.text}
        placeholderColor={theme.faint}
        backgroundColor={theme.well}
        focusedBackgroundColor={theme.wellFocus}
        cursorColor={theme.cursor}
        onInput={(v) => board.setField(field, v)}
      />
    </box>
  )
}

/** Folders listed while picking: sized by every folder, not the matches, so the form holds still while typing. */
function listRows(form: Form, height: number): number {
  return Math.max(3, Math.min(8, form.folders.length, height - 24))
}

function formRows(form: Form, height: number): number {
  const folderRows = form.picking || !form.folder ? 1 + listRows(form, height) + 1 : 2
  // A gap, the folder, three fields with their notes, a gap and the buttons.
  return 1 + folderRows + 2 * 3 + 1 + 1
}

export function ProjectForm({ board, form, width, height }: { board: Board; form: Form; width: number; height: number }) {
  const contentWidth = Math.min(76, width - GUTTER - 1)
  const inputWidth = contentWidth - LABEL
  const err = (f: FormField) => (form.error?.field === f ? form.error.message : undefined)
  const list = listRows(form, height)
  const picking = form.focus === "folder" && form.picking
  const showPicker = form.picking || !form.folder

  // Folder: a filter over the projects root while picking, else the choice.
  const matches = board.matches()
  const first = Math.max(0, Math.min(form.highlight - list + 1, matches.length - list))
  const start = Math.max(0, Math.min(first, form.highlight))
  const visible = matches.slice(start, start + list)

  const port = form.port.trim()
  const portNote = /^\d+$/.test(port) && Number(port) > 0 ? `→ http://localhost:${port}` : `auto: ${AUTO_PORTS.start}–${AUTO_PORTS.end}`

  const command = form.command.trim()
  let commandNote = ""
  if (form.folder) {
    const r = form.recipe
    if (!command && hasDevScript(form.folder)) commandNote = `runs ${devScriptCommand(form.folder) ?? ""}`
    else if (r && r.command === command && r.readsEnv) commandNote = `detected ${r.name} · the server must listen on $PORT`
    else if (r && r.command === command) commandNote = `detected ${r.name} · gets its port from $PORT`
    else if (!command) commandNote = "set the command that starts the server"
    else commandNote = "the server gets its port in $PORT"
  }

  const folderTags = form.folder ? tagsFor(form.folder).join(" ") : ""
  const folderRows = showPicker ? 1 + list + 1 : 2

  return (
    <Dialog title={form.editing === undefined ? "New project" : "Edit project"} width={contentWidth} rows={formRows(form, height)} screen={[width, height]}>
      {showPicker ? (
        <box flexDirection="column" height={folderRows}>
          <box height={1} flexDirection="row">
            <Label text="Folder" focused={form.focus === "folder"} />
            <input
              width={inputWidth}
              value={form.query}
              focused={picking}
              placeholder="filter, or type a path"
              textColor={theme.text}
              focusedTextColor={theme.text}
              placeholderColor={theme.faint}
              backgroundColor={theme.well}
              focusedBackgroundColor={theme.wellFocus}
              cursorColor={theme.cursor}
              onInput={(v) => board.setField("query", v)}
            />
          </box>
          <box flexDirection="column" height={list} paddingLeft={LABEL}>
            {visible.length === 0 && (
              <text fg={theme.faint} wrapMode="none">
                {`Nothing matches in ${displayPath(board.m.config.projectsRoot)}`}
              </text>
            )}
            {visible.map((f, i) => {
              const on = start + i === form.highlight
              return (
                <box
                  key={f.path}
                  height={1}
                  flexDirection="row"
                  justifyContent="space-between"
                  backgroundColor={on ? theme.wellFocus : undefined}
                  onMouseDown={() => board.pickFolder(f.path)}
                >
                  <text wrapMode="none" fg={theme.text}>
                    <span fg={on ? theme.accent : theme.faint}>{on ? "› " : "  "}</span>
                    <span fg={theme.text} attributes={on ? BOLD : DIM}>{f.name}</span>
                  </text>
                  <text fg={theme.faint}>{f.tags.join(" ") + " "}</text>
                </box>
              )
            })}
          </box>
          <Note error={err("folder")}>
            {`${truncStart(displayPath(board.m.config.projectsRoot), inputWidth - 26)} · or type any path`}
          </Note>
        </box>
      ) : (
        <box flexDirection="column" height={folderRows}>
          <box
            height={1}
            flexDirection="row"
            onMouseDown={() => {
              form.focus = "folder"
              form.picking = true
              board.changed()
            }}
          >
            <Label text="Folder" focused={form.focus === "folder"} />
            <text wrapMode="none" fg={theme.text}>
              <span fg={theme.text}>{truncStart(displayPath(form.folder!), inputWidth - 2 - folderTags.length)}</span>
              <span fg={theme.faint}>{"  " + folderTags}</span>
            </text>
          </box>
          <Note error={err("folder")}>{form.focus === "folder" ? "enter to pick another folder" : ""}</Note>
        </box>
      )}
      <Field board={board} form={form} field="name" label="Name" placeholder="my-app" width={inputWidth} />
      <Note error={err("name")} />
      <box height={1} flexDirection="row">
        <Field board={board} form={form} field="port" label="Port" placeholder="auto" width={10} />
        <text fg={theme.faint} wrapMode="none">{`  ${portNote}`}</text>
      </box>
      <Note error={err("port")} />
      <Field board={board} form={form} field="command" label="Command" placeholder="detected from the folder" width={inputWidth} />
      <Note error={err("command")}>{commandNote}</Note>
      <box height={1} />
      <box height={1} flexDirection="row" justifyContent="flex-end" width={contentWidth}>
        <text onMouseDown={() => board.closeForm()} fg={theme.text}>
          <span fg={theme.text} attributes={BOLD}>esc</span>
          <span fg={theme.text} attributes={DIM}>{" cancel    "}</span>
        </text>
        <text onMouseDown={() => board.saveForm()} bg={theme.accent} fg={theme.onAccent} attributes={BOLD}>
          {form.editing === undefined ? " enter  add project " : " enter  save "}
        </text>
      </box>
    </Dialog>
  )
}

/** A gap, the question, a gap and the path. */
const SAVE_ROWS = 4

export function SaveDialog({ board, width, height }: { board: Board; width: number; height: number }) {
  const n = board.index.lines.length
  const contentWidth = Math.min(72, width - GUTTER - 1)
  return (
    <Dialog title="Save logs" width={contentWidth} rows={SAVE_ROWS} screen={[width, height]}>
      <text fg={theme.text} attributes={DIM}>{`Write the ${n} line${n === 1 ? "" : "s"} shown, as plain text, to:`}</text>
      <box height={1} />
      <input
        width={contentWidth}
        value={board.savePath}
        focused
        textColor={theme.text}
        focusedTextColor={theme.text}
        backgroundColor={theme.well}
        focusedBackgroundColor={theme.wellFocus}
        cursorColor={theme.cursor}
        onInput={(v) => board.setSavePath(v)}
      />
    </Dialog>
  )
}

/** Tools listed at once: all of them if the screen has room. */
function agentListRows(board: Board, height: number): number {
  return Math.max(1, Math.min(board.tools.length, height - 8))
}

/** A gap, the tools and a gap. */
function agentRows(board: Board, height: number): number {
  return 1 + agentListRows(board, height) + 1
}

export function AgentDialog({ board, width, height }: { board: Board; width: number; height: number }) {
  const e = board.entry
  const list = agentListRows(board, height)
  const start = Math.max(0, Math.min(board.toolHighlight - list + 1, board.tools.length - list))
  const visible = board.tools.slice(start, start + list)
  const contentWidth = Math.min(56, width - GUTTER - 1)
  return (
    <Dialog title={`Open ${e?.project.name ?? ""} in`} width={contentWidth} rows={agentRows(board, height)} screen={[width, height]}>
      {visible.map((tool, i) => {
        const index = start + i
        const on = index === board.toolHighlight
        return (
          <box
            key={tool.name}
            height={1}
            flexDirection="row"
            justifyContent="space-between"
            backgroundColor={on ? theme.wellFocus : undefined}
            onMouseDown={() => board.pickTool(index)}
          >
            <text wrapMode="none" fg={theme.text}>
              <span fg={on ? theme.accent : theme.faint}>{on ? "› " : "  "}</span>
              <span fg={theme.accent} attributes={BOLD}>{index < 9 ? `${index + 1}  ` : "   "}</span>
              <span fg={theme.text} attributes={on ? BOLD : DIM}>{tool.name}</span>
            </text>
            <text fg={theme.faint}>{(tool.kind === "agent" ? "in a terminal" : "editor") + " "}</text>
          </box>
        )
      })}
    </Dialog>
  )
}

const HELP: [string, string][][] = [
  [
    ["↑ ↓  j k", "select project"],
    ["enter  space", "start / stop"],
    ["r", "restart"],
    ["a  x", "start all / stop all"],
    ["n", "new project"],
    ["e", "edit"],
    ["d  del", "remove (press twice)"],
    ["alt+↑↓  J K", "move up / down"],
    ["o  c", "open / copy URL"],
    ["i", "open in editor / agent"],
  ],
  [
    ["/  ctrl+f", "filter logs"],
    ["esc", "clear filter"],
    ["pgup pgdn", "scroll logs"],
    ["ctrl+u ctrl+d", "half a page"],
    ["g  G", "oldest / newest"],
    ["y", "copy the lines shown"],
    ["w", "save them to a file"],
    ["L", "clear logs"],
    ["q  ctrl+c", "quit, stopping servers"],
  ],
]

const helpColumns = (width: number) => (width >= 84 ? HELP : [HELP.flat()])

/** A gap, the keys, a gap and two lines under them. */
function helpRows(width: number): number {
  return 1 + Math.max(...helpColumns(width).map((c) => c.length)) + 1 + 2
}

export function HelpDialog({ width, height }: { width: number; height: number }) {
  const columns = helpColumns(width)
  return (
    <Dialog title="Keys" width={columns.length > 1 ? 80 : 56} rows={helpRows(width)} screen={[width, height]}>
      <box flexDirection="row" gap={4}>
        {columns.map((col, i) => (
          <box key={i} flexDirection="column">
            {col.map(([key, action]) => (
              <text key={key} wrapMode="none" fg={theme.text}>
                <span fg={theme.accent} attributes={BOLD}>
                  {key.padEnd(15)}
                </span>
                <span fg={theme.text} attributes={DIM}>{action}</span>
              </text>
            ))}
          </box>
        ))}
      </box>
      <box height={1} />
      <text fg={theme.faint}>Mouse: click a project, scroll the logs, drag to copy.</text>
      <text fg={theme.faint}>{`blueprint v${pkg.version}`}</text>
    </Dialog>
  )
}

/** A gap and the notice. */
const QUITTING_ROWS = 2

export function QuittingDialog({ board, width, height }: { board: Board; width: number; height: number }) {
  const n = board.m.runningCount()
  return (
    <Dialog title="Quitting" width={44} rows={QUITTING_ROWS} screen={[width, height]}>
      <text fg={theme.text}>{n ? `Stopping ${n} server${n === 1 ? "" : "s"}…` : "Done."}</text>
    </Dialog>
  )
}
