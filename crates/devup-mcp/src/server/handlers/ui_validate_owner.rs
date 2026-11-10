//! Finds the app that owns a single `tsx`, so its tokens are checked against
//! that app's `devup.json` instead of being left unverified at a repository
//! root with several themes.
//!
//! The owner is taken from the first of these that decides it:
//! 1. `sourceName` as a path inside the project (relative or absolute): the
//!    nearest `devup.json` above that location. The named file need not exist.
//! 2. `sourceName` as the tail of a path (`src/Hero.tsx`, `Hero.tsx`): the
//!    project files that end with it, when they all sit under one theme.
//! 3. No usable name: the project files whose text is this `tsx`, when they
//!    all sit under one theme.
//!
//! Anything else (no match, or matches under different themes) stays
//! unverified, and the candidates are named.
use std::path::Path;

use devup_mcp_devup_ui::ui_validate::SourceFile;
use devup_mcp_devup_ui::ui_validate::theme_scopes::nearest_scope;
use serde_json::{Value, json};

use super::project_context::ThemeLookup;
use super::scopes::{NO_THEME_ABOVE, disk_scopes};

/// How many directory entries the search may visit and how many candidate
/// files it may open: a bounded look, not an index of the repository.
const MAX_VISITED: usize = 20_000;
const MAX_OPENED: usize = 256;
const MAX_DEPTH: usize = 12;
const MAX_CANDIDATES: usize = 8;

pub(super) struct Owned {
    pub lookup: ThemeLookup,
    /// How the owner was resolved, for `tokens.owner`.
    pub owner: Option<Value>,
}

/// `sourceName` as a path relative to the project root: a clean relative
/// path as given, or an absolute one that lies inside the root.
fn relative_name(root: &Path, name: &str) -> Option<String> {
    let name = name.trim().replace('\\', "/");
    let absolute = name.starts_with('/') || name.as_bytes().get(1) == Some(&b':');
    // An absolute path may spell the same directory differently (a short
    // 8.3 name, a symlink, another case): compare what both resolve to. The
    // file itself need not exist, so the deepest existing directory decides.
    let resolved = absolute.then(|| {
        let path = Path::new(&name);
        path.ancestors().find_map(|ancestor| {
            let real = std::fs::canonicalize(ancestor).ok()?;
            Some(real.join(path.strip_prefix(ancestor).ok()?))
        })
    });
    if let Some(Some(inside)) = resolved.map(|path| path.and_then(|path| path.strip_prefix(root).ok().map(Path::to_path_buf))) {
        let relative = inside.to_string_lossy().replace('\\', "/");
        return (!relative.is_empty() && relative.split('/').all(|part| part != "..")).then_some(relative);
    }
    let relative = if absolute {
        let strip = |path: &str| path.trim_start_matches("//?/").trim_end_matches('/').to_owned();
        let (root, name) = (strip(&root.to_string_lossy().replace('\\', "/")), strip(&name));
        // Windows paths differ in case and nothing else; elsewhere case counts.
        let inside = if cfg!(windows) {
            name.len() > root.len() && name[..root.len()].eq_ignore_ascii_case(&root)
        } else {
            name.starts_with(&root)
        };
        if !inside || name.as_bytes().get(root.len()) != Some(&b'/') {
            return None;
        }
        name[root.len() + 1..].to_owned()
    } else {
        name
    };
    let clean = !relative.is_empty() && relative.split('/').all(|part| !part.is_empty() && part != ".." && part != ".");
    clean.then_some(relative)
}

/// Source files of the project as `(relative path, byte length)`.
fn project_sources(root: &Path) -> Vec<(String, u64)> {
    let mut found = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0)];
    let mut visited = 0;
    while let Some((dir, depth)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > MAX_VISITED {
                return found;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if depth < MAX_DEPTH && !name.starts_with('.') && !["node_modules", "dist", "build", "target", "df"].contains(&name.as_ref()) {
                    pending.push((entry.path(), depth + 1));
                }
            } else if kind.is_file()
                && matches!(entry.path().extension().and_then(|value| value.to_str()), Some("tsx" | "ts" | "jsx" | "js"))
                && let (Ok(relative), Ok(metadata)) = (entry.path().strip_prefix(root).map(Path::to_path_buf), entry.metadata())
            {
                found.push((relative.to_string_lossy().replace('\\', "/"), metadata.len()));
            }
        }
    }
    found.sort();
    found
}

/// Text as compared: without a byte-order mark, with `\n` line ends and no
/// surrounding blank space.
fn comparable(text: &str) -> String {
    text.trim_start_matches('\u{feff}').replace("\r\n", "\n").trim().to_owned()
}

fn matching_files(root: &Path, name: Option<&str>, tsx: &str) -> (Vec<String>, &'static str) {
    let sources = project_sources(root);
    if let Some(name) = name {
        let tail = format!("/{name}");
        let named: Vec<String> = sources.iter().filter(|(path, _)| path == name || path.ends_with(&tail)).map(|(path, _)| path.clone()).collect();
        if !named.is_empty() {
            return (named, "sourceName-suffix");
        }
    }
    let wanted = comparable(tsx);
    // A file that holds this text is about as long as it: line ends and a
    // trailing newline are the only slack.
    let lines = wanted.matches('\n').count() as u64;
    let (shortest, longest) = (wanted.len() as u64, wanted.len() as u64 + lines + 8);
    let same: Vec<String> = sources
        .iter()
        .filter(|(_, bytes)| (shortest..=longest).contains(bytes))
        .take(MAX_OPENED)
        .filter(|(path, _)| std::fs::read_to_string(root.join(path)).is_ok_and(|content| comparable(&content) == wanted))
        .map(|(path, _)| path.clone())
        .collect();
    (same, "content-match")
}

