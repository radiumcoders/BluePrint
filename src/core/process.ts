//! Spawning dev servers and streaming their output.

import { spawn as spawnChild, type ChildProcess } from "node:child_process"
import type { Readable } from "node:stream"
import { isDir, type Project } from "./config"
import { adopt, shell, type Tree } from "./platform"
import { KILL_GRACE_MS, STOP_GRACE_MS, now } from "./timing"

export { STOP_GRACE_MS } from "./timing"

/** Longest line of output kept, in bytes; the rest of a longer line is dropped. */
export const MAX_LINE = 4096

/** Ports handed out to projects without a fixed one. */
export const AUTO_PORTS = { start: 4000, end: 4999 }

export interface ExitStatus {
  /** `null` when killed by a signal. */
  code: number | null
}

/**
 * Output lines waiting for the manager. Once it holds `limit` lines, the
 * streams feeding it pause, so a server that writes faster than blueprint
 * keeps up waits (as it would writing to a slow terminal) instead of memory
 * growing without bound.
 */
export class LogQueue {
  private items: { id: number; line: string }[] = []
  private paused = new Set<Readable>()

  constructor(readonly limit: number) {}

  push(id: number, line: string, from: Readable) {
    this.items.push({ id, line })
    if (this.items.length >= this.limit && !this.paused.has(from)) {
      from.pause()
      this.paused.add(from)
    }
  }

  /** Take up to `max` lines, oldest first, and let paused streams go on once there's room. */
  take(max: number): { id: number; line: string }[] {
    const out = this.items.splice(0, max)
    if (this.items.length < this.limit) {
      for (const s of this.paused) s.resume()
      this.paused.clear()
    }
    return out
  }
}

export class Running {
  readonly started = now()
  /** The user asked it to stop. */
  stopRequested = false
  /** When the tree was asked to exit, which starts the clock to SIGKILL. */
  private termSent?: number
  private killed?: number
  /** The shell's exit status, once {@link poll} has seen it exit. */
  private exited?: ExitStatus
  /** The exit status as reported, waiting for the next {@link poll}. */
  private reported?: ExitStatus

  constructor(
    private readonly child: ChildProcess,
    private readonly tree: Tree,
    /** The project as it was when started; edits apply on the next start. */
    readonly project: Project,
    /** The command that ran, with an empty one resolved. */
    readonly command: string,
    /** The port it was given in `PORT`. */
    readonly port: number,
  ) {
    const exited = (code: number | null) => (this.reported ??= { code })
    if (child.exitCode !== null || child.signalCode !== null) exited(child.exitCode)
    child.on("exit", (code) => exited(code))
  }

  get pid(): number {
    return this.child.pid ?? 0
  }

  /**
   * Check on the server. Its exit status comes back once the shell has exited
   * *and* nothing it started is left. A shell that exits leaving processes
   * behind (say, a server whose parent died) gets those asked to exit and
   * killed after the grace period, like a stop.
   */
  poll(): ExitStatus | undefined {
    this.exited ??= this.reported
    if (!this.exited) {
      this.enforceDeadline()
      return undefined
    }
    const gaveUp = this.killed !== undefined && now() - this.killed >= KILL_GRACE_MS
    if (!this.tree.alive() || gaveUp) {
      this.tree.release()
      return this.exited
    }
    if (this.termSent === undefined) {
      this.tree.terminate()
      this.termSent = now()
    }
    this.enforceDeadline()
    return undefined
  }

  /** The shell exited but processes it started are still being stopped. */
  lingering(): boolean {
    return this.exited !== undefined
  }

  /** On its way out: asked to stop, or its shell has exited. */
  stopping(): boolean {
    return this.termSent !== undefined || this.exited !== undefined
  }

  /**
   * Ask the whole tree to exit; signalling the group also catches
   * grandchildren (e.g. `sh` -> `npm run dev` -> `node`).
   */
  stop() {
    this.stopRequested = true
    if (this.termSent === undefined) {
      this.tree.terminate()
      this.termSent = now()
    }
  }

  /** Escalate to SIGKILL once the grace period has elapsed. */
  private enforceDeadline() {
    if (this.termSent !== undefined && this.killed === undefined && now() - this.termSent >= STOP_GRACE_MS) {
      this.tree.kill()
      this.killed = now()
    }
  }

