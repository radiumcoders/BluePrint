use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Port the portless proxy listens on. 443 gives clean `https://app.localhost`
/// URLs but needs root once (`portless service install`).
pub const DEFAULT_PROXY_PORT: u16 = 443;
/// Unprivileged alternative; URLs then look like `https://app.localhost:1355`.
pub const FALLBACK_PROXY_PORT: u16 = 1355;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_proxy_port")]
    pub proxy_port: u16,
    /// Folder the project picker opens in.
    #[serde(default = "default_projects_root")]
    pub projects_root: PathBuf,
    #[serde(default)]
    pub projects: Vec<Project>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    /// portless app name, e.g. `api.shop` -> `https://api.shop.localhost`.
    pub name: String,
    pub path: PathBuf,
    /// Fixed port for the dev server (`--app-port`). `None` lets portless pick one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Command to run. Empty means `portless run`, i.e. the package.json `dev` script.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
}

fn default_proxy_port() -> u16 {
    DEFAULT_PROXY_PORT
}

fn default_projects_root() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let projects = home.join("Projects");
    if projects.is_dir() { projects } else { home }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            proxy_port: default_proxy_port(),
            projects_root: default_projects_root(),
            projects: Vec::new(),
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        if let Some(p) = std::env::var_os("BLUEPRINT_CONFIG") {
            return PathBuf::from(p);
        }
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("blueprint")
            .join("config.toml")
    }

    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        cfg.projects_root = expand_tilde(&cfg.projects_root);
        for p in &mut cfg.projects {
            p.path = expand_tilde(&p.path);
        }
        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self)?;
        // Write-then-rename so a crash never leaves a half-written config.
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }
}

pub fn expand_tilde(p: &Path) -> PathBuf {
    match p.strip_prefix("~") {
        Ok(rest) => dirs::home_dir()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| p.to_path_buf()),
        Err(_) => p.to_path_buf(),
    }
}

/// Shorten a path for display by replacing the home directory with `~`.
pub fn display_path(p: &Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(rest) = p.strip_prefix(&home)
    {
        if rest.as_os_str().is_empty() {
            return "~".into();
        }
        return format!("~/{}", rest.display());
    }
    p.display().to_string()
}

/// Turn arbitrary text into a valid portless name: lowercase letters, digits,
/// `-` and `.` (dots make subdomains, e.g. `api.shop`).
pub fn slugify(s: &str) -> String {
    let s = s.rsplit('/').next().unwrap_or(s); // drop npm scope: @acme/web -> web
    let mut out = String::new();
    for c in s.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if (c == '.' || c == '-' || c == '_' || c == ' ') && !out.ends_with(['-', '.']) {
            out.push(if c == '.' { '.' } else { '-' });
        }
    }
    out.trim_matches(['-', '.']).to_string()
}

pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("name is required".into());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
    {
        return Err("use only a-z, 0-9, '-' and '.'".into());
    }
    if name
        .split('.')
        .any(|label| label.is_empty() || label.starts_with('-') || label.ends_with('-'))
    {
        return Err("each part must not be empty or start/end with '-'".into());
    }
    Ok(())
}

/// Suggest a name for a folder: package.json `name` if present, else the folder name.
pub fn suggest_name(dir: &Path) -> String {
    let from_pkg = fs::read_to_string(dir.join("package.json"))
        .ok()
        .and_then(|t| package_name(&t))
        .map(|n| slugify(&n))
        .filter(|n| !n.is_empty());
    from_pkg.unwrap_or_else(|| {
        slugify(
            &dir.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        )
    })
}

/// Pull `"name": "..."` out of package.json without a JSON dependency. Only the
/// first top-level-looking match is used, which is good enough for a suggestion.
fn package_name(json: &str) -> Option<String> {
    let idx = json.find("\"name\"")?;
    let rest = json[idx + 6..].trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    Some(rest[..rest.find('"')?].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_cases() {
        assert_eq!(slugify("My Cool_App"), "my-cool-app");
        assert_eq!(slugify("@acme/web"), "web");
        assert_eq!(slugify("api.shop"), "api.shop");
        assert_eq!(slugify("--x--"), "x");
        assert_eq!(slugify("a..b"), "a.b");
    }

    #[test]
    fn validate() {
        assert!(validate_name("api.shop").is_ok());
        assert!(validate_name("my-app2").is_ok());
        assert!(validate_name("").is_err());
        assert!(validate_name("Bad").is_err());
        assert!(validate_name("a..b").is_err());
        assert!(validate_name("-a").is_err());
    }

    #[test]
    fn pkg_name() {
        assert_eq!(
            package_name(r#"{ "name" : "@x/y", "version": "1" }"#).as_deref(),
            Some("@x/y")
        );
        assert_eq!(package_name("{}"), None);
    }

    #[test]
    fn roundtrip() {
        let dir = std::env::temp_dir().join(format!("blueprint-test-{}", std::process::id()));
        let path = dir.join("config.toml");
        let cfg = Config {
            proxy_port: 1400,
            projects_root: "/tmp".into(),
            projects: vec![
                Project {
                    name: "a".into(),
                    path: "/tmp/a".into(),
                    port: Some(3000),
                    command: "pnpm dev".into(),
                },
                Project {
                    name: "b".into(),
                    path: "/tmp/b".into(),
                    port: None,
                    command: String::new(),
                },
            ],
        };
        cfg.save(&path).unwrap();
        let back = Config::load(&path).unwrap();
        assert_eq!(back.proxy_port, 1400);
        assert_eq!(back.projects, cfg.projects);
        fs::remove_dir_all(dir).unwrap();
    }
}
