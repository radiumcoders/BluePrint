//! Working out how to start a project.
//!
//! blueprint hands every server its port in the `PORT` environment variable,
//! so a recipe either uses a tool that reads `PORT` itself or passes `$PORT`
//! on the command line.
//!
//! Recipes keep servers on localhost: where a tool would otherwise listen on
//! every interface (Python's `http.server`, Streamlit), the recipe binds it
//! to 127.0.0.1 so a project's files aren't served to the local network.

use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Recipe {
    /// What was detected, e.g. "Trunk" or "Django".
    pub name: &'static str,
    /// Command to run; empty means the package.json dev script (see
    /// [`dev_script_command`]).
    pub command: String,
    /// The command doesn't pass a port itself, so the server must read `PORT`.
    pub reads_env: bool,
}

/// Python's name on PATH: Windows installs it as `python`.
const PYTHON: &str = if cfg!(windows) { "python" } else { "python3" };

fn recipe(name: &'static str, command: impl Into<String>) -> Option<Recipe> {
    Some(Recipe { name, command: command.into(), reads_env: false })
}

fn env_recipe(name: &'static str, command: impl Into<String>) -> Option<Recipe> {
    Some(Recipe { name, command: command.into(), reads_env: true })
}

fn read(dir: &Path, file: &str) -> String {
    fs::read_to_string(dir.join(file)).unwrap_or_default()
}

fn has(dir: &Path, file: &str) -> bool {
    dir.join(file).exists()
}

/// The body of package.json's `scripts.<script>`, if there is one.
fn script(pkg: &str, script: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(pkg).ok()?;
    json.get("scripts")?.get(script)?.as_str().map(String::from)
}

fn has_script(pkg: &str, name: &str) -> bool {
    script(pkg, name).is_some()
}

/// True when an empty command can run: there's a package.json dev script.
pub fn has_dev_script(dir: &Path) -> bool {
    has_script(&read(dir, "package.json"), "dev")
}

/// The package manager a JS project uses, judged by its lockfile.
fn package_manager(dir: &Path) -> &'static str {
    if has(dir, "bun.lock") || has(dir, "bun.lockb") {
        "bun"
    } else if has(dir, "pnpm-lock.yaml") {
        "pnpm"
    } else if has(dir, "yarn.lock") {
        "yarn"
    } else {
        "npm"
    }
}

/// Flags for dev servers that ignore `PORT` (Vite would otherwise also
/// wander to the next free port, away from the one blueprint checks).
fn port_flags(script: &str) -> Option<&'static str> {
    // Skip `NAME=value` and `cross-env NAME=value` prefixes to find the tool.
    let tool = script
        .split_whitespace()
        .find(|w| !w.contains('=') && *w != "cross-env" && *w != "npx" && *w != "bunx")?;
    let tool = tool.rsplit(['/', '\\']).next().unwrap_or(tool);
    match tool {
        "vite" => Some("--port $PORT --strictPort"),
        "astro" | "ng" | "react-router" | "remix" | "expo" => Some("--port $PORT"),
        _ => None,
    }
}

/// The command that runs package.json's dev script with the project's
/// package manager, passing `--port $PORT` to tools that need it.
pub fn dev_script_command(dir: &Path) -> Option<String> {
    let body = script(&read(dir, "package.json"), "dev")?;
    let pm = package_manager(dir);
    let run = match pm {
        "npm" => "npm run dev".to_string(),
        "bun" => "bun run dev".to_string(),
        pm => format!("{pm} dev"),
    };
    Some(match port_flags(&body) {
        // npm needs `--` before arguments meant for the script.
        Some(flags) if pm == "npm" => format!("{run} -- {flags}"),
        Some(flags) => format!("{run} {flags}"),
        None => run,
    })
}

/// The best guess at how to start the project in `dir`, if any.
pub fn detect(dir: &Path) -> Option<Recipe> {
    node(dir)
        .or_else(|| deno(dir))
        .or_else(|| rust(dir))
        .or_else(|| go(dir))
        .or_else(|| python(dir))
        .or_else(|| ruby(dir))
        .or_else(|| php(dir))
        .or_else(|| elixir(dir))
        .or_else(|| has(dir, "index.html").then(|| recipe("static site", format!("{PYTHON} -m http.server $PORT --bind 127.0.0.1"))).flatten())
}

