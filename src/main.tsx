#!/usr/bin/env bun
import { createCliRenderer } from "@opentui/core"
import { createRoot } from "@opentui/react"
import pkg from "../package.json"
import { installedTools, openIn } from "./core/agents"
import { configPath, loadConfig } from "./core/config"
import * as guardian from "./core/guardian"
import { Manager } from "./core/manager"
import * as platform from "./core/platform"
import { STOP_GRACE_MS } from "./core/timing"
import { App } from "./ui/app"
import { Board } from "./ui/board"
import { applyTerminalColors, theme } from "./ui/theme"

const args = process.argv.slice(2)
if (args[0] === guardian.FLAG) await guardian.run()

if (args.includes("-h") || args.includes("--help")) {
  console.log(
    `blueprint ${pkg.version}\nRun many dev servers at once, each on its own port.\n\n` +
      `Usage: blueprint\n\nConfig: ${configPath()}\n(override with BLUEPRINT_CONFIG=/path/to/config.toml)`,
  )
  process.exit(0)
}
if (args.includes("-V") || args.includes("--version")) {
  console.log(`blueprint ${pkg.version}`)
  process.exit(0)
}
if (!process.stdin.isTTY || !process.stdout.isTTY) {
  console.error("blueprint: needs to run in a terminal")
  process.exit(1)
}

const path = configPath()
let manager: Manager
try {
  manager = new Manager(loadConfig(path), path)
} catch (e) {
  console.error(`blueprint: ${(e as Error).message}`)
  process.exit(1)
}

await platform.init()
guardian.start()

const renderer = await createCliRenderer({
  exitOnCtrlC: false,
  exitSignals: [],
  useMouse: true,
  targetFps: 30,
  backgroundColor: theme.bg,
})

let exiting = false
function exit(code: number) {
  if (exiting) return
  exiting = true
  renderer.destroy()
  process.exit(code)
}

const copy = (text: string) => renderer.copyToClipboardOSC52(text)
const board = new Board(manager, {
  copy,
  openUrl: platform.openUrl,
  tools: () => installedTools(),
  openIn,
  exit: () => exit(0),
})

// Closing the terminal or a kill stops every server on the way out. If that
// stalls, exit anyway; the guardian (or the job objects) stop the servers.
for (const sig of ["SIGHUP", "SIGTERM", "SIGINT"] as const) {
  process.on(sig, () => {
    setTimeout(() => exit(130), STOP_GRACE_MS + 2000).unref()
    void board.quit()
  })
}
process.on("uncaughtException", (e) => {
  renderer.destroy()
  console.error("blueprint crashed:", e)
  process.exit(1)
})

// The quiet fills are mixed from the terminal's own colors. Ask for them now,
// and again when its theme may have changed: OpenTUI re-asks on a light/dark
// switch, and coming back to the window catches a switch between two themes.
renderer.on("palette", (colors) => {
  applyTerminalColors(colors)
  board.changed()
})
const askPalette = () => renderer.getPalette({ timeout: 1000 }).catch(() => {})
renderer.on("focus", () => {
  renderer.clearPaletteCache()
  void askPalette()
})
void askPalette()

setInterval(() => board.tick(), 100)
createRoot(renderer).render(<App board={board} onCopy={copy} />)
