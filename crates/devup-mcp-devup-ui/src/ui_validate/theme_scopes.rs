//! Which `devup.json` governs which source file.
//!
//! A repository with several apps has several themes, and a `$token` is only
//! meaningful against the theme of the app that file belongs to. Checking
//! `apps/admin` code against `apps/front`'s theme calls real tokens unknown
//! and suggests the other app's token as the fix; checking against none and
//! answering `clean` passes code nobody verified. Each file is therefore
//! matched to the nearest `devup.json` above it, and a file with none is
//! reported as unverified rather than counted as checked.
use serde::Serialize;

use super::{SourceFile, UiValidation, structure::validate_sources_with};
use crate::theme::{ProjectTheme, parse_project_theme};

/// One theme and the directory it governs. `dir` is `/`-separated and
/// relative to the validated root; the root itself is the empty string.
pub struct ThemeScope {
    pub dir: String,
    pub devup_json: String,
    pub theme: ProjectTheme,
}

/// What was, and was not, checked against one theme.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenScope {
    /// `verified`: `$token` references were looked up in `devupJson`.
    /// `unverified`: no theme governs these files, so nothing was looked up.
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devup_json: Option<String>,
    pub files: usize,
    pub referenced_by_tsx: usize,
    pub defined_by_theme: usize,
    /// Named only for unverified files, at most [`MAX_LISTED_PATHS`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

pub const MAX_LISTED_PATHS: usize = 32;

pub struct ScopedValidation {
    pub report: UiValidation,
    /// Verified scopes in `devupJson` order, then the unverified one if any.
    pub scopes: Vec<TokenScope>,
}

impl ScopedValidation {
    pub fn unverified(&self) -> Option<&TokenScope> {
        self.scopes.iter().find(|scope| scope.status == "unverified")
    }
}

fn normalize(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_owned()
}

