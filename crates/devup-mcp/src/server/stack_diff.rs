//! `devup_stack_diff` — cross-layer drift detection across the devup
//! stack (`vespertide model -> sea-orm entity -> vespera route ->
//! openapi.json -> @devup-api client`). This is the one ground-truth tool
//! that cannot be reduced to "read one file and report its contents": it
//! compares independently-authored layers that a human reviewer would
//! normally have to cross-reference by hand.
//!
//! Every check here is text/JSON-based, not a real compiler front end for
//! Rust or TypeScript. That is a deliberate, disclosed limitation, not an
//! oversight: extraction can miss macro-generated routes (e.g.
//! `vespera::export_app!`-merged sub-apps), non-standard formatting, or
//! re-exported client wrappers. Every reported drift and every skipped
//! layer carries an explicit `confidence` (`"low"` or `"medium"`) — never
//! `"high"`, since none of these checks is a real parse — and the tool
//! never claims a clean layer is drift-free with unwarranted certainty;
//! see each layer's doc comment for exactly what it can and cannot see.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use devup_mcp_figma::{DevupError, ErrorCode};
use serde_json::{Value, json};

use super::project_root::{
    PROJECT_ROOT_NOT_FOUND_MESSAGE, display_path, find_dirs_named, find_files_named,
    find_project_root, json_files_in, not_found_response,
};

const ALL_LAYERS: &[&str] = &[
    "db-entity",
    "entity-route",
    "route-openapi",
    "openapi-client",
];

pub async fn run(project_root: Option<&str>, layers: &[String]) -> Result<Value, DevupError> {
    let requested = if layers.is_empty() {
        ALL_LAYERS
            .iter()
            .map(|layer| (*layer).to_owned())
            .collect::<Vec<_>>()
    } else {
        layers.to_vec()
    };
    for layer in &requested {
        if !ALL_LAYERS.contains(&layer.as_str()) {
            return Err(DevupError::with_details(
                ErrorCode::DevupInvalidInput,
                "Each layers entry must be one of db-entity, entity-route, route-openapi, openapi-client.",
                false,
                json!({ "invalidLayer": layer }),
            ));
        }
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

    let model_dirs = find_dirs_named(&root, "models", 5);
    let mut layers_out = serde_json::Map::new();
    for layer in &requested {
        let result = match layer.as_str() {
            "db-entity" => db_entity_layer(&model_dirs),
            "entity-route" => entity_route_layer(&root, &model_dirs),
            "route-openapi" => route_openapi_layer(&root),
            "openapi-client" => openapi_client_layer(&root),
            _ => unreachable!("validated above"),
        };
        layers_out.insert(layer.clone(), result);
    }

    Ok(json!({
        "found": true,
        "projectRoot": display_path(&root),
        "layers": Value::Object(layers_out),
    }))
}

// ---------------------------------------------------------------------
// db-entity: vespertide models/*.json columns vs sea-orm src/models/*.rs
// ---------------------------------------------------------------------

/// Compares each Vespertide model's declared columns against the field
/// names in its generated sea-orm `Model` struct
/// (`<vespertide-project>/src/models/<table>.rs`, per `vespertide.json`'s
/// default `modelExportDir`). Field extraction is a brace-depth text scan
/// for `pub struct Model { ... }`, not a Rust parser, so it can miss
/// fields hidden behind `#[cfg(...)]` or unusual formatting — hence
/// `confidence: "medium"` rather than `"high"`.
fn db_entity_layer(model_dirs: &[PathBuf]) -> Value {
    if model_dirs.is_empty() {
        return json!({
            "checked": false,
            "reason": "No models/ directory found (no Vespertide models).",
            "drifts": [],
        });
    }
    let mut drifts = Vec::new();
    let mut tables_checked = 0usize;
    for models_dir in model_dirs {
        let vespertide_root = models_dir.parent().map(Path::to_path_buf);
        for model_file in json_files_in(models_dir) {
            let Ok(source) = std::fs::read_to_string(&model_file) else {
                continue;
            };
            let Ok(model) = serde_json::from_str::<Value>(&source) else {
                continue;
            };
            let Some(table) = model.get("name").and_then(Value::as_str) else {
                continue;
            };
            let column_names = model
                .get("columns")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|column| column.get("name").and_then(Value::as_str))
                .map(str::to_owned)
                .collect::<BTreeSet<_>>();
            if column_names.is_empty() {
                continue;
            }
            tables_checked += 1;
            let Some(vespertide_root) = &vespertide_root else {
                continue;
            };
            let entity_path = vespertide_root
                .join("src")
                .join("models")
                .join(format!("{table}.rs"));
            let Ok(entity_source) = std::fs::read_to_string(&entity_path) else {
                drifts.push(json!({
                    "table": table,
                    "kind": "entity-not-generated",
                    "message": format!(
                        "No sea-orm entity ({}) found for the {table} model. Check that `vespertide export --orm seaorm` was run.",
                        display_path(&entity_path)
                    ),
                    "confidence": "low",
                }));
                continue;
            };
            let entity_fields = extract_model_struct_fields(&entity_source);
            let missing_in_entity = column_names
                .difference(&entity_fields)
                .cloned()
                .collect::<Vec<_>>();
            let missing_in_model = entity_fields
                .difference(&column_names)
                .cloned()
                .collect::<Vec<_>>();
            if !missing_in_entity.is_empty() || !missing_in_model.is_empty() {
                drifts.push(json!({
                    "table": table,
                    "kind": "column-entity-mismatch",
                    "entityPath": display_path(&entity_path),
                    "columnsMissingInEntity": missing_in_entity,
                    "fieldsMissingInModel": missing_in_model,
                    "confidence": "medium",
                }));
            }
        }
    }
    json!({
        "checked": true,
        "tablesChecked": tables_checked,
        "drifts": drifts,
    })
}

