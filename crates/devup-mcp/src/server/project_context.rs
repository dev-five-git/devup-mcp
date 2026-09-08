//! `devup_project_context` — the ground-truth reader. Reads a project's
//! real `devup.json` (theme tokens), `openapi.json` (endpoints/schemas),
//! and Vespertide `models/*.json` (database tables/columns) so an agent
//! never has to guess what identifiers a project actually has.
//!
//! Every scope reads its target file(s) fresh on every call (no session
//! cache — see `project_root.rs`'s module docs) and, when a target file is
//! missing, returns the shared `{"found":false,"guardrail":{...}}`
//! envelope rather than an empty/ambiguous success.

use std::path::{Path, PathBuf};

use devup_mcp_devup_ui::theme::parse_project_theme;
use devup_mcp_figma::{DevupError, ErrorCode};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::project_root::{
    PROJECT_ROOT_NOT_FOUND_MESSAGE, display_path, find_dirs_named, find_files_named,
    find_project_root, guardrail_object, json_files_in, not_found_response,
};

// ---------------------------------------------------------------------
// Nested checkouts (devup-mcp-defects.md D7)
// ---------------------------------------------------------------------

/// Directory names that mark a *nested checkout*: a second, independent
/// working copy of the same project living inside it. `git worktree add
/// .worktrees/<branch>` is the common shape.
///
/// A file under one of these belongs to a different branch's copy of the
/// project. Returned beside the real ones with nothing to tell them apart
/// — and, worse, sorting first, because `.` precedes every letter — they
/// are how an agent ends up writing code against a stale branch's tokens
/// or diffing against a stale branch's spec.
pub(super) const NESTED_CHECKOUT_DIRS: &[&str] = &[".worktrees", ".worktree", ".git-worktrees"];

/// Why `path` does not speak for `root`, or `None` when it does.
///
/// Two independent signals, because neither alone is enough: a directory
/// named like a worktree container catches the conventional layout even
/// when the checkout inside it is not a git one, and a directory below
/// `root` carrying its own `.git` catches every other nested clone or
/// worktree whatever it happens to be called. (`git worktree` writes a
/// `.git` *file* there, a clone or submodule a `.git` *directory*;
/// `exists()` accepts both.)
///
/// `root` itself is never examined — the project being scanned is of
/// course itself a checkout, and often is a worktree.
pub(super) fn nested_checkout_reason(root: &Path, path: &Path) -> Option<&'static str> {
    let relative = path.strip_prefix(root).ok()?;
    let mut current = root.to_path_buf();
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        if components.peek().is_none() {
            // The last component is the file or directory that was found,
            // not one of the directories containing it.
            break;
        }
        let name = component.as_os_str();
        if NESTED_CHECKOUT_DIRS.contains(&name.to_string_lossy().as_ref()) {
            return Some("nested-checkout-directory");
        }
        current.push(name);
        if current.join(".git").exists() {
            return Some("nested-git-checkout");
        }
    }
    None
}

/// Splits scan results into the paths that speak for this project and a
/// JSON list of the ones dropped, each with its reason.
///
/// The dropped list is meant to be put in the response. Quietly returning
/// fewer files than the filesystem holds is its own way of being wrong:
/// the caller cannot tell "this project has one theme" from "this tool
/// decided which theme you meant".
pub(super) fn partition_nested_checkouts(
    root: &Path,
    found: Vec<PathBuf>,
) -> (Vec<PathBuf>, Vec<Value>) {
    let mut kept = Vec::new();
    let mut excluded = Vec::new();
    for path in found {
        match nested_checkout_reason(root, &path) {
            Some(reason) => excluded.push(json!({
                "path": relative_display(root, &path),
                "reason": reason,
            })),
            None => kept.push(path),
        }
    }
    (kept, excluded)
}

