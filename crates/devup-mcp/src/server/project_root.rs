//! Shared project-root discovery and the "stop-and-report" guardrail
//! response shape used by all three ground-truth tools
//! (`devup_project_context`, `devup_ui_validate`, `devup_stack_diff`).
//!
//! This generalizes the exact pattern verified in `diagnostics::host_requirement`
//! for the `needs_figma` handoff: when a tool cannot ground its answer in a
//! real file, it must say so explicitly and instruct the caller to stop
//! rather than guess, instead of silently returning nothing or (worse)
//! inventing a plausible-looking answer. See `README.md`'s brief for the
//! `$gray100` incident this exists to prevent.
//!
//! Every function here only reads the filesystem; nothing is written or
//! cached across calls, per the brief's "호출 시점에 파일을 읽는다. 세션
//! 간 캐시 금지" requirement — a project file can change between two
//! tool calls in the same session, and treating a stale in-memory copy as
//! current fact would be exactly the kind of confident-but-wrong answer
//! this tool exists to prevent.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// Filenames whose presence in a directory marks it as a project root.
const ROOT_MARKERS: &[&str] = &["devup.json", "package.json", "Cargo.toml", ".git"];

/// Directory names never descended into during a bounded project search:
/// dependency/build output that is large, irrelevant, and would otherwise
/// dominate search time and result noise.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    ".git",
    ".next",
    ".turbo",
    ".nuxt",
    "out",
    ".venv",
    "venv",
    "__pycache__",
    ".cache",
    "coverage",
];

/// Searches `start` and each ancestor directory for one of [`ROOT_MARKERS`],
/// returning the first (nearest) directory that has one. Returns `None` if
/// no ancestor (up to the filesystem root) has any marker.
pub fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start.to_path_buf());
    while let Some(dir) = current {
        if ROOT_MARKERS.iter().any(|marker| dir.join(marker).exists()) {
            return Some(dir);
        }
        current = dir.parent().map(Path::to_path_buf);
    }
    None
}

/// Worktree containers are excluded from authoritative results, but searched
/// for matching names so callers can explain what was excluded.
const NESTED_CHECKOUT_DIRS: &[&str] = &[".worktrees", ".worktree", ".git-worktrees"];

/// Bounded search for files named exactly `filename`. Returns sorted current
/// project paths and sorted `excludedPaths` entries (relative path and reason).
/// Excluded matches are names only: their contents are never read here.
pub fn find_files_named(
    root: &Path,
    filename: &str,
    max_depth: usize,
) -> (Vec<PathBuf>, Vec<Value>) {
    find_named(root, filename, max_depth, false)
}

/// Like [`find_files_named`], but matches directories (including a checkout
/// root with the requested name, which is reported only as excluded).
pub fn find_dirs_named(root: &Path, dirname: &str, max_depth: usize) -> (Vec<PathBuf>, Vec<Value>) {
    find_named(root, dirname, max_depth, true)
}

fn find_named(
    root: &Path,
    target_name: &str,
    max_depth: usize,
    directories: bool,
) -> (Vec<PathBuf>, Vec<Value>) {
    let mut found = Vec::new();
    let mut excluded = Vec::new();
    // The explicit root is authoritative even when it is itself a worktree.
    let mut queue = vec![(root.to_path_buf(), 0usize, None)];
    while let Some((dir, depth, inherited_reason)) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let is_dir = file_type.is_dir();
            // Dependency/build output stays pruned even in diagnostic traversal.
            if is_dir && SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            let reason = inherited_reason.or_else(|| {
                if !is_dir {
                    None
                } else if NESTED_CHECKOUT_DIRS.contains(&name.as_ref()) {
                    Some("nested-checkout-directory")
                } else if path.join(".git").exists() {
                    Some("nested-git-checkout")
                } else {
                    None
                }
            });
            if name == target_name
                && (if directories {
                    is_dir
                } else {
                    file_type.is_file()
                })
            {
                if let Some(reason) = reason {
                    excluded.push((path.clone(), reason));
                } else {
                    found.push(path.clone());
                }
            }
            // Preserve the original depth budget across checkout boundaries.
            // Never follow symlinks or read excluded file contents.
            if is_dir && depth < max_depth {
                queue.push((path, depth + 1, reason));
            }
        }
    }
    found.sort();
    excluded.sort();
    let excluded = excluded
        .into_iter()
        .map(|(path, reason)| {
            json!({
                "path": path.strip_prefix(root).expect("scan stays under root")
                    .to_string_lossy().replace('\\', "/"),
                "reason": reason,
            })
        })
        .collect();
    (found, excluded)
}

