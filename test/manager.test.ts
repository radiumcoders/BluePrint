import { describe, expect, test } from "bun:test"
import { writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { parse } from "../src/core/ansi"
import { defaultConfig, type Project } from "../src/core/config"
import { LOG_QUEUE, Manager, MAX_LOG_BYTES, MAX_LOG_LINES } from "../src/core/manager"
import { MAX_LINE, portInUse, STOP_GRACE_MS } from "../src/core/process"
import { addHelper, logged, manager, tempDir, waitUntil } from "./testkit"

const SOON = 15_000

describe("projects", () => {
  test("validation", () => {
    const m = manager("validate")
    const tmp = tmpdir()
    m.add({ name: "taken", path: tmp, port: 3000, command: "" })
    const field = (name: string, port: string) => {
      const r = m.validate(undefined, name, tmp, port, "serve")
      return r.ok ? r.project : r.field
    }
    expect(field("taken", "")).toBe("name")
    expect(field("Bad Name", "")).toBe("name")
    expect(field("fresh", "3000")).toBe("port")
    expect(field("fresh", "99999")).toBe("port")
    expect(field("fresh", "3001")).toMatchObject({ port: 3001 })
    expect(m.validate(undefined, "fresh", undefined, "", "")).toMatchObject({ field: "folder" })
    // No dev script in the folder, so an empty command can't run.
    expect(m.validate(undefined, "fresh", tempDir("empty"), "", "")).toMatchObject({ field: "command" })
    // Editing a project may keep its own name and port.
    expect(m.validate(m.entries[0]!.id, "taken", tmp, "3000", "pnpm dev").ok).toBe(true)
  })

  test("urls follow the port", () => {
    const m = manager("urls")
    const tmp = tmpdir()
    m.add({ name: "a", path: tmp, port: 3000, command: "" })
    m.add({ name: "b", path: tmp, command: "" })
    expect(m.entries[0]!.url()).toBe("http://localhost:3000")
    // An auto port only counts while the server runs.
    m.entries[1]!.appPort = 4001
    expect(m.entries[1]!.url()).toBeUndefined()
    expect(m.takenPorts(1)).toEqual([3000])
  })

  test("reordering is saved", () => {
    const m = manager("order")
    const a = m.add({ name: "a", path: tmpdir(), command: "x" })
    m.add({ name: "b", path: tmpdir(), command: "x" })
    m.moveBy(a, 1)
    expect(m.entries.map((e) => e.project.name)).toEqual(["b", "a"])
    expect(m.config.projects.map((p) => p.name)).toEqual(["b", "a"])
  })

  test("a failed write isn't reported as saved", () => {
    // The config's parent is a file, so the folder can't be created.
    const blocker = join(tempDir("blocker"), "file")
    writeFileSync(blocker, "")
    const m = new Manager(defaultConfig(), join(blocker, "config.toml"))
    const project = (name: string): Project => ({ name, path: tmpdir(), command: "x" })
    const isError = () => m.message?.kind === "error" && m.message.text.startsWith("Couldn't save")
    const id = m.add(project("a"))
    expect(isError()).toBe(true)
    m.message = undefined
    m.update(id, project("b"))
    expect(isError()).toBe(true)
    m.message = undefined
    m.remove(id)
    expect(isError()).toBe(true)

    // And a successful one is.
    const ok = manager("saved")
    ok.add(project("a"))
    expect(ok.message).toMatchObject({ kind: "info" })
    expect(ok.message!.text.startsWith("Added a")).toBe(true)
  })
})

describe("servers", () => {
  /** Starts a server and waits until it answers on the port it was given in `PORT`. */
  test("runs a server on its port", async () => {
    const m = manager("run")
    const id = addHelper(m, "web", "serve", 4900)
    m.start(id)
    await waitUntil(m, id, SOON, "running", (m) => m.get(id)!.status() === "running")
    const e = m.get(id)!
    expect(e.port()).toBe(4900)
    expect(e.url()).toBe("http://localhost:4900")
    expect(e.logs[0]!.startsWith("$ PORT=4900 BLUEPRINT_HELPER=serve")).toBe(true)
    expect(await (await fetch("http://127.0.0.1:4900")).text()).toBe("ok")
    await m.shutdown()
    expect(m.runningCount()).toBe(0)
  })

  /** Edits to a running project wait for a restart: until then its URL is the port it really listens on. */
  test("edits apply on restart", async () => {
    const m = manager("edit")
    const id = addHelper(m, "web", "serve", 4910)
    m.start(id)
    await waitUntil(m, id, SOON, "running", (m) => m.get(id)!.status() === "running")
    m.update(id, { ...m.get(id)!.project, port: 4911 })
    const e = m.get(id)!
    expect(e.editedWhileRunning()).toBe(true)
    expect(e.url()).toBe("http://localhost:4910")
    expect(m.message!.text).toContain("Restart web")
    m.restart(id)
    await waitUntil(m, id, SOON, "the restart", (m) => m.get(id)!.port() === 4911 && m.get(id)!.status() === "running")
    expect(m.get(id)!.editedWhileRunning()).toBe(false)
    await m.shutdown()
  })

  /**
   * A shell that exits leaving a server behind that ignores SIGTERM: the
   * server is killed at the deadline, and the project shows "stopping" until
   * it's gone.
   */
  test("stops what the command leaves behind", async () => {
    const m = manager("orphan")
    const id = addHelper(m, "orphan", "orphan", 4920)
    m.start(id)
    await waitUntil(m, id, SOON, "the shell to exit", (m) => logged(m, id, "stopping what it left running"))
    const line = m.get(id)!.logs.find((l) => l.startsWith("child "))
    expect(line).toBeDefined()
    const child = Number(line!.slice(6))
    expect(m.get(id)!.isActive()).toBe(true)
    expect(m.get(id)!.status()).toBe("stopping")
    const started = performance.now()
    await waitUntil(m, id, STOP_GRACE_MS + 4000, "the leftover to be killed", (m) => !m.get(id)!.isActive())
    expect(performance.now() - started).toBeLessThan(STOP_GRACE_MS + 4000)
    // Gone, not just untracked (allowing a moment for it to be reaped).
    const alive = () => {
      try {
        process.kill(child, 0)
        return true
      } catch {
        return false
      }
    }
    const deadline = performance.now() + 2000
    while (alive() && performance.now() < deadline) await Bun.sleep(20)
    expect(alive()).toBe(false)
  }, 20_000)

  /** Far more output than the log keeps: memory stays bounded, a huge line is cut, and the newest lines survive. */
  test("output floods stay bounded", async () => {
    const m = manager("flood")
    const id = addHelper(m, "flood", "flood", 4930)
    m.start(id)
    let biggestBatch = 0
    const deadline = performance.now() + 60_000
    while (m.get(id)!.isActive() || !logged(m, id, "flood done")) {
      expect(performance.now()).toBeLessThan(deadline)
      const before = m.get(id)!.logRev
      m.tick()
      biggestBatch = Math.max(biggestBatch, m.get(id)!.logRev - before)
      await Bun.sleep(5)
    }
    // One tick never takes in more than a queue's worth (plus a status line or two).
    expect(biggestBatch).toBeLessThanOrEqual(LOG_QUEUE + 4)
    const e = m.get(id)!
    expect(e.logs.length).toBeLessThanOrEqual(MAX_LOG_LINES)
    expect(e.logBytes).toBeLessThanOrEqual(MAX_LOG_BYTES)
    expect(e.logBytes).toBe(e.logs.reduce((n, l) => n + l.length, 0))
    const huge = e.logs.find((l) => l.startsWith("xxxx"))!
    expect(huge.length).toBeLessThanOrEqual(MAX_LINE + 16)
    expect(huge.endsWith("[cut]")).toBe(true)
    const last = e.logs.findIndex((l) => l.startsWith("line 19999 "))
    expect(last).toBeGreaterThanOrEqual(0)
    // Dropped lines are counted, so line numbers stay stable.
    expect(e.logStart).toBeGreaterThan(0)
    expect(last + e.logStart).toBeGreaterThanOrEqual(20_000)
  }, 70_000)

  /** Removing a running project stops its server without blocking. */
  test("removing stops the server", async () => {
    const m = manager("remove")
    const id = addHelper(m, "web", "serve", 4945)
    m.start(id)
    await waitUntil(m, id, SOON, "running", (m) => m.get(id)!.status() === "running")
    const started = performance.now()
    m.remove(id)
    expect(performance.now() - started).toBeLessThan(500)
    expect(m.get(id)).toBeUndefined()
    const deadline = performance.now() + STOP_GRACE_MS + 2000
    while (portInUse(4945) && performance.now() < deadline) await Bun.sleep(50)
    expect(portInUse(4945)).toBe(false)
  }, 15_000)

  /** Invalid UTF-8 and broken escapes come through as lines and parse without throwing. */
  test("malformed output", async () => {
    const m = manager("malformed")
    const id = addHelper(m, "bytes", "malformed", 4940)
    m.start(id)
    await waitUntil(m, id, SOON, "exit", (m) => !m.get(id)!.isActive() && logged(m, id, "malformed done"))
    const e = m.get(id)!
    expect(e.logs.some((l) => l.includes("�� not utf-8"))).toBe(true)
    expect(e.logs).toContain("progress 100%")
    for (const line of e.logs) parse(line)
    expect(e.status()).toBe("stopped")
  })

  /** A server that stops accepting connections is noticed. */
  test("notices a server that stops responding", async () => {
    const m = manager("unresponsive")
    const id = addHelper(m, "flaky", "serve-briefly", 4950)
    m.start(id)
    await waitUntil(m, id, SOON, "running", (m) => m.get(id)!.status() === "running")
    await waitUntil(m, id, SOON, "not responding", (m) => m.get(id)!.status() === "unresponsive")
    expect(logged(m, id, "port 4950 stopped accepting connections")).toBe(true)
    await m.shutdown()
  }, 35_000)

  test("a fixed port that's taken is refused", async () => {
    const m = manager("busy")
    const held = Bun.listen({ hostname: "127.0.0.1", port: 4955, socket: { data() {} } })
    const id = m.add({ name: "busy", path: tmpdir(), port: 4955, command: "x" })
    m.start(id)
    expect(m.get(id)!.isActive()).toBe(false)
    expect(m.message).toMatchObject({ kind: "error" })
    expect(m.message!.text).toContain("Port 4955 is already in use")
    held.stop(true)
  })
})
