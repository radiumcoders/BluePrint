//! End-to-end checks of start recipes with real toolchains: detect the
//! project, start it, get an HTTP answer on the port blueprint picked, and
//! find the port free again after shutdown.
//!
//! They need Node, Python and Go on PATH, so they're ignored by default.
//! CI runs them with `cargo test -- --include-ignored`; run them locally the
//! same way. A missing tool fails the test instead of skipping it.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::Project;
use crate::manager::Status;
use crate::process;
use crate::stack;
use crate::testkit::{manager, wait_until};

fn require(tool: &str) {
    let found = std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| {
            ["", ".exe", ".cmd"].iter().any(|ext| dir.join(format!("{tool}{ext}")).is_file())
        }));
    assert!(found, "{tool} isn't on PATH; these end-to-end tests need it");
}

fn project_dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("blueprint-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (file, body) in files {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    dir
}

fn http_get(port: u16) -> String {
    let mut conn = TcpStream::connect(("127.0.0.1", port)).unwrap();
    conn.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    conn.write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n").unwrap();
    let mut out = String::new();
    let _ = conn.read_to_string(&mut out);
    out
}

/// Detect, start, answer, stop. `command` is what detection should suggest
/// ("" for the package.json dev script); `body` is expected in the reply.
/// `port` is offered first, so tests running at once don't race for one.
fn run(name: &str, dir: &Path, command: &str, body: &str, port: u16, startup: Duration) {
    let recipe = stack::detect(dir).expect("a recipe");
    assert_eq!(recipe.command, command);
    let mut m = manager(name);
    let id = m.add(Project { name: name.into(), path: dir.into(), port: None, command: recipe.command });
    let i = m.index(id).unwrap();
    m.entries[i].app_port = Some(port);
    m.start(id);
    wait_until(&mut m, id, startup, "the server to accept connections", |m| {
        m.get(id).unwrap().status() == Status::Running
    });
    let port = m.get(id).unwrap().port().unwrap();
    let reply = http_get(port);
    assert!(reply.starts_with("HTTP/1."), "{reply}");
    assert!(reply.contains(body), "{reply}");

    m.shutdown();
    assert_eq!(m.running_count(), 0);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while process::port_in_use(port) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!process::port_in_use(port), "port {port} still held after shutdown");
    let _ = std::fs::remove_dir_all(dir);
}

const NODE_SERVER: &str = r#"
const port = Number(process.argv.includes("--port") ? process.argv[process.argv.indexOf("--port") + 1] : process.env.PORT);
require("http").createServer((_, res) => res.end("hello from node")).listen(port, "127.0.0.1");
"#;

#[test]
#[ignore = "needs Node; run with --include-ignored"]
fn node_dev_script_reading_port() {
    require("node");
    require("npm");
    let dir = project_dir("node", &[
        ("package.json", r#"{"name":"e2e","scripts":{"dev":"node server.js"}}"#),
        ("server.js", NODE_SERVER),
    ]);
    run("node", &dir, "", "hello from node", 4960, Duration::from_secs(30));
}

/// A dev script whose tool ignores `PORT` and must be passed `--port`, the
/// way Vite is: a stand-in `vite` in node_modules/.bin that only reads its
/// arguments.
#[test]
#[ignore = "needs Node; run with --include-ignored"]
fn dev_tool_given_port_flag() {
    require("node");
    require("npm");
    let server = NODE_SERVER.replace("process.env.PORT", "NaN");
    let dir = project_dir("vite", &[
        ("package.json", r#"{"name":"e2e","scripts":{"dev":"vite"}}"#),
        ("fake-vite.js", &server),
        ("node_modules/.bin/vite", "#!/bin/sh\nexec node \"$(dirname \"$0\")/../../fake-vite.js\" \"$@\"\n"),
        ("node_modules/.bin/vite.cmd", "@node \"%~dp0\\..\\..\\fake-vite.js\" %*\r\n"),
    ]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let bin = dir.join("node_modules/.bin/vite");
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert_eq!(
        stack::dev_script_command(&dir).as_deref(),
        Some("npm run dev -- --port $PORT --strictPort")
    );
    run("vite", &dir, "", "hello from node", 4965, Duration::from_secs(30));
}

#[test]
#[ignore = "needs Python; run with --include-ignored"]
fn static_site() {
    let python = if cfg!(windows) { "python" } else { "python3" };
    require(python);
    let dir = project_dir("static", &[("index.html", "<p>hello from a static site</p>")]);
    let command = format!("{python} -m http.server $PORT --bind 127.0.0.1");
    run("static", &dir, &command, "hello from a static site", 4970, Duration::from_secs(30));
}

#[test]
#[ignore = "needs Go; run with --include-ignored"]
fn go_module_reading_port() {
    require("go");
    let dir = project_dir("go", &[
        ("go.mod", "module e2e\n\ngo 1.21\n"),
        (
            "main.go",
            r#"package main

import (
	"net/http"
	"os"
)

func main() {
	http.HandleFunc("/", func(w http.ResponseWriter, _ *http.Request) { w.Write([]byte("hello from go")) })
	http.ListenAndServe("127.0.0.1:"+os.Getenv("PORT"), nil)
}
"#,
        ),
    ]);
    // `go run` compiles first, which can take a while on a cold cache.
    run("go", &dir, "go run .", "hello from go", 4975, Duration::from_secs(180));
}
