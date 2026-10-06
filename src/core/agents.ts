//! Opening a project in an editor or a coding agent. Editors open the folder
//! in a window of their own; terminal agents start in a new terminal window
//! in the project's folder, so the board stays in view.

import { spawn } from "node:child_process"
import { basename } from "node:path"
import { isWindows } from "./platform"

export interface Tool {
  name: string
  /** Commands that start it, the first on PATH wins. */
  commands: string[]
  /** Editors take the folder as an argument; agents run in a terminal in it. */
  kind: "editor" | "agent"
}

/** What blueprint knows how to open, in the order the picker lists them. */
export const TOOLS: Tool[] = [
  { name: "Claude Code", commands: ["claude"], kind: "agent" },
  { name: "Cursor", commands: ["cursor"], kind: "editor" },
  { name: "OpenCode", commands: ["opencode"], kind: "agent" },
  { name: "Codex", commands: ["codex"], kind: "agent" },
  { name: "Gemini CLI", commands: ["gemini"], kind: "agent" },
  { name: "Crush", commands: ["crush"], kind: "agent" },
  { name: "Amp", commands: ["amp"], kind: "agent" },
  { name: "Aider", commands: ["aider"], kind: "agent" },
  { name: "Copilot CLI", commands: ["copilot"], kind: "agent" },
  { name: "VS Code", commands: ["code"], kind: "editor" },
  { name: "VSCodium", commands: ["codium"], kind: "editor" },
  { name: "Windsurf", commands: ["windsurf"], kind: "editor" },
  { name: "Zed", commands: ["zed", "zeditor"], kind: "editor" },
]

/** A tool that's installed, with the path of its command. */
export interface Installed extends Tool {
  bin: string
}

/** The tools found on `path` (the PATH by default). */
export function installedTools(path = process.env.PATH ?? ""): Installed[] {
  const found: Installed[] = []
  for (const tool of TOOLS) {
    for (const command of tool.commands) {
      const bin = Bun.which(command, { PATH: path })
      if (bin) {
        found.push({ ...tool, bin })
        break
      }
    }
  }
  return found
}

type Launch = { file: string; args: string[] }

/** Terminals by command name, and how each runs `bin` in `dir`. */
const TERMINALS: Record<string, (dir: string, bin: string) => string[]> = {
  "xdg-terminal-exec": (dir, bin) => [`--dir=${dir}`, bin],
  ghostty: (dir, bin) => [`--working-directory=${dir}`, "-e", bin],
  kitty: (dir, bin) => ["--directory", dir, bin],
  alacritty: (dir, bin) => ["--working-directory", dir, "-e", bin],
  foot: (dir, bin) => [`--working-directory=${dir}`, bin],
  wezterm: (dir, bin) => ["start", "--cwd", dir, "--", bin],
  konsole: (dir, bin) => ["--workdir", dir, "-e", bin],
  "gnome-terminal": (dir, bin) => [`--working-directory=${dir}`, "--", bin],
  "xfce4-terminal": (dir, bin) => [`--working-directory=${dir}`, "-x", bin],
  xterm: (_dir, bin) => ["-e", bin],
}

/** The terminal to start agents in on Linux: $TERMINAL, else the first known one installed. */
function linuxTerminal(dir: string, bin: string, env: NodeJS.ProcessEnv): Launch | undefined {
  const preferred = env.TERMINAL?.trim()
  if (preferred) {
    const [file, ...extra] = preferred.split(/\s+/)
    const args = TERMINALS[basename(file!)]
    return { file: file!, args: [...extra, ...(args ? args(dir, bin) : ["-e", bin])] }
  }
  for (const [name, args] of Object.entries(TERMINALS)) {
    const file = Bun.which(name, { PATH: env.PATH ?? "" })
    if (file) return { file, args: args(dir, bin) }
  }
  return undefined
}

const applescript = (s: string) => `"${s.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`
const posix = (s: string) => `'${s.replace(/'/g, `'\\''`)}'`

/** How to open `dir` in `tool`, or why it can't be. */
export function launchFor(tool: Installed, dir: string, env = process.env): Launch | string {
  if (tool.kind === "editor") {
    // Editors on Windows are usually .cmd scripts, which need cmd.exe.
    return isWindows ? { file: "cmd.exe", args: ["/d", "/c", tool.bin, dir] } : { file: tool.bin, args: [dir] }
  }
  if (isWindows) return { file: "cmd.exe", args: ["/d", "/c", "start", '""', "/D", dir, "cmd", "/k", tool.bin] }
  if (process.platform === "darwin") {
    const script = `cd ${posix(dir)} && ${posix(tool.bin)}`
    return {
      file: "osascript",
      args: ["-e", `tell application "Terminal" to do script ${applescript(script)}`, "-e", 'tell application "Terminal" to activate'],
    }
  }
  return linuxTerminal(dir, tool.bin, env) ?? "No terminal found to run it in. Set $TERMINAL to yours."
}

/** Open `dir` in `tool`, in a window of its own that outlives blueprint. */
export function openIn(tool: Installed, dir: string): Promise<void> {
  const launch = launchFor(tool, dir)
  if (typeof launch === "string") return Promise.reject(new Error(launch))
  return new Promise((resolve, reject) => {
    try {
      const child = spawn(launch.file, launch.args, { cwd: dir, stdio: "ignore", detached: true, windowsHide: true })
      child.once("error", reject)
      child.once("spawn", () => {
        child.unref()
        resolve()
      })
    } catch (e) {
      reject(e as Error)
    }
  })
}
