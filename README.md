# blueprint

A terminal app for running many dev servers at once. Each project gets its own port
(fixed, or picked for it), live logs, and a link to `http://localhost:PORT`.

```
  ◆ blueprint  v0.3.0                                                ● 2 running  ·  3 projects

  PORT    PROJECT   STATUS            UPTIME    COMMAND
▌ 3000    shop      ● RUNNING         4m        npm run dev
  3001    docs      ● RUNNING         12s       npm run dev
  4321    api       ✕ CRASHED         exit 1    go run .

── shop ── http://localhost:3000 ── ~/Projects/shop ── pid 48213 ──────────────────── 7 lines ──
  $ PORT=3000 npm run dev

  > dev
  > next dev

  ready on http://localhost:3000
  GET / 200 in 41ms

  enter stop   r restart   n new   e edit   o open   / filter   ? help   q quit
```

Built with [Bun](https://bun.sh) and [OpenTUI](https://github.com/anomalyco/opentui) (its React
renderer). It wears your terminal's theme: text and background are the terminal's own, every
color is a slot in its palette, and color appears only where it means something: status, the
focused thing, the main action. Switch the terminal's theme and blueprint follows.

## Requirements

- Linux or Windows, in a terminal with 256 or true colors (most are). macOS should work from
  source but isn't tested.
