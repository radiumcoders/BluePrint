//! The board's state and what keys do to it. Components only render it; it
//! tells them to redraw through `subscribe`.

import { writeFileSync } from "node:fs"
import { isAbsolute, join, resolve } from "node:path"
import type { KeyEvent } from "@opentui/core"
import { displayPath, expandTilde, isDir, suggestName } from "../core/config"
import { fuzzy, listFolders, type Folder } from "../core/folders"
import { LogIndex } from "../core/logindex"
import type { Entry, Field, Id, Manager } from "../core/manager"
import { detect, type Recipe } from "../core/stack"

export type Mode = "normal" | "filter" | "form" | "save" | "help" | "quitting"

export type FormField = "folder" | "name" | "port" | "command"
const FIELDS: FormField[] = ["folder", "name", "port", "command"]

export interface Form {
  editing?: Id
  focus: FormField
  /** Choosing a folder: the folder field is a filter over the projects root. */
  picking: boolean
  query: string
  /** The highlighted match while picking. */
  highlight: number
  folders: Folder[]
  folder?: string
  /** How the picked folder would start, if recognized. */
  recipe?: Recipe
  name: string
  port: string
  command: string
  error?: { field: Field; message: string }
}

/** What the board needs from the terminal around it. */
export interface Host {
  /** Put text on the clipboard; false if the terminal can't. */
  copy(text: string): boolean
  openUrl(url: string): void
  /** Called once every server has stopped after a quit. */
  exit(): void
}

export class Board {
  selected?: Id
  mode: Mode = "normal"
  form?: Form
  /** Remove needs a second press; this is the project awaiting it. */
  confirmRemove?: Id
  /** The log filter as typed. */
  filter = ""
  readonly index = new LogIndex()
  /** The console sticks to the newest line until scrolled up. */
  follow = true
  /** The first shown line, as an index into `index.lines`. */
  top = 0
  /** Rows the console has, set by the layout. */
  viewRows = 10
  /** Where "save logs" writes, while its prompt is open. */
  savePath = ""
  private listeners = new Set<() => void>()
  private lastSecond = performance.now()