/// Text-scans a sea-orm entity source for `pub struct Model { ... }` and
/// extracts each `pub <field>: <Type>,` line's field name via brace-depth
/// tracking (not a real Rust parser).
fn extract_model_struct_fields(source: &str) -> BTreeSet<String> {
    let mut fields = BTreeSet::new();
    let Some(struct_start) = source.find("struct Model") else {
        return fields;
    };
    let Some(open_brace_offset) = source[struct_start..].find('{') else {
        return fields;
    };
    let body_start = struct_start + open_brace_offset + 1;
    let mut depth = 1i32;
    let mut end = body_start;
    for (offset, character) in source[body_start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = body_start + offset;
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &source[body_start..end];
    for line in body.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("pub ") else {
            continue;
        };
        let Some(colon) = rest.find(':') else {
            continue;
        };
        let field_name = rest[..colon].trim();
        if !field_name.is_empty() && field_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            fields.insert(field_name.to_owned());
        }
    }
    fields
}

// ---------------------------------------------------------------------
// entity-route: does any route file even mention each entity field?
// ---------------------------------------------------------------------

/// For each Vespertide column, checks whether its snake_case name or its
/// PascalCase sea-orm `Column::Variant` form appears as a plain substring
/// anywhere under a sibling `src/routes/` tree. This is a *presence*
/// check, not a semantic one: a column could appear in a comment, an
/// unrelated string, or a route that never actually serializes it, and a
/// column genuinely unused by any route (by design, e.g. an internal-only
/// audit column) will still be flagged. `confidence: "low"` reflects this;
/// treat every reported item as a lead to verify, not a confirmed bug.
fn entity_route_layer(root: &Path, model_dirs: &[PathBuf]) -> Value {
    if model_dirs.is_empty() {
        return json!({
            "checked": false,
            "reason": "No models/ directory found (no Vespertide models).",
            "drifts": [],
        });
    }
    let mut drifts = Vec::new();
    let mut columns_checked = 0usize;
    for models_dir in model_dirs {
        let Some(vespertide_root) = models_dir.parent() else {
            continue;
        };
        let routes_dir = vespertide_root.join("src").join("routes");
        let route_sources = collect_rust_sources(&routes_dir, 6)
            .iter()
            .filter_map(|path| std::fs::read_to_string(path).ok())
            .collect::<Vec<_>>();
        if route_sources.is_empty() {
            drifts.push(json!({
                "kind": "no-routes-dir",
                "message": format!(
                    "No route files under {}, so entity-route correspondence cannot be checked.",
                    display_path(&routes_dir)
                ),
                "confidence": "low",
            }));
            continue;
        }
        for model_file in json_files_in(models_dir) {
            let Ok(source) = std::fs::read_to_string(&model_file) else {
                continue;
            };
            let Ok(model) = serde_json::from_str::<Value>(&source) else {
                continue;
            };
            let Some(table) = model.get("name").and_then(Value::as_str) else {
                continue;
            };
            for column in model
                .get("columns")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(column_name) = column.get("name").and_then(Value::as_str) else {
                    continue;
                };
                columns_checked += 1;
                let pascal = snake_to_pascal(column_name);
                let mentioned = route_sources
                    .iter()
                    .any(|source| source.contains(column_name) || source.contains(&pascal));
                if !mentioned {
                    drifts.push(json!({
                        "table": table,
                        "column": column_name,
                        "kind": "column-never-referenced-in-routes",
                        "message": format!(
                            "No route referencing {table}.{column_name} was found. It may be an intentionally internal-only column."
                        ),
                        "confidence": "low",
                    }));
                }
            }
        }
    }
    let _ = root; // reserved for future cross-app route roots; kept explicit rather than unused
    json!({
        "checked": true,
        "columnsChecked": columns_checked,
        "drifts": drifts,
    })
}

