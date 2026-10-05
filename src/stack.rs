//! Working out how to start a project that isn't a plain package.json app.
//!
//! portless hands every server its port in the `PORT` environment variable
//! (and adds `--port` only for a few JS frameworks), so a recipe either uses a
//! tool that reads `PORT` itself or passes `$PORT` on the command line.

use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Recipe {
    /// What was detected, e.g. "Trunk" or "Django".
    pub name: &'static str,
    /// Command to run; empty means `portless run` (the package.json dev script).
    pub command: String,
    /// The command doesn't pass a port itself, so the server must read `PORT`.
    pub reads_env: bool,
}

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

/// Whether package.json's `scripts` has an entry named `script`.
fn has_script(pkg: &str, script: &str) -> bool {
    pkg.find("\"scripts\"").is_some_and(|at| {
        let rest = &pkg[at..];
        let block = &rest[..rest.find('}').unwrap_or(rest.len())];
        block.contains(&format!("\"{script}\""))
    })
}

/// True when `portless run` can start the folder on its own.
pub fn has_dev_script(dir: &Path) -> bool {
    has(dir, "portless.json") || has_script(&read(dir, "package.json"), "dev")
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
        .or_else(|| has(dir, "index.html").then(|| recipe("static site", "python3 -m http.server $PORT")).flatten())
}

fn node(dir: &Path) -> Option<Recipe> {
    if has_dev_script(dir) {
        return recipe("dev script", "");
    }
    let pkg = read(dir, "package.json");
    if pkg.is_empty() || !has_script(&pkg, "start") {
        return None;
    }
    let pm = if has(dir, "bun.lock") || has(dir, "bun.lockb") {
        "bun run"
    } else if has(dir, "pnpm-lock.yaml") {
        "pnpm"
    } else if has(dir, "yarn.lock") {
        "yarn"
    } else {
        "npm"
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
        let py = if run.is_empty() { "python3" } else { "python" };
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
        return recipe("Streamlit", format!("{run}streamlit run {entry} --server.port $PORT"));
    }
    let py = if run.is_empty() { "python3" } else { "python" };
    env_recipe("Python", format!("{run}{py} {entry}"))
}

fn ruby(dir: &Path) -> Option<Recipe> {
    if !has(dir, "Gemfile") {
        return None;
    }
    if has(dir, "bin/rails") {
        return recipe("Rails", "bin/rails server -p $PORT");
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
        assert_eq!(command(&[("Gemfile", ""), ("bin/rails", "")]).as_deref(), Some("bin/rails server -p $PORT"));
        assert_eq!(command(&[("artisan", "")]).as_deref(), Some("php artisan serve --port=$PORT"));
        assert_eq!(command(&[("index.html", "<p>")]).as_deref(), Some("python3 -m http.server $PORT"));
        assert_eq!(command(&[("README.md", "")]), None);
    }
}