  constructor(
    readonly m: Manager,
    private readonly host: Host,
  ) {
    this.selected = m.entries[0]?.id
  }

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn)
    return () => this.listeners.delete(fn)
  }

  changed() {
    for (const fn of this.listeners) fn()
  }

  get entry(): Entry | undefined {
    return this.m.get(this.selected)
  }

  /** Called ~10 times a second. */
  tick() {
    let changed = this.m.tick()
    if (performance.now() - this.lastSecond >= 1000) {
      this.lastSecond = performance.now()
      // Uptimes tick over.
      changed ||= this.m.runningCount() > 0
    }
    if (!this.entry && this.selected !== this.m.entries[0]?.id) {
      this.selected = this.m.entries[0]?.id
      changed = true
    }
    if (changed) this.changed()
  }

  /** Bring the console's lines and scroll position in step with the selected log. */
  syncLogs() {
    const change = this.index.sync(this.entry, this.filter)
    if (change.kind === "reset") this.follow = true
    else if (change.kind === "splice" && !this.follow) this.top = Math.max(0, this.top - change.dropped)
    const max = Math.max(0, this.index.lines.length - this.viewRows)
    if (this.follow || this.top >= max) {
      this.top = max
      this.follow = true
    }
  }

  scrollLogs(delta: number) {
    const max = Math.max(0, this.index.lines.length - this.viewRows)
    this.top = Math.max(0, Math.min(max, this.top + delta))
    this.follow = this.top >= max
    this.changed()
  }

  openUrl(url: string) {
    this.host.openUrl(url)
  }

  select(id: Id) {
    if (this.selected !== id) {
      this.selected = id
      this.confirmRemove = undefined
    }
    this.changed()
  }

  async quit() {
    if (this.mode === "quitting") return
    this.mode = "quitting"
    this.form = undefined
    this.changed()
    await this.m.shutdown()
    this.host.exit()
  }

  // -------------------------------------------------------------------------
  // Keys

  key(k: KeyEvent) {
    if (k.ctrl && k.name === "c") {
      void this.quit()
      return
    }
    switch (this.mode) {
      case "quitting":
        return
      case "help":
        this.mode = "normal"
        return this.changed()
      case "form":
        return this.formKey(k)
      case "save":
        return this.saveKey(k)
      case "filter":
        return this.filterKey(k)
      case "normal":
        return this.normalKey(k)
    }
  }

  private normalKey(k: KeyEvent) {
    const m = this.m
    const sel = this.selected
    const ids = m.entries.map((e) => e.id)
    const pos = sel === undefined ? -1 : ids.indexOf(sel)
    const page = Math.max(1, this.viewRows - 2)
    const name = k.name

    // Reordering: Alt+↑/↓, or Shift+J/K.
    if (((k.meta || k.option) && (name === "up" || name === "down")) || (k.shift && (name === "j" || name === "k"))) {
      if (sel !== undefined) m.moveBy(sel, name === "up" || name === "k" ? -1 : 1)
      return this.changed()
    }
    if (k.ctrl) {
      if (name === "f") this.startFilter()
      else if (name === "u") this.scrollLogs(-Math.ceil(page / 2))
      else if (name === "d") this.scrollLogs(Math.ceil(page / 2))
      return
    }
    if (k.shift && name === "l") {
      this.entry?.clearLogs()
      return this.changed()
    }
    if (k.shift && name === "g") return this.scrollLogs(Infinity)

    switch (k.sequence === "?" ? "?" : name) {
      case "up":
      case "k":
        if (ids.length) this.select(ids[pos < 0 ? 0 : Math.max(0, pos - 1)]!)
        return
      case "down":
      case "j":
        if (ids.length) this.select(ids[pos < 0 ? 0 : Math.min(ids.length - 1, pos + 1)]!)
        return
      case "return":
      case "space":
        if (sel !== undefined) m.toggle(sel)
        break
      case "r":
        if (sel !== undefined) m.restart(sel)
        break
      case "a":
        m.startAll()
        break
      case "x":
        m.stopAll()
        break
      case "n":
        return this.openForm(undefined)
      case "e":
        if (sel !== undefined) this.openForm(sel)
        return
      case "o": {
        const url = this.entry?.url()
        if (url) this.openUrl(url)
        return
      }
      case "c": {
        const url = this.entry?.url()
        if (url) this.copy(url, `Copied ${url}`)
        break
      }
      case "d":
      case "delete":
        if (sel !== undefined) this.remove(sel)
        return
      case "/":
        return this.startFilter()
      case "y":
        return this.copyLogs()
      case "w":
        return this.openSave()
      case "pageup":
        return this.scrollLogs(-page)
      case "pagedown":
        return this.scrollLogs(page)
      case "home":
      case "g":
        return this.scrollLogs(-Infinity)
      case "end":
        return this.scrollLogs(Infinity)
      case "escape":
        if (this.filter) this.filter = ""
        this.confirmRemove = undefined
        break
      case "?":
        this.mode = "help"
        break
      case "q":
        void this.quit()
        return
      default:
        return
    }
    this.changed()
  }

  remove(id: Id) {
    if (this.confirmRemove !== id) {
      this.confirmRemove = id
    } else {
      this.m.remove(id)
      this.confirmRemove = undefined
      this.selected = this.m.entries[0]?.id
    }
    this.changed()
  }

  private copy(text: string, done: string) {
    if (this.host.copy(text)) this.m.info(done)
    else this.m.error("This terminal doesn't let programs set the clipboard")
  }

  // -------------------------------------------------------------------------
  // Log filter, copy and save

  private startFilter() {
    if (!this.entry) return
    this.mode = "filter"
    this.changed()
  }

  setFilter(text: string) {
    if (text === this.filter) return
    this.filter = text
    this.changed()
  }

  private filterKey(k: KeyEvent) {
    if (k.name === "escape") {
      this.filter = ""
      this.mode = "normal"
    } else if (k.name === "return" || k.name === "tab") {
      this.mode = "normal"
    } else {
      return
    }
    this.changed()
  }

  private copyLogs() {
    const e = this.entry
    if (!e || !this.index.lines.length) return
    this.copy(this.index.plainText(e), `Copied ${this.index.lines.length} log lines`)
    this.changed()
  }

  private openSave() {
    const e = this.entry
    if (!e || !this.index.lines.length) return
    this.savePath = displayPath(join(e.project.path, `${e.project.name}-logs.txt`))
    this.mode = "save"
    this.changed()
  }

  setSavePath(text: string) {
    this.savePath = text
  }

  private saveKey(k: KeyEvent) {
    if (k.name === "escape") {
      this.mode = "normal"
    } else if (k.name === "return") {
      const e = this.entry
      if (e && this.savePath.trim()) {
        let path = expandTilde(this.savePath.trim())
        if (!isAbsolute(path)) path = resolve(e.project.path, path)
        try {
          writeFileSync(path, this.index.plainText(e))
          this.m.info(`Saved logs to ${displayPath(path)}`)
        } catch (err) {
          this.m.error(`Couldn't save the logs: ${(err as Error).message}`)
        }
      }
      this.mode = "normal"
    } else {
      return
    }
    this.changed()
  }

  // -------------------------------------------------------------------------
  // Add / edit

  openForm(editing: Id | undefined) {
    const project = this.m.get(editing)?.project
    this.form = {
      editing,
      focus: project ? "name" : "folder",
      picking: !project,
      query: "",
      highlight: 0,
      folders: listFolders(this.m.config.projectsRoot),
      folder: project?.path,
      recipe: project ? detect(project.path) : undefined,
      name: project?.name ?? "",
      port: project?.port?.toString() ?? "",
      command: project?.command ?? "",
    }
    this.mode = "form"
    this.changed()
  }

  /** Folders matching the folder filter. */
  matches(): Folder[] {
    const f = this.form
    return f ? f.folders.filter((d) => fuzzy(f.query.trim(), d.name)) : []
  }

  setField(field: "query" | "name" | "port" | "command", value: string) {
    const f = this.form
    if (!f || f[field] === value) return
    f[field] = value
    if (field === "query") f.highlight = 0
    f.error = undefined
    this.changed()
  }

  private formKey(k: KeyEvent) {
    const f = this.form
    if (!f) return
    const move = (delta: number) => {
      const i = (FIELDS.indexOf(f.focus) + delta + FIELDS.length) % FIELDS.length
      if (f.focus === "folder" && f.folder) f.picking = false
      f.focus = FIELDS[i]!
    }
    if (k.name === "escape") {
      if (f.picking && f.folder) f.picking = false
      else this.closeForm()
    } else if (k.name === "tab") {
      move(k.shift ? -1 : 1)
    } else if (f.focus === "folder" && f.picking) {
      const count = this.matches().length
      if (k.name === "up") f.highlight = Math.max(0, f.highlight - 1)
      else if (k.name === "down") f.highlight = Math.min(Math.max(0, count - 1), f.highlight + 1)
      else if (k.name === "return") this.pickFromQuery()
      else return
    } else if (f.focus === "folder" && (k.name === "return" || k.name === "space")) {
      f.picking = true
      f.query = ""
      f.highlight = 0
    } else if (k.name === "up") {
      move(-1)
    } else if (k.name === "down") {
      move(1)
    } else if (k.name === "return") {
      this.saveForm()
    } else {
      return
    }
    this.changed()
  }

  /** Enter while picking: a typed path if it looks like one, else the highlighted match. */
  private pickFromQuery() {
    const f = this.form!
    const q = f.query.trim()
    if (/^([~./\\]|[A-Za-z]:[\\/])/.test(q)) {
      const path = resolve(this.m.config.projectsRoot, expandTilde(q))
      if (isDir(path)) this.pickFolder(path)
      else f.error = { field: "folder", message: `There's no folder at ${displayPath(path)}` }
      return
    }
    const match = this.matches()[f.highlight]
    if (match) this.pickFolder(match.path)
  }

  pickFolder(path: string) {
    const f = this.form
    if (!f) return
    // Refresh the suggested name unless the user typed their own.
    const oldName = f.folder ? suggestName(f.folder) : ""
    if (!f.name.trim() || f.name === oldName) f.name = suggestName(path)
    // Same for the command: fill in what the folder looks like it needs.
    const recipe = detect(path)
    if (!f.command.trim() || f.command === (f.recipe?.command ?? "")) f.command = recipe?.command ?? ""
    f.recipe = recipe
    f.folder = path
    f.picking = false
    f.error = undefined
    f.focus = "name"
    this.changed()
  }

  saveForm() {
    const f = this.form
    if (!f) return
    const result = this.m.validate(f.editing, f.name, f.folder, f.port, f.command)
    if (!result.ok) {
      f.error = { field: result.field, message: result.error }
      if (result.field === "folder") {
        f.focus = "folder"
        f.picking = true
      } else {
        f.focus = result.field
      }
      return this.changed()
    }
    if (f.editing !== undefined) this.m.update(f.editing, result.project)
    else this.selected = this.m.add(result.project)
    this.closeForm()
  }

  closeForm() {
    this.form = undefined
    this.mode = "normal"
    this.changed()
  }
}
