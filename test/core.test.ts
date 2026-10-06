import { describe, expect, test } from "bun:test"
import { readFileSync, rmSync, writeFileSync } from "node:fs"
import { join } from "node:path"
import { parse, strip } from "../src/core/ansi"
import { loadConfig, packageName, saveConfig, slugify, validateName, type Config } from "../src/core/config"
import { fuzzy, listFolders } from "../src/core/folders"
import { fmtDuration, LogIndex } from "../src/core/logindex"
import { AUTO_PORTS, freePort, Lines, MAX_LINE } from "../src/core/process"
import { detect, devScriptCommand, PYTHON } from "../src/core/stack"
import { translate } from "../src/core/wincmd"
import { manager, projectDir, tempDir } from "./testkit"

describe("config", () => {
  test("slugify", () => {
    expect(slugify("My Cool_App")).toBe("my-cool-app")
    expect(slugify("@acme/web")).toBe("web")
    expect(slugify("api.shop")).toBe("api.shop")
    expect(slugify("--x--")).toBe("x")
    expect(slugify("a..b")).toBe("a.b")
  })

  test("names", () => {
    expect(validateName("api.shop")).toBeUndefined()
    expect(validateName("my-app2")).toBeUndefined()
    for (const bad of ["", "Bad", "a..b", "-a"]) expect(validateName(bad)).toBeString()
  })

  test("package names", () => {
    expect(packageName('{ "name" : "@x/y", "version": "1" }')).toBe("@x/y")
    expect(packageName("{}")).toBeUndefined()
    // Only the top-level name counts, not one nested earlier in the file.
    expect(packageName('{ "author": { "name": "me" }, "name": "app" }')).toBe("app")
    expect(packageName('{ "name": "a\\"b" }')).toBe('a"b')
    expect(packageName('{ "name": 3 }')).toBeUndefined()
    expect(packageName("not json")).toBeUndefined()
  })

  test("round trip, and files written by the Rust version", () => {
    const dir = tempDir("config")
    const path = join(dir, "config.toml")
    const cfg: Config = {
      projectsRoot: "/tmp",
      projects: [
        { name: "a", path: "/tmp/a", port: 3000, command: "pnpm dev" },
        { name: "b", path: "/tmp/b", port: undefined, command: "" },
      ],
    }
    saveConfig(cfg, path)
    expect(loadConfig(path)).toEqual(cfg)
    const text = readFileSync(path, "utf8")
    expect(text).toContain('projects_root = "/tmp"')
    expect(text).not.toContain("command = \"\"")
    // Earlier configs still load; unknown keys like portless' proxy_port are ignored.
    writeFileSync(path, 'proxy_port = 443\nprojects_root = "~/x"\n\n[[projects]]\nname = "s"\npath = "~/s"\nport = 3000\n')
    const old = loadConfig(path)
    expect(old.projects[0]).toMatchObject({ name: "s", port: 3000, command: "" })
    expect(old.projects[0]!.path.startsWith("~")).toBe(false)
    rmSync(dir, { recursive: true })
  })
})

describe("stack detection", () => {
  const command = (files: Record<string, string>) => {
    const dir = projectDir("stack", files)
    const r = detect(dir)?.command
    rmSync(dir, { recursive: true })
    // Every suggestion has to run on Windows too.
    if (r !== undefined) expect(translate(r).ok).toBe(true)
    return r
  }

  test("node", () => {
    expect(command({ "package.json": '{"scripts":{"dev":"vite"}}' })).toBe("")
    expect(command({ "package.json": '{"scripts":{"start":"node ."}}', "pnpm-lock.yaml": "" })).toBe("pnpm start")
    // "dev" outside scripts doesn't count.
    expect(command({ "package.json": '{"name":"dev"}' })).toBeUndefined()
  })

  test("dev scripts", () => {
    const run = (files: Record<string, string>) => {
      const dir = projectDir("dev", files)
      const r = devScriptCommand(dir)
      rmSync(dir, { recursive: true })
      return r
    }
    expect(run({ "package.json": '{"scripts":{"dev":"next dev"}}' })).toBe("npm run dev")
    expect(run({ "package.json": '{"scripts":{"dev":"vite"}}' })).toBe("npm run dev -- --port $PORT --strictPort")
    expect(run({ "package.json": '{"scripts":{"dev":"NODE_ENV=dev astro dev"}}', "pnpm-lock.yaml": "" })).toBe(
      "pnpm dev --port $PORT",
    )
    expect(run({ "package.json": '{"scripts":{"dev":"vite dev"}}', "bun.lock": "" })).toBe(
      "bun run dev --port $PORT --strictPort",
    )
    expect(run({ "package.json": '{"scripts":{"build":"vite build"}}' })).toBeUndefined()
  })

  test("rust", () => {
    expect(command({ "Cargo.toml": "[package]" })).toBe("cargo run")
    expect(command({ "Cargo.toml": "[package]", "Trunk.toml": "" })).toBe("trunk serve --port $PORT")
    expect(command({ "Cargo.toml": "[package]", "Dioxus.toml": "" })).toBe("dx serve --port $PORT")
    expect(command({ "Cargo.toml": '[package.metadata.leptos]\nsite-addr = "x"' })).toBe(
      "LEPTOS_SITE_ADDR=127.0.0.1:$PORT cargo leptos watch",
    )
  })

  test("other stacks", () => {
    expect(command({ "go.mod": "module x" })).toBe("go run .")
    expect(command({ "manage.py": "", "requirements.txt": "django", "uv.lock": "" })).toBe(
      "uv run python manage.py runserver $PORT",
    )
    expect(command({ "pyproject.toml": "fastapi", "main.py": "" })).toBe("uvicorn main:app --reload --port $PORT")
    expect(command({ Gemfile: "", "bin/rails": "" })).toBe(
      process.platform === "win32" ? "ruby bin/rails server -p $PORT" : "bin/rails server -p $PORT",
    )
    expect(command({ artisan: "" })).toBe("php artisan serve --port=$PORT")
    expect(command({ "index.html": "<p>" })).toBe(`${PYTHON} -m http.server $PORT --bind 127.0.0.1`)
    expect(command({ "README.md": "" })).toBeUndefined()
  })
})