fn snake_to_pascal(input: &str) -> String {
    input
        .split('_')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn collect_rust_sources(dir: &Path, max_depth: usize) -> Vec<PathBuf> {
    find_files_by_extension(dir, "rs", max_depth)
}

fn find_files_by_extension(dir: &Path, extension: &str, max_depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if !dir.is_dir() {
        return found;
    }
    let mut queue = vec![(dir.to_path_buf(), 0usize)];
    const SKIP: &[&str] = &["node_modules", "target", "dist", "build", ".git", ".next"];
    while let Some((current, depth)) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if file_type.is_file()
                && path.extension().and_then(|ext| ext.to_str()) == Some(extension)
            {
                found.push(path);
            } else if file_type.is_dir() && depth < max_depth && !SKIP.contains(&name.as_ref()) {
                queue.push((path, depth + 1));
            }
        }
    }
    found.sort();
    found
}

// ---------------------------------------------------------------------
// route-openapi: #[vespera::route(...)] handlers vs openapi.json paths
// ---------------------------------------------------------------------

/// Scans every `.rs` file under each `src/routes/` tree found in the
/// project for `#[vespera::route(<method> [, path = "..."])]` attributes,
/// derives each handler's URL from Vespera's documented file-structure
/// convention (`src/routes/users.rs` -> `/users`, `src/routes/admin/mod.rs`
/// -> `/admin`, `path = "/{id}"` appended), and compares the resulting
/// `(METHOD, path)` set against `openapi.json`'s `paths`. Attribute
/// extraction is a bracket-balanced text scan for the macro call, not a
/// real Rust/proc-macro parse, so multi-app merges
/// (`vespera::export_app!`/`merge = [...]`) and non-standard route-macro
/// formatting can produce false positives — `confidence: "medium"`.
fn route_openapi_layer(root: &Path) -> Value {
    let routes_dirs = find_dirs_named(root, "routes", 5)
        .into_iter()
        .filter(|dir| dir.join("mod.rs").is_file() || !collect_rust_sources(dir, 0).is_empty())
        .collect::<Vec<_>>();
    let openapi_files = find_files_named(root, "openapi.json", 4);
    if routes_dirs.is_empty() && openapi_files.is_empty() {
        return json!({
            "checked": false,
            "reason": "Found neither src/routes/ nor openapi.json.",
            "drifts": [],
        });
    }

    let mut code_routes = BTreeSet::<(String, String)>::new();
    for routes_dir in &routes_dirs {
        for file in collect_rust_sources(routes_dir, 6) {
            let Ok(source) = std::fs::read_to_string(&file) else {
                continue;
            };
            let Ok(relative) = file.strip_prefix(routes_dir) else {
                continue;
            };
            let prefix = route_url_prefix(relative);
            for (method, path_attr) in extract_vespera_route_attributes(&source) {
                let url = join_route_url(&prefix, path_attr.as_deref());
                code_routes.insert((method.to_ascii_uppercase(), url));
            }
        }
    }

    let mut spec_routes = BTreeSet::<(String, String)>::new();
    let mut specs_checked = Vec::new();
    for file in &openapi_files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        let Ok(spec) = serde_json::from_str::<Value>(&source) else {
            continue;
        };
        specs_checked.push(display_path(file));
        for (method, path) in extract_openapi_path_methods(&spec) {
            spec_routes.insert((method, path));
        }
    }

    if routes_dirs.is_empty() {
        return json!({
            "checked": false,
            "reason": "No src/routes/ found, so the code-side routes cannot be checked.",
            "openapiSpecsFound": specs_checked,
            "drifts": [],
        });
    }
    if openapi_files.is_empty() {
        return json!({
            "checked": false,
            "reason": "No openapi.json found, so there is no spec to compare against.",
            "codeRoutesFound": code_routes.len(),
            "drifts": [],
        });
    }

    let stale_spec = code_routes
        .difference(&spec_routes)
        .map(|(method, path)| json!({ "method": method, "path": path }))
        .collect::<Vec<_>>();
    let stale_code_or_merged = spec_routes
        .difference(&code_routes)
        .map(|(method, path)| json!({ "method": method, "path": path }))
        .collect::<Vec<_>>();

    let mut drifts = Vec::new();
    if !stale_spec.is_empty() {
        drifts.push(json!({
            "kind": "route-missing-from-openapi",
            "message": "A route present in the code is missing from openapi.json. The spec may be stale (rebuild needed).",
            "routes": stale_spec,
            "confidence": "medium",
        }));
    }
    if !stale_code_or_merged.is_empty() {
        drifts.push(json!({
            "kind": "openapi-path-not-found-in-scanned-routes",
            "message": "A path in openapi.json was not found in the scanned route files. It may come from a merged sub-app, or the scan may have missed a non-standard route macro form.",
            "routes": stale_code_or_merged,
            "confidence": "low",
        }));
    }

    json!({
        "checked": true,
        "codeRouteCount": code_routes.len(),
        "openapiRouteCount": spec_routes.len(),
        "openapiSpecsFound": specs_checked,
        "drifts": drifts,
    })
}

