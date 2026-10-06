//! The UI-independent core: projects and their processes.

import { connect } from "node:net"
import { isDir, saveConfig, validateName, type Config, type Project } from "./config"
import * as proc from "./process"
import { LogQueue, Running } from "./process"
import { devScriptCommand, detect, hasDevScript } from "./stack"
import { now, STOP_GRACE_MS } from "./timing"

/** A project's log keeps its newest lines, up to both of these limits. */
export const MAX_LOG_LINES = 5000
export const MAX_LOG_BYTES = 4 << 20
/**
 * Output lines waiting for the manager. When the queue is full, servers
 * writing more wait, rather than memory growing without bound.
 */
export const LOG_QUEUE = 2048
export const MESSAGE_TTL_MS = 6000
/** How often a starting server is checked for accepting connections, and how often a running one is checked again. */
const PROBE_STARTING_MS = 400
const PROBE_RUNNING_MS = 2000
/** Failed checks in a row before a running server counts as not responding. */
const PROBE_MISSES = 2
/** After this long without accepting connections, say so in the logs. */
export const SLOW_START_MS = 60_000

export type Id = number

export type Status =
  | "stopped"
  /** Process running, server not accepting connections yet. */
  | "starting"
  /** Accepting connections on its port. */
  | "running"
  /** Was accepting connections, but no longer is. */
  | "unresponsive"
  | "stopping"
  | "crashed"

export type MsgKind = "info" | "error"

/** Which form field a validation error belongs to. */
export type Field = "name" | "folder" | "port" | "command"

export class Entry {
  run?: Running
  /** Start again as soon as the current process exits. */
  restart = false
  lastExit?: number
  /**
   * The port the server was last given in `PORT`. Kept after it stops so an
   * auto-assigned port is reused on the next start.
   */
  appPort?: number
  /** The server accepted a connection at the last check. */
  ready = false
  /**
   * It has accepted connections since it started, so not being ready means
   * it stopped responding rather than that it is still starting.
   */
  wasReady = false
  misses = 0
  probing = false
  probed?: number
  slowWarned = false
  logs: string[] = []
  /**
   * The line number of `logs[0]`, counting every line ever logged, so a line
   * keeps its number as older ones are dropped or cleared.
   */
  logStart = 0
  logBytes = 0
  /** Bumped whenever `logs` changes, so views can tell cheaply. */
  logRev = 0

  constructor(
    /** Stable id used to route process output; survives reordering and edits. */
    readonly id: Id,
    public project: Project,
  ) {}

  status(): Status {
    const r = this.run
    if (r?.stopping()) return "stopping"
    if (r && this.ready) return "running"
    if (r && this.wasReady) return "unresponsive"
    if (r) return "starting"
    if (this.lastExit !== undefined && this.lastExit !== 0) return "crashed"
    return "stopped"
  }

  isActive(): boolean {
    return this.run !== undefined
  }

  /**
   * The port the server listens on: the one it was given while it runs (even
   * if the project has since been edited), else its fixed one.
   */
  port(): number | undefined {
    return this.run ? this.run.port : this.project.port
  }

  /** Where to open it, once its port is known. */
  url(): string | undefined {
    const p = this.port()
    return p === undefined ? undefined : `http://localhost:${p}`
  }

  /** The project was edited since it started, so a restart would change it. */
  editedWhileRunning(): boolean {
    const r = this.run?.project
    if (!r) return false
    const p = this.project
    return r.name !== p.name || r.path !== p.path || r.port !== p.port || r.command !== p.command
  }

  log(line: string) {
    this.logBytes += line.length
    this.logs.push(line)
    let drop = 0
    while (this.logs.length - drop > MAX_LOG_LINES || this.logBytes > MAX_LOG_BYTES) {
      this.logBytes -= this.logs[drop]!.length
      drop++
    }
    if (drop) {
      this.logs.splice(0, drop)
      this.logStart += drop
    }
    this.logRev++
  }

