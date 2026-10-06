//! Working out how to start a project.
//!
//! blueprint hands every server its port in the `PORT` environment variable,
//! so a recipe either uses a tool that reads `PORT` itself or passes `$PORT`
//! on the command line.
//!
//! Recipes keep servers on localhost: where a tool would otherwise listen on
//! every interface (Python's `http.server`, Streamlit), the recipe binds it
//! to 127.0.0.1 so a project's files aren't served to the local network.

import { existsSync, readFileSync } from "node:fs"
import { join } from "node:path"

export interface Recipe {
  /** What was detected, e.g. "Trunk" or "Django". */
  name: string
  /** Command to run; empty means the package.json dev script (see {@link devScriptCommand}). */
  command: string
  /** The command doesn't pass a port itself, so the server must read `PORT`. */
  readsEnv: boolean
}

/** Python's name on PATH: Windows installs it as `python`. */
export const PYTHON = process.platform === "win32" ? "python" : "python3"

const recipe = (name: string, command: string): Recipe => ({ name, command, readsEnv: false })
const envRecipe = (name: string, command: string): Recipe => ({ name, command, readsEnv: true })

function read(dir: string, file: string): string {
  try {
    return readFileSync(join(dir, file), "utf8")
  } catch {
    return ""
  }
}

const has = (dir: string, file: string) => existsSync(join(dir, file))

/** The body of package.json's `scripts.<name>`, if there is one. */
function script(pkg: string, name: string): string | undefined {
  try {
    const body = JSON.parse(pkg)?.scripts?.[name]
    return typeof body === "string" ? body : undefined
  } catch {
    return undefined
  }
}

/** True when an empty command can run: there's a package.json dev script. */
export function hasDevScript(dir: string): boolean {
  return script(read(dir, "package.json"), "dev") !== undefined
}

/** The package manager a JS project uses, judged by its lockfile. */
function packageManager(dir: string): string {
  if (has(dir, "bun.lock") || has(dir, "bun.lockb")) return "bun"
  if (has(dir, "pnpm-lock.yaml")) return "pnpm"
  if (has(dir, "yarn.lock")) return "yarn"
  return "npm"
}

/**
 * Flags for dev servers that ignore `PORT` (Vite would otherwise also wander
 * to the next free port, away from the one blueprint checks).
 */
function portFlags(body: string): string | undefined {
  // Skip `NAME=value` and `cross-env NAME=value` prefixes to find the tool.
  const word = body.split(/\s+/).find((w) => w && !w.includes("=") && !["cross-env", "npx", "bunx"].includes(w))
  const tool = word?.split(/[/\\]/).pop()
  if (tool === "vite") return "--port $PORT --strictPort"
  if (["astro", "ng", "react-router", "remix", "expo"].includes(tool ?? "")) return "--port $PORT"
  return undefined
}

/**
 * The command that runs package.json's dev script with the project's package
 * manager, passing `--port $PORT` to tools that need it.
 */
export function devScriptCommand(dir: string): string | undefined {
  const body = script(read(dir, "package.json"), "dev")
  if (body === undefined) return undefined
  const pm = packageManager(dir)
  const run = pm === "npm" ? "npm run dev" : pm === "bun" ? "bun run dev" : `${pm} dev`
  const flags = portFlags(body)
  if (!flags) return run
  // npm needs `--` before arguments meant for the script.
  return pm === "npm" ? `${run} -- ${flags}` : `${run} ${flags}`
}

/** The best guess at how to start the project in `dir`, if any. */
export function detect(dir: string): Recipe | undefined {
  return (
    node(dir) ??
    deno(dir) ??
    rust(dir) ??
    go(dir) ??
    python(dir) ??
    ruby(dir) ??
    php(dir) ??
    elixir(dir) ??
    (has(dir, "index.html") ? recipe("static site", `${PYTHON} -m http.server $PORT --bind 127.0.0.1`) : undefined)
  )
}