/// Extracts `(method, path_attribute)` pairs from every
/// `#[vespera::route(...)]` (or `#[route(...)]` when `vespera::route` is
/// imported directly) attribute in `source`, matched to the very next
/// `pub async fn` per Vespera's "route handlers MUST be `pub async fn`"
/// requirement — attributes not immediately followed by one are ignored.
fn extract_vespera_route_attributes(source: &str) -> Vec<(String, Option<String>)> {
    let mut results = Vec::new();
    let mut search_from = 0usize;
    while let Some(relative) = source[search_from..].find("route(") {
        let start = search_from + relative;
        // Require this `route(` to be a `#[...route(` attribute, not an
        // unrelated identifier ending in `route`. `start` points at the
        // `r` of `route(`, so the text immediately preceding it is either
        // `::` (`#[vespera::route(`) or `[`/whitespace (`#[route(`).
        let before = source[..start].trim_end();
        if !before.ends_with("::") && !before.ends_with('[') {
            search_from = start + "route(".len();
            continue;
        }
        let Some(open_paren) = source[start..].find('(') else {
            break;
        };
        let args_start = start + open_paren + 1;
        let mut depth = 1i32;
        let mut args_end = args_start;
        for (offset, character) in source[args_start..].char_indices() {
            match character {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        args_end = args_start + offset;
                        break;
                    }
                }
                _ => {}
            }
        }
        let args = &source[args_start..args_end];
        search_from = args_end + 1;
        let Some(method) = args
            .split(',')
            .next()
            .map(str::trim)
            .filter(|token| !token.is_empty())
        else {
            continue;
        };
        // Only accept the method token if it looks like a bare identifier
        // (get/post/put/patch/delete), not `path = "..."` appearing first
        // in an unusual ordering.
        if !method.chars().all(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        let path_attr = extract_quoted_value_after(args, "path");
        // Confirm the next non-attribute, non-blank line is `pub async fn`
        // per Vespera's handler requirement; otherwise this `route(...)` is
        // not a real handler attribute (e.g. inside a doc example string).
        let after = &source[search_from..];
        let next_code = after.lines().map(str::trim).find(|line| {
            !line.is_empty()
                && !line.starts_with('#')
                && !line.starts_with("///")
                // Skip the attribute macro's own closing bracket(s), e.g. a
                // lone `]` left on its own line after `route(...)`'s `)`.
                && !line.chars().all(|c| matches!(c, ']' | ')' | ','))
        });
        if next_code.is_some_and(|line| line.starts_with("pub async fn")) {
            results.push((method.to_owned(), path_attr));
        }
    }
    results
}

