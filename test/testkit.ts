// Test helpers: stand-in dev servers, temporary folders and waiting on a Manager.

import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { defaultConfig } from "../src/core/config"
import { Manager, type Id } from "../src/core/manager"

const HELPER = join(import.meta.dir, "helper.ts")

/** A fresh temporary folder. */
export function tempDir(name: string): string {
  return mkdtempSync(join(tmpdir(), `blueprint-${name}-`))
}

/** A folder holding `files` (path -> contents). */
export function projectDir(name: string, files: Record<string, string>): string {
  const dir = tempDir(name)
  for (const [file, body] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, file)), { recursive: true })
    writeFileSync(join(dir, file), body)
  }
  return dir
}

/** A manager with its config in a fresh temporary folder. */
export function manager(name: string): Manager {
  return new Manager(defaultConfig(), join(tempDir(name), "config.toml"))
}

/**
 * A shell command that runs the stand-in in `mode`. The `NAME=value` prefix
 * also exercises the Windows translation of shell syntax.
 */
export function helperCommand(mode: string): string {
  return `BLUEPRINT_HELPER=${mode} "${process.execPath}" "${HELPER}"`
}

/** A project running the stand-in in `mode`, first offered `port` (so tests don't race for one). */
export function addHelper(m: Manager, name: string, mode: string, port: number): Id {
  const id = m.add({ name, path: tmpdir(), command: helperCommand(mode) })
  m.get(id)!.appPort = port
  return id
}

/** Tick `m` until `done` holds, or throw with the project's logs after `timeoutMs`. */
export async function waitUntil(m: Manager, id: Id, timeoutMs: number, what: string, done: (m: Manager) => boolean) {
  const deadline = performance.now() + timeoutMs
  while (!done(m)) {
    if (performance.now() > deadline) {
      throw new Error(`timed out waiting for ${what}; logs:\n${m.get(id)?.logs.join("\n")}`)
    }
    await Bun.sleep(20)
    m.tick()
  }
}

export function logged(m: Manager, id: Id, needle: string): boolean {
  return m.get(id)?.logs.some((l) => l.includes(needle)) ?? false
}
