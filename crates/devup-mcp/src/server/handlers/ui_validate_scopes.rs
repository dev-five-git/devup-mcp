//! Resolves, from disk, the `devup.json` that governs each validated file.
use std::collections::{BTreeMap, BTreeSet};

use devup_mcp_devup_ui::theme::parse_project_theme;
use devup_mcp_devup_ui::ui_validate::SourceFile;
use devup_mcp_devup_ui::ui_validate::theme_scopes::{ScopedValidation, ThemeScope};
use serde_json::{Value, json};

pub(super) const NO_THEME_ABOVE: &str = "No devup.json sits above these files inside projectRoot, and no single project theme governs them. Their $token references and token-matching advice were not checked.";
pub(super) const NO_THEME_IN_BUNDLE: &str = "No supplied devup.json sits above these files. Their $token references and token-matching advice were not checked. Bundle validation never reads the filesystem.";

/// Themes found by walking up from every file to `root`, and the package
/// directories passed on the way.
pub(super) struct DiskScopes {
    pub scopes: Vec<ThemeScope>,
    /// Directories below `root` that hold a `package.json`: another package's
    /// theme is not this package's, so a project-wide fallback stops here.
    packages: BTreeSet<String>,
}

impl DiskScopes {
    /// Whether a theme found elsewhere in the project may stand in for a file
    /// that has none above it: only when the file is not inside a package of
    /// its own below `root`.
    pub(super) fn fallback_applies(&self, path: &str) -> bool {
        !ancestors(path).any(|dir| !dir.is_empty() && self.packages.contains(dir))
    }
}

/// `a/b/c.tsx` -> `a/b`, `a`, `` (the root).
fn ancestors(path: &str) -> impl Iterator<Item = &str> {
    let mut current = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut done = false;
    std::iter::from_fn(move || {
        if done {
            return None;
        }
        let dir = current;
        match current.rsplit_once('/') {
            Some((parent, _)) => current = parent,
            None if current.is_empty() => done = true,
            None => current = "",
        }
        Some(dir)
    })
}

pub(super) fn disk_scopes(root: &str, files: &[SourceFile]) -> Result<DiskScopes, String> {
    let root = std::fs::canonicalize(root).map_err(|error| format!("projectRoot: {error}"))?;
    let mut seen = BTreeSet::new();
    let mut themes = BTreeMap::new();
    let mut packages = BTreeSet::new();
    for file in files {
        for dir in ancestors(&file.path) {
            if !seen.insert(dir.to_owned()) {
                continue;
            }
            let directory = if dir.is_empty() { root.clone() } else { root.join(dir) };
            if !dir.is_empty() && directory.join("package.json").is_file() {
                packages.insert(dir.to_owned());
            }
            let theme_file = directory.join("devup.json");
            if theme_file.is_file() {
                let label = if dir.is_empty() { "devup.json".to_owned() } else { format!("{dir}/devup.json") };
                let source = std::fs::read_to_string(&theme_file).map_err(|error| format!("{label}: {error}"))?;
                let theme = parse_project_theme(&source).map_err(|error| format!("{label}: {error}"))?;
                themes.insert(dir.to_owned(), ThemeScope { dir: dir.to_owned(), devup_json: label, theme });
            }
        }
    }
    Ok(DiskScopes { scopes: themes.into_values().collect(), packages })
}

/// The `tokens` object for a scoped run. `unknownTokenCheckRan` is true only
/// when every file was checked: a partly checked project did not pass.
pub(super) fn tokens(scoped: &ScopedValidation) -> Value {
    let unverified = scoped.unverified();
    let verified = scoped.scopes.iter().filter(|scope| scope.status == "verified").count();
    json!({
        "referencedByTsx": scoped.report.checked_tokens,
        "definedByTheme": scoped.report.available_token_count,
        "unknownTokenCheckRan": unverified.is_none() && verified > 0,
        "verification": match (verified, unverified.is_some()) {
            (0, _) => "skipped",
            (_, true) => "partial",
            _ => "verified",
        },
        "unverifiedReferences": unverified.map_or(0, |scope| scope.referenced_by_tsx),
        "unverifiedFiles": unverified.map_or(0, |scope| scope.files),
        "scopes": scoped.scopes,
    })
}

/// The guardrail for files no theme governs; `lookup` is the project-wide
/// answer (ambiguous themes, none found), kept because it names the choices.
pub(super) fn guardrail(scoped: &ScopedValidation, lookup: Option<Value>) -> Option<Value> {
    let unverified = scoped.unverified()?;
    let mut guardrail = lookup.unwrap_or_else(|| json!({ "message": unverified.reason }));
    guardrail["unverifiedFiles"] = json!(unverified.files);
    guardrail["unverifiedPaths"] = json!(unverified.paths);
    if unverified.files > unverified.paths.len() {
        guardrail["unverifiedPathsTruncated"] = json!(true);
    }
    Some(guardrail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ancestors_end_at_the_root() {
        assert_eq!(ancestors("apps/front/src/Hero.tsx").collect::<Vec<_>>(), ["apps/front/src", "apps/front", "apps", ""]);
        assert_eq!(ancestors("Hero.tsx").collect::<Vec<_>>(), [""]);
    }

    #[test]
    fn a_fallback_stops_at_another_package() {
        let scopes = DiskScopes { scopes: Vec::new(), packages: BTreeSet::from(["apps/admin".to_owned()]) };
        assert!(!scopes.fallback_applies("apps/admin/src/Panel.tsx"));
        assert!(scopes.fallback_applies("apps/administrator/Panel.tsx"));
        assert!(scopes.fallback_applies("src/Home.tsx"));
    }
}