/// The deepest scope whose directory contains `path`.
pub fn nearest_scope<'s>(scopes: &'s [ThemeScope], path: &str) -> Option<&'s ThemeScope> {
    let path = normalize(path);
    scopes
        .iter()
        .filter(|scope| {
            scope.dir.is_empty()
                || path
                    .strip_prefix(scope.dir.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
        .max_by_key(|scope| scope.dir.len())
}

/// Every `devup.json` supplied in a bundle, each governing its own directory.
pub fn bundle_scopes(files: &[SourceFile]) -> Result<Vec<ThemeScope>, String> {
    let mut scopes = Vec::new();
    for file in files {
        let path = normalize(&file.path);
        let Some(dir) = path
            .strip_suffix("devup.json")
            .filter(|dir| dir.is_empty() || dir.ends_with('/'))
        else {
            continue;
        };
        scopes.push(ThemeScope {
            dir: dir.trim_end_matches('/').to_owned(),
            theme: parse_project_theme(&file.content)
                .map_err(|error| format!("{path}: {error}"))?,
            devup_json: path,
        });
    }
    Ok(scopes)
}

/// Validates each file against its nearest scope. A file no scope contains
/// uses `fallback` when that is `Some` and `fallback_applies` accepts its
/// path; otherwise its token checks are skipped and it is listed as
/// unverified with `unverified_reason`.
pub fn validate_scoped(
    files: &[SourceFile],
    scopes: &[ThemeScope],
    fallback: Option<(&str, &ProjectTheme)>,
    fallback_applies: &dyn Fn(&str) -> bool,
    unverified_reason: &str,
    strict: bool,
) -> ScopedValidation {
    let fallback_for = |path: &str| fallback.filter(|_| fallback_applies(path));
    let label_of = |path: &str| {
        nearest_scope(scopes, path)
            .map(|scope| (scope.devup_json.as_str(), &scope.theme))
            .or_else(|| fallback_for(path))
    };
    let (mut report, references) =
        validate_sources_with(files, strict, &|path| label_of(path).map(|(_, theme)| theme));
    let mut verified = std::collections::BTreeMap::<&str, TokenScope>::new();
    let mut unverified = TokenScope {
        status: "unverified",
        devup_json: None,
        files: 0,
        referenced_by_tsx: 0,
        defined_by_theme: 0,
        paths: Vec::new(),
        reason: Some(unverified_reason.to_owned()),
    };
    for (path, referenced) in &references {
        let scope = match label_of(path) {
            Some((label, theme)) => verified.entry(label).or_insert_with(|| TokenScope {
                status: "verified",
                devup_json: Some(label.to_owned()),
                files: 0,
                referenced_by_tsx: 0,
                defined_by_theme: theme.token_count(),
                paths: Vec::new(),
                reason: None,
            }),
            None => {
                if unverified.paths.len() < MAX_LISTED_PATHS {
                    unverified.paths.push(path.clone());
                }
                &mut unverified
            }
        };
        scope.files += 1;
        scope.referenced_by_tsx += referenced;
    }
    let mut scopes: Vec<_> = verified.into_values().collect();
    report.available_token_count = scopes.iter().map(|scope| scope.defined_by_theme).sum();
    if unverified.files > 0 {
        scopes.push(unverified);
    }
    ScopedValidation { report, scopes }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRONT: &str = r##"{"theme":{"colors":{"default":{"bg":"#F7F3EC"}}}}"##;
    const ADMIN: &str = r##"{"theme":{"colors":{"default":{"adminBg":"#000000","ink":"#111111"}}}}"##;

    fn file(path: &str, content: &str) -> SourceFile {
        SourceFile { path: path.into(), content: content.into() }
    }

    fn bundle() -> Vec<SourceFile> {
        vec![
            file("apps/front/devup.json", FRONT),
            file("apps/admin/devup.json", ADMIN),
            file("apps/front/src/Hero.tsx", r#"export const Hero = () => <Box bg="$bg" />"#),
            file(
                "apps/admin/src/Panel.tsx",
                r#"export const Panel = () => <Box bg="$bg" color="$ink" />"#,
            ),
            file("shared/Badge.tsx", r#"export const Badge = () => <Box bg="$bg" />"#),
        ]
    }

    fn run(files: &[SourceFile]) -> ScopedValidation {
        let scopes = bundle_scopes(files).unwrap();
        validate_scoped(files, &scopes, None, &|_| true, "no devup.json above these files", false)
    }

    fn unknown_token_paths(result: &ScopedValidation) -> Vec<String> {
        let mut paths: Vec<_> = result
            .report
            .violations
            .iter()
            .filter(|violation| violation.rule == "unknown-token")
            .map(|violation| violation.context["path"].as_str().unwrap().to_owned())
            .collect();
        paths.sort();
        paths
    }

    /// `$bg` exists in the front theme and not in the admin one: the same
    /// token is valid in one app and unknown in the other.
    #[test]
    fn each_file_is_checked_against_its_own_apps_theme() {
        let result = run(&bundle());
        assert_eq!(unknown_token_paths(&result), ["apps/admin/src/Panel.tsx"]);
        assert!(!result.report.ok);
    }

    #[test]
    fn a_file_under_no_theme_is_unverified_and_never_counted_as_checked() {
        let result = run(&bundle());
        let summary: Vec<_> = result
            .scopes
            .iter()
            .map(|scope| {
                (
                    scope.status,
                    scope.devup_json.as_deref(),
                    scope.files,
                    scope.referenced_by_tsx,
                    scope.defined_by_theme,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("verified", Some("apps/admin/devup.json"), 1, 2, 2),
                ("verified", Some("apps/front/devup.json"), 1, 1, 1),
                ("unverified", None, 1, 1, 0),
            ]
        );
        let unverified = result.unverified().unwrap();
        assert_eq!(unverified.paths, ["shared/Badge.tsx"]);
        // Its `$bg` produced no finding either way: it was not looked up.
        assert!(
            result
                .report
                .violations
                .iter()
                .all(|violation| violation.context["path"] != "shared/Badge.tsx")
        );
        assert_eq!(result.report.available_token_count, 3);
    }

    #[test]
    fn the_nearest_theme_wins_over_one_further_up() {
        let mut files = bundle();
        files.push(file(
            "devup.json",
            r##"{"theme":{"colors":{"default":{"rootOnly":"#222222"}}}}"##,
        ));
        files.push(file(
            "apps/front/src/Root.tsx",
            r#"export const Root = () => <Box bg="$rootOnly" />"#,
        ));
        let result = run(&files);
        // The root theme governs `shared/`, not the apps that have their own.
        assert_eq!(
            unknown_token_paths(&result),
            ["apps/admin/src/Panel.tsx", "apps/front/src/Root.tsx", "shared/Badge.tsx"]
        );
        assert!(result.unverified().is_none());
    }

    #[test]
    fn a_directory_name_prefix_is_not_an_ancestor() {
        let files = [
            file("apps/front/devup.json", FRONT),
            file("apps/frontier/Card.tsx", r#"export const Card = () => <Box bg="$bg" />"#),
        ];
        let scopes = bundle_scopes(&files).unwrap();
        assert!(nearest_scope(&scopes, "apps/frontier/Card.tsx").is_none());
        assert!(nearest_scope(&scopes, "apps\\front\\src\\Card.tsx").is_some());
    }

    #[test]
    fn a_fallback_theme_applies_only_where_the_caller_allows_it() {
        let files = [
            file("src/Home.tsx", r#"export const Home = () => <Box bg="$missing" />"#),
            file("apps/admin/src/Panel.tsx", r#"export const Panel = () => <Box bg="$missing" />"#),
        ];
        let theme = parse_project_theme(FRONT).unwrap();
        let result = validate_scoped(
            &files,
            &[],
            Some(("packages/ui/devup.json", &theme)),
            &|path| !path.starts_with("apps/admin/"),
            "another package",
            false,
        );
        assert_eq!(unknown_token_paths(&result), ["src/Home.tsx"]);
        assert_eq!(result.unverified().unwrap().paths, ["apps/admin/src/Panel.tsx"]);
    }
}
