//! Everything that floats over the board: the add/edit form, the save-logs
//! prompt, help, and the notice shown while servers stop on the way out.

import { TextAttributes } from "@opentui/core"
import type { ReactNode } from "react"
import { displayPath } from "../core/config"
import { tagsFor } from "../core/folders"
import { AUTO_PORTS } from "../core/process"
import { devScriptCommand, hasDevScript } from "../core/stack"
import type { Board, Form, FormField } from "./board"
import { theme, truncStart } from "./theme"

const LABEL = 10

/** A centered, bordered sheet above the board. */
function Dialog(props: { title: string; width: number; height: number; screen: [number, number]; children: ReactNode }) {
  const [w, h] = props.screen
  const width = Math.min(props.width, w - 2)
  const height = Math.min(props.height, h - 2)
  return (
    <>
      {/* Dim the board behind, so the dialog is the one thing to look at. */}
      <box position="absolute" left={0} top={0} width={w} height={h} zIndex={9} backgroundColor={theme.backdrop} />
      <box
      position="absolute"
      left={Math.max(0, Math.floor((w - width) / 2))}
      top={Math.max(0, Math.floor((h - height) / 2))}
      width={width}
      height={height}
      zIndex={10}
      border
      borderStyle="rounded"
      borderColor={theme.accent}
      backgroundColor={theme.overlay}
      title={` ${props.title} `}
      titleAlignment="left"
      flexDirection="column"
      paddingLeft={2}
      paddingRight={2}
      paddingTop={1}
    >
      {props.children}
      </box>
    </>
  )
}

function Label({ text, focused }: { text: string; focused: boolean }) {
  return (
    <text fg={focused ? theme.accent : theme.muted} attributes={focused ? TextAttributes.BOLD : TextAttributes.NONE}>
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
        cursorColor={theme.accent}
        onInput={(v) => board.setField(field, v)}
      />
    </box>
  )
}

export function ProjectForm({ board, form, width, height }: { board: Board; form: Form; width: number; height: number }) {
  const dialogWidth = Math.min(78, width - 2)
  const inputWidth = dialogWidth - 4 - 2 - LABEL
  const err = (f: FormField) => (form.error?.field === f ? form.error.message : undefined)
  // Sized by every folder, not the matches, so the dialog holds still while typing.
  const listRows = Math.max(3, Math.min(8, form.folders.length, height - 24))
  const picking = form.focus === "folder" && form.picking
  const showPicker = form.picking || !form.folder

  // Folder: a filter over the projects root while picking, else the choice.
  const matches = board.matches()
  const first = Math.max(0, Math.min(form.highlight - listRows + 1, matches.length - listRows))
  const start = Math.max(0, Math.min(first, form.highlight))
  const visible = matches.slice(start, start + listRows)

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
  const folderRows = showPicker ? 1 + listRows + 1 : 2
  // Borders, top padding, the folder, three fields with their notes, a gap, the buttons and a bottom margin.
  const dialogHeight = 2 + 1 + folderRows + 2 * 3 + 1 + 1 + 1

  return (
    <Dialog title={form.editing === undefined ? "New project" : "Edit project"} width={dialogWidth} height={dialogHeight} screen={[width, height]}>
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
              cursorColor={theme.accent}
              onInput={(v) => board.setField("query", v)}
            />
          </box>
          <box flexDirection="column" height={listRows} paddingLeft={LABEL}>
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
                  <text wrapMode="none">
                    <span fg={on ? theme.accent : theme.faint}>{on ? "› " : "  "}</span>
                    <span fg={on ? theme.text : theme.muted}>{f.name}</span>
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
            <text wrapMode="none">
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
      <box height={1} flexDirection="row" justifyContent="flex-end">
        <text onMouseDown={() => board.closeForm()}>
          <span fg={theme.text}>esc</span>
          <span fg={theme.muted}>{" cancel    "}</span>
        </text>
        <text onMouseDown={() => board.saveForm()} bg={theme.accent} fg={theme.onAccent} attributes={TextAttributes.BOLD}>
          {form.editing === undefined ? " enter  add project " : " enter  save "}
        </text>
      </box>
    </Dialog>
  )
}

export function SaveDialog({ board, width, height }: { board: Board; width: number; height: number }) {
  const n = board.index.lines.length
  const dialogWidth = Math.min(72, width - 2)
  return (
    <Dialog title="Save logs" width={dialogWidth} height={7} screen={[width, height]}>
      <text fg={theme.muted}>{`Write the ${n} line${n === 1 ? "" : "s"} shown, as plain text, to:`}</text>
      <box height={1} />
      <input
        width={dialogWidth - 6}
        value={board.savePath}
        focused
        textColor={theme.text}
        focusedTextColor={theme.text}
        backgroundColor={theme.well}
        focusedBackgroundColor={theme.wellFocus}
        cursorColor={theme.accent}
        onInput={(v) => board.setSavePath(v)}
      />
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

export function HelpDialog({ width, height }: { width: number; height: number }) {
  const wide = width >= 84
  const columns = wide ? HELP : [HELP.flat()]
  const rows = Math.max(...columns.map((c) => c.length))
  return (
    <Dialog title="Keys" width={wide ? 82 : 44} height={rows + 5} screen={[width, height]}>
      <box flexDirection="row" gap={4}>
        {columns.map((col, i) => (
          <box key={i} flexDirection="column">
            {col.map(([key, action]) => (
              <text key={key} wrapMode="none">
                <span fg={theme.text} attributes={TextAttributes.BOLD}>
                  {key.padEnd(15)}
                </span>
                <span fg={theme.muted}>{action}</span>
              </text>
            ))}
          </box>
        ))}
      </box>
      <box height={1} />
      <text fg={theme.faint}>Mouse: click a project, scroll the logs, drag to copy.</text>
    </Dialog>
  )
}

export function QuittingDialog({ board, width, height }: { board: Board; width: number; height: number }) {
  const n = board.m.runningCount()
  return (
    <Dialog title="Quitting" width={44} height={5} screen={[width, height]}>
      <text fg={theme.text}>{n ? `Stopping ${n} server${n === 1 ? "" : "s"}…` : "Done."}</text>
    </Dialog>
  )
}