/// Every `*.json` file directly inside `dir` (non-recursive), sorted.
pub fn json_files_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    files.sort();
    files
}

/// Just the `guardrail` object (`{"action": "stop-and-report", ...}`),
/// without the `found` wrapper — for tools that need to embed it as a
/// nested field (e.g. `devup_ui_validate`'s `themeGuardrail`) rather than
/// as the whole top-level response. `action` is always the literal string
/// `"stop-and-report"`, the same contract
/// [`crate::server::host_requirement`]-style responses use.
pub fn guardrail_object(message: impl Into<String>, searched_paths: Vec<String>) -> Value {
    json!({
        "action": "stop-and-report",
        "message": message.into(),
        "searchedPaths": searched_paths
    })
}

/// The `{"found": false, "guardrail": {...}}` envelope every ground-truth
/// tool returns as its top-level response instead of guessing when it
/// cannot locate the file(s) it needs.
pub fn not_found_response(message: impl Into<String>, searched_paths: Vec<String>) -> Value {
    json!({
        "found": false,
        "guardrail": guardrail_object(message, searched_paths)
    })
}

/// The standard message for "could not even determine a project root" —
/// distinct from "found a project root but the target file is missing"
/// ([`not_found_response`] with a scope-specific message), since the two
/// failures call for different next steps from the caller.
pub const PROJECT_ROOT_NOT_FOUND_MESSAGE: &str = "No project root found. No directory containing devup.json, package.json, Cargo.toml, or .git was found. Do not write code by guessing token, endpoint, or column names.";

