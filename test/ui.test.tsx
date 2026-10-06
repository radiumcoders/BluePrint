import { afterEach, expect, test } from "bun:test"
import { testRender } from "@opentui/react/test-utils"
import { join } from "node:path"
import { act } from "react"
import type { TestRendererSetup } from "@opentui/core/testing"
import { Manager } from "../src/core/manager"
import { App } from "../src/ui/app"
import { Board } from "../src/ui/board"
import { projectDir, tempDir } from "./testkit"

let current: TestRendererSetup | undefined

afterEach(() => {
  act(() => current?.renderer.destroy())
  current = undefined
})

/** The app on a test screen, with a projects root holding `shop` (node) and `api` (go). */
async function setup(opts: { projects?: boolean; width?: number; height?: number } = {}) {
  const root = projectDir("ui", {
    "shop/package.json": '{"name":"shop","scripts":{"dev":"next dev"}}',
    "api/go.mod": "module api",
  })
  const m = new Manager({ projectsRoot: root, projects: [] }, join(tempDir("ui-config"), "config.toml"))
  if (opts.projects) {
    m.add({ name: "shop", path: join(root, "shop"), port: 3000, command: "" })
    m.add({ name: "api", path: join(root, "api"), command: "go run ." })
    m.message = undefined
  }
  const copied: string[] = []
  const opened: string[] = []
  const board = new Board(m, {
    copy: (t) => (copied.push(t), true),
    openUrl: (u) => opened.push(u),
    exit() {},
  })
  const selections: string[] = []
  const t = await testRender(<App board={board} onCopy={(text) => (selections.push(text), true)} />, {
    width: opts.width ?? 100,
    height: opts.height ?? 30,
    useMouse: true,
  })
  current = t
  const frame = async () => {
    await act(async () => {
      await t.renderOnce()
    })
    return t.captureCharFrame()
  }
  const keys = async (...input: string[]) => {
    for (const k of input) {
      await act(async () => {
        if (k === "escape") {
          // A lone ESC could start a sequence, so the parser waits a moment for more.
          t.mockInput.pressEscape()
          await Bun.sleep(60)
        } else {
          t.mockInput.pressKey(k)
        }
        await t.renderOnce()
      })
    }
  }
  const type = async (text: string) => {
    await act(async () => {
      await t.mockInput.typeText(text)
      await t.renderOnce()
    })
  }
  /** Lines arriving in project `i`'s log, as process output would. */
  const log = async (i: number, ...lines: string[]) => {
    await act(async () => {
      for (const line of lines) m.entries[i]!.log(line)
      board.changed()
      await t.renderOnce()
    })
  }
  const mouse = async (fn: (mouse: TestRendererSetup["mockMouse"]) => Promise<void>) => {
    await act(async () => {
      await fn(t.mockMouse)
      await t.renderOnce()
    })
  }
  return { m, board, root, frame, keys, type, log, mouse, selections, copied, opened }
}

test("an empty board says how to start", async () => {
  const { frame } = await setup()
  const f = await frame()
  expect(f).toContain("◆ blueprint")
  expect(f).toContain("No projects yet.")
  expect(f).toContain("Press n to add one.")
  expect(f).toContain("n new project")
})

test("adding a project: pick a folder, name and command fill in, enter saves", async () => {
  const { m, frame, keys, type, root } = await setup()
  await keys("n")
  let f = await frame()
  expect(f).toContain("New project")
  expect(f).toContain("api")
  expect(f).toContain("shop")
  await type("sh")
  f = await frame()
  expect(f).not.toContain("› api")
  await keys("\r")
  f = await frame()
  expect(m.entries).toHaveLength(0)
  expect(f).toMatch(/Folder +\S+shop +node/)
  expect(f).toMatch(/Name +shop/)
  expect(f).toContain("runs npm run dev")
  await keys("\r")
  expect(m.entries.map((e) => e.project)).toEqual([
    { name: "shop", path: join(root, "shop"), port: undefined, command: "" },
  ])
  f = await frame()
  expect(f).not.toContain("New project")
  expect(f).toContain(" shop ")
  expect(f).toContain("Added shop")
})

test("form errors stay on their field", async () => {
  const { m, frame, keys, type } = await setup()
  await keys("n")
  await type("ap")
  await keys("\r", "\t")
  // Port: not a number.
  await type("abc")
  await keys("\r")
  const f = await frame()
  expect(f).toContain("Use a port from 1 to 65535")
  expect(m.entries).toHaveLength(0)
})