/// `(authority, appliesTo)` for a file the scan kept: whether it sits at
/// the project root or belongs to one package inside it, and the directory
/// it governs. A monorepo has no project-wide `devup.json`, and saying
/// which directory each one covers is what lets a caller pick correctly.
fn file_authority(root: &Path, file: &Path) -> (&'static str, String) {
    let directory = file.parent().unwrap_or(root);
    if directory == root {
        ("project-root", ".".to_owned())
    } else {
        ("package-local", relative_display(root, directory))
    }
}

/// Adds `excludedPaths` (what the scan dropped and why) and, when more
/// than one file survived, `authorityNote` (that none of them governs the
/// whole project). Both appear only when they have something to say, so an
/// unambiguous single-file project's response is unchanged.
fn attach_scan_notes(response: &mut Value, excluded: Vec<Value>, kept: usize, filename: &str) {
    let Some(object) = response.as_object_mut() else {
        return;
    };
    if !excluded.is_empty() {
        object.insert("excludedPaths".to_owned(), Value::Array(excluded));
    }
    if kept > 1 {
        object.insert(
            "authorityNote".to_owned(),
            json!(format!(
                "This project has {kept} {filename} files and none of them governs all of it. Each applies to the directory in its own appliesTo; use the one whose appliesTo contains the file being written."
            )),
        );
    }
}

/// The `{"found": false, ...}` envelope, plus the nested checkouts that
/// were dropped. "Nothing found" and "nothing found that this project
/// owns" are different answers and the caller has to be able to tell them
/// apart — otherwise a project whose only `devup.json` lives in
/// `.worktrees/` reads as a project with no theme at all.
fn not_found_with_exclusions(message: &str, searched: Vec<String>, excluded: Vec<Value>) -> Value {
    let message = if excluded.is_empty() {
        message.to_owned()
    } else {
        format!(
            "{message} ({} file(s) were found only inside nested checkouts and skipped — a nested checkout is another branch's copy of this project, not its current source.)",
            excluded.len()
        )
    };
    let mut response = not_found_response(message, searched);
    if !excluded.is_empty()
        && let Some(object) = response.as_object_mut()
    {
        object.insert("excludedPaths".to_owned(), Value::Array(excluded));
    }
    response
}

/// A project's `devup.json` theme, resolved for `devup_ui_validate` — or,
/// when unavailable, the same `{"found":false,"guardrail":{...}}` shape
/// `devup_project_context` would have returned, surfaced under a distinct
/// key so callers can tell "no theme was available, token checks were
/// skipped" apart from "every $token check passed".
pub struct ThemeLookup {
    pub theme: Option<devup_mcp_devup_ui::theme::ProjectTheme>,
    pub guardrail: Option<Value>,
}