/// Finds `<key> = "<value>"` inside `source` and returns `<value>`.
fn extract_quoted_value_after(source: &str, key: &str) -> Option<String> {
    let index = source.find(key)?;
    let rest = &source[index + key.len()..];
    let equals = rest.find('=')?;
    let rest = &rest[equals + 1..];
    let first_quote = rest.find('"')?;
    let rest = &rest[first_quote + 1..];
    let second_quote = rest.find('"')?;
    Some(rest[..second_quote].to_owned())
}

/// Vespera's file-structure-to-URL convention: `users.rs` -> `/users`,
/// `mod.rs` (at any nesting) -> the directory path itself, `admin/stats.rs`
/// -> `/admin/stats`. Root `mod.rs` maps to the empty prefix.
fn route_url_prefix(relative_path: &Path) -> String {
    let mut components = relative_path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    if let Some(last) = components.last_mut() {
        if last == "mod.rs" {
            components.pop();
        } else if let Some(stripped) = last.strip_suffix(".rs") {
            *last = stripped.to_owned();
        }
    }
    if components.is_empty() {
        String::new()
    } else {
        format!("/{}", components.join("/"))
    }
}

fn join_route_url(prefix: &str, path_attr: Option<&str>) -> String {
    match path_attr {
        Some(path) if !path.is_empty() => format!("{prefix}{path}"),
        _ if prefix.is_empty() => "/".to_owned(),
        _ => prefix.to_owned(),
    }
}

fn extract_openapi_path_methods(spec: &Value) -> Vec<(String, String)> {
    const METHODS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "options"];
    let mut results = Vec::new();
    if let Some(paths) = spec.get("paths").and_then(Value::as_object) {
        for (path, methods) in paths {
            let Some(methods) = methods.as_object() else {
                continue;
            };
            for method in METHODS {
                if methods.contains_key(*method) {
                    results.push((method.to_ascii_uppercase(), path.clone()));
                }
            }
        }
    }
    results
}

// ---------------------------------------------------------------------
// openapi-client: does the frontend call endpoints the spec has?
// ---------------------------------------------------------------------

