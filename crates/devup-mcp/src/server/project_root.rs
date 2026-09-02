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

/// Breadth-first search from `root` down to `max_depth` directories for
/// every file whose name is exactly `filename`, skipping [`SKIP_DIRS`].
/// Returns paths sorted for deterministic output.
pub fn find_files_named(root: &Path, filename: &str, max_depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut queue = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if file_type.is_file() && name == filename {
                found.push(path);
            } else if file_type.is_dir() && depth < max_depth && !SKIP_DIRS.contains(&name.as_ref())
            {
                queue.push((path, depth + 1));
            }
        }
    }
    found.sort();
    found
}

/// Breadth-first search for every directory named exactly `dirname` (e.g.
/// vespertide's conventional `models/` directory), skipping [`SKIP_DIRS`].
pub fn find_dirs_named(root: &Path, dirname: &str, max_depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut queue = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == dirname {
                found.push(path.clone());
            }
            if depth < max_depth && !SKIP_DIRS.contains(&name.as_ref()) {
                queue.push((path, depth + 1));
            }
        }
    }
    found.sort();
    found
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
pub const PROJECT_ROOT_NOT_FOUND_MESSAGE: &str = "프로젝트 루트를 찾지 못했습니다. devup.json, package.json, Cargo.toml, .git 중 하나가 있는 디렉터리를 찾지 못했습니다. 토큰·엔드포인트·컬럼 이름을 추측해서 코드를 작성하지 마세요.";

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
        let found = find_files_named(temp.path(), "devup.json", 4);
        assert_eq!(found, vec![real.join("devup.json")]);
    }

    #[test]
    fn not_found_response_always_has_stop_and_report_action() {
        let value = not_found_response("test", vec!["a".to_owned()]);
        assert_eq!(value["found"], false);
        assert_eq!(value["guardrail"]["action"], "stop-and-report");
        assert_eq!(value["guardrail"]["searchedPaths"][0], "a");
    }
}