/// Resolves the theme `devup_ui_validate` should check `$token` references
/// against: the project root's own `devup.json` if present, otherwise the
/// first `devup.json` found within the project (bounded search), otherwise
/// `None` with an explanatory guardrail. Never caches: reads fresh on every
/// call, per this module's no-session-cache requirement.
pub fn theme_for_validation(project_root: Option<&str>) -> Result<ThemeLookup, DevupError> {
    let start = match project_root {
        Some(root) => PathBuf::from(root),
        None => std::env::current_dir().map_err(|error| {
            DevupError::with_details(
                ErrorCode::DevupInvalidInput,
                "Could not determine the current directory.",
                false,
                json!({ "ioError": error.to_string() }),
            )
        })?,
    };
    let Some(root) = find_project_root(&start) else {
        return Ok(ThemeLookup {
            theme: None,
            guardrail: Some(guardrail_object(
                PROJECT_ROOT_NOT_FOUND_MESSAGE,
                vec![display_path(&start)],
            )),
        });
    };
    let root_level = root.join("devup.json");
    // Nested checkouts are dropped before "the first one found" is taken.
    // `find_files_named` sorts, and `.worktrees` sorts ahead of `apps`, so
    // on a project with a git worktree this used to validate every `$token`
    // against a *stale branch's* theme without saying so.
    let (file, excluded) = if root_level.is_file() {
        (Some(root_level), Vec::new())
    } else {
        let (candidates, excluded) =
            partition_nested_checkouts(&root, find_files_named(&root, "devup.json", 4));
        (candidates.into_iter().next(), excluded)
    };
    let Some(file) = file else {
        let message = if excluded.is_empty() {
            "No devup.json found. $token references cannot be verified, so the unknown-token check is skipped. Do not guess and use tokens that do not exist.".to_owned()
        } else {
            format!(
                "The only devup.json files under this project ({}) are inside nested checkouts — another branch's copy, not this project's current source — so none was used. $token references cannot be verified, so the unknown-token check is skipped. Do not guess and use tokens that do not exist.",
                excluded.len()
            )
        };
        return Ok(ThemeLookup {
            theme: None,
            guardrail: Some(guardrail_object(
                message,
                vec![display_path(&root.join("devup.json"))],
            )),
        });
    };
    let source = std::fs::read_to_string(&file).map_err(|error| {
        DevupError::with_details(
            ErrorCode::DevupInvalidInput,
            "Could not read devup.json.",
            false,
            json!({ "path": display_path(&file), "ioError": error.to_string() }),
        )
    })?;
    let theme = parse_project_theme(&source)?;
    Ok(ThemeLookup {
        theme: Some(theme),
        guardrail: None,
    })
}

pub async fn run(
    scope: &str,
    project_root: Option<&str>,
    filter: Option<&str>,
) -> Result<Value, DevupError> {
    if !["theme", "api", "db", "all"].contains(&scope) {
        return Err(DevupError::new(
            ErrorCode::DevupInvalidInput,
            "scope must be theme, api, db, or all.",
            false,
        ));
    }
    let start = match project_root {
        Some(root) => PathBuf::from(root),
        None => std::env::current_dir().map_err(|error| {
            DevupError::with_details(
                ErrorCode::DevupInvalidInput,
                "Could not determine the current directory.",
                false,
                json!({ "ioError": error.to_string() }),
            )
        })?,
    };
    let Some(root) = find_project_root(&start) else {
        return Ok(not_found_response(
            PROJECT_ROOT_NOT_FOUND_MESSAGE,
            vec![display_path(&start)],
        ));
    };

    match scope {
        "theme" => Ok(theme_scope(&root, filter)),
        "api" => Ok(api_scope(&root, filter)),
        "db" => Ok(db_scope(&root, filter)),
        "all" => {
            let mut all = Map::new();
            all.insert("found".to_owned(), Value::Bool(true));
            all.insert("projectRoot".to_owned(), json!(display_path(&root)));
            all.insert("theme".to_owned(), theme_scope(&root, filter));
            all.insert("api".to_owned(), api_scope(&root, filter));
            all.insert("db".to_owned(), db_scope(&root, filter));
            Ok(Value::Object(all))
        }
        _ => unreachable!("scope validated above"),
    }
}

// ---------------------------------------------------------------------
// theme scope
// ---------------------------------------------------------------------