describe("folders", () => {
  test("fuzzy", () => {
    expect(fuzzy("bpt", "blueprint")).toBe(true)
    expect(fuzzy("BP", "blueprint")).toBe(true)
    expect(fuzzy("xyz", "blueprint")).toBe(false)
    expect(fuzzy("", "anything")).toBe(true)
  })

  test("lists and tags", () => {
    const root = projectDir("folders", { "beta/package.json": "{}", "Alpha/.keep": "", ".hidden/.keep": "" })
    expect(listFolders(root).map((f) => [f.name, f.tags])).toEqual([
      ["Alpha", []],
      ["beta", ["node"]],
    ])
    rmSync(root, { recursive: true })
  })
})

describe("ansi", () => {
  test("strips everything but SGR", () => {
    expect(strip("\x1b[2J\x1b[Hhello\x1b]8;;http://x\x07link\x1b]8;;\x07")).toBe("hellolink")
    expect(strip("a\tb")).toBe("a    b")
  })

  test("styled runs", () => {
    const runs = parse("\x1b[1;32mok\x1b[0m done \x1b[38;2;1;2;3mrgb")
    expect(runs.map((r) => r.text)).toEqual(["ok", " done ", "rgb"])
    expect(runs[0]!.style).toMatchObject({ fg: { kind: "basic", index: 2 }, bold: true })
    expect(runs[1]!.style.fg).toBeUndefined()
    expect(runs[2]!.style.fg).toEqual({ kind: "rgb", r: 1, g: 2, b: 3 })
  })

  test("background parameters are skipped", () => {
    expect(parse("\x1b[48;5;12;31mx")[0]!.style.fg).toEqual({ kind: "basic", index: 1 })
  })

  test("escapes before multibyte characters, and truncated ones", () => {
    expect(strip("a\x1béb")).toBe("ab")
    expect(strip("\x1b😀ok")).toBe("ok")
    expect(strip("\x1b[émx")).toBe("x")
    expect(strip("\x1b]é")).toBe("")
    expect(strip("abc\x1b[")).toBe("abc")
    expect(strip("abc\x1b")).toBe("abc")
    // Every split of a mixed line parses without throwing.
    const line = "\x1b[1;3é1m✓ ok\x1b]8;;é\x07\x1bé\x1b"
    for (let i = 0; i < line.length; i++) {
      parse(line.slice(0, i))
      parse(line.slice(i))
    }
  })
})

describe("windows commands", () => {
  const ok = (c: string) => {
    const t = translate(c)
    if (!t.ok) throw new Error(`${c}: ${t.error}`)
    return t.command
  }

  test("variables", () => {
    expect(ok("trunk serve --port $PORT")).toBe("trunk serve --port %PORT%")
    expect(ok("serve --port=${PORT}")).toBe("serve --port=%PORT%")
    expect(ok('echo "on $PORT"')).toBe('echo "on %PORT%"')
    // Not variables: positional parameters and a bare $.
    expect(ok("echo $5 a=b $")).toBe("echo $5 a=b $")
    // Single quotes keep $ literal.
    expect(ok("echo '$PORT'")).toBe('echo "$PORT"')
  })

  test("assignments", () => {
    expect(ok("LEPTOS_SITE_ADDR=127.0.0.1:$PORT cargo leptos watch")).toBe(
      'set "LEPTOS_SITE_ADDR=127.0.0.1:%PORT%" && cargo leptos watch',
    )
    expect(ok("A=\"x y\" B='$z' run")).toBe('set "A=x y" && set "B=$z" && run')
    // At the start of every command in a chain, and nowhere else.
    expect(ok("cd web && NODE_ENV=dev npm start a=b")).toBe('cd web && set "NODE_ENV=dev" && npm start a=b')
    expect(translate("A='say \"hi\"' run").ok).toBe(false)
    expect(translate("echo $(date)")).toEqual({
      ok: false,
      error: "cmd.exe has no equivalent of command substitution, $(...)",
    })
  })

  test("quoting and operators", () => {
    expect(ok("pnpm dev")).toBe("pnpm dev")
    expect(ok("echo 'a b' \"c\"d ''")).toBe('echo "a b" "cd" ""')
    expect(ok("echo 'a&b' a|b")).toBe('echo "a&b" a|b')
    expect(ok("serve 2>&1 >/dev/null; next || x")).toBe("serve 2>&1 >NUL& next || x")
    expect(ok("(a && b) > log.txt")).toBe("(a && b) > log.txt")
    // Windows paths keep their backslashes, quoted or not.
    expect(ok('X=1 "C:\\Program Files\\a.exe" C:\\x\\y')).toBe('set "X=1" && "C:\\Program Files\\a.exe" C:\\x\\y')
  })

  test("refuses what cmd.exe can't do", () => {
    for (const bad of ["echo $(date)", "echo `date`", "echo ${A:-x}", "echo ${A", "echo 'open", 'echo "open', "cat <<EOF"]) {
      expect(translate(bad).ok).toBe(false)
    }
  })
})