- Whatever your projects use (Node, Cargo, Python, ...). Nothing else.
- To run from source or build it: [Bun](https://bun.sh) 1.4 or newer.

## Install

Download the latest release from the
[releases page](https://github.com/radiumcoders/BluePrint/releases). Each is a single file with
nothing else to install:

- **Linux**: `blueprint-<version>-linux-x86_64.tar.gz`. Unpack it and put `blueprint` somewhere
  on your `PATH`, like `~/.local/bin`.
- **Windows**: `blueprint-<version>-windows-x86_64.zip` or the plain `.exe`. Run it from Windows
  Terminal or PowerShell. It isn't code-signed, so SmartScreen may ask first: "More info → Run
  anyway".

Or run it from source:

```sh
git clone https://github.com/radiumcoders/BluePrint.git
cd BluePrint
bun install
bun start                 # run it
bun run build             # or build the single-file binary into dist/blueprint
```

## Using it

Run `blueprint` in a terminal.

- **Add a project**: press `n`. Pick a folder from your projects directory (type to filter, `↑`
  `↓` to choose, `Enter` to pick) or type any path, like `~/code/app`.
  - **name**: what the project is called in the list. It defaults to the package.json name or
    the folder name.
  - **port**: a fixed port for the dev server. Leave it blank and blueprint picks a free one
    from 3000–3999, keeping the same one across restarts while it's free.
  - **command**: filled in from the folder (see below). Leave it blank to run the package.json
    `dev` script, or enter anything, like `pnpm dev`. Shell syntax (`&&`, `|`, `$PORT`) works.
  - `Tab` moves between fields, `Enter` saves, `Esc` cancels.
- **Run them**: `Enter` starts or stops the selected project; `a` starts all and `x` stops all.
  Run as many as you like. A project shows *starting* until its server accepts connections,
  then *running*.
- **The board**: every project on one line, port first: its port (`auto` until an automatic
  one is picked), status, uptime or exit code, and command. Under it, a rule carries the selected
  project's name, URL, folder and PID, and heads its live, colored logs. The console keeps the last 5000 lines and follows the newest
  until you scroll up (`PgUp`, `g`, or the mouse wheel; `G` or `End` follows again). Filter it
  with `/` (case-insensitive; `Esc` clears it), copy the lines shown with `y`, or save them to a
  file with `w`.
- **Editing a running project** changes nothing until you restart it: the board keeps showing
  the port, URL and command it runs with, marked "edited, restart to apply".
- **Mouse**: click a project to select it, click a running project's URL to open it, scroll the
  logs, and drag over text to copy it.

| Key | Action |
| --- | --- |
| `↑` `↓` / `j` `k` | select project |
| `Enter` / `Space` | start / stop |
| `r` | restart |
| `a` / `x` | start all / stop all |
| `n` | new project |
| `e` | edit |
| `d` / `Delete` | remove (press twice) |
| `Alt+↑` `Alt+↓` / `J` `K` | move the project up / down |
| `o` / `c` | open in the browser / copy the URL |
| `/` or `Ctrl+F` | filter logs |
| `Esc` | clear the filter, close the form, help or a prompt |
| `PgUp` `PgDn`, `Ctrl+U` `Ctrl+D` | scroll logs |
| `g` / `G` | oldest / newest line |
| `y` / `w` | copy / save the lines shown |
| `L` | clear logs |
| `?` | all keys |
| `q` / `Ctrl+C` | quit |

Quitting stops every server.

## Config

The config is stored at `~/.config/blueprint/config.toml` (`%APPDATA%\blueprint\config.toml` on
Windows). Set `BLUEPRINT_CONFIG` to use a different file. `blueprint --help` prints the path.

```toml
projects_root = "~/Projects"   # where the add form looks for folders

[[projects]]
name = "shop"
path = "~/Projects/shop"
port = 3000                    # optional
command = "pnpm dev"           # optional
```

Configs from the earlier desktop version (0.2) load as they are.

## How it works

- Each project's command runs through `sh -c` (`cmd.exe` on Windows) in its folder, with its
  port in the `PORT` environment variable. The logs start with the exact command, e.g.
  `$ PORT=4001 pnpm dev --port $PORT`.
- A blank command runs the package.json `dev` script with the project's package manager (from
  its lockfile). Vite, Astro, Angular, React Router, Remix and Expo ignore `PORT`, so blueprint
  adds `--port $PORT` for them (and `--strictPort` for Vite, so it fails instead of quietly
  moving to another port). Next, Nuxt and most others read `PORT` themselves.
- Not just Node: picking a folder fills in a start command for Rust (`cargo run`, Trunk,
  Dioxus, Leptos, Zola), Go (`go run .`, Air, Hugo), Python (Django, FastAPI, Flask, Streamlit,
  with uv or Poetry), Ruby (Rails, Rack), PHP (Laravel, `php -S`), Phoenix, Deno and static
  folders. A plain `cargo run` or `go run .` app must read `PORT` itself.
- **No orphaned servers**: each server runs in its own session and process group (a job object
  on Windows), and stopping it stops the whole group: SIGTERM, then SIGKILL after 4 seconds. If
  a command exits but leaves processes behind, they get the same treatment, and the project
  shows *stopping* until they're gone. A small guardian process (`blueprint --guardian`) watches
  a pipe from blueprint; if blueprint exits for any reason, including a crash, a closed
  terminal or `kill -9`, it stops every server the same way. On Windows the system kills the
  job objects when blueprint exits.

## What is guaranteed, and what isn't

Some behavior is checked by tests on Linux and Windows for every change. Some is a best guess
that works for common setups. This section says which is which.

**Statuses.** *Running* means something accepts TCP connections on `localhost:PORT` (127.0.0.1
or ::1). It does not mean the app is healthy: blueprint doesn't speak HTTP to it, so a server
that accepts connections but answers with errors still shows *running*. Running servers are
checked every 2 seconds, and two failed checks in a row turn the status into *not responding*.
There is no startup timeout. A server that hasn't accepted a connection after 60 seconds gets a
warning in its logs (usually it isn't listening on `$PORT`), but it keeps running, because a
first build can take longer than that.

**Start recipes.** Detection only suggests a command; you can always change it. Tested end to
end (started for real, answered over HTTP, port free after stopping): a package.json dev script
that reads `PORT`, a Vite-style tool that has to be passed `--port`, a static site served by
Python, and a Go module. Every other recipe (Rust, Hugo, Air, Django, FastAPI, Flask,
Streamlit, Rails, Rack, Laravel, PHP, Phoenix, Deno) is checked only for the command it
suggests, so whether that tool honors the port is up to the tool and its version.

**Network exposure.** Recipes keep servers on localhost: where a tool would listen on every
interface by default (Python's `http.server`, Streamlit), the suggested command binds it to
127.0.0.1. Commands you write yourself are run as written.

**Stopping (tested).** Stop, restart, remove and quit end the whole process tree, including
children that ignore SIGTERM and processes left behind by a command that already exited. On
Unix, a process that moves itself out of its session (`setsid`, daemons, Docker containers run
by the Docker daemon) isn't tracked. On Windows, stopping kills the tree at once, because
console programs in a hidden console can't be asked to close politely. A server joins its job
object as soon as it has started, so a program its shell starts in the first instant could in
principle slip out of it.

**Logs (tested).** Each project keeps its newest 5000 lines, up to 4 MiB. Lines longer than
4 KiB are cut and marked `…[cut]`. Invalid UTF-8 and stray escape sequences are shown, never
fatal. A server that writes faster than blueprint takes output in has to wait (as it would
writing to a slow terminal) instead of blueprint's memory growing.

**Clipboard.** Copying (`y`, `c`, or a mouse selection) uses the terminal's clipboard escape
(OSC 52). Most terminals support it; some (and tmux, unless `set-clipboard` is on) ignore it.
Saving with `w` always works.

**Commands on Windows.** Commands are written the way `sh` reads them and translated for
`cmd.exe`: `$NAME` and `${NAME}` become `%NAME%`, `NAME=value` before a command becomes
`set "NAME=value" &&`, single quotes become double quotes, `;` becomes `&`, and `/dev/null`
becomes `NUL`. `&&`, `||`, pipes, redirections and backslash paths pass through. Commands using
`$(...)`, backticks, `${NAME:-default}` or here-documents don't start, with an error naming the
problem. Two known gaps: `%NAME%` in a command still expands as in `cmd.exe`, and a variable set
before one command in a chain stays set for the rest of the chain.

## Troubleshooting

- **A project stays on *starting*.** Its server isn't listening on the port blueprint gave it.
  Check the first log line for the port, then make sure the command passes `$PORT` or the app
  reads the `PORT` variable. A fixed port in the project settings helps when a tool insists on
  its own.
- **"Port N is already in use".** Something else holds that fixed port, maybe an earlier run
  started outside blueprint. Find it with `lsof -i :N` (Linux) or `netstat -ano | findstr :N`
  (Windows), or leave the port blank to have one picked.
- **"command not found".** blueprint runs commands with the `PATH` of the terminal it was
  started in. If a tool works in your shell but not here, start blueprint from that same shell,
  or use the tool's full path in the command.
- **A command fails on Windows only.** See *Commands on Windows* above; the log shows the error
  if the command couldn't be translated.
- **Copying does nothing.** Your terminal ignores OSC 52 (see *Clipboard*). Save the logs with
  `w` instead, or hold Shift while dragging to use the terminal's own selection.
- **A server was left running after blueprint closed.** That shouldn't happen. If it does,
  please open an issue with the project's command and platform.

## Development

```sh
bun install
bun start                       # run from source
bun test                        # unit, stand-in server and UI tests; nothing else to install
BLUEPRINT_E2E=1 bun test        # also the end-to-end tests, which need node, npm,
                                # python (python3 on Linux) and go on PATH
bun run typecheck
bun run build                   # single-file binary in dist/
```

The code is in two parts: `src/core` runs the servers and knows nothing about the screen
(config, start recipes, processes, logs), and `src/ui` draws it with OpenTUI. CI runs the
typecheck and every test on Linux and Windows for each push and pull request.