fn theme_scope(root: &Path, filter: Option<&str>) -> Value {
    let (mut files, excluded) =
        partition_nested_checkouts(root, find_files_named(root, "devup.json", 4));
    files.sort();
    files.dedup();
    if files.is_empty() {
        return not_found_with_exclusions(
            "No devup.json found. Do not write code by guessing color, typography, length, or shadow token names.",
            vec![display_path(&root.join("devup.json"))],
            excluded,
        );
    }
    let mut projects = Vec::new();
    for file in &files {
        let relative = relative_display(root, file);
        let (authority, applies_to) = file_authority(root, file);
        let source = match std::fs::read_to_string(file) {
            Ok(source) => source,
            Err(error) => {
                projects.push(json!({
                    "path": relative,
                    "readError": error.to_string()
                }));
                continue;
            }
        };
        let theme = match parse_project_theme(&source) {
            Ok(theme) => theme,
            Err(error) => {
                projects.push(json!({
                    "path": relative,
                    "parseError": error.message
                }));
                continue;
            }
        };
        let modes = theme.modes();
        let matches_filter = |name: &str| filter.is_none_or(|needle| name.contains(needle));
        let colors = filtered_mode_map(&theme.colors, matches_filter);
        let length = filtered_mode_map(&theme.length, matches_filter);
        let shadow = filtered_mode_map(&theme.shadow, matches_filter);
        let typography = theme
            .typography
            .iter()
            .filter(|(name, _)| matches_filter(name))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect::<Map<_, _>>();
        projects.push(json!({
            "path": relative,
            "authority": authority,
            "appliesTo": applies_to,
            "modes": modes,
            "tokenCount": theme.token_count(),
            "colors": colors,
            "typography": typography,
            "length": length,
            "shadow": shadow,
        }));
    }
    let mut response = json!({
        "found": true,
        "scope": "theme",
        "projectRoot": display_path(root),
        "files": projects,
    });
    attach_scan_notes(&mut response, excluded, files.len(), "devup.json");
    response
}

fn filtered_mode_map(
    map: &std::collections::BTreeMap<String, std::collections::BTreeMap<String, Value>>,
    matches_filter: impl Fn(&str) -> bool,
) -> Map<String, Value> {
    map.iter()
        .map(|(mode, tokens)| {
            let tokens = tokens
                .iter()
                .filter(|(name, _)| matches_filter(name))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect::<Map<_, _>>();
            (mode.clone(), Value::Object(tokens))
        })
        .collect()
}

// ---------------------------------------------------------------------
// api scope
// ---------------------------------------------------------------------

const HTTP_METHODS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "head", "options", "trace",
];

fn api_scope(root: &Path, filter: Option<&str>) -> Value {
    let (files, excluded) =
        partition_nested_checkouts(root, find_files_named(root, "openapi.json", 4));
    if files.is_empty() {
        return not_found_with_exclusions(
            "No openapi.json found. Do not write code by guessing API endpoint or schema names.",
            vec![format!("{} (up to depth 4)", display_path(root))],
            excluded,
        );
    }
    let mut specs = Vec::new();
    for file in &files {
        let relative = relative_display(root, file);
        let (authority, applies_to) = file_authority(root, file);
        let source = match std::fs::read_to_string(file) {
            Ok(source) => source,
            Err(error) => {
                specs.push(json!({ "path": relative, "readError": error.to_string() }));
                continue;
            }
        };
        let parsed: Value = match serde_json::from_str(&source) {
            Ok(value) => value,
            Err(error) => {
                specs.push(json!({ "path": relative, "parseError": error.to_string() }));
                continue;
            }
        };
        specs.push(project_openapi_spec(
            &relative,
            authority,
            &applies_to,
            &parsed,
            filter,
        ));
    }
    let mut response = json!({
        "found": true,
        "scope": "api",
        "projectRoot": display_path(root),
        "specs": specs,
    });
    attach_scan_notes(&mut response, excluded, files.len(), "openapi.json");
    response
}