describe("output lines", () => {
  const enc = (s: string) => new TextEncoder().encode(s)

  test("splitting", () => {
    const l = new Lines()
    expect(l.feed(enc("a\nb\r\nc"))).toEqual(["a", "b"])
    // Progress bars redraw with \r; only what would be visible is kept.
    expect(l.feed(enc("\r10%\r50%\r\n"))).toEqual(["50%"])
    expect(l.feed(enc("x\r"))).toEqual([])
    expect(l.feed(enc("\r\n"))).toEqual(["x"])
    expect(l.feed(enc("tail"))).toEqual([])
    expect(l.finish()).toBe("tail")
    expect(l.finish()).toBeUndefined()
    // Invalid UTF-8 is replaced, not dropped or fatal.
    expect(l.feed(new Uint8Array([0xff, 0xfe, ...enc("ok\n")]))).toEqual(["\ufffd\ufffdok"])
  })

  test("long lines are cut", () => {
    const l = new Lines()
    expect(l.feed(new Uint8Array(MAX_LINE * 3).fill(0x78))).toEqual([])
    const out = l.feed(enc("\nnext\n"))
    expect(out[0]).toBe("x".repeat(MAX_LINE) + " …[cut]")
    expect(out[1]).toBe("next")
  })

  test("free ports", () => {
    const first = freePort(undefined, [])!
    expect(first).toBeGreaterThanOrEqual(AUTO_PORTS.start)
    expect(first).toBeLessThanOrEqual(AUTO_PORTS.end)
    expect(freePort(undefined, [first])).not.toBe(first)
    // A preferred port outside the range isn't handed out.
    expect(freePort(80, [])).not.toBe(80)
    const held = Bun.listen({ hostname: "127.0.0.1", port: 4999, socket: { data() {} } })
    expect(freePort(4999, [])).not.toBe(4999)
    held.stop(true)
  })

  test("durations", () => {
    expect(fmtDuration(5000)).toBe("5s")
    expect(fmtDuration(125_000)).toBe("2m")
    expect(fmtDuration(3_720_000)).toBe("1h02")
  })
})

describe("log index", () => {
  const withLines = (lines: string[]) => {
    const m = manager("logindex")
    m.add({ name: "a", path: tempDir("logindex"), command: "x" })
    for (const l of lines) m.entries[0]!.log(l)
    return m
  }

  test("follows the log", () => {
    const m = withLines(["one", "two", "three"])
    const e = m.entries[0]!
    const ix = new LogIndex()
    expect(ix.sync(e, "")).toEqual({ kind: "reset" })
    expect(ix.lines).toEqual([0, 1, 2])
    expect(ix.sync(e, "")).toEqual({ kind: "none" })
    e.log("four")
    expect(ix.sync(e, "")).toEqual({ kind: "splice", dropped: 0, added: 1 })
    // Clearing drops everything shown, and new lines keep counting up.
    e.clearLogs()
    e.log("five")
    expect(ix.sync(e, "")).toEqual({ kind: "splice", dropped: 4, added: 1 })
    expect(ix.lines).toEqual([4])
    expect(ix.plainText(e)).toBe("five\n")
    expect(ix.sync(undefined, "")).toEqual({ kind: "reset" })
  })

  test("filters", () => {
    const m = withLines(["GET /a", "\x1b[31merror\x1b[0m: boom", "GET /b"])
    const e = m.entries[0]!
    const ix = new LogIndex()
    expect(ix.sync(e, " get ")).toEqual({ kind: "reset" })
    expect(ix.lines).toEqual([0, 2])
    expect(ix.filtering()).toBe(true)
    e.log("POST /c")
    expect(ix.sync(e, "get")).toEqual({ kind: "none" })
    e.log("get /d")
    expect(ix.sync(e, "get")).toEqual({ kind: "splice", dropped: 0, added: 1 })
    // Escapes don't count toward matches, and copies come out plain.
    expect(ix.sync(e, "error:")).toEqual({ kind: "reset" })
    expect(ix.plainText(e)).toBe("error: boom\n")
  })
})
