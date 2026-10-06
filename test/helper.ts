// The stand-in dev server the tests start, behaving as `BLUEPRINT_HELPER` says.
// It needs nothing installed beyond Bun, and can misbehave on purpose.

import { spawn } from "node:child_process"
import { createServer } from "node:http"

/** How long a stand-in lives at most, so a failed test can't leave one behind. */
const LIFETIME_MS = 60_000

const out = (data: string | Uint8Array) => Bun.write(Bun.stdout, data)

function listen(ms: number): Promise<void> {
  const port = Number(process.env.PORT)
  const server = createServer((_, res) => res.end("ok")).listen(port, "127.0.0.1")
  return new Promise((resolve) => {
    server.on("listening", () => {
      console.log(`listening on 127.0.0.1:${port}`)
      setTimeout(() => server.close(() => resolve()), ms)
    })
  })
}

const ignoreSigterm = () => process.on("SIGTERM", () => {})

switch (process.env.BLUEPRINT_HELPER) {
  // Answer HTTP on $PORT.
  case "serve":
    await listen(LIFETIME_MS)
    break
  // Answer for a moment, then stop listening but keep running.
  case "serve-briefly":
    await listen(1000)
    await Bun.sleep(LIFETIME_MS)
    break
  // Far more output than the log keeps, including one huge line.
  case "flood": {
    let text = ""
    for (let i = 0; i < 20_000; i++) text += `line ${i} ${"-".repeat(60)}\n`
    await out(text + "x".repeat(1_000_000) + "\nflood done\n")
    break
  }
  // Bytes no terminal would be proud of.
  case "malformed": {
    const enc = new TextEncoder()
    await out(
      new Uint8Array([
        0xff,
        0xfe,
        ...enc.encode(" not utf-8\n"),
        ...enc.encode("\x1b[31mred then a lone ESC\x1b\n"),
        ...enc.encode("ESC before a multibyte char: \x1bé\n"),
        ...enc.encode("\x1b]8;;an unterminated link\n"),
        ...enc.encode("progress 10%\rprogress 100%\n"),
        ...enc.encode("malformed done\n"),
      ]),
    )
    break
  }
  // Leave a child behind that ignores SIGTERM, and exit at once.
  case "orphan": {
    ignoreSigterm()
    const child = spawn(process.execPath, [import.meta.path], {
      env: { ...process.env, BLUEPRINT_HELPER: "stubborn" },
      stdio: "inherit",
    })
    child.unref()
    await out(`child ${child.pid}\n`)
    process.exit(0)
  }
  // eslint-disable-next-line no-fallthrough
  case "stubborn":
    ignoreSigterm()
    await Bun.sleep(LIFETIME_MS)
    break
  default:
    console.error(`unknown helper mode ${process.env.BLUEPRINT_HELPER}`)
    process.exit(2)
}
process.exit(0)