fn project_openapi_spec(
    relative_path: &str,
    authority: &str,
    applies_to: &str,
    spec: &Value,
    filter: Option<&str>,
) -> Value {
    let matches_filter = |haystack: &str| filter.is_none_or(|needle| haystack.contains(needle));
    let mut endpoints = Vec::new();
    if let Some(paths) = spec.get("paths").and_then(Value::as_object) {
        for (path, methods) in paths {
            let Some(methods) = methods.as_object() else {
                continue;
            };
            for method in HTTP_METHODS {
                let Some(operation) = methods.get(*method) else {
                    continue;
                };
                let operation_id = operation.get("operationId").and_then(Value::as_str);
                let haystack = format!("{path} {} {}", method, operation_id.unwrap_or(""));
                if !matches_filter(&haystack) {
                    continue;
                }
                endpoints.push(json!({
                    "method": method.to_ascii_uppercase(),
                    "path": path,
                    "operationId": operation_id,
                }));
            }
        }
    }
    let mut schemas = Vec::new();
    let schema_container = spec
        .get("components")
        .and_then(|components| components.get("schemas"))
        .or_else(|| spec.get("definitions")); // OpenAPI 2 / Swagger fallback
    if let Some(Value::Object(schema_map)) = schema_container {
        for (name, schema) in schema_map {
            if !matches_filter(name) {
                continue;
            }
            let required = schema
                .get("required")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let properties = schema
                .get("properties")
                .and_then(Value::as_object)
                .map(|props| props.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            schemas.push(json!({
                "name": name,
                "requiredFields": required,
                "properties": properties,
            }));
        }
    }
    json!({
        "path": relative_path,
        "authority": authority,
        "appliesTo": applies_to,
        "endpointCount": endpoints.len(),
        "schemaCount": schemas.len(),
        "endpoints": endpoints,
        "schemas": schemas,
    })
}

// ---------------------------------------------------------------------
// db scope (Vespertide models)
// ---------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct VespertideModel {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    columns: Vec<VespertideColumn>,
}

#[derive(Debug, Deserialize)]
struct VespertideColumn {
    name: String,
    #[serde(rename = "type")]
    column_type: Value,
    #[serde(default)]
    nullable: bool,
    #[serde(default)]
    primary_key: Option<Value>,
    #[serde(default)]
    unique: Option<Value>,
    #[serde(default)]
    foreign_key: Option<Value>,
    #[serde(default)]
    index: Option<Value>,
    #[serde(default)]
    comment: Option<String>,
}

fn db_scope(root: &Path, filter: Option<&str>) -> Value {
    let (model_dirs, excluded) =
        partition_nested_checkouts(root, find_dirs_named(root, "models", 4));
    let mut model_files = Vec::new();
    for dir in &model_dirs {
        model_files.extend(json_files_in(dir));
    }
    model_files.sort();
    model_files.dedup();
    if model_files.is_empty() {
        return not_found_with_exclusions(
            "No Vespertide models (models/*.json) found. Do not write code by guessing table or column names or types.",
            vec![format!(
                "{} (models/*.json, up to depth 4)",
                display_path(root)
            )],
            excluded,
        );
    }
    let matches_filter = |haystack: &str| filter.is_none_or(|needle| haystack.contains(needle));
    let mut tables = Vec::new();
    for file in &model_files {
        let relative = relative_display(root, file);
        let source = match std::fs::read_to_string(file) {
            Ok(source) => source,
            Err(error) => {
                tables.push(json!({ "path": relative, "readError": error.to_string() }));
                continue;
            }
        };
        let model: VespertideModel = match serde_json::from_str(&source) {
            Ok(model) => model,
            Err(error) => {
                // Not every *.json in a `models/` directory is necessarily a
                // Vespertide model (e.g. `vespertide.json` config sitting
                // one level up would not match this dir name, but a stray
                // non-model JSON inside `models/` itself would land here).
                // Report the parse failure rather than silently skipping,
                // so the caller can see exactly why a file didn't surface.
                tables.push(json!({ "path": relative, "parseError": error.to_string() }));
                continue;
            }
        };
        if !matches_filter(&model.name) {
            continue;
        }
        let columns = model.columns.iter().map(column_to_json).collect::<Vec<_>>();
        let enums = model
            .columns
            .iter()
            .filter_map(enum_definition)
            .collect::<Vec<_>>();
        tables.push(json!({
            "path": relative,
            "table": model.name,
            "description": model.description,
            "columns": columns,
            "enums": enums,
        }));
    }
    let mut response = json!({
        "found": true,
        "scope": "db",
        "projectRoot": display_path(root),
        "tables": tables,
    });
    attach_scan_notes(&mut response, excluded, model_dirs.len(), "models/");
    response
}