fn node(dir: &Path) -> Option<Recipe> {
    if has_dev_script(dir) {
        return recipe("dev script", "");
    }
    let pkg = read(dir, "package.json");
    if pkg.is_empty() || !has_script(&pkg, "start") {
        return None;
    }
    let pm = match package_manager(dir) {
        "bun" => "bun run",
        pm => pm,
    };
    env_recipe("start script", format!("{pm} start"))
}

fn deno(dir: &Path) -> Option<Recipe> {
    let cfg = read(dir, "deno.json") + &read(dir, "deno.jsonc");
    if cfg.is_empty() {
        return None;
    }
    if cfg.contains("\"dev\"") {
        return env_recipe("deno task", "deno task dev");
    }
    has(dir, "main.ts").then(|| env_recipe("deno", "deno run -A --watch main.ts")).flatten()
}

fn rust(dir: &Path) -> Option<Recipe> {
    let cargo = read(dir, "Cargo.toml");
    if has(dir, "config.toml") && has(dir, "content") && has(dir, "templates") {
        return recipe("Zola", "zola serve --port $PORT");
    }
    if cargo.is_empty() {
        return None;
    }
    if has(dir, "Dioxus.toml") {
        return recipe("Dioxus", "dx serve --port $PORT");
    }
    if has(dir, "Trunk.toml") || (has(dir, "index.html") && read(dir, "index.html").contains("data-trunk")) {
        return recipe("Trunk", "trunk serve --port $PORT");
    }
    if cargo.contains("[package.metadata.leptos]") || cargo.contains("[[workspace.metadata.leptos]]") {
        return recipe("Leptos", "LEPTOS_SITE_ADDR=127.0.0.1:$PORT cargo leptos watch");
    }
    env_recipe("Cargo", "cargo run")
}

fn go(dir: &Path) -> Option<Recipe> {
    if ["hugo.toml", "hugo.yaml", "hugo.json"].iter().any(|f| has(dir, f)) {
        return recipe("Hugo", "hugo server --port $PORT");
    }
    if !has(dir, "go.mod") {
        return None;
    }
    if has(dir, ".air.toml") {
        return env_recipe("Air", "air");
    }
    env_recipe("Go", "go run .")
}

fn python(dir: &Path) -> Option<Recipe> {
    let deps = read(dir, "pyproject.toml") + &read(dir, "requirements.txt") + &read(dir, "Pipfile");
    let manage = has(dir, "manage.py");
    if deps.is_empty() && !manage {
        return None;
    }
    let run = if has(dir, "uv.lock") {
        "uv run "
    } else if has(dir, "poetry.lock") {
        "poetry run "
    } else {
        ""
    };
    let deps = deps.to_lowercase();
    if manage {
        let py = if run.is_empty() { PYTHON } else { "python" };
        return recipe("Django", format!("{run}{py} manage.py runserver $PORT"));
    }
    let entry = ["main.py", "app.py"].into_iter().find(|f| has(dir, f))?;
    let module = entry.trim_end_matches(".py");
    if deps.contains("fastapi") {
        return recipe("FastAPI", format!("{run}uvicorn {module}:app --reload --port $PORT"));
    }
    if deps.contains("flask") {
        return recipe("Flask", format!("{run}flask --app {module} run --port $PORT"));
    }
    if deps.contains("streamlit") {
        return recipe("Streamlit", format!("{run}streamlit run {entry} --server.port $PORT --server.address localhost"));
    }
    let py = if run.is_empty() { PYTHON } else { "python" };
    env_recipe("Python", format!("{run}{py} {entry}"))
}

fn ruby(dir: &Path) -> Option<Recipe> {
    if !has(dir, "Gemfile") {
        return None;
    }
    if has(dir, "bin/rails") {
        // Windows can't run the script directly.
        let rails = if cfg!(windows) { "ruby bin/rails" } else { "bin/rails" };
        return recipe("Rails", format!("{rails} server -p $PORT"));
    }
    has(dir, "config.ru").then(|| recipe("Rack", "bundle exec rackup -p $PORT")).flatten()
}

fn php(dir: &Path) -> Option<Recipe> {
    if has(dir, "artisan") {
        return recipe("Laravel", "php artisan serve --port=$PORT");
    }
    has(dir, "index.php").then(|| recipe("PHP", "php -S 127.0.0.1:$PORT")).flatten()
}