  /** Kill the tree at once. */
  killNow() {
    this.tree.kill()
    this.killed = now()
    this.tree.release()
  }

  /** Stop it and keep checking in the background until it's gone. */
  stopInBackground() {
    this.stop()
    const timer = setInterval(() => {
      if (this.poll()) clearInterval(timer)
    }, 100)
  }
}

/**
 * Run `command` (shell syntax) in the project's folder with `PORT` set,
 * streaming its output to `queue`.
 */
export function spawn(project: Project, command: string, port: number, id: number, queue: LogQueue): Running {
  if (!isDir(project.path)) throw new Error(`folder not found: ${project.path}`)
  let sh: ReturnType<typeof shell>
  try {
    sh = shell(command)
  } catch (e) {
    throw new Error(`can't run this command: ${(e as Error).message}`)
  }
  let child: ChildProcess
  try {
    child = spawnChild(sh.file, sh.args, {
      ...sh.options,
      cwd: project.path,
      env: { ...process.env, PORT: String(port), FORCE_COLOR: "1" },
      stdio: ["ignore", "pipe", "pipe"],
    })
  } catch (e) {
    throw new Error(`failed to start: ${(e as Error).message}`)
  }
  if (child.pid === undefined) {
    // Spawn errors (like a missing shell) arrive as an event, and leave no pid.
    child.on("error", () => {})
    throw new Error(`failed to start: couldn't run ${sh.file}`)
  }
  child.on("error", () => {})
  let tree: Tree
  try {
    tree = adopt(child)
  } catch (e) {
    // Untracked, it could outlive blueprint, so it doesn't get to run.
    child.kill("SIGKILL")
    throw new Error(`failed to start: ${(e as Error).message}`)
  }
  for (const stream of [child.stdout, child.stderr]) if (stream) pipeLines(stream, id, queue)
  return new Running(child, tree, { ...project }, command, port)
}

function pipeLines(stream: Readable, id: number, queue: LogQueue) {
  const lines = new Lines()
  stream.on("data", (chunk: Buffer) => {
    for (const line of lines.feed(chunk)) queue.push(id, line, stream)
  })
  stream.on("end", () => {
    const last = lines.finish()
    if (last !== undefined) queue.push(id, last, stream)
  })
  stream.on("error", () => {})
}

const decoder = new TextDecoder("utf-8", { fatal: false })

/**
 * Splits a byte stream into display lines. A carriage return not followed by
 * a newline starts the line over, as a progress bar redrawing itself would
 * look; lines are cut at {@link MAX_LINE} bytes.
 */
export class Lines {
  private buf = new Uint8Array(MAX_LINE)
  private len = 0
  /** A `\r` was seen; what follows decides whether it ended the line. */
  private cr = false
  private cut = false

  feed(bytes: Uint8Array): string[] {
    const out: string[] = []
    for (const b of bytes) {
      if (b === 0x0d) {
        this.cr = true
      } else if (b === 0x0a) {
        this.cr = false
        out.push(this.take())
      } else {
        if (this.cr) {
          this.cr = false
          this.len = 0
          this.cut = false
        }
        if (this.len < MAX_LINE) this.buf[this.len++] = b
        else this.cut = true
      }
    }
    return out
  }

  finish(): string | undefined {
    return this.len > 0 ? this.take() : undefined
  }

  private take(): string {
    let line = decoder.decode(this.buf.subarray(0, this.len))
    if (this.cut) line += " …[cut]"
    this.cut = false
    this.len = 0
    return line
  }
}

export function portInUse(port: number): boolean {
  try {
    const listener = Bun.listen({ hostname: "127.0.0.1", port, socket: { data() {} } })
    listener.stop(true)
    return false
  } catch {
    return true
  }
}

/**
 * A free port from {@link AUTO_PORTS}, trying `preferred` first (so a project
 * keeps its port across restarts) and skipping `taken`.
 */
export function freePort(preferred: number | undefined, taken: number[]): number | undefined {
  const inRange = (p: number) => p >= AUTO_PORTS.start && p <= AUTO_PORTS.end
  const ok = (p: number) => inRange(p) && !taken.includes(p) && !portInUse(p)
  if (preferred !== undefined && ok(preferred)) return preferred
  for (let p = AUTO_PORTS.start; p <= AUTO_PORTS.end; p++) if (ok(p)) return p
  return undefined
}
