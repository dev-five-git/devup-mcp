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
                "현재 디렉터리를 확인하지 못했습니다.",
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
    let file = if root_level.is_file() {
        Some(root_level)
    } else {
        find_files_named(&root, "devup.json", 4).into_iter().next()
    };
    let Some(file) = file else {
        return Ok(ThemeLookup {
            theme: None,
            guardrail: Some(guardrail_object(
                "devup.json을 찾지 못했습니다. $token 참조를 검증할 수 없어 unknown-token 검사를 건너뜁니다. 존재하지 않는 토큰을 추측해서 사용하지 마세요.",
                vec![display_path(&root.join("devup.json"))],
            )),
        });
    };
    let source = std::fs::read_to_string(&file).map_err(|error| {
        DevupError::with_details(
            ErrorCode::DevupInvalidInput,
            "devup.json을 읽지 못했습니다.",
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
            "scope는 theme, api, db 또는 all이어야 합니다.",
            false,
        ));
    }
    let start = match project_root {
        Some(root) => PathBuf::from(root),
        None => std::env::current_dir().map_err(|error| {
            DevupError::with_details(
                ErrorCode::DevupInvalidInput,
                "현재 디렉터리를 확인하지 못했습니다.",
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
    let mut files = find_files_named(root, "devup.json", 4);
    if !root.join("devup.json").is_file() {
        // find_files_named already includes root/devup.json if present via
        // the breadth-first walk starting at root itself; this branch only
        // guards against a root walk that (by construction) never omits
        // depth-0 files, kept as a defensive no-op.
    }
    files.sort();
    files.dedup();
    if files.is_empty() {
        return not_found_response(
            "devup.json을 찾지 못했습니다. 색상·타이포그래피·길이·그림자 토큰 이름을 추측해서 코드를 작성하지 마세요.",
            vec![display_path(&root.join("devup.json"))],
        );
    }
    let mut projects = Vec::new();
    for file in &files {
        let relative = relative_display(root, file);
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
            "modes": modes,
            "tokenCount": theme.token_count(),
            "colors": colors,
            "typography": typography,
            "length": length,
            "shadow": shadow,
        }));
    }
    json!({
        "found": true,
        "scope": "theme",
        "projectRoot": display_path(root),
        "files": projects,
    })
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
    let files = find_files_named(root, "openapi.json", 4);
    if files.is_empty() {
        return not_found_response(
            "openapi.json을 찾지 못했습니다. API 엔드포인트나 스키마 이름을 추측해서 코드를 작성하지 마세요.",
            vec![format!("{} (up to depth 4)", display_path(root))],
        );
    }
    let mut specs = Vec::new();
    for file in &files {
        let relative = relative_display(root, file);
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
        specs.push(project_openapi_spec(&relative, &parsed, filter));
    }
    json!({
        "found": true,
        "scope": "api",
        "projectRoot": display_path(root),
        "specs": specs,
    })
}

fn project_openapi_spec(relative_path: &str, spec: &Value, filter: Option<&str>) -> Value {
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
    let model_dirs = find_dirs_named(root, "models", 4);
    let mut model_files = Vec::new();
    for dir in &model_dirs {
        model_files.extend(json_files_in(dir));
    }
    model_files.sort();
    model_files.dedup();
    if model_files.is_empty() {
        return not_found_response(
            "Vespertide 모델(models/*.json)을 찾지 못했습니다. 테이블·컬럼 이름이나 타입을 추측해서 코드를 작성하지 마세요.",
            vec![format!(
                "{} (models/*.json, up to depth 4)",
                display_path(root)
            )],
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
    json!({
        "found": true,
        "scope": "db",
        "projectRoot": display_path(root),
        "tables": tables,
    })
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

fn relative_display(root: &Path, file: &Path) -> String {
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