test("details, help and removal", async () => {
  const { m, frame, keys } = await setup({ projects: true })
  let f = await frame()
  expect(f).toContain("http://localhost:3000")
  // The board: port, name, status and command on one line.
  expect(f).toMatch(/3000 +shop +○ STOPPED +npm run dev/)
  expect(f).toMatch(/auto +api +○ STOPPED +go run \./)
  expect(f).toContain("Not running. Press enter to start it.")

  await keys("?")
  f = await frame()
  expect(f).toContain("Keys")
  expect(f).toContain("start / stop")
  await keys("x") // any key closes help
  expect(await frame()).not.toContain("start / stop")

  await keys("j")
  f = await frame()
  expect(f).toContain("go run .")
  await keys("d")
  expect(await frame()).toContain("Remove api?")
  await keys("d")
  expect(m.entries.map((e) => e.project.name)).toEqual(["shop"])
})

test("the log filter shows matching lines, and esc clears it", async () => {
  const { frame, keys, type, log, copied } = await setup({ projects: true })
  await log(0, "GET /", "\x1b[31merror\x1b[0m: boom", "GET /about", "ready in 20ms")
  let f = await frame()
  expect(f).toContain("ready in 20ms")
  expect(f).toContain("4 lines")

  await keys("/")
  await type("get")
  f = await frame()
  expect(f).toContain("GET /about")
  expect(f).not.toContain("ready in 20ms")
  expect(f).toContain("2 of 4 lines")

  await keys("\r", "y")
  expect(copied).toEqual(["GET /\nGET /about\n"])

  await keys("escape")
  f = await frame()
  expect(f).toContain("ready in 20ms")
  expect(f).toContain("error: boom")
})

test("the console follows new lines until scrolled up", async () => {
  const { board, frame, keys, log } = await setup({ projects: true, height: 24 })
  await log(0, ...Array.from({ length: 200 }, (_, i) => `line ${i}`))
  let f = await frame()
  expect(f).toContain("line 199")
  await keys("g") // oldest
  f = await frame()
  expect(f).toContain("line 0 ")
  expect(f).toContain("end ↓ latest")
  await log(0, "line 200")
  f = await frame()
  expect(f).not.toContain("line 200")
  await keys("G") // newest, and follow again
  await log(0, "line 201")
  f = await frame()
  expect(f).toContain("line 201")
  expect(board.follow).toBe(true)
})

test("narrow terminals still lay out", async () => {
  const { frame } = await setup({ projects: true, width: 60, height: 16 })
  const f = await frame()
  expect(f).toMatch(/3000 +shop +○ STOPPED/)
  expect(f).toContain("── shop ── http://localhost:3000")
})

test("the mouse: clicking selects, the wheel scrolls, dragging copies", async () => {
  const { board, m, frame, log, mouse, selections, opened } = await setup({ projects: true, height: 24 })
  await frame()
  // The second project's row: under the top line, a gap, the column heads and the first.
  await mouse((mm) => mm.click(5, 4))
  expect(board.selected).toBe(m.entries[1]!.id)
  expect(selections).toEqual([])
  expect(await frame()).not.toContain("Copied")

  await mouse((mm) => mm.click(5, 3))
  await log(0, ...Array.from({ length: 100 }, (_, i) => `line ${i}`))
  await mouse((mm) => mm.scroll(60, 15, "up"))
  expect(board.follow).toBe(false)
  await mouse((mm) => mm.scroll(60, 15, "down"))
  await mouse((mm) => mm.scroll(60, 15, "down"))
  expect(board.follow).toBe(true)

  await mouse((mm) => mm.drag(2, 10, 12, 10))
  expect(selections.length).toBe(1)
  expect(selections[0]!.trim()).not.toBe("")
  expect(opened).toEqual([])
})

test("a long project list keeps the selection in view", async () => {
  const { board, m, frame, keys } = await setup({ height: 12 })
  for (let i = 0; i < 20; i++) m.add({ name: `p${String(i).padStart(2, "0")}`, path: tempDir("many"), command: "x" })
  board.selected = m.entries[0]!.id
  await keys(...Array<string>(15).fill("j"))
  const f = await frame()
  expect(f).toContain("p15")
  expect(f).not.toContain("p00")
  expect(f).toContain("16 of 20")
})