/// Scans `.ts`/`.tsx` files (skipping generated `df/` client output and
/// the usual dependency directories) for `@devup-api/fetch`-style calls —
/// `api.get('operationIdOrPath', ...)`, `queryClient.useQuery('get',
/// 'operationIdOrPath', ...)`, `useMutation('post', 'operationIdOrPath',
/// ...)` — and checks whether each referenced identifier exists as an
/// `operationId` or raw path template in any discovered `openapi.json`.
/// String-literal extraction is done by scanning for the call-site
/// substrings and reading the following quoted literal, not a TS parser,
/// so template-built identifiers, re-exported wrapper functions, and
/// destructured/aliased `api` bindings will not be detected —
/// `confidence: "low"`.
fn openapi_client_layer(root: &Path) -> Value {
    let ts_files = find_frontend_sources(root, 6);
    let openapi_files = find_files_named(root, "openapi.json", 4);
    if ts_files.is_empty() {
        return json!({
            "checked": false,
            "reason": "No frontend .ts/.tsx files found.",
            "drifts": [],
        });
    }
    if openapi_files.is_empty() {
        return json!({
            "checked": false,
            "reason": "No openapi.json found, so frontend calls cannot be verified.",
            "drifts": [],
        });
    }

    let mut known_identifiers = BTreeSet::<String>::new();
    for file in &openapi_files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        let Ok(spec) = serde_json::from_str::<Value>(&source) else {
            continue;
        };
        if let Some(paths) = spec.get("paths").and_then(Value::as_object) {
            for (path, methods) in paths {
                known_identifiers.insert(path.clone());
                if let Some(methods) = methods.as_object() {
                    for operation in methods.values() {
                        if let Some(operation_id) =
                            operation.get("operationId").and_then(Value::as_str)
                        {
                            known_identifiers.insert(operation_id.to_owned());
                        }
                    }
                }
            }
        }
    }

    let mut drifts = Vec::new();
    let mut calls_checked = 0usize;
    for file in &ts_files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        for (call_site, identifier) in extract_devup_api_calls(&source) {
            calls_checked += 1;
            if !known_identifiers.contains(&identifier) {
                drifts.push(json!({
                    "kind": "client-call-not-in-openapi",
                    "file": relative_or_absolute(root, file),
                    "callSite": call_site,
                    "identifier": identifier,
                    "message": "The endpoint/operationId the frontend calls was not found in openapi.json.",
                    "confidence": "low",
                }));
            }
        }
    }

    json!({
        "checked": true,
        "filesScanned": ts_files.len(),
        "callsChecked": calls_checked,
        "knownIdentifierCount": known_identifiers.len(),
        "drifts": drifts,
    })
}

fn relative_or_absolute(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| display_path(file))
}

fn find_frontend_sources(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    const SKIP: &[&str] = &[
        "node_modules",
        "dist",
        "build",
        ".git",
        ".next",
        ".turbo",
        "df",
        "target",
    ];
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
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if file_type.is_file() {
                let is_ts = matches!(
                    path.extension().and_then(|ext| ext.to_str()),
                    Some("ts") | Some("tsx")
                );
                if is_ts && !name.ends_with(".d.ts") {
                    found.push(path);
                }
            } else if file_type.is_dir() && depth < max_depth && !SKIP.contains(&name.as_ref()) {
                queue.push((path, depth + 1));
            }
        }
    }
    found.sort();
    found
}

const DEVUP_API_CALL_SITES: &[&str] = &[
    "api.get(",
    "api.post(",
    "api.put(",
    "api.patch(",
    "api.delete(",
];
const DEVUP_API_HOOK_SITES: &[&str] = &[
    "useQuery(",
    "useMutation(",
    "useSuspenseQuery(",
    "useInfiniteQuery(",
];