fn column_to_json(column: &VespertideColumn) -> Value {
    let (type_name, enum_values) = describe_column_type(&column.column_type);
    json!({
        "name": column.name,
        "type": type_name,
        "nullable": column.nullable,
        "primaryKey": column.primary_key.is_some(),
        "unique": column.unique.is_some(),
        "indexed": column.index.is_some(),
        "foreignKey": column.foreign_key,
        "enumValues": enum_values,
        "comment": column.comment,
    })
}

fn enum_definition(column: &VespertideColumn) -> Option<Value> {
    let object = column.column_type.as_object()?;
    if object.get("kind").and_then(Value::as_str) != Some("enum") {
        return None;
    }
    Some(json!({
        "column": column.name,
        "name": object.get("name"),
        "values": object.get("values").cloned().unwrap_or(Value::Null),
    }))
}

/// Returns `(type_name, enum_values)`: for simple string types, the string
/// itself with no enum values; for complex `{"kind": ..., ...}` types, the
/// `kind` string, and — for `kind: "enum"` — the raw `values` array.
fn describe_column_type(column_type: &Value) -> (String, Option<Value>) {
    match column_type {
        Value::String(simple) => (simple.clone(), None),
        Value::Object(object) => {
            let kind = object
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned();
            let enum_values = if kind == "enum" {
                object.get("values").cloned()
            } else {
                None
            };
            (kind, enum_values)
        }
        other => (other.to_string(), None),
    }
}