fn elixir(dir: &Path) -> Option<Recipe> {
    // Phoenix's generated dev config reads PORT.
    read(dir, "mix.exs").contains(":phoenix").then(|| env_recipe("Phoenix", "mix phx.server")).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn project(files: &[(&str, &str)]) -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir()
            .join(format!("blueprint-stack-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        for (name, body) in files {
            let path = dir.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn command(files: &[(&str, &str)]) -> Option<String> {
        let dir = project(files);
        let r = detect(&dir).map(|r| r.command);
        fs::remove_dir_all(dir).unwrap();
        r
    }

    #[test]
    fn node_projects() {
        assert_eq!(command(&[("package.json", r#"{"scripts":{"dev":"vite"}}"#)]).as_deref(), Some(""));
        assert_eq!(
            command(&[("package.json", r#"{"scripts":{"start":"node ."}}"#), ("pnpm-lock.yaml", "")]).as_deref(),
            Some("pnpm start")
        );
        // "dev" outside scripts doesn't count.
        assert_eq!(command(&[("package.json", r#"{"name":"dev"}"#)]), None);
    }

    #[test]
    fn dev_scripts() {
        let run = |files: &[(&str, &str)]| {
            let dir = project(files);
            let r = dev_script_command(&dir);
            fs::remove_dir_all(dir).unwrap();
            r
        };
        assert_eq!(run(&[("package.json", r#"{"scripts":{"dev":"next dev"}}"#)]).as_deref(), Some("npm run dev"));
        assert_eq!(
            run(&[("package.json", r#"{"scripts":{"dev":"vite"}}"#)]).as_deref(),
            Some("npm run dev -- --port $PORT --strictPort")
        );
        assert_eq!(
            run(&[("package.json", r#"{"scripts":{"dev":"NODE_ENV=dev astro dev"}}"#), ("pnpm-lock.yaml", "")])
                .as_deref(),
            Some("pnpm dev --port $PORT")
        );
        assert_eq!(
            run(&[("package.json", r#"{"scripts":{"dev":"vite dev"}}"#), ("bun.lock", "")]).as_deref(),
            Some("bun run dev --port $PORT --strictPort")
        );
        assert_eq!(run(&[("package.json", r#"{"scripts":{"build":"vite build"}}"#)]), None);
    }

    #[test]
    fn rust_projects() {
        assert_eq!(command(&[("Cargo.toml", "[package]")]).as_deref(), Some("cargo run"));
        assert_eq!(
            command(&[("Cargo.toml", "[package]"), ("Trunk.toml", "")]).as_deref(),
            Some("trunk serve --port $PORT")
        );
        assert_eq!(
            command(&[("Cargo.toml", "[package]"), ("Dioxus.toml", "")]).as_deref(),
            Some("dx serve --port $PORT")
        );
        assert_eq!(
            command(&[("Cargo.toml", "[package.metadata.leptos]\nsite-addr = \"x\"")]).as_deref(),
            Some("LEPTOS_SITE_ADDR=127.0.0.1:$PORT cargo leptos watch")
        );
    }

    #[test]
    fn other_stacks() {
        assert_eq!(command(&[("go.mod", "module x")]).as_deref(), Some("go run ."));
        assert_eq!(
            command(&[("manage.py", ""), ("requirements.txt", "django"), ("uv.lock", "")]).as_deref(),
            Some("uv run python manage.py runserver $PORT")
        );
        assert_eq!(
            command(&[("pyproject.toml", "fastapi"), ("main.py", "")]).as_deref(),
            Some("uvicorn main:app --reload --port $PORT")
        );
        assert_eq!(command(&[("Gemfile", ""), ("bin/rails", "")]).as_deref(), Some(if cfg!(windows) { "ruby bin/rails server -p $PORT" } else { "bin/rails server -p $PORT" }));
        assert_eq!(command(&[("artisan", "")]).as_deref(), Some("php artisan serve --port=$PORT"));
        assert_eq!(command(&[("index.html", "<p>")]).as_deref(), Some(format!("{PYTHON} -m http.server $PORT --bind 127.0.0.1").as_str()));
        assert_eq!(command(&[("README.md", "")]), None);
    }
}