  clearLogs() {
    this.logStart += this.logs.length
    this.logs = []
    this.logBytes = 0
    this.logRev++
  }
}

/** The answer to whether a run's port accepts connections. */
interface Probe {
  id: Id
  /** Identifies the run that was checked. */
  started: number
  ok: boolean
}

export class Manager {
  entries: Entry[] = []
  message?: { text: string; kind: MsgKind; at: number }
  private nextId = 0
  private queue = new LogQueue(LOG_QUEUE)
  private probes: Probe[] = []

  constructor(
    public config: Config,
    readonly configPath: string,
  ) {
    for (const p of config.projects) this.pushEntry({ ...p })
  }

  private pushEntry(project: Project): Id {
    this.entries.push(new Entry(++this.nextId, project))
    return this.nextId
  }

  info(text: string) {
    this.message = { text, kind: "info", at: now() }
  }

  error(text: string) {
    this.message = { text, kind: "error", at: now() }
  }

  index(id: Id): number {
    return this.entries.findIndex((e) => e.id === id)
  }

  get(id: Id | undefined): Entry | undefined {
    return this.entries.find((e) => e.id === id)
  }

  runningCount(): number {
    return this.entries.filter((e) => e.run).length
  }

  /**
   * Write the projects to the config file. On failure the error replaces any
   * message and `false` comes back, so callers only report success that
   * really happened.
   */
  private save(): boolean {
    this.config.projects = this.entries.map((e) => e.project)
    try {
      saveConfig(this.config, this.configPath)
      return true
    } catch (e) {
      this.error(`Couldn't save the config: ${(e as Error).message}`)
      return false
    }
  }

  // -------------------------------------------------------------------------
  // Background work

  /**
   * Take in process output, reap exited servers and keep track of which ones
   * accept connections. Never blocks: output is taken a bounded batch at a
   * time and ports are checked asynchronously. Returns whether anything
   * visible changed.
   */
  tick(): boolean {
    let changed = false
    for (const { id, line } of this.queue.take(LOG_QUEUE)) {
      changed = true
      this.get(id)?.log(line)
    }

    const crashed: string[] = []
    const restart: Id[] = []
    for (const e of this.entries) {
      const run = e.run
      if (!run) continue
      const wasLingering = run.lingering()
      const status = run.poll()
      if (status) {
        changed = true
        const byUs = run.stopRequested
        e.run = undefined
        e.ready = false
        e.lastExit = byUs ? 0 : (status.code ?? -1)
        e.log(
          byUs
            ? "── stopped ──"
            : status.code !== null
              ? `── exited with code ${status.code} ──`
              : "── killed by a signal ──",
        )
        if (e.restart) {
          e.restart = false
          restart.push(e.id)
        } else if (!byUs && status.code !== 0) {
          crashed.push(e.project.name)
        }
      } else if (!wasLingering && run.lingering() && !run.stopRequested) {
        changed = true
        e.ready = false
        e.log("── the command exited; stopping what it left running ──")
      }
    }
    if (crashed[0]) this.error(`${crashed[0]} stopped unexpectedly. See its logs.`)
    for (const id of restart) this.start(id)

    changed = this.checkPorts() || changed

    if (this.message && now() - this.message.at > MESSAGE_TTL_MS) {
      this.message = undefined
      changed = true
    }
    return changed
  }