function node(dir: string): Recipe | undefined {
  if (hasDevScript(dir)) return recipe("dev script", "")
  const pkg = read(dir, "package.json")
  if (!pkg || script(pkg, "start") === undefined) return undefined
  const pm = packageManager(dir)
  return envRecipe("start script", `${pm === "bun" ? "bun run" : pm} start`)
}

function deno(dir: string): Recipe | undefined {
  const cfg = read(dir, "deno.json") + read(dir, "deno.jsonc")
  if (!cfg) return undefined
  if (cfg.includes('"dev"')) return envRecipe("deno task", "deno task dev")
  return has(dir, "main.ts") ? envRecipe("deno", "deno run -A --watch main.ts") : undefined
}

function rust(dir: string): Recipe | undefined {
  const cargo = read(dir, "Cargo.toml")
  if (has(dir, "config.toml") && has(dir, "content") && has(dir, "templates")) {
    return recipe("Zola", "zola serve --port $PORT")
  }
  if (!cargo) return undefined
  if (has(dir, "Dioxus.toml")) return recipe("Dioxus", "dx serve --port $PORT")
  if (has(dir, "Trunk.toml") || read(dir, "index.html").includes("data-trunk")) {
    return recipe("Trunk", "trunk serve --port $PORT")
  }
  if (cargo.includes("[package.metadata.leptos]") || cargo.includes("[[workspace.metadata.leptos]]")) {
    return recipe("Leptos", "LEPTOS_SITE_ADDR=127.0.0.1:$PORT cargo leptos watch")
  }
  return envRecipe("Cargo", "cargo run")
}

function go(dir: string): Recipe | undefined {
  if (["hugo.toml", "hugo.yaml", "hugo.json"].some((f) => has(dir, f))) {
    return recipe("Hugo", "hugo server --port $PORT")
  }
  if (!has(dir, "go.mod")) return undefined
  if (has(dir, ".air.toml")) return envRecipe("Air", "air")
  return envRecipe("Go", "go run .")
}

function python(dir: string): Recipe | undefined {
  const deps = (read(dir, "pyproject.toml") + read(dir, "requirements.txt") + read(dir, "Pipfile")).toLowerCase()
  const manage = has(dir, "manage.py")
  if (!deps && !manage) return undefined
  const run = has(dir, "uv.lock") ? "uv run " : has(dir, "poetry.lock") ? "poetry run " : ""
  const py = run ? "python" : PYTHON
  if (manage) return recipe("Django", `${run}${py} manage.py runserver $PORT`)
  const entry = ["main.py", "app.py"].find((f) => has(dir, f))
  if (!entry) return undefined
  const module = entry.replace(/\.py$/, "")
  if (deps.includes("fastapi")) return recipe("FastAPI", `${run}uvicorn ${module}:app --reload --port $PORT`)
  if (deps.includes("flask")) return recipe("Flask", `${run}flask --app ${module} run --port $PORT`)
  if (deps.includes("streamlit")) {
    return recipe("Streamlit", `${run}streamlit run ${entry} --server.port $PORT --server.address localhost`)
  }
  return envRecipe("Python", `${run}${py} ${entry}`)
}

function ruby(dir: string): Recipe | undefined {
  if (!has(dir, "Gemfile")) return undefined
  if (has(dir, "bin/rails")) {
    // Windows can't run the script directly.
    const rails = process.platform === "win32" ? "ruby bin/rails" : "bin/rails"
    return recipe("Rails", `${rails} server -p $PORT`)
  }
  return has(dir, "config.ru") ? recipe("Rack", "bundle exec rackup -p $PORT") : undefined
}

function php(dir: string): Recipe | undefined {
  if (has(dir, "artisan")) return recipe("Laravel", "php artisan serve --port=$PORT")
  return has(dir, "index.php") ? recipe("PHP", "php -S 127.0.0.1:$PORT") : undefined
}

function elixir(dir: string): Recipe | undefined {
  // Phoenix's generated dev config reads PORT.
  return read(dir, "mix.exs").includes(":phoenix") ? envRecipe("Phoenix", "mix phx.server") : undefined
}
