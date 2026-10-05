# portboard

A desktop app for running many dev servers at once through
[portless](https://portless.sh). Each project gets a stable, named URL
(`https://shop.localhost`) instead of a port number.

Built with Rust and [GPUI](https://www.gpui.rs) (through `gpui-kit`), drawn like a blueprint:
solid blue panels on a `#0552e1` field with a fine white grid, set in Geist and Geist Mono.
Lines come in three weights (hairline, line, solid ink), and only the details sheet and the
main action are solid ink, so the eye always has one place to land.

## Requirements

- portless: `npm install -g portless` (needs Node 24+)
- Rust toolchain to build
- Linux (Wayland or X11)

## Install

```sh
cargo install --path .
portboard
```

## Using it

- **Add a project**: click "add project" or press `N`. Pick a folder from your projects
  directory (type to filter, Enter picks the first match) or use "browse…" for any folder.
  - **name**: the portless name. `shop` becomes `https://shop.localhost`, and dots make
    subdomains (`api.shop`). It defaults to the package.json name or the folder name.
  - **port**: a fixed port for the dev server. Leave it blank and portless picks one (4000–4999).
  - **command**: leave it blank to run the package.json `dev` script, or enter something like
    `pnpm dev`. Shell syntax (`&&`, `|`) works.
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

## Clean URLs (one-time setup)

`https://name.localhost` with no port needs the portless proxy on port 443, which needs root
once. The first time you start a project, portboard asks how to set it up:

- **install as a service** (recommended): opens a terminal that runs `portless service install`.
  You enter your password once. The proxy then starts on boot and the HTTPS certificate is trusted.
- **start once**: opens a terminal that runs `portless proxy start -p 443`. Lasts until you reboot.
- **skip, use port 1355**: no password, but URLs end in `:1355`.

The title block (bottom left) shows the proxy state. Click its row to start or stop the proxy.
If you're on `:1355`, "make clean" switches back to clean URLs.

If you skipped the service and your browser shows certificate warnings, run `portless trust` once.

## Config

The config is stored at `~/.config/portboard/config.toml`. Set `PORTBOARD_CONFIG` to use a different file.

```toml
proxy_port = 443               # 1355 = no root needed, URLs get :1355
projects_root = "~/Projects"   # where the add dialog looks for folders

[[projects]]
name = "shop"
path = "~/Projects/shop"
port = 3000                    # optional
command = "pnpm dev"           # optional
```

## How it works

- Each project runs as `portless run --name <name> [--app-port N]` (the dev script) or
  `portless --name <name> [--app-port N] -- <command>`. portless injects `PORT` and `--port`,
  so Vite, Next and others bind to the right port.
- When no proxy is running, portboard starts it once and queues projects until it's up.
  This avoids several portless processes racing to start it.
- **No orphaned servers**: each server runs in its own process group. A small guardian process
  (`portboard --guardian`) watches a pipe from portboard. If portboard exits for any reason,
  including a crash or `kill -9`, the guardian sends SIGTERM to every server and then SIGKILL
  after 4 seconds.

## Fonts

[Geist and Geist Mono](https://github.com/vercel/geist-font) are bundled under the SIL Open Font License. See `assets/fonts/`.