  /**
   * Starting -> Running once a server accepts connections, Running ->
   * Unresponsive if it stops. Checks run asynchronously, since a connection
   * attempt can take a while.
   */
  private checkPorts(): boolean {
    let changed = false
    for (const p of this.probes.splice(0)) {
      const e = this.get(p.id)
      if (!e) continue
      e.probing = false
      const run = e.run
      if (!run || run.started !== p.started || run.stopping()) continue
      if (p.ok) {
        e.misses = 0
        if (!e.ready) {
          changed = true
          if (e.wasReady) e.log(`── accepting connections on port ${run.port} again ──`)
          e.ready = true
          e.wasReady = true
        }
      } else if (e.ready && ++e.misses >= PROBE_MISSES) {
        changed = true
        e.ready = false
        e.log(`── port ${run.port} stopped accepting connections ──`)
      }
    }

    let slow: string | undefined
    for (const e of this.entries) {
      const run = e.run
      if (!run || run.stopping()) continue
      if (!e.wasReady && !e.slowWarned && now() - run.started >= SLOW_START_MS) {
        e.slowWarned = true
        changed = true
        const msg = `nothing has accepted connections on port ${run.port} for ${SLOW_START_MS / 1000}s. If the server is up, it isn't listening on $PORT.`
        e.log(`── ${msg} ──`)
        slow ??= `${e.project.name}: ${msg}`
      }
      const every = e.ready ? PROBE_RUNNING_MS : PROBE_STARTING_MS
      if (e.probing || (e.probed !== undefined && now() - e.probed < every)) continue
      e.probing = true
      e.probed = now()
      const { id } = e
      const { started, port } = run
      void listening(port).then((ok) => this.probes.push({ id, started, ok }))
    }
    if (slow) this.error(slow)
    return changed
  }

  // -------------------------------------------------------------------------
  // Process control

  start(id: Id) {
    const i = this.index(id)
    const e = this.entries[i]
    if (!e || e.isActive()) return
    const missing = missingCommand(e.project.path, e.project.command)
    if (missing) {
      const msg = `${e.project.name} can't start: ${missing}`
      e.log(`── ${msg} ──`)
      this.error(msg)
      return
    }
    if (e.project.port !== undefined && proc.portInUse(e.project.port)) {
      const msg = `Port ${e.project.port} is already in use, so ${e.project.name} can't start`
      e.log(`── ${msg} ──`)
      this.error(msg)
      return
    }
    this.spawn(i)
  }

  /** Ports other projects hold: every fixed port, and the ports running projects were given. */
  takenPorts(except: number): number[] {
    return this.entries
      .filter((_, j) => j !== except)
      .flatMap((e) => [e.project.port, e.run?.port])
      .filter((p): p is number => p !== undefined)
  }

  private spawn(i: number) {
    const e = this.entries[i]!
    const command = e.project.command.trim() || devScriptCommand(e.project.path) || ""
    const port = e.project.port ?? proc.freePort(e.appPort, this.takenPorts(i))
    if (port === undefined) {
      const msg = `No free port between ${proc.AUTO_PORTS.start} and ${proc.AUTO_PORTS.end} for ${e.project.name}`
      e.log(`── ${msg} ──`)
      this.error(msg)
      return
    }
    if (e.logs.length) e.log("")
    e.appPort = port
    e.ready = false
    e.wasReady = false
    e.misses = 0
    e.probed = undefined
    e.slowWarned = false
    e.lastExit = undefined
    try {
      const run = proc.spawn(e.project, command, port, e.id, this.queue)
      // Logged first so the output shows exactly what ran.
      e.log(`$ PORT=${port} ${command}`)
      e.run = run
    } catch (err) {
      const msg = (err as Error).message
      e.log(`── ${msg} ──`)
      e.lastExit = -1
      this.error(msg)
    }
  }

  stop(id: Id) {
    const e = this.get(id)
    if (!e) return
    e.restart = false
    e.run?.stop()
  }

  toggle(id: Id) {
    const e = this.get(id)
    if (e?.isActive()) this.stop(id)
    else if (e) this.start(id)
  }

  /** Stop, then start again from `tick` once the old process has exited. */
  restart(id: Id) {
    const e = this.get(id)
    if (!e) return
    if (e.run) {
      this.stop(id)
      e.restart = true
    } else {
      this.start(id)
    }
  }

  startAll() {
    for (const e of [...this.entries]) this.start(e.id)
  }

  stopAll() {
    for (const e of this.entries) this.stop(e.id)
  }