/// Returns `(call_site_label, referenced_identifier)` pairs found in
/// `source`.
fn extract_devup_api_calls(source: &str) -> Vec<(String, String)> {
    let mut results = Vec::new();
    for call_site in DEVUP_API_CALL_SITES {
        let mut search_from = 0usize;
        while let Some(relative) = source[search_from..].find(call_site) {
            let start = search_from + relative + call_site.len();
            if let Some(identifier) = read_next_string_literal(&source[start..]) {
                results.push(((*call_site).to_owned(), identifier));
            }
            search_from = start;
        }
    }
    for call_site in DEVUP_API_HOOK_SITES {
        let mut search_from = 0usize;
        while let Some(relative) = source[search_from..].find(call_site) {
            let start = search_from + relative + call_site.len();
            let tail = &source[start..];
            // First literal is the HTTP method ('get'/'post'/...); the
            // identifier we care about is the second.
            if let Some(after_method) = skip_past_string_literal(tail)
                && let Some(identifier) = read_next_string_literal(after_method)
            {
                results.push(((*call_site).to_owned(), identifier));
            }
            search_from = start;
        }
    }
    results
}

fn read_next_string_literal(text: &str) -> Option<String> {
    let mut chars = text.char_indices().peekable();
    let (start, quote) = loop {
        let (index, character) = chars.next()?;
        match character {
            '\'' | '"' => break (index, character),
            // Bail out if we hit something that isn't whitespace, a comma,
            // or an opening paren before finding a string — this argument
            // position isn't a plain string literal (e.g. a variable).
            character if character.is_whitespace() || character == ',' => continue,
            _ => return None,
        }
    };
    let rest = &text[start + 1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_owned())
}

