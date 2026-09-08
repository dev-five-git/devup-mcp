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

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use devup_mcp_figma::{DevupError, ErrorCode};
use serde_json::{Value, json};

// The nested-checkout filter lives with `devup_project_context` because
// both tools need exactly the same one, and the walk they share
// (`project_root.rs`) is owned elsewhere. See that module for why a
// `.worktrees/` copy must never be reported as this project's own file.
use super::project_context::partition_nested_checkouts;
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

    let (model_dirs, excluded_models) =
        partition_nested_checkouts(&root, find_dirs_named(&root, "models", 5));
    let mut layers_out = serde_json::Map::new();
    for layer in &requested {
        let mut result = match layer.as_str() {
            "db-entity" => db_entity_layer(&model_dirs),
            "entity-route" => entity_route_layer(&root, &model_dirs),
            "route-openapi" => route_openapi_layer(&root),
            "openapi-client" => openapi_client_layer(&root),
            _ => unreachable!("validated above"),
        };
        if matches!(layer.as_str(), "db-entity" | "entity-route") {
            attach_excluded_paths(&mut result, excluded_models.clone());
        }
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

/// sea-orm relation accessors that `#[sea_orm::model]` lets an entity
/// declare inside the `Model` struct itself — `pub user:
/// HasOne<super::user::Entity>`, `pub projects:
/// HasMany<super::project::Entity>`.
///
/// They are how this entity reaches a *related table*, not columns of
/// this one: the column backing a `HasOne` is a separate field
/// (`user_id`) and is in the Vespertide model. Counting them as columns
/// reported a missing column for every relation in the schema — 70 of
/// them across 22 of 31 tables on a real project, not one of them real.
const RELATION_FIELD_TYPES: &[&str] = &["HasOne", "HasMany"];

fn is_relation_field(declared_type: &str) -> bool {
    RELATION_FIELD_TYPES.iter().any(|wrapper| {
        declared_type
            .strip_prefix(*wrapper)
            .is_some_and(|rest| rest.starts_with('<'))
    })
}

/// Text-scans a sea-orm entity source for `pub struct Model { ... }` and
/// extracts the name of the **database column** each `pub <field>:
/// <Type>,` line stands for, via brace-depth tracking (not a real Rust
/// parser).
///
/// A field's name and its column's name are not always the same string,
/// and this returns the column's:
///
/// - `#[sea_orm(column_name = "...")]` names the column outright and wins.
/// - `pub r#type:` is the column `type`; the raw-identifier escape is
///   Rust's, not the database's. (Two of these in one real schema were
///   reported as columns the entity is missing.)
/// - A [relation accessor](RELATION_FIELD_TYPES) is not a column at all
///   and is skipped.
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
    let mut column_name_attribute: Option<String> = None;
    for line in body.lines() {
        let line = line.trim();
        if line.starts_with("#[") {
            if let Some(name) = extract_quoted_value_after(line, "column_name") {
                column_name_attribute = Some(name);
            }
            continue;
        }
        let Some(rest) = line.strip_prefix("pub ") else {
            continue;
        };
        let Some(colon) = rest.find(':') else {
            continue;
        };
        let declared_type = rest[colon + 1..].trim();
        let named_by_attribute = column_name_attribute.take();
        if is_relation_field(declared_type) {
            continue;
        }
        if let Some(column) = named_by_attribute {
            fields.insert(column);
            continue;
        }
        let field_name = rest[..colon].trim();
        let field_name = field_name.strip_prefix("r#").unwrap_or(field_name);
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
        // `find_dirs_named` matches any directory called `models`, and a
        // real project has several that hold no Vespertide model at all:
        // `src/models/` is the *generated sea-orm entities*, and another
        // app's `models/` may not be Rust. Announcing "entity-route
        // correspondence cannot be checked" for a directory with nothing
        // to check is a finding about this scan, not about the project.
        if json_files_in(models_dir).is_empty() {
            continue;
        }
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
    const SKIP: &[&str] = &[
        "node_modules",
        "target",
        "dist",
        "build",
        ".git",
        ".next",
        // A `git worktree` container holds another branch's copy of the
        // same tree; scanning it doubles every route (defects D7/D17).
        ".worktrees",
    ];
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
    let (routes_dirs, excluded_routes) =
        partition_nested_checkouts(root, find_dirs_named(root, "routes", 5));
    let routes_dirs = routes_dirs
        .into_iter()
        .filter(|dir| dir.join("mod.rs").is_file() || !collect_rust_sources(dir, 0).is_empty())
        .collect::<Vec<_>>();
    let (openapi_files, excluded_specs) =
        partition_nested_checkouts(root, find_files_named(root, "openapi.json", 4));
    let mut excluded = excluded_routes;
    excluded.extend(excluded_specs);
    if routes_dirs.is_empty() && openapi_files.is_empty() {
        let mut layer = json!({
            "checked": false,
            "reason": "Found neither src/routes/ nor openapi.json.",
            "drifts": [],
        });
        attach_excluded_paths(&mut layer, excluded);
        return layer;
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
        let mut layer = json!({
            "checked": false,
            "reason": "No src/routes/ found, so the code-side routes cannot be checked.",
            "openapiSpecsFound": specs_checked,
            "drifts": [],
        });
        attach_excluded_paths(&mut layer, excluded);
        return layer;
    }
    if openapi_files.is_empty() {
        let mut layer = json!({
            "checked": false,
            "reason": "No openapi.json found, so there is no spec to compare against.",
            "codeRoutesFound": code_routes.len(),
            "drifts": [],
        });
        attach_excluded_paths(&mut layer, excluded);
        return layer;
    }

    let code_by_key = group_by_comparison_key(&code_routes);
    let spec_by_key = group_by_comparison_key(&spec_routes);
    let stale_spec = routes_absent_from(&code_by_key, &spec_by_key);
    let stale_code_or_merged = routes_absent_from(&spec_by_key, &code_by_key);
    let spelling_only_matches = spelling_only_match_count(&code_by_key, &spec_by_key);

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

    let mut notes = Vec::new();
    if spelling_only_matches > 0 {
        notes.push(json!(format!(
            "{spelling_only_matches} route(s) exist on both sides in different spellings and were matched rather than reported. Vespera writes a handler's URL into openapi.json in kebab-case — including inside {{...}} parameters — and gives a module-root route a trailing slash, while this scan reads the file name and path attribute verbatim. Only `_` vs `-` and a trailing slash are folded; every other difference is still reported as drift."
        )));
    }

    let mut layer = json!({
        "checked": true,
        "codeRouteCount": code_routes.len(),
        "openapiRouteCount": spec_routes.len(),
        "openapiSpecsFound": specs_checked,
        "spellingNormalizedMatches": spelling_only_matches,
        "drifts": drifts,
    });
    if !notes.is_empty()
        && let Some(object) = layer.as_object_mut()
    {
        object.insert("notes".to_owned(), Value::Array(notes));
    }
    let mirrored = mirrored_drift_count(&stale_spec, &stale_code_or_merged);
    if mirrored > 0
        && let Some(object) = layer.as_object_mut()
    {
        object.insert(
            "selfCheck".to_owned(),
            json!({
                "kind": "mirrored-drift",
                "count": mirrored,
                "message": "These routes are on both drift lists in different spellings, which means the two sides write the same route differently — not that a route is missing. Treat them as unverified rather than as drift, and report the spelling difference instead.",
                "confidence": "low",
            }),
        );
    }
    attach_excluded_paths(&mut layer, excluded);
    layer
}

/// Adds `excludedPaths` — what the scan found and dropped, and why — to a
/// layer result. Silently returning fewer files than the filesystem holds
/// is its own way of being wrong, so the drop is always visible.
fn attach_excluded_paths(layer: &mut Value, excluded: Vec<Value>) {
    if excluded.is_empty() {
        return;
    }
    if let Some(object) = layer.as_object_mut() {
        object.insert("excludedPaths".to_owned(), Value::Array(excluded));
    }
}

/// Folds the two spellings the same route can have on the two sides of
/// this comparison into one key.
///
/// Vespera writes a handler's URL into `openapi.json` in kebab-case —
/// `src/routes/ai_character.rs` becomes `/ai-character`, and even a
/// `path = "/{order_number}"` attribute becomes `/order/{order-number}`
/// — and gives a module-root handler (one with no `path` attribute) a
/// trailing slash, `/order/`. The scan on this side reads the file name
/// and the attribute verbatim.
///
/// Comparing the two verbatim reports every snake_case route twice, once
/// from each side, as mirror images of each other. Measured on a real
/// project with 222 code routes, 222 spec routes and no actual drift,
/// that was ~158 false positives — the tool's entire output
/// (`devup-mcp-defects.md` D17).
///
/// So the key folds exactly those two spellings and nothing else: case,
/// parameter names, and every other difference still count as drift.
/// Reported drifts always carry the path *as written in their own layer*;
/// only the matching is spelling-insensitive, and the layer says how many
/// routes needed it.
fn route_comparison_key(method: &str, path: &str) -> (String, String) {
    let folded = path.replace('_', "-");
    let trimmed = folded.trim_end_matches('/');
    let path = if trimmed.is_empty() {
        "/".to_owned()
    } else {
        trimmed.to_owned()
    };
    (method.to_ascii_uppercase(), path)
}

/// Groups `(METHOD, path as written)` routes by their comparison key,
/// keeping every original spelling so drift can be reported verbatim.
fn group_by_comparison_key(
    routes: &BTreeSet<(String, String)>,
) -> BTreeMap<(String, String), BTreeSet<String>> {
    let mut grouped: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for (method, path) in routes {
        grouped
            .entry(route_comparison_key(method, path))
            .or_default()
            .insert(path.clone());
    }
    grouped
}

/// The routes in `only_here` that `also_there` has no counterpart for,
/// each written the way its own layer writes it.
fn routes_absent_from(
    only_here: &BTreeMap<(String, String), BTreeSet<String>>,
    also_there: &BTreeMap<(String, String), BTreeSet<String>>,
) -> Vec<Value> {
    let mut routes = Vec::new();
    for (key, paths) in only_here {
        if also_there.contains_key(key) {
            continue;
        }
        let (method, _) = key;
        for path in paths {
            routes.push(json!({ "method": method, "path": path }));
        }
    }
    routes
}

/// How many routes are the same route on both sides but written
/// differently, so the fold is disclosed rather than silently applied.
fn spelling_only_match_count(
    code: &BTreeMap<(String, String), BTreeSet<String>>,
    spec: &BTreeMap<(String, String), BTreeSet<String>>,
) -> usize {
    code.iter()
        .filter(|(key, code_paths)| {
            spec.get(*key)
                .is_some_and(|spec_paths| spec_paths != *code_paths)
        })
        .count()
}

/// The self-check `devup-mcp-defects.md` D17 asks for: two drift lists
/// that are each other's mirror image mean the two sides spell the same
/// route differently, not that a route is missing.
///
/// [`route_comparison_key`] already folds the two spellings that are known
/// to differ. This catches whatever is left — a case difference, a renamed
/// parameter, a doubled separator — before it is reported as drift with a
/// straight face. Separators are removed rather than unified so that the
/// path's `/` and `{}` structure still has to agree: `/users/{id}` and
/// `/user/{sid}` are not each other's mirror.
fn mirrored_drift_count(left: &[Value], right: &[Value]) -> usize {
    fn loose_key(route: &Value) -> (String, String) {
        let method = route["method"]
            .as_str()
            .unwrap_or_default()
            .to_ascii_uppercase();
        let path = route["path"]
            .as_str()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .replace(['_', '-'], "");
        (method, path.trim_end_matches('/').to_owned())
    }
    let right_keys = right.iter().map(loose_key).collect::<BTreeSet<_>>();
    left.iter()
        .filter(|route| right_keys.contains(&loose_key(route)))
        .count()
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
    let (openapi_files, excluded) =
        partition_nested_checkouts(root, find_files_named(root, "openapi.json", 4));
    if ts_files.is_empty() {
        return json!({
            "checked": false,
            "reason": "No frontend .ts/.tsx files found.",
            "drifts": [],
        });
    }
    if openapi_files.is_empty() {
        let mut layer = json!({
            "checked": false,
            "reason": "No openapi.json found, so frontend calls cannot be verified.",
            "drifts": [],
        });
        attach_excluded_paths(&mut layer, excluded);
        return layer;
    }

    let mut known_identifiers = BTreeSet::<String>::new();
    let mut known_paths = BTreeSet::<String>::new();
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
                known_paths.insert(client_path_key(path));
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
    let mut spelling_only_matches = 0usize;
    for file in &ts_files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        for (call_site, identifier) in extract_devup_api_calls(&source) {
            calls_checked += 1;
            if known_identifiers.contains(&identifier) {
                continue;
            }
            if identifier.starts_with('/') && known_paths.contains(&client_path_key(&identifier)) {
                spelling_only_matches += 1;
                continue;
            }
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

    let mut layer = json!({
        "checked": true,
        "filesScanned": ts_files.len(),
        "callsChecked": calls_checked,
        "knownIdentifierCount": known_identifiers.len(),
        "spellingNormalizedMatches": spelling_only_matches,
        "drifts": drifts,
    });
    if spelling_only_matches > 0
        && let Some(object) = layer.as_object_mut()
    {
        object.insert("notes".to_owned(), json!([format!(
            "{spelling_only_matches} call(s) name an endpoint that openapi.json has under a different spelling and were matched rather than reported. openapi.json carries Vespera's kebab-case paths, parameters included (`/order/{{order-number}}`), while the generated @devup-api client and the code calling it use the camelCase parameter name (`/order/{{orderNumber}}`). Case and the `_`/`-` separators are folded; the path's `/` and `{{}}` structure still has to agree exactly."
        )]));
    }
    attach_excluded_paths(&mut layer, excluded);
    layer
}

/// Folds the ways the same endpoint path is spelled on the two sides of
/// the `openapi-client` comparison.
///
/// `openapi.json` carries Vespera's kebab-case paths, parameters included
/// (`/order/{order-number}`), while the generated `@devup-api` client and
/// the code calling it use the camelCase parameter name
/// (`/order/{orderNumber}`). Comparing them verbatim reported every
/// parameterised call the frontend makes as an endpoint the spec does not
/// have — 12 of them on a real project, not one real. That is the same
/// false-positive class `route-openapi` had (`devup-mcp-defects.md` D17),
/// arriving through a second door.
///
/// Case and the `_`/`-` separators are folded and nothing else; the
/// path's `/` and `{}` structure still has to agree exactly, so
/// `/users/{id}` and `/user/{sid}` remain different endpoints.
fn client_path_key(path: &str) -> String {
    let folded = path.to_ascii_lowercase().replace(['_', '-'], "");
    let trimmed = folded.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_owned()
    } else {
        trimmed.to_owned()
    }
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
        // Another branch's copy of the same frontend (defect D7).
        ".worktrees",
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
    fn a_relation_accessor_is_not_a_column_and_r_hash_is_not_part_of_the_name() {
        // The shape `#[sea_orm::model]` produces: relation accessors live
        // in the `Model` struct beside the columns, a Rust keyword column
        // is escaped, and `column_name` names the column outright.
        let source = r##"
            #[sea_orm::model]
            #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
            #[sea_orm(table_name = "alarm")]
            pub struct Model {
                #[sea_orm(primary_key)]
                pub id: i64,
                #[sea_orm(indexed)]
                pub user_id: i64,
                /// The alarm kind.
                #[sea_orm(indexed, default_value = "message", column_name = "type")]
                pub r#type: AlarmType,
                #[sea_orm(belongs_to, relation_enum = "User", from = "user_id", to = "id")]
                pub user: HasOne<super::user::Entity>,
                #[sea_orm(has_many)]
                pub projects: HasMany<super::project::Entity>,
            }
        "##;
        assert_eq!(
            extract_model_struct_fields(source),
            BTreeSet::from(["id".to_owned(), "user_id".to_owned(), "type".to_owned()])
        );
    }

    #[tokio::test]
    async fn db_entity_layer_does_not_call_a_relation_a_missing_column() {
        let temp = ScopedTempDir::new("db-entity-relations");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let api_root = temp.path().join("apis").join("api");
        let models_dir = api_root.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();
        std::fs::write(
            models_dir.join("alarm.json"),
            r##"{ "name": "alarm", "columns": [
                { "name": "id", "type": "bigint" },
                { "name": "user_id", "type": "bigint" },
                { "name": "type", "type": "text" }
            ] }"##,
        )
        .unwrap();
        let entity_dir = api_root.join("src").join("models");
        std::fs::create_dir_all(&entity_dir).unwrap();
        std::fs::write(
            entity_dir.join("alarm.rs"),
            r##"
            pub struct Model {
                #[sea_orm(primary_key)]
                pub id: i64,
                pub user_id: i64,
                #[sea_orm(column_name = "type")]
                pub r#type: AlarmType,
                #[sea_orm(belongs_to, from = "user_id", to = "id")]
                pub user: HasOne<super::user::Entity>,
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
        let layer = &result["layers"]["db-entity"];
        assert_eq!(layer["tablesChecked"], 1);
        assert_eq!(
            layer["drifts"].as_array().unwrap().len(),
            0,
            "the entity and the model agree; `user` is a relation and \
             `r#type` is the `type` column: {layer}"
        );
    }

    #[tokio::test]
    async fn entity_route_layer_ignores_a_models_directory_with_no_vespertide_models() {
        let temp = ScopedTempDir::new("entity-route-empty-models");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let api_root = temp.path().join("apis").join("api");
        // The real Vespertide models, with a routes tree beside them.
        std::fs::create_dir_all(api_root.join("models")).unwrap();
        std::fs::write(
            api_root.join("models").join("user.json"),
            r##"{ "name": "user", "columns": [ { "name": "email", "type": "text" } ] }"##,
        )
        .unwrap();
        let routes_dir = api_root.join("src").join("routes");
        std::fs::create_dir_all(&routes_dir).unwrap();
        std::fs::write(
            routes_dir.join("user.rs"),
            r##"
            #[vespera::route(get, tags = ["user"])]
            pub async fn list_users() -> Json<()> { let _ = "email"; todo!() }
            "##,
        )
        .unwrap();
        // Generated sea-orm entities, and another app's models: both are
        // called `models` and neither holds a Vespertide model.
        std::fs::create_dir_all(api_root.join("src").join("models")).unwrap();
        std::fs::write(
            api_root.join("src").join("models").join("user.rs"),
            "pub struct Model { pub email: String, }",
        )
        .unwrap();
        let other_app = temp.path().join("apis").join("ai").join("app");
        std::fs::create_dir_all(other_app.join("models")).unwrap();
        std::fs::write(other_app.join("models").join("schema.py"), "").unwrap();

        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["entity-route".to_owned()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["entity-route"];
        assert_eq!(layer["checked"], true);
        assert!(
            !layer["drifts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|drift| drift["kind"] == "no-routes-dir"),
            "a directory with no Vespertide model has no correspondence to \
             report on: {layer}"
        );
    }

    #[tokio::test]
    async fn openapi_client_layer_matches_a_camel_case_parameter_to_the_kebab_case_spec() {
        let temp = ScopedTempDir::new("openapi-client-camel");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        std::fs::write(
            temp.path().join("openapi.json"),
            r##"{ "paths": {
                "/order/{order-number}": { "get": { "operationId": "getOrder" } },
                "/delivery/projects/{project-id}": { "get": { "operationId": "getDelivery" } }
            } }"##,
        )
        .unwrap();
        let front = temp.path().join("apps").join("front").join("src");
        std::fs::create_dir_all(&front).unwrap();
        std::fs::write(
            front.join("page.tsx"),
            r##"
            const order = await api.get('/order/{orderNumber}')
            const delivery = await api.get('/delivery/projects/{projectId}')
            const nope = await api.get('/order/{orderNumber}/refund')
            "##,
        )
        .unwrap();

        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["openapi-client".to_owned()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["openapi-client"];
        assert_eq!(layer["callsChecked"], 3);
        assert_eq!(layer["spellingNormalizedMatches"], 2, "{layer}");
        let drifts = layer["drifts"].as_array().unwrap();
        assert_eq!(drifts.len(), 1, "{layer}");
        assert_eq!(drifts[0]["identifier"], "/order/{orderNumber}/refund");
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

    /// A project whose handlers are snake_case and whose `openapi.json`
    /// Vespera therefore wrote in kebab-case — including inside `{...}`
    /// parameters, and with a trailing slash on the module-root route.
    /// This is the shape of the real project in `devup-mcp-defects.md`
    /// D17, where 222 code routes and 222 spec routes with no actual drift
    /// produced ~158 mirror-image false positives.
    fn write_kebab_case_spec_project(root: &Path) {
        std::fs::write(root.join("package.json"), "{}").unwrap();
        let api_root = root.join("apis").join("api");
        let routes_dir = api_root.join("src").join("routes");
        std::fs::create_dir_all(&routes_dir).unwrap();
        std::fs::write(
            routes_dir.join("ai_character.rs"),
            r##"
            #[vespera::route(get, tags = ["ai_character"])]
            pub async fn list_ai_characters() -> Json<()> { todo!() }

            #[vespera::route(get, path = "/{id}", tags = ["ai_character"])]
            pub async fn get_ai_character() -> Json<()> { todo!() }
            "##,
        )
        .unwrap();
        std::fs::write(
            routes_dir.join("order.rs"),
            r##"
            #[vespera::route(get, path = "/{order_number}", tags = ["order"])]
            pub async fn get_order() -> Json<()> { todo!() }
            "##,
        )
        .unwrap();
        std::fs::write(
            api_root.join("openapi.json"),
            r##"{ "paths": {
                "/ai-character/": { "get": { "operationId": "listAiCharacters" } },
                "/ai-character/{id}": { "get": { "operationId": "getAiCharacter" } },
                "/order/{order-number}": { "get": { "operationId": "getOrder" } }
            } }"##,
        )
        .unwrap();
    }

    #[tokio::test]
    async fn route_openapi_layer_reports_no_drift_when_only_the_spelling_differs() {
        let temp = ScopedTempDir::new("route-openapi-kebab");
        write_kebab_case_spec_project(temp.path());

        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".to_owned()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        assert_eq!(layer["checked"], true);
        assert_eq!(layer["codeRouteCount"], 3);
        assert_eq!(layer["openapiRouteCount"], 3);
        assert_eq!(
            layer["drifts"].as_array().unwrap().len(),
            0,
            "snake_case handlers against Vespera's kebab_case spec are the \
             same routes, not drift: {layer}"
        );
    }

    #[tokio::test]
    async fn route_openapi_layer_says_how_many_routes_only_matched_on_spelling() {
        let temp = ScopedTempDir::new("route-openapi-kebab-note");
        write_kebab_case_spec_project(temp.path());

        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".to_owned()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        // All three differ in spelling: `/ai_character` vs `/ai-character/`,
        // `/ai_character/{id}` vs `/ai-character/{id}`, and
        // `/order/{order_number}` vs `/order/{order-number}`.
        assert_eq!(layer["spellingNormalizedMatches"], 3, "{layer}");
        assert!(
            layer["notes"][0].as_str().unwrap().contains("separator")
                || layer["notes"][0].as_str().unwrap().contains("spelling"),
            "the fold must be disclosed, not silent: {layer}"
        );
    }

    #[tokio::test]
    async fn route_openapi_layer_still_reports_a_route_the_spec_really_lacks() {
        let temp = ScopedTempDir::new("route-openapi-real-drift");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let api_root = temp.path().join("apis").join("api");
        let routes_dir = api_root.join("src").join("routes");
        std::fs::create_dir_all(&routes_dir).unwrap();
        std::fs::write(
            routes_dir.join("story_comment.rs"),
            r##"
            #[vespera::route(get, path = "/{comment_id}", tags = ["story_comment"])]
            pub async fn get_comment() -> Json<()> { todo!() }

            #[vespera::route(delete, path = "/{comment_id}", tags = ["story_comment"])]
            pub async fn delete_comment() -> Json<()> { todo!() }
            "##,
        )
        .unwrap();
        // The spec has the GET in Vespera's kebab spelling but no DELETE.
        std::fs::write(
            api_root.join("openapi.json"),
            r##"{ "paths": {
                "/story-comment/{comment-id}": { "get": { "operationId": "getComment" } }
            } }"##,
        )
        .unwrap();

        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".to_owned()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        let drifts = layer["drifts"].as_array().unwrap();
        let missing = drifts
            .iter()
            .find(|drift| drift["kind"] == "route-missing-from-openapi")
            .expect("the DELETE really is absent from the spec");
        let routes = missing["routes"].as_array().unwrap();
        assert_eq!(routes.len(), 1, "{layer}");
        assert_eq!(routes[0]["method"], "DELETE");
        assert_eq!(
            routes[0]["path"], "/story_comment/{comment_id}",
            "a drift must be reported in the spelling its own layer uses"
        );
        assert!(
            !drifts
                .iter()
                .any(|drift| drift["kind"] == "openapi-path-not-found-in-scanned-routes"),
            "the GET matched, so nothing may be reported from the spec side: {layer}"
        );
    }

    #[tokio::test]
    async fn stack_diff_skips_openapi_specs_inside_a_nested_worktree() {
        let temp = ScopedTempDir::new("stackdiff-worktree");
        write_kebab_case_spec_project(temp.path());
        let checkout = temp.path().join(".worktrees").join("revert-some-branch");
        std::fs::create_dir_all(checkout.join("apis").join("api")).unwrap();
        std::fs::write(checkout.join(".git"), "gitdir: ../../.git/worktrees/x").unwrap();
        std::fs::write(
            checkout.join("apis").join("api").join("openapi.json"),
            r##"{ "paths": { "/deleted-last-month": { "get": {} } } }"##,
        )
        .unwrap();

        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".to_owned()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        let specs = layer["openapiSpecsFound"].as_array().unwrap();
        assert_eq!(specs.len(), 1, "{layer}");
        assert!(
            !specs[0].as_str().unwrap().contains(".worktrees"),
            "{layer}"
        );
        assert_eq!(
            layer["drifts"].as_array().unwrap().len(),
            0,
            "a stale branch's endpoint is not this project's drift: {layer}"
        );
        assert_eq!(layer["excludedPaths"].as_array().unwrap().len(), 1);
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