  /** SIGTERM everything, wait for the grace period, then SIGKILL stragglers. */
  async shutdown(): Promise<void> {
    for (const e of this.entries) {
      e.restart = false
      e.run?.stop()
    }
    // Long enough for `tick` to SIGKILL at the deadline and see them go.
    const deadline = now() + STOP_GRACE_MS + 1000
    while (this.runningCount() > 0 && now() < deadline) {
      await Bun.sleep(50)
      this.tick()
    }
    for (const e of this.entries) {
      e.run?.killNow()
      e.run = undefined
    }
  }

  // -------------------------------------------------------------------------
  // Projects

  /** Validate form input. `editing` is the project being edited, if any. */
  validate(
    editing: Id | undefined,
    nameInput: string,
    folder: string | undefined,
    portInput: string,
    commandInput: string,
  ): { ok: true; project: Project } | { ok: false; field: Field; error: string } {
    const others = this.entries.filter((e) => e.id !== editing).map((e) => e.project)
    const name = nameInput.trim()
    const bad = validateName(name)
    if (bad) return { ok: false, field: "name", error: bad }
    if (others.some((p) => p.name === name)) return { ok: false, field: "name", error: `“${name}” is already taken` }
    if (!folder || !isDir(folder)) return { ok: false, field: "folder", error: "Pick a project folder" }
    let port: number | undefined
    const portText = portInput.trim()
    if (portText) {
      const n = /^\d+$/.test(portText) ? Number(portText) : NaN
      if (!(n >= 1 && n <= 65535)) return { ok: false, field: "port", error: "Use a port from 1 to 65535" }
      const holder = others.find((p) => p.port === n)
      if (holder) return { ok: false, field: "port", error: `${holder.name} already uses port ${n}` }
      port = n
    }
    const command = commandInput.trim()
    const missing = missingCommand(folder, command)
    if (missing) return { ok: false, field: "command", error: missing }
    return { ok: true, project: { name, path: folder, port, command } }
  }

  add(project: Project): Id {
    const id = this.pushEntry(project)
    if (this.save()) this.info(`Added ${project.name}. Press enter to start it.`)
    return id
  }

  update(id: Id, project: Project) {
    const e = this.get(id)
    if (!e) return
    e.project = project
    const needsRestart = e.editedWhileRunning()
    if (this.save()) this.info(needsRestart ? `Saved. Restart ${project.name} to apply the changes.` : "Saved")
  }

  remove(id: Id) {
    const i = this.index(id)
    const e = this.entries[i]
    if (!e) return
    e.run?.stopInBackground()
    e.run = undefined
    this.entries.splice(i, 1)
    if (this.save()) this.info(`Removed ${e.project.name}`)
  }

  moveBy(id: Id, delta: number) {
    const i = this.index(id)
    const j = i + delta
    if (i < 0 || j < 0 || j >= this.entries.length) return
    ;[this.entries[i], this.entries[j]] = [this.entries[j]!, this.entries[i]!]
    this.save()
  }
}

/** Whether something accepts TCP connections on localhost:`port`. */
export function listening(port: number): Promise<boolean> {
  const attempt = (host: string) =>
    new Promise<boolean>((resolve) => {
      const socket = connect({ host, port })
      const done = (ok: boolean) => {
        socket.destroy()
        resolve(ok)
      }
      socket.setTimeout(250, () => done(false))
      socket.once("connect", () => done(true))
      socket.once("error", () => done(false))
    })
  return attempt("127.0.0.1").then((ok) => ok || attempt("::1"))
}

/** Why an empty command can't work in `dir`: it means the package.json dev script. `undefined` when it's fine. */
export function missingCommand(dir: string, command: string): string | undefined {
  if (command.trim() || hasDevScript(dir)) return undefined
  const r = detect(dir)
  return r?.command
    ? `there's no dev script here, so set a command, e.g. ${r.command}`
    : "there's no package.json dev script here, so set the command that starts the server"
}