fn skip_past_string_literal(text: &str) -> Option<&str> {
    let mut chars = text.char_indices().peekable();
    let (start, quote) = loop {
        let (index, character) = chars.next()?;
        match character {
            '\'' | '"' => break (index, character),
            character if character.is_whitespace() || character == ',' => continue,
            _ => return None,
        }
    };
    let rest = &text[start + 1..];
    let end = rest.find(quote)?;
    Some(&rest[end + 1..])
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
                "devup-mcp-stackdiff-test-{label}-{}-{unique}",
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
    fn extracts_model_struct_fields_ignoring_derive_attributes() {
        let source = r##"
            use sea_orm::entity::prelude::*;

            #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
            #[sea_orm(table_name = "user")]
            pub struct Model {
                #[sea_orm(primary_key, auto_increment = false)]
                pub id: Uuid,
                #[sea_orm(unique)]
                pub email: String,
                pub name: String,
                pub avatar_url: Option<String>,
            }

            #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
            pub enum Relation {}
        "##;
        let fields = extract_model_struct_fields(source);
        assert_eq!(
            fields,
            BTreeSet::from([
                "id".to_owned(),
                "email".to_owned(),
                "name".to_owned(),
                "avatar_url".to_owned(),
            ])
        );
    }

    #[test]
    fn route_url_prefix_matches_vespera_file_structure_convention() {
        assert_eq!(route_url_prefix(Path::new("mod.rs")), "");
        assert_eq!(route_url_prefix(Path::new("users.rs")), "/users");
        assert_eq!(route_url_prefix(Path::new("admin/mod.rs")), "/admin");
        assert_eq!(
            route_url_prefix(Path::new("admin/stats.rs")),
            "/admin/stats"
        );
    }

    #[test]
    fn extracts_vespera_route_attributes_and_matches_path() {
        let source = r##"
            #[vespera::route(get, path = "/{id}", tags = ["users"])]
            pub async fn get_user(Path(id): Path<u32>) -> Json<User> { todo!() }

            #[vespera::route(post, tags = ["users"])]
            pub async fn create_user() -> Json<User> { todo!() }
        "##;
        let routes = extract_vespera_route_attributes(source);
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].0, "get");
        assert_eq!(routes[0].1.as_deref(), Some("/{id}"));
        assert_eq!(routes[1].0, "post");
        assert_eq!(routes[1].1, None);
    }

    #[test]
    fn extracts_devup_api_client_calls() {
        let source = r##"
            const user = await api.get('getUser', { params: { id: '1' } })
            await api.put('/users/{id}', { params: { id: '1' } })
            queryClient.useQuery('get', '/users/{id}', { params: { id: userId } })
        "##;
        let calls = extract_devup_api_calls(source);
        let identifiers = calls.iter().map(|(_, id)| id.as_str()).collect::<Vec<_>>();
        assert!(identifiers.contains(&"getUser"));
        assert!(identifiers.contains(&"/users/{id}"));
    }

    #[tokio::test]
    async fn db_entity_layer_flags_missing_entity_field() {
        let temp = ScopedTempDir::new("db-entity");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let api_root = temp.path().join("apis").join("api");
        let models_dir = api_root.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();
        std::fs::write(
            models_dir.join("user.json"),
            r##"{ "name": "user", "columns": [
                { "name": "id", "type": "uuid", "nullable": false },
                { "name": "phone_number", "type": "text", "nullable": true }
            ] }"##,
        )
        .unwrap();
        let entity_dir = api_root.join("src").join("models");
        std::fs::create_dir_all(&entity_dir).unwrap();
        std::fs::write(
            entity_dir.join("user.rs"),
            r##"
            pub struct Model {
                pub id: Uuid,
            }
            "##,
        )
        .unwrap();

        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["db-entity".to_owned()],
        )
        .await
        .unwrap();
        let drifts = result["layers"]["db-entity"]["drifts"].as_array().unwrap();
        assert!(!drifts.is_empty());
        let drift = &drifts[0];
        assert_eq!(drift["columnsMissingInEntity"][0], "phone_number");
    }

    #[tokio::test]
    async fn route_openapi_layer_flags_route_missing_from_spec() {
        let temp = ScopedTempDir::new("route-openapi");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let api_root = temp.path().join("apis").join("api");
        let routes_dir = api_root.join("src").join("routes");
        std::fs::create_dir_all(&routes_dir).unwrap();
        std::fs::write(
            routes_dir.join("users.rs"),
            r##"
            #[vespera::route(get, path = "/{id}", tags = ["users"])]
            pub async fn get_user() -> Json<()> { todo!() }
            "##,
        )
        .unwrap();
        std::fs::write(api_root.join("openapi.json"), r##"{ "paths": {} }"##).unwrap();

        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".to_owned()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        assert_eq!(layer["checked"], true);
        let drifts = layer["drifts"].as_array().unwrap();
        assert!(
            drifts
                .iter()
                .any(|drift| drift["kind"] == "route-missing-from-openapi")
        );
    }

    #[tokio::test]
    async fn openapi_client_layer_flags_unknown_operation_id() {
        let temp = ScopedTempDir::new("openapi-client");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        std::fs::write(
            temp.path().join("openapi.json"),
            r##"{ "paths": { "/users": { "get": { "operationId": "getUsers" } } } }"##,
        )
        .unwrap();
        let front = temp.path().join("apps").join("front").join("src");
        std::fs::create_dir_all(&front).unwrap();
        std::fs::write(
            front.join("page.tsx"),
            r##"const users = await api.get('getUsersThatDoesNotExist')"##,
        )
        .unwrap();

        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["openapi-client".to_owned()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["openapi-client"];
        assert_eq!(layer["checked"], true);
        let drifts = layer["drifts"].as_array().unwrap();
        assert!(
            drifts
                .iter()
                .any(|drift| drift["identifier"] == "getUsersThatDoesNotExist")
        );
    }

    #[tokio::test]
    async fn missing_project_root_reports_guardrail() {
        let temp = ScopedTempDir::new("stackdiff-no-root");
        let nested = temp.path().join("deep");
        std::fs::create_dir_all(&nested).unwrap();
        let result = run(Some(&nested.to_string_lossy()), &[]).await.unwrap();
        assert_eq!(result["found"], false);
        assert_eq!(result["guardrail"]["action"], "stop-and-report");
    }

    #[tokio::test]
    async fn invalid_layer_name_is_rejected() {
        let temp = ScopedTempDir::new("stackdiff-bad-layer");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let error = run(
            Some(&temp.path().to_string_lossy()),
            &["bogus-layer".to_owned()],
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
    }
}