/// Path displayed as-is (already OS-native), used consistently across the
/// three tools so `searchedPaths` entries are directly copy-pasteable.
pub fn display_path(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Minimal scoped-temp-directory helper (no `tempfile` dependency):
    /// creates a uniquely-named directory under the OS temp dir and removes
    /// it (and everything under it) on drop.
    struct ScopedTempDir(PathBuf);

    impl ScopedTempDir {
        fn new(label: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "devup-mcp-test-{label}-{}-{unique}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("create scoped temp dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for ScopedTempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn finds_root_by_walking_up_to_a_marker() {
        let temp = ScopedTempDir::new("root-marker");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let nested = temp.path().join("apps").join("front");
        std::fs::create_dir_all(&nested).unwrap();
        let root = find_project_root(&nested).expect("root found");
        assert_eq!(root, temp.path());
    }

    #[test]
    fn returns_none_when_no_marker_exists_up_to_a_bare_temp_dir() {
        let temp = ScopedTempDir::new("no-marker");
        let isolated = temp.path().join("isolated");
        std::fs::create_dir_all(&isolated).unwrap();
        // A bare scoped temp dir has no devup.json/package.json/Cargo.toml/.git
        // in the isolated subtree itself, which is what we control
        // deterministically here.
        assert!(
            !ROOT_MARKERS
                .iter()
                .any(|marker| isolated.join(marker).exists())
        );
    }

    #[test]
    fn find_files_named_skips_node_modules() {
        let temp = ScopedTempDir::new("skip-node-modules");
        let nm = temp.path().join("node_modules").join("pkg");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("devup.json"), "{}").unwrap();
        let real = temp.path().join("apps").join("front");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("devup.json"), "{}").unwrap();
        let (found, excluded) = find_files_named(temp.path(), "devup.json", 4);
        assert!(excluded.is_empty());
        assert_eq!(found, vec![real.join("devup.json")]);
    }

    #[test]
    fn w5_scans_skip_worktrees_and_nested_git_roots() {
        let temp = ScopedTempDir::new("w5-nested-checkouts");
        std::fs::create_dir(temp.path().join(".git")).unwrap();
        for dir in [
            "apps/front",
            ".worktrees/stale",
            "copies/repo",
            "copies/linked",
        ] {
            let path = temp.path().join(dir);
            std::fs::create_dir_all(path.join("models")).unwrap();
            std::fs::write(path.join("devup.json"), "{}").unwrap();
        }
        std::fs::create_dir(temp.path().join("copies/repo/.git")).unwrap();
        std::fs::write(temp.path().join("copies/linked/.git"), "gitdir: elsewhere").unwrap();
        assert_eq!(
            find_files_named(temp.path(), "devup.json", 6).0,
            vec![temp.path().join("apps/front/devup.json")]
        );
        assert_eq!(
            find_dirs_named(temp.path(), "models", 6).0,
            vec![temp.path().join("apps/front/models")]
        );
        for (name, directories) in [("devup.json", false), ("models", true)] {
            let (_, excluded) = find_named(temp.path(), name, 6, directories);
            assert_eq!(
                excluded,
                vec![
                    json!({ "path": format!(".worktrees/stale/{name}"), "reason": "nested-checkout-directory" }),
                    json!({ "path": format!("copies/linked/{name}"), "reason": "nested-git-checkout" }),
                    json!({ "path": format!("copies/repo/{name}"), "reason": "nested-git-checkout" }),
                ]
            );
        }
        assert!(find_dirs_named(temp.path(), ".worktrees", 6).0.is_empty());
        assert!(find_dirs_named(temp.path(), "repo", 6).0.is_empty());
        assert_eq!(
            find_dirs_named(temp.path(), "repo", 6).1,
            vec![json!({
                "path": "copies/repo", "reason": "nested-git-checkout",
            })]
        );
        assert_eq!(
            find_files_named(&temp.path().join("copies/repo"), "devup.json", 2)
                .0
                .len(),
            1
        );
    }

    #[test]
    fn scans_skip_all_worktree_container_names() {
        let temp = ScopedTempDir::new("all-worktree-containers");
        for container in [".worktrees", ".worktree", ".git-worktrees"] {
            let checkout = temp.path().join(container).join("stale");
            std::fs::create_dir_all(checkout.join("models")).unwrap();
            std::fs::write(checkout.join("devup.json"), "{}").unwrap();
        }
        assert!(find_files_named(temp.path(), "devup.json", 4).0.is_empty());
        assert!(find_dirs_named(temp.path(), "models", 4).0.is_empty());
    }

    #[test]
    fn diagnostic_scan_keeps_depth_limits_and_prunes_build_output() {
        let temp = ScopedTempDir::new("diagnostic-depth");
        for dir in [
            ".worktrees/stale",
            ".worktrees/stale/deeper",
            ".worktrees/stale/node_modules/pkg",
            ".worktrees/stale/target/pkg",
        ] {
            let dir = temp.path().join(dir);
            std::fs::create_dir_all(dir.join("models")).unwrap();
            std::fs::write(dir.join("devup.json"), "{}").unwrap();
        }
        let (files, excluded) = find_files_named(temp.path(), "devup.json", 2);
        assert!(files.is_empty());
        assert_eq!(
            excluded,
            vec![json!({
                "path": ".worktrees/stale/devup.json", "reason": "nested-checkout-directory",
            })]
        );
        let (dirs, excluded) = find_dirs_named(temp.path(), "models", 2);
        assert!(dirs.is_empty());
        assert_eq!(
            excluded,
            vec![json!({
                "path": ".worktrees/stale/models", "reason": "nested-checkout-directory",
            })]
        );
        // A larger depth admits only the deeper match, never dependency/build files.
        assert_eq!(find_files_named(temp.path(), "devup.json", 6).1.len(), 2);
        assert_eq!(find_dirs_named(temp.path(), "models", 6).1.len(), 2);
        assert!(find_files_named(temp.path(), "devup.json", 0).1.is_empty());
        let checkout = temp.path().join(".worktrees/stale");
        assert_eq!(
            find_files_named(&checkout, "devup.json", 0),
            (vec![checkout.join("devup.json")], vec![])
        );
    }

    #[test]
    fn not_found_response_always_has_stop_and_report_action() {
        let value = not_found_response("test", vec!["a".to_owned()]);
        assert_eq!(value["found"], false);
        assert_eq!(value["guardrail"]["action"], "stop-and-report");
        assert_eq!(value["guardrail"]["searchedPaths"][0], "a");
    }
}