pub(super) fn relative_display(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| display_path(file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct ScopedTempDir(PathBuf);

    impl ScopedTempDir {
        fn new(label: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "devup-mcp-context-test-{label}-{}-{unique}",
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

    #[tokio::test]
    async fn theme_scope_reads_real_devup_json_tokens() {
        let temp = ScopedTempDir::new("theme-ok");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        std::fs::write(
            temp.path().join("devup.json"),
            r##"{ "theme": { "colors": { "default": { "captionLight": "#999999" } } } }"##,
        )
        .unwrap();
        let result = run("theme", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        assert_eq!(result["found"], true);
        assert_eq!(
            result["files"][0]["colors"]["default"]["captionLight"],
            "#999999"
        );
    }

    #[tokio::test]
    async fn theme_scope_reports_not_found_guardrail_without_devup_json() {
        let temp = ScopedTempDir::new("theme-missing");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let result = run("theme", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        assert_eq!(result["found"], false);
        assert_eq!(result["guardrail"]["action"], "stop-and-report");
    }

    #[tokio::test]
    async fn missing_project_root_reports_guardrail() {
        let temp = ScopedTempDir::new("no-root");
        // No package.json/devup.json/Cargo.toml/.git anywhere under temp.
        let nested = temp.path().join("deep").join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let result = run("theme", Some(&nested.to_string_lossy()), None)
            .await
            .unwrap();
        assert_eq!(result["found"], false);
        assert_eq!(result["guardrail"]["action"], "stop-and-report");
    }

    #[tokio::test]
    async fn api_scope_extracts_endpoints_and_required_fields() {
        let temp = ScopedTempDir::new("api-ok");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        std::fs::write(
            temp.path().join("openapi.json"),
            r##"{
                "paths": {
                    "/users/{id}": {
                        "get": { "operationId": "getUser" }
                    }
                },
                "components": {
                    "schemas": {
                        "User": { "required": ["id", "email"], "properties": { "id": {}, "email": {}, "name": {} } }
                    }
                }
            }"##,
        )
        .unwrap();
        let result = run("api", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        assert_eq!(result["found"], true);
        assert_eq!(result["specs"][0]["endpoints"][0]["operationId"], "getUser");
        assert_eq!(result["specs"][0]["endpoints"][0]["method"], "GET");
        assert_eq!(result["specs"][0]["schemas"][0]["requiredFields"][0], "id");
    }

    #[tokio::test]
    async fn db_scope_extracts_columns_and_enum_values() {
        let temp = ScopedTempDir::new("db-ok");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let models = temp.path().join("apis").join("api").join("models");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(
            models.join("user.json"),
            r##"{
                "name": "user",
                "columns": [
                    { "name": "id", "type": "uuid", "nullable": false, "primary_key": true },
                    { "name": "status", "type": { "kind": "enum", "name": "user_status", "values": ["pending", "active"] }, "nullable": false }
                ]
            }"##,
        )
        .unwrap();
        let result = run("db", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        assert_eq!(result["found"], true);
        let table = &result["tables"][0];
        assert_eq!(table["table"], "user");
        assert_eq!(table["columns"][0]["name"], "id");
        assert_eq!(table["columns"][0]["primaryKey"], true);
        assert_eq!(table["enums"][0]["values"][0], "pending");
    }

    #[tokio::test]
    async fn invalid_scope_is_rejected() {
        let temp = ScopedTempDir::new("bad-scope");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let error = run("bogus", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
    }

    /// Writes the `.worktrees/<branch>/` shape `git worktree add` produces:
    /// a second copy of the project, carrying its own `.git` (a *file* for
    /// a worktree, which is why the check uses `exists()` and not
    /// `is_dir()`).
    fn write_nested_worktree(root: &Path, relative_file: &str, contents: &str) {
        let checkout = root.join(".worktrees").join("revert-some-branch");
        std::fs::create_dir_all(&checkout).unwrap();
        std::fs::write(checkout.join(".git"), "gitdir: ../../.git/worktrees/x").unwrap();
        let file = checkout.join(relative_file);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, contents).unwrap();
    }

    #[tokio::test]
    async fn theme_scope_skips_devup_json_inside_a_nested_worktree() {
        let temp = ScopedTempDir::new("theme-worktree");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let real = temp.path().join("apps").join("front");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(
            real.join("devup.json"),
            r##"{ "theme": { "colors": { "default": { "current": "#111111" } } } }"##,
        )
        .unwrap();
        write_nested_worktree(
            temp.path(),
            "apps/front/devup.json",
            r##"{ "theme": { "colors": { "default": { "stale": "#222222" } } } }"##,
        );

        // The walk itself still descends into `.worktrees` —
        // `project_root.rs`'s SKIP_DIRS does not list it, and that file is
        // outside this worktree's ownership — so both copies really are
        // found and it is the filter below that drops the stale one.
        assert_eq!(find_files_named(temp.path(), "devup.json", 4).len(), 2);

        let result = run("theme", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        let files = result["files"].as_array().unwrap();
        assert_eq!(files.len(), 1, "{result}");
        assert_eq!(files[0]["path"], "apps/front/devup.json");
        assert_eq!(files[0]["appliesTo"], "apps/front");
        assert_eq!(files[0]["authority"], "package-local");
        assert!(
            files[0]["colors"]["default"].get("stale").is_none(),
            "a stale branch's token must never be reported: {result}"
        );
        let excluded = result["excludedPaths"].as_array().unwrap();
        assert_eq!(excluded.len(), 1);
        assert!(
            excluded[0]["path"]
                .as_str()
                .unwrap()
                .contains(".worktrees/"),
            "{result}"
        );
    }

    #[tokio::test]
    async fn api_and_db_scopes_skip_nested_worktrees_too() {
        let temp = ScopedTempDir::new("api-db-worktree");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let api = temp.path().join("apis").join("api");
        std::fs::create_dir_all(api.join("models")).unwrap();
        std::fs::write(
            api.join("openapi.json"),
            r##"{ "paths": { "/current": { "get": { "operationId": "current" } } } }"##,
        )
        .unwrap();
        std::fs::write(
            api.join("models").join("user.json"),
            r##"{ "name": "user", "columns": [ { "name": "id", "type": "uuid" } ] }"##,
        )
        .unwrap();
        write_nested_worktree(
            temp.path(),
            "apis/api/openapi.json",
            r##"{ "paths": { "/stale": { "get": { "operationId": "stale" } } } }"##,
        );
        write_nested_worktree(
            temp.path(),
            "apis/api/models/ghost.json",
            r##"{ "name": "ghost", "columns": [ { "name": "id", "type": "uuid" } ] }"##,
        );

        let api_result = run("api", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        let specs = api_result["specs"].as_array().unwrap();
        assert_eq!(specs.len(), 1, "{api_result}");
        assert_eq!(specs[0]["path"], "apis/api/openapi.json");
        assert_eq!(specs[0]["endpoints"][0]["operationId"], "current");
        assert_eq!(api_result["excludedPaths"].as_array().unwrap().len(), 1);

        let db_result = run("db", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        let tables = db_result["tables"].as_array().unwrap();
        assert_eq!(tables.len(), 1, "{db_result}");
        assert_eq!(tables[0]["table"], "user");
        assert_eq!(db_result["excludedPaths"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn theme_for_validation_never_picks_a_stale_branch_over_the_real_one() {
        // `find_files_named` sorts, and `.worktrees` sorts ahead of `apps`,
        // so "the first devup.json found" used to be the stale branch's.
        let temp = ScopedTempDir::new("validation-worktree");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let real = temp.path().join("apps").join("front");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(
            real.join("devup.json"),
            r##"{ "theme": { "colors": { "default": { "current": "#111111" } } } }"##,
        )
        .unwrap();
        write_nested_worktree(
            temp.path(),
            "apps/front/devup.json",
            r##"{ "theme": { "colors": { "default": { "stale": "#222222" } } } }"##,
        );

        let lookup = theme_for_validation(Some(&temp.path().to_string_lossy())).unwrap();
        let theme = lookup.theme.expect("the project's own devup.json");
        assert!(theme.contains_token("current"));
        assert!(
            !theme.contains_token("stale"),
            "validated against a nested checkout's theme"
        );
    }

    #[tokio::test]
    async fn a_theme_that_exists_only_in_a_nested_checkout_says_so() {
        let temp = ScopedTempDir::new("only-worktree-theme");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        write_nested_worktree(
            temp.path(),
            "apps/front/devup.json",
            r##"{ "theme": { "colors": { "default": { "stale": "#222222" } } } }"##,
        );

        let result = run("theme", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        assert_eq!(result["found"], false);
        assert!(
            result["guardrail"]["message"]
                .as_str()
                .unwrap()
                .contains("nested checkout"),
            "{result}"
        );
        assert_eq!(result["excludedPaths"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn one_theme_needs_no_authority_note_and_several_do() {
        let temp = ScopedTempDir::new("authority-note");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let front = temp.path().join("apps").join("front");
        std::fs::create_dir_all(&front).unwrap();
        std::fs::write(front.join("devup.json"), r##"{"theme":{}}"##).unwrap();

        let single = run("theme", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        assert!(single.get("authorityNote").is_none(), "{single}");
        assert!(single.get("excludedPaths").is_none(), "{single}");

        let admin = temp.path().join("apps").join("admin");
        std::fs::create_dir_all(&admin).unwrap();
        std::fs::write(admin.join("devup.json"), r##"{"theme":{}}"##).unwrap();
        let several = run("theme", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        assert!(
            several["authorityNote"]
                .as_str()
                .unwrap()
                .contains("appliesTo"),
            "{several}"
        );
    }

    #[tokio::test]
    async fn all_scope_combines_every_axis() {
        let temp = ScopedTempDir::new("all-scope");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        std::fs::write(temp.path().join("devup.json"), r##"{"theme":{}}"##).unwrap();
        let result = run("all", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        assert_eq!(result["found"], true);
        assert!(result.get("theme").is_some());
        assert!(result.get("api").is_some());
        assert!(result.get("db").is_some());
    }
}
