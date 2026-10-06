// End-to-end checks of start recipes with real toolchains: detect the
// project, start it, get an HTTP answer on the port blueprint picked, and find
// the port free again after shutdown.
//
// They need Node, Python and Go on PATH, so they only run with
// BLUEPRINT_E2E=1 (CI sets it). A missing tool then fails the test instead of
// skipping it.

import { describe, expect, test } from "bun:test"
import { chmodSync, rmSync } from "node:fs"
import { join } from "node:path"
import { portInUse } from "../src/core/process"
import { detect, devScriptCommand, PYTHON } from "../src/core/stack"
import { manager, projectDir, waitUntil } from "./testkit"

const enabled = process.env.BLUEPRINT_E2E === "1"

function needs(tool: string) {
  if (!Bun.which(tool)) throw new Error(`${tool} isn't on PATH; these end-to-end tests need it`)
}

/**
 * Detect, start, answer, stop. `command` is what detection should suggest
 * ("" for the package.json dev script); `body` is expected in the reply.
 * `port` is offered first, so tests don't race for one.
 */
async function run(name: string, dir: string, command: string, body: string, port: number, startupMs: number) {
  const recipe = detect(dir)
  expect(recipe?.command).toBe(command)
  const m = manager(name)
  const id = m.add({ name, path: dir, command: recipe!.command })
  m.get(id)!.appPort = port
  m.start(id)
  await waitUntil(m, id, startupMs, "the server to accept connections", (m) => m.get(id)!.status() === "running")
  const actual = m.get(id)!.port()!
  const reply = await (await fetch(`http://127.0.0.1:${actual}/`)).text()
  expect(reply).toContain(body)

  await m.shutdown()
  expect(m.runningCount()).toBe(0)
  const deadline = performance.now() + 5000
  while (portInUse(actual) && performance.now() < deadline) await Bun.sleep(50)
  expect(portInUse(actual)).toBe(false)
  rmSync(dir, { recursive: true, force: true })
}

const NODE_SERVER = `
const port = Number(process.argv.includes("--port") ? process.argv[process.argv.indexOf("--port") + 1] : process.env.PORT);
require("http").createServer((_, res) => res.end("hello from node")).listen(port, "127.0.0.1");
`

describe.skipIf(!enabled)("end to end", () => {
  test("a node dev script that reads PORT", async () => {
    needs("node")
    needs("npm")
    const dir = projectDir("e2e-node", {
      "package.json": '{"name":"e2e","scripts":{"dev":"node server.js"}}',
      "server.js": NODE_SERVER,
    })
    await run("node", dir, "", "hello from node", 4960, 30_000)
  }, 40_000)

  /** A dev script whose tool ignores `PORT` and must be passed `--port`, the way Vite is. */
  test("a dev tool given --port", async () => {
    needs("node")
    needs("npm")
    const dir = projectDir("e2e-vite", {
      "package.json": '{"name":"e2e","scripts":{"dev":"vite"}}',
      "fake-vite.js": NODE_SERVER.replace("process.env.PORT", "NaN"),
      "node_modules/.bin/vite": '#!/bin/sh\nexec node "$(dirname "$0")/../../fake-vite.js" "$@"\n',
      "node_modules/.bin/vite.cmd": '@node "%~dp0\\..\\..\\fake-vite.js" %*\r\n',
    })
    if (process.platform !== "win32") chmodSync(join(dir, "node_modules/.bin/vite"), 0o755)
    expect(devScriptCommand(dir)).toBe("npm run dev -- --port $PORT --strictPort")
    await run("vite", dir, "", "hello from node", 4965, 30_000)
  }, 40_000)

  test("a static site", async () => {
    needs(PYTHON)
    const dir = projectDir("e2e-static", { "index.html": "<p>hello from a static site</p>" })
    await run("static", dir, `${PYTHON} -m http.server $PORT --bind 127.0.0.1`, "hello from a static site", 4970, 30_000)
  }, 40_000)

  test("a go module that reads PORT", async () => {
    needs("go")
    const dir = projectDir("e2e-go", {
      "go.mod": "module e2e\n\ngo 1.21\n",
      "main.go": `package main

import (
	"net/http"
	"os"
)

func main() {
	http.HandleFunc("/", func(w http.ResponseWriter, _ *http.Request) { w.Write([]byte("hello from go")) })
	http.ListenAndServe("127.0.0.1:"+os.Getenv("PORT"), nil)
}
`,
    })
    // \`go run\` compiles first, which can take a while on a cold cache.
    await run("go", dir, "go run .", "hello from go", 4975, 180_000)
  }, 200_000)
})
