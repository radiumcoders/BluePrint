# blueprint

A desktop app for running many dev servers at once. Each project gets its own port
(fixed, or picked for it), live logs, and a link to `http://localhost:PORT`.

Built with Rust and [GPUI](https://www.gpui.rs) (through `gpui-kit`), drawn like a blueprint:
solid blue panels on a `#0552e1` field with a fine white grid, set entirely in Geist Mono.
Lines come in three weights (hairline, line, solid ink), and only the details sheet and the
main action are solid ink, so the eye always has one place to land.

## Requirements

- Rust toolchain to build
- Linux (Wayland or X11) or Windows
- Whatever your projects use (Node, Cargo, Python, ...). Nothing else.

## Install

```sh
cargo install --path .
blueprint
```

## Using it

- **Add a project**: click "add project" or press `N`. Pick a folder from your projects
  directory (type to filter, Enter picks the first match) or use "browse…" for any folder.
  - **name**: what the project is called in the list. It defaults to the package.json name
    or the folder name.
  - **port**: a fixed port for the dev server. Leave it blank and blueprint picks a free one
    from 4000–4999, keeping the same one across restarts while it's free.
  - **command**: filled in from the folder (see below). Leave it blank to run the
    package.json `dev` script, or enter anything, like `pnpm dev`. Shell syntax (`&&`, `|`,
    `$PORT`) works.
- **Run them**: start and stop each project from its row, or with "start all"/"stop all".
  Run as many as you like. A project shows *starting* until its server accepts connections,
  then *running*.
- **Details and logs**: select a project to see its URL, folder, port, command, PID and
  uptime, plus its live, colored logs.

| Key | Action |
| --- | --- |
| `↑` `↓` | select project |
| `Enter` / `Space` | start / stop |
| `N` | add project |
| `E` | edit |
| `R` | restart |
| `O` | open in browser |
| `Delete` | remove (press twice) |
| `Alt+↑` `Alt+↓` | reorder |
| `Esc` | close dialog |

Closing the window stops every server.

## Config

The config is stored at `~/.config/blueprint/config.toml`. Set `BLUEPRINT_CONFIG` to use a different file.

```toml
projects_root = "~/Projects"   # where the add dialog looks for folders

[[projects]]
name = "shop"
path = "~/Projects/shop"
port = 3000                    # optional
command = "pnpm dev"           # optional
```

## How it works

- Each project's command runs through `sh -c` (`cmd.exe` on Windows) in its folder, with
  its port in the `PORT` environment variable. The logs start with the exact command, e.g.
  `$ PORT=4001 pnpm dev --port $PORT`.
- A blank command runs the package.json `dev` script with the project's package manager
  (from its lockfile). Vite, Astro, Angular, React Router, Remix and Expo ignore `PORT`, so
  blueprint adds `--port $PORT` for them (and `--strictPort` for Vite, so it fails instead of
  quietly moving to another port). Next, Nuxt and most others read `PORT` themselves.
- Not just Node: picking a folder fills in a start command for Rust (`cargo run`, Trunk,
  Dioxus, Leptos, Zola), Go (`go run .`, Air, Hugo), Python (Django, FastAPI, Flask,
  Streamlit, with uv or Poetry), Ruby (Rails, Rack), PHP (Laravel, `php -S`), Phoenix,
  Deno and static folders. A plain `cargo run` or `go run .` app must read `PORT` itself.
- A project shows *running* once something accepts connections on its port.
- Started from an app launcher, blueprint imports your login shell's `PATH` so tools
  installed through mise, cargo, bun or go are found.
- **No orphaned servers**: each server runs in its own process group. A small guardian process
  (`blueprint --guardian`) watches a pipe from blueprint. If blueprint exits for any reason,
  including a crash or `kill -9`, the guardian sends SIGTERM to every server and then SIGKILL
  after 4 seconds. On Windows, servers join a job object that the system kills when
  blueprint exits.

## Fonts

[Geist Mono](https://github.com/vercel/geist-font) is bundled under the SIL Open Font License. See `assets/fonts/`.
