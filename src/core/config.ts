//! The config file: where projects live and how each one starts.

import { existsSync, mkdirSync, readFileSync, renameSync, statSync, writeFileSync } from "node:fs"
import { homedir } from "node:os"
import { basename, dirname, join, relative, isAbsolute } from "node:path"
import { parse, stringify } from "smol-toml"

export interface Project {
  /** Display name, e.g. `shop` or `api.shop`. */
  name: string
  path: string
  /** Fixed port for the dev server. `undefined` picks a free one on each start. */
  port?: number
  /** Command to run. Empty means the package.json `dev` script. */
  command: string
}

export interface Config {
  /** Folder the project picker opens in. */
  projectsRoot: string
  projects: Project[]
}

export function isDir(p: string): boolean {
  try {
    return statSync(p).isDirectory()
  } catch {
    return false
  }
}

function defaultProjectsRoot(): string {
  const projects = join(homedir(), "Projects")
  return isDir(projects) ? projects : homedir()
}

export function defaultConfig(): Config {
  return { projectsRoot: defaultProjectsRoot(), projects: [] }
}

/** `$BLUEPRINT_CONFIG`, else `blueprint/config.toml` in the user's config folder. */
export function configPath(): string {
  const override = process.env.BLUEPRINT_CONFIG
  if (override) return override
  const base =
    process.platform === "win32"
      ? (process.env.APPDATA ?? join(homedir(), "AppData", "Roaming"))
      : process.env.XDG_CONFIG_HOME || join(homedir(), ".config")
  return join(base, "blueprint", "config.toml")
}

export function loadConfig(path: string): Config {
  if (!existsSync(path)) return defaultConfig()
  let raw: Record<string, unknown>
  try {
    raw = parse(readFileSync(path, "utf8")) as Record<string, unknown>
  } catch (e) {
    throw new Error(`parsing ${path}: ${e instanceof Error ? e.message : e}`)
  }
  const root = typeof raw.projects_root === "string" ? expandTilde(raw.projects_root) : defaultProjectsRoot()
  const list = Array.isArray(raw.projects) ? raw.projects : []
  const projects: Project[] = list.map((p, i) => {
    if (typeof p !== "object" || p === null) throw new Error(`parsing ${path}: projects[${i}] isn't a table`)
    const t = p as Record<string, unknown>
    if (typeof t.name !== "string" || typeof t.path !== "string") {
      throw new Error(`parsing ${path}: projects[${i}] needs a name and a path`)
    }
    const port = typeof t.port === "number" || typeof t.port === "bigint" ? Number(t.port) : undefined
    return {
      name: t.name,
      path: expandTilde(t.path),
      port,
      command: typeof t.command === "string" ? t.command : "",
    }
  })
  return { projectsRoot: root, projects }
}

/** Write-then-rename, so a crash never leaves a half-written config. */
export function saveConfig(config: Config, path: string): void {
  mkdirSync(dirname(path), { recursive: true })
  const doc = {
    projects_root: config.projectsRoot,
    projects: config.projects.map((p) => {
      const out: Record<string, string | number> = { name: p.name, path: p.path }
      if (p.port !== undefined) out.port = p.port
      if (p.command) out.command = p.command
      return out
    }),
  }
  const tmp = `${path}.tmp`
  writeFileSync(tmp, stringify(doc) + "\n")
  renameSync(tmp, path)
}

export function expandTilde(p: string): string {
  if (p === "~") return homedir()
  if (p.startsWith("~/") || p.startsWith("~\\")) return join(homedir(), p.slice(2))
  return p
}

/** Shorten a path for display by replacing the home directory with `~`. */
export function displayPath(p: string): string {
  const home = homedir()
  const rest = relative(home, p)
  if (rest === "") return "~"
  if (!rest.startsWith("..") && !isAbsolute(rest)) return `~/${rest.replaceAll("\\", "/")}`
  return p
}

/**
 * Turn arbitrary text into a valid project name: lowercase letters, digits,
 * `-` and `.` (e.g. `api.shop`).
 */
export function slugify(s: string): string {
  s = s.split("/").pop() ?? s // drop npm scope: @acme/web -> web
  let out = ""
  for (const ch of s) {
    const c = ch.toLowerCase()
    if (/^[a-z0-9]$/.test(c)) out += c
    else if (".-_ ".includes(c) && !out.endsWith("-") && !out.endsWith(".")) out += c === "." ? "." : "-"
  }
  return out.replace(/^[-.]+|[-.]+$/g, "")
}

/** Why `name` can't be a project name, or `undefined` when it can. */
export function validateName(name: string): string | undefined {
  if (!name) return "name is required"
  if (!/^[a-z0-9.-]+$/.test(name)) return "use only a-z, 0-9, '-' and '.'"
  if (name.split(".").some((l) => !l || l.startsWith("-") || l.endsWith("-"))) {
    return "each part must not be empty or start/end with '-'"
  }
  return undefined
}

/** Suggest a name for a folder: package.json `name` if present, else the folder name. */
export function suggestName(dir: string): string {
  let fromPkg = ""
  try {
    const name = packageName(readFileSync(join(dir, "package.json"), "utf8"))
    if (name) fromPkg = slugify(name)
  } catch {}
  return fromPkg || slugify(basename(dir))
}

/** The top-level `name` of a package.json. */
export function packageName(json: string): string | undefined {
  try {
    const value = JSON.parse(json)
    return typeof value?.name === "string" ? value.name : undefined
  } catch {
    return undefined
  }
}
