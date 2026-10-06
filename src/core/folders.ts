//! Finding project folders for the add dialog.

import { existsSync, readdirSync } from "node:fs"
import { join } from "node:path"
import { isDir } from "./config"

export interface Folder {
  name: string
  path: string
  /** Short markers like "node" / "git" so project folders stand out. */
  tags: string[]
}

const MARKERS: [string, string][] = [
  ["package.json", "node"],
  ["deno.json", "deno"],
  ["Cargo.toml", "rust"],
  ["go.mod", "go"],
  ["pyproject.toml", "python"],
  ["requirements.txt", "python"],
  ["Gemfile", "ruby"],
  ["composer.json", "php"],
  ["mix.exs", "elixir"],
  [".git", "git"],
]

export function tagsFor(dir: string): string[] {
  const tags = MARKERS.filter(([f]) => existsSync(join(dir, f))).map(([, t]) => t)
  return tags.filter((t, i) => tags[i - 1] !== t)
}

/** Visible subfolders of `root`, sorted by name. */
export function listFolders(root: string): Folder[] {
  let names: string[]
  try {
    names = readdirSync(root)
  } catch {
    return []
  }
  return names
    .filter((name) => !name.startsWith(".") && isDir(join(root, name))) // follows symlinks
    .map((name) => ({ name, path: join(root, name), tags: tagsFor(join(root, name)) }))
    .sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()))
}

/** Case-insensitive subsequence match ("bpt" matches "blueprint"). */
export function fuzzy(needle: string, hay: string): boolean {
  const h = hay.toLowerCase()
  let i = 0
  for (const n of needle.toLowerCase()) {
    i = h.indexOf(n, i)
    if (i < 0) return false
    i += n.length
  }
  return true
}
