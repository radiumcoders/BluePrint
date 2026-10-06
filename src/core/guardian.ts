//! Cleanup watchdog so dev servers never outlive blueprint (Unix; on Windows
//! job objects do the same).
//!
//! blueprint starts itself again as `blueprint --guardian` with a pipe on
//! stdin and reports each server's process group as `+<pgid>` / `-<pgid>`
//! lines. When blueprint exits for any reason (normal quit, terminal closed,
//! crash, SIGKILL) the pipe closes, and the guardian SIGTERMs every group it
//! still knows about, then SIGKILLs whatever survives the grace period.

import { spawn } from "node:child_process"
import type { Writable } from "node:stream"
import { STOP_GRACE_MS } from "./timing"

export const FLAG = "--guardian"

let pipe: Writable | undefined

/** The command line that runs this program again, compiled or not. */
export function selfCommand(): [string, string[]] {
  const compiled = Bun.main.startsWith("/$bunfs/") || /^[A-Za-z]:[\\/]~BUN[\\/]/.test(Bun.main)
  return compiled ? [process.execPath, []] : [process.execPath, [Bun.main]]
}

/** Start the guardian. Failure is non-fatal: a normal quit still stops servers. */
export function start(): void {
  if (process.platform === "win32") return
  const [file, args] = selfCommand()
  try {
    const child = spawn(file, [...args, FLAG], {
      stdio: ["pipe", "ignore", "ignore"],
      // A session of its own, so signals aimed at blueprint's terminal miss it.
      detached: true,
    })
    child.on("error", () => {})
    child.stdin?.on("error", () => {})
    pipe = child.stdin ?? undefined
    child.unref()
  } catch {}
}

function send(line: string) {
  pipe?.write(`${line}\n`)
}

export const watch = (pgid: number) => send(`+${pgid}`)
export const unwatch = (pgid: number) => send(`-${pgid}`)

/** Entry point for `blueprint --guardian`. */
export async function run(): Promise<never> {
  for (const sig of ["SIGHUP", "SIGINT", "SIGTERM"] as const) process.on(sig, () => {})
  const { groupAlive } = await import("./platform")
  const groups = new Set<number>()
  let buffered = ""
  for await (const chunk of process.stdin) {
    buffered += chunk.toString()
    const lines = buffered.split("\n")
    buffered = lines.pop() ?? ""
    for (const line of lines) {
      const pgid = Number.parseInt(line.slice(1), 10)
      if (!Number.isFinite(pgid) || pgid <= 0) continue
      if (line.startsWith("+")) groups.add(pgid)
      else if (line.startsWith("-")) groups.delete(pgid)
    }
  }

  // blueprint is gone.
  const signal = (sig: NodeJS.Signals) => {
    for (const g of groups) {
      try {
        process.kill(-g, sig)
      } catch {}
    }
  }
  signal("SIGTERM")
  const deadline = performance.now() + STOP_GRACE_MS
  while (performance.now() < deadline && [...groups].some(groupAlive)) await Bun.sleep(100)
  signal("SIGKILL")
  process.exit(0)
}
