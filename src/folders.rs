//! Finding project folders for the add dialog.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Folder {
    pub name: String,
    pub path: PathBuf,
    /// Short markers like "node" / "git" so project folders stand out.
    pub tags: Vec<&'static str>,
}

const MARKERS: &[(&str, &str)] = &[
    ("package.json", "node"),
    ("deno.json", "deno"),
    ("Cargo.toml", "rust"),
    ("go.mod", "go"),
    ("pyproject.toml", "python"),
    (".git", "git"),
];

pub fn tags_for(dir: &Path) -> Vec<&'static str> {
    MARKERS.iter().filter(|(f, _)| dir.join(f).exists()).map(|(_, t)| *t).collect()
}

/// Visible subfolders of `root`, sorted by name.
pub fn list(root: &Path) -> Vec<Folder> {
    let Ok(rd) = fs::read_dir(root) else { return Vec::new() };
    let mut v: Vec<Folder> = rd
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir()) // follows symlinks
        .map(|e| {
            let path = e.path();
            Folder { name: e.file_name().to_string_lossy().into_owned(), tags: tags_for(&path), path }
        })
        .filter(|f| !f.name.starts_with('.'))
        .collect();
    v.sort_by_key(|f| f.name.to_lowercase());
    v
}

/// Case-insensitive subsequence match ("prt" matches "portboard").
pub fn fuzzy(needle: &str, hay: &str) -> bool {
    let mut hay = hay.chars().flat_map(char::to_lowercase);
    needle.chars().flat_map(char::to_lowercase).all(|n| hay.any(|h| h == n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_match() {
        assert!(fuzzy("prt", "portboard"));
        assert!(fuzzy("PB", "portboard"));
        assert!(!fuzzy("xyz", "portboard"));
        assert!(fuzzy("", "anything"));
    }

    #[test]
    fn lists_and_tags() {
        let root = std::env::temp_dir().join(format!("portboard-folders-{}", std::process::id()));
        fs::create_dir_all(root.join("beta")).unwrap();
        fs::create_dir_all(root.join("Alpha")).unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::write(root.join("beta/package.json"), "{}").unwrap();
        let names: Vec<_> = list(&root).into_iter().map(|f| (f.name, f.tags)).collect();
        assert_eq!(names, [("Alpha".to_string(), vec![]), ("beta".to_string(), vec!["node"])]);
        fs::remove_dir_all(root).unwrap();
    }
}