fn unverified(message: &str, paths: Vec<String>) -> ThemeLookup {
    ThemeLookup { theme: None, devup_json: None, guardrail: Some(json!({ "message": message, "unverifiedFiles": 1, "unverifiedPaths": paths })) }
}

pub(super) fn for_single_tsx(root: &str, source_name: Option<&str>, tsx: &str, lookup: ThemeLookup) -> Result<Owned, String> {
    let root_path = std::fs::canonicalize(root).map_err(|error| format!("projectRoot: {error}"))?;
    let name = source_name.and_then(|name| relative_name(&root_path, name));
    // 1. A location inside the project decides by itself.
    if let Some(path) = name.as_deref().filter(|path| path.contains('/')) {
        let file = SourceFile { path: path.to_owned(), content: String::new() };
        let mut disk = disk_scopes(root, std::slice::from_ref(&file))?;
        if let Some(dir) = nearest_scope(&disk.scopes, path).map(|scope| scope.dir.clone()) {
            let index = disk.scopes.iter().position(|scope| scope.dir == dir).expect("nearest scope");
            let scope = disk.scopes.swap_remove(index);
            let owner = json!({ "resolvedFrom": "sourceName", "path": path, "devupJson": scope.devup_json });
            return Ok(Owned { lookup: ThemeLookup { theme: Some(scope.theme), devup_json: Some(scope.devup_json), guardrail: None }, owner: Some(owner) });
        }
        // Another package's theme is not this file's: unverified, not checked.
        if lookup.theme.is_some() && !disk.fallback_applies(path) {
            return Ok(Owned { lookup: unverified(NO_THEME_ABOVE, vec![path.to_owned()]), owner: None });
        }
    }
    // A single project theme already answers; there is no owner to choose.
    if lookup.theme.is_some() {
        return Ok(Owned { lookup, owner: None });
    }
    // 2 and 3. Look for the file itself: by the tail of its name, then by its text.
    let (matches, resolved_from) = matching_files(&root_path, name.as_deref(), tsx);
    if matches.is_empty() {
        return Ok(Owned { lookup, owner: None });
    }
    let files: Vec<SourceFile> = matches.iter().map(|path| SourceFile { path: path.clone(), content: String::new() }).collect();
    let mut disk = disk_scopes(root, &files)?;
    let owners: Vec<Option<String>> = matches.iter().map(|path| nearest_scope(&disk.scopes, path).map(|scope| scope.dir.clone())).collect();
    let first = owners[0].clone();
    if let Some(dir) = first.filter(|_| owners.iter().all(|owner| *owner == owners[0])) {
        let index = disk.scopes.iter().position(|scope| scope.dir == dir).expect("owning scope");
        let scope = disk.scopes.swap_remove(index);
        let owner = json!({ "resolvedFrom": resolved_from, "path": matches[0], "matches": matches.len(), "devupJson": scope.devup_json });
        return Ok(Owned { lookup: ThemeLookup { theme: Some(scope.theme), devup_json: Some(scope.devup_json), guardrail: None }, owner: Some(owner) });
    }
    // Found, but under different themes or under none: say which, and do not
    // pick one.
    let mut guardrail = lookup.guardrail.clone().unwrap_or_else(|| json!({ "message": NO_THEME_ABOVE }));
    guardrail["ownerCandidates"] = json!(matches.iter().take(MAX_CANDIDATES).collect::<Vec<_>>());
    guardrail["ownerResolution"] = json!(format!("{resolved_from} found {} file(s) that no single devup.json governs; pass sourceName as the file's path from projectRoot.", matches.len()));
    Ok(Owned { lookup: ThemeLookup { theme: None, devup_json: None, guardrail: Some(guardrail) }, owner: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_name_becomes_a_path_inside_the_project_or_nothing() {
        let root = Path::new(if cfg!(windows) { r"\\?\C:\Work\repo" } else { "/work/repo" });
        let inside = if cfg!(windows) { r"c:\work\repo\apps\a\src\Hero.tsx" } else { "/work/repo/apps/a/src/Hero.tsx" };
        assert_eq!(relative_name(root, inside).as_deref(), Some("apps/a/src/Hero.tsx"));
        assert_eq!(relative_name(root, "apps\\a\\src\\Hero.tsx").as_deref(), Some("apps/a/src/Hero.tsx"));
        assert_eq!(relative_name(root, "Hero.tsx").as_deref(), Some("Hero.tsx"));
        let outside = if cfg!(windows) { r"C:\Work\repository\apps\a\Hero.tsx" } else { "/work/repository/apps/a/Hero.tsx" };
        for name in [outside, "../apps/a/Hero.tsx", "apps//Hero.tsx", "./Hero.tsx", ""] {
            assert_eq!(relative_name(root, name), None, "{name}");
        }
    }

    #[test]
    fn text_is_compared_without_line_end_or_edge_differences() {
        assert_eq!(comparable("\u{feff}a\r\nb\r\n"), comparable("a\nb"));
        assert_ne!(comparable("a\n b"), comparable("a\nb"));
    }
}
