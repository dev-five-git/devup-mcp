//! `devup_stack_diff` — cross-layer drift detection across the devup
//! stack (`vespertide model -> sea-orm entity -> vespera route ->
//! openapi.json -> @devup-api client`). This is the one ground-truth tool
//! that cannot be reduced to "read one file and report its contents": it
//! compares authored sources with derived layers that a human reviewer would
//! normally have to cross-reference by hand.
//!
//! Every check here is text/JSON-based, not a real compiler front end for
//! Rust or TypeScript. That is a deliberate, disclosed limitation, not an
//! oversight: extraction can miss macro-generated routes (e.g.
//! `vespera::export_app!`-merged sub-apps), non-standard formatting, or
//! re-exported client wrappers. Every reported drift and every skipped
//! layer carries an explicit `confidence` (`"low"` or `"medium"`) — never
//! `"high"`, since a bounded parse cannot prove scan completeness — and the tool
//! never claims a clean layer is drift-free with unwarranted certainty;
//! see each layer's doc comment for exactly what it can and cannot see.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use devup_mcp_figma::{DevupError, ErrorCode};
use serde_json::{Value, json};

#[path = "stack_diff_parse.rs"]
pub(super) mod parse;

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

    let (model_dirs, excluded_models) = find_dirs_named(&root, "models", 5);
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
        attach_source_ownership(&mut result, layer);
        layers_out.insert(layer.clone(), result);
    }

    Ok(json!({
        "found": true,
        "projectRoot": display_path(&root),
        "layers": Value::Object(layers_out),
    }))
}

/// Ownership is part of every finding, including scan self-checks. Generated
/// artifacts are evidence to compare, never a destination for manual repairs.
pub(super) fn attach_source_ownership(result: &mut Value, layer: &str) {
    let repair = match layer {
        "db-entity" => {
            "Review and edit the Vespertide model in models/*.json, then run vespertide revision and vespertide export --orm seaorm to regenerate entities and migrations."
        }
        "entity-route" => {
            "Review the human-authored route shapes and whether the column is intentionally internal. Edit route handlers for API exposure; for database changes, edit models/*.json and run vespertide revision/export, then rebuild the routes and regenerate the client."
        }
        "route-openapi" => {
            "Review the human-authored route handlers and merged app configuration, then rebuild so Vespera regenerates openapi.json and regenerate the downstream client."
        }
        "openapi-client" => {
            "Review the human-authored frontend call and upstream route handlers or devup tags. Correct the call or route declaration, rebuild so Vespera regenerates openapi.json, then regenerate the @devup-api client from that spec."
        }
        _ => unreachable!("validated layer"),
    };
    let ownership = json!({
        "humanAuthored": ["models/*.json (Vespertide models)", "src/routes/**/*.rs (Vespera route handlers and schema shapes)", "frontend .ts/.tsx call sites"],
        "generated": [
            {"path": "src/models/**/*.rs", "generator": "vespertide", "source": "models/*.json"},
            {"path": "migrations/**", "generator": "vespertide", "source": "models/*.json"},
            {"path": "openapi.json", "generator": "vespera", "source": "route handlers"},
            {"path": "@devup-api client", "generator": "@devup-api", "source": "openapi.json"}
        ],
        "repairDirection": repair,
    });
    if let Some(drifts) = result.get_mut("drifts").and_then(Value::as_array_mut) {
        for drift in drifts {
            drift["sourceOwnership"] = ownership.clone();
        }
    }
    if let Some(check) = result.get_mut("selfCheck") {
        check["sourceOwnership"] = ownership;
    }
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

/// Maps explicit schema_type! pick/omit shapes and crate::models references
/// to their tables and columns. Whole Model/Entity references conservatively
/// count as use of every column; this is presence, not proof of serialization.
/// Files without resolvable references retain the old substring heuristic,
/// with a distinct low-confidence finding kind. Macro expansion, cfg and
/// re-exports remain outside this bounded parser; no finding is high confidence.
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
        let route_mappings = route_sources
            .iter()
            .map(|source| parse::route_mapping(source))
            .collect::<Vec<_>>();
        let unresolved = route_sources
            .iter()
            .zip(&route_mappings)
            .filter(|(_, mapping)| mapping.unresolved)
            .map(|(source, _)| source)
            .collect::<Vec<_>>();
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
                let parsed_mention = route_mappings
                    .iter()
                    .any(|mapping| mapping.mentions(table, column_name));
                let fallback_mention = unresolved
                    .iter()
                    .any(|source| source.contains(column_name) || source.contains(&pascal));
                if !parsed_mention && !fallback_mention {
                    let fallback = !unresolved.is_empty();
                    drifts.push(json!({
                        "table": table,
                        "column": column_name,
                        "kind": if fallback { "column-never-referenced-in-routes-text-fallback" } else { "column-never-referenced-in-routes" },
                        "message": format!(
                            "No route referencing {table}.{column_name} was found. It may be an intentionally internal-only column. {}",
                            if fallback { "Unresolved route files were checked only by substring; comments and unrelated identifiers can affect this weak signal." }
                            else { "Compared explicit model and column references; cfg, macro expansion and re-exports are not resolved." }
                        ),
                        "confidence": if fallback { "low" } else { "medium" },
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

/// Parses Rust handler attributes and compares their declared method/path
/// against generated OpenAPI. Macro expansion, merged apps and cfg evaluation
/// remain outside the scan, so successful parsing never raises confidence.
fn route_openapi_layer(root: &Path) -> Value {
    let (routes_dirs, excluded_routes) = find_dirs_named(root, "routes", 5);
    let routes_dirs = routes_dirs
        .into_iter()
        .filter(|dir| dir.join("mod.rs").is_file() || !collect_rust_sources(dir, 0).is_empty())
        .collect::<Vec<_>>();
    let (openapi_files, excluded_specs) = find_files_named(root, "openapi.json", 4);
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
    let mut conditional_routes = BTreeSet::<(String, String)>::new();
    let mut drifts = Vec::new();
    for routes_dir in &routes_dirs {
        let mut files = Vec::new();
        let mut conditional_modules = Vec::new();
        for file in collect_rust_sources(routes_dir, 6) {
            let Ok(relative) = file.strip_prefix(routes_dir) else {
                continue;
            };
            let prefix = route_url_prefix(relative);
            let source = std::fs::read_to_string(&file);
            let parsed = source
                .as_ref()
                .ok()
                .map(|source| parse::route_attributes(source));
            if let Some(parsed) = &parsed {
                let mut module_base = relative.with_extension("");
                if relative.file_name().is_some_and(|name| name == "mod.rs") {
                    module_base.pop();
                }
                for module in &parsed.conditional_modules {
                    let mut target = module_base.clone();
                    target.extend(module);
                    conditional_modules.push(target);
                }
            }
            files.push((file.clone(), relative.to_owned(), prefix, source, parsed));
        }
        for (file, relative, prefix, source, parsed) in files {
            let module = relative.with_extension("");
            let inherited_gate = conditional_modules
                .iter()
                .any(|gate| module == *gate || relative.starts_with(gate));
            let unresolved = parsed.as_ref().is_none_or(|parsed| parsed.unresolved);
            if let Some(parsed) = parsed {
                for route in parsed.routes {
                    let url = join_route_url(&prefix, route.path.as_deref());
                    let key = (route.method.to_ascii_uppercase(), url);
                    if inherited_gate || route.conditional {
                        conditional_routes.insert(key);
                    } else {
                        code_routes.insert(key);
                    }
                }
            }
            if unresolved {
                let guesses = source
                    .as_ref()
                    .map(|source| {
                        fallback_vespera_route_attributes(source)
                            .into_iter()
                            .map(|(method, path)| {
                                json!({
                                    "method": method.to_ascii_uppercase(),
                                    "path": join_route_url(&prefix, path.as_deref()),
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                drifts.push(json!({
                    "kind": "route-openapi-unresolved-fallback",
                    "message": "Could not resolve all route declarations in this file. These legacy text-scan candidates are unverified, may include comments or unrelated strings, and are excluded from declared route counts; review the source and build configuration before rebuilding the generated spec.",
                    "file": display_path(&file),
                    "routes": guesses,
                    "confidence": "low",
                }));
            }
        }
    }
    if !conditional_routes.is_empty() {
        drifts.push(json!({
            "kind": "conditional-route-openapi",
            "message": "These route declarations depend on cfg/cfg_attr configuration and may be compiled out. They are excluded from definite route counts and missing-from-spec findings; inspect the build configuration before rebuilding the generated spec.",
            "routes": conditional_routes.iter().map(|(method, path)| json!({
                "method": method, "path": path,
            })).collect::<Vec<_>>(),
            "confidence": "low",
        }));
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
            "drifts": drifts,
        });
        attach_excluded_paths(&mut layer, excluded);
        return layer;
    }

    let code_by_key = group_by_comparison_key(&code_routes);
    let spec_by_key = group_by_comparison_key(&spec_routes);
    let stale_spec = routes_absent_from(&code_by_key, &spec_by_key);
    // A conditional declaration was scanned, but is not proof of an active
    // operation. Avoid also labelling its spec path as absent from the scan.
    let mut scanned_routes = code_routes.clone();
    scanned_routes.extend(conditional_routes.iter().cloned());
    let stale_code_or_merged =
        routes_absent_from(&spec_by_key, &group_by_comparison_key(&scanned_routes));
    let spelling_only_matches = spelling_only_match_count(&code_by_key, &spec_by_key);

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
        "conditionalRouteCount": conditional_routes.len(),
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
pub(super) fn route_comparison_key(method: &str, path: &str) -> (String, String) {
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

/// Returns only resolved, unconditional handler declarations. Conditional
/// routes and unresolved syntax are disclosed separately by route_openapi_layer.
pub(super) fn extract_vespera_route_attributes(source: &str) -> Vec<(String, Option<String>)> {
    parse::route_attributes(source)
        .routes
        .into_iter()
        .filter(|route| !route.conditional)
        .map(|route| (route.method, route.path))
        .collect()
}

/// Legacy text evidence is retained only in a distinctly low-confidence
/// unresolved finding; never use these guesses as declared route keys.
fn fallback_vespera_route_attributes(source: &str) -> Vec<(String, Option<String>)> {
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
        if depth != 0 {
            break;
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
pub(super) fn route_url_prefix(relative_path: &Path) -> String {
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

pub(super) fn join_route_url(prefix: &str, path_attr: Option<&str>) -> String {
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
/// Literal calls, JSX forms, server imports, Zod schemas and query keys are
/// extracted from tokens; CRUD config names are expanded through devup tags.
/// This is a bounded parser, so re-exported wrapper functions and
/// destructured/aliased `api` bindings will not be detected —
/// `confidence: "low"`.
fn openapi_client_layer(root: &Path) -> Value {
    let ts_files = find_frontend_sources(root, 6);
    let (openapi_files, excluded) = find_files_named(root, "openapi.json", 4);
    if ts_files.is_empty() {
        let mut layer = json!({
            "checked": false,
            "reason": "No frontend .ts/.tsx files found.",
            "drifts": [],
        });
        attach_excluded_paths(&mut layer, excluded);
        return layer;
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
    let mut crud_operations = BTreeMap::<String, BTreeSet<String>>::new();
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
                        for tag in operation
                            .get("tags")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                        {
                            let parts = tag.split(':').collect::<Vec<_>>();
                            if let ["devup", name, "one" | "create" | "edit" | "fix"] =
                                parts.as_slice()
                                && !name.is_empty()
                            {
                                let identifier = operation
                                    .get("operationId")
                                    .and_then(Value::as_str)
                                    .unwrap_or(path);
                                crud_operations
                                    .entry((*name).to_owned())
                                    .or_default()
                                    .insert(identifier.to_owned());
                            }
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
        let (mut calls, configs) = parse::client_references(&source);
        for (call_site, name) in configs {
            if let Some(operations) = crud_operations.get(&name) {
                calls.extend(
                    operations
                        .iter()
                        .map(|operation| (call_site.clone(), operation.clone())),
                );
            } else {
                calls_checked += 1;
                drifts.push(json!({
                    "kind": "client-crud-config-not-in-openapi",
                    "file": relative_or_absolute(root, file),
                    "callSite": call_site,
                    "identifier": name,
                    "message": "The CRUD configuration has no matching devup:NAME:one/create/edit/fix operation tags in openapi.json.",
                    "confidence": "low",
                }));
            }
        }
        for (call_site, identifier) in calls {
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
pub(super) fn client_path_key(path: &str) -> String {
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

#[cfg(test)]
fn extract_devup_api_calls(source: &str) -> Vec<(String, String)> {
    parse::client_references(source).0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn route_arguments_ignore_comment_paths() {
        for source in [
            "#[route(get, /* path = \"/fake\" */ tags = [\"/tag\"])]\npub async fn real() {}",
            "#[route(get, // path = \"/fake\"\n)]\npub async fn real() {}",
            "#[route(get, /* outer /* path = \"/fake\" */ end */)]\npub async fn real() {}",
        ] {
            assert_eq!(
                extract_vespera_route_attributes(source),
                vec![("get".into(), None)]
            );
        }
    }

    #[test]
    fn route_arguments_ignore_doc_comment_and_macro_examples() {
        for source in [
            "/// #[route(get, path = \"/fake\")]\npub async fn real() {}",
            "/* #[route(get, path = \"/fake\")] */\npub async fn real() {}",
            "unrelated! { #[route(get, path = \"/fake\")]\npub async fn real() {} }",
        ] {
            assert!(
                extract_vespera_route_attributes(source).is_empty(),
                "{source}"
            );
        }
    }

    #[test]
    fn route_arguments_decode_rust_string_literals() {
        for (literal, expected) in [
            (r##"r"/raw""##, "/raw"),
            (r##"r#"/raw"quote"#"##, "/raw\"quote"),
            (r#""/escaped\"quote""#, "/escaped\"quote"),
            ("\"/continued\\\n    /path\"", "/continued/path"),
            (r#""/\u{61}\x62""#, "/ab"),
        ] {
            let source =
                format!("#[vespera::route(get, path = {literal})]\npub async fn real() {{}}");
            assert_eq!(
                extract_vespera_route_attributes(&source),
                vec![("get".into(), Some(expected.into()))]
            );
        }
    }

    #[test]
    fn route_arguments_multiline_reversed_and_two_handlers() {
        let source = r#"
            #[vespera :: route (
                path = "/first", tags = ["path = fake"], get,
            )]
            #[allow(
                dead_code
            )]
            pub async fn first() {}
            #[route(post, path = "/second")]
            pub async fn second() {}
        "#;
        assert_eq!(
            extract_vespera_route_attributes(source),
            vec![
                ("get".into(), Some("/first".into())),
                ("post".into(), Some("/second".into())),
            ]
        );
    }

    #[test]
    fn route_arguments_reject_unsupported_or_ambiguous_paths() {
        for args in [
            "get, \"/positional\"",
            "get, path = PATH",
            "get, path = concat!(\"/x\")",
            "get, path = \"/one\", path = \"/two\"",
            "get, post",
            "get, path = b\"/bytes\"",
        ] {
            let source = format!("#[route({args})]\npub async fn real() {{}}");
            assert!(
                extract_vespera_route_attributes(&source).is_empty(),
                "{source}"
            );
        }
    }

    #[tokio::test]
    async fn route_arguments_cfg_handlers_are_conditional_with_ownership() {
        let temp = ScopedTempDir::new("route-cfg");
        mapping_project(
            temp.path(),
            &[(
                "user.rs",
                "#[route(get)]\n#[cfg(feature = \"optional\")]\npub async fn user() {}",
            )],
        );
        std::fs::write(temp.path().join("openapi.json"), r#"{"paths":{}}"#).unwrap();
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".into()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        assert_eq!(layer["codeRouteCount"], 0);
        let findings = layer["drifts"].as_array().unwrap();
        let conditional = findings
            .iter()
            .find(|f| f["kind"] == "conditional-route-openapi")
            .unwrap();
        assert_eq!(conditional["confidence"], "low");
        assert!(conditional["sourceOwnership"].is_object());
        assert!(
            !findings
                .iter()
                .any(|f| f["kind"] == "route-missing-from-openapi")
        );
    }

    #[tokio::test]
    async fn route_arguments_unresolved_file_keeps_low_confidence_fallback() {
        let temp = ScopedTempDir::new("route-fallback");
        mapping_project(
            temp.path(),
            &[(
                "user.rs",
                "#[route(get, path = PATH)]\npub async fn user() {}",
            )],
        );
        std::fs::write(temp.path().join("openapi.json"), r#"{"paths":{}}"#).unwrap();
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".into()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        assert_eq!(layer["codeRouteCount"], 0);
        let fallback = layer["drifts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["kind"] == "route-openapi-unresolved-fallback")
            .unwrap();
        assert_eq!(fallback["confidence"], "low");
        assert!(fallback["sourceOwnership"].is_object());
    }

    #[test]
    fn route_arguments_do_not_promote_cfg_modules_or_cfg_attr() {
        for source in [
            "#![cfg(feature = \"optional\")]\n#[route(get)]\npub async fn real() {}",
            "#[cfg(feature = \"optional\")] mod inner { #[route(get)]\npub async fn real() {} }",
            "#[cfg_attr(feature = \"optional\", cfg(unix))]\n#[route(get)]\npub async fn real() {}",
        ] {
            assert!(
                extract_vespera_route_attributes(source).is_empty(),
                "{source}"
            );
        }
    }

    #[test]
    fn route_arguments_do_not_extract_other_namespaces_or_function_bodies() {
        for source in [
            "#[other::route(get, path = \"/fake\")]\npub async fn real() {}",
            "pub async fn outer() { #[route(get)]\npub async fn inner() {} }",
            "#[route(get)]\nfn private() {}\npub async fn unrelated() {}",
        ] {
            assert!(
                extract_vespera_route_attributes(source).is_empty(),
                "{source}"
            );
        }
    }

    #[tokio::test]
    async fn route_arguments_malformed_file_preserves_fallback_evidence() {
        let temp = ScopedTempDir::new("route-malformed");
        mapping_project(
            temp.path(),
            &[(
                "user.rs",
                "#[route(get, path = \"/known\")]\npub async fn user() {}\nfn broken(",
            )],
        );
        std::fs::write(temp.path().join("openapi.json"), r#"{"paths":{}}"#).unwrap();
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".into()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        assert_eq!(layer["codeRouteCount"], 0);
        let fallback = layer["drifts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["kind"] == "route-openapi-unresolved-fallback")
            .unwrap();
        assert_eq!(fallback["confidence"], "low");
        assert_eq!(fallback["routes"][0]["path"], "/user/known");
    }

    #[tokio::test]
    async fn route_arguments_unfinished_unicode_attribute_does_not_panic() {
        let temp = ScopedTempDir::new("route-unicode");
        mapping_project(temp.path(), &[("user.rs", "#[route(경로")]);
        std::fs::write(temp.path().join("openapi.json"), r#"{"paths":{}}"#).unwrap();
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".into()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        assert_eq!(layer["codeRouteCount"], 0);
        assert_eq!(
            layer["drifts"][0]["kind"],
            "route-openapi-unresolved-fallback"
        );
    }

    #[tokio::test]
    async fn route_arguments_cfg_module_declaration_gates_child_file() {
        let temp = ScopedTempDir::new("route-module-cfg");
        mapping_project(
            temp.path(),
            &[
                ("mod.rs", "#[cfg(feature = \"optional\")]\npub mod user;"),
                ("user.rs", "#[route(get)]\npub async fn user() {}"),
            ],
        );
        std::fs::write(
            temp.path().join("openapi.json"),
            r#"{"paths":{"/user/":{"get":{}}}}"#,
        )
        .unwrap();
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".into()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        assert_eq!(layer["codeRouteCount"], 0);
        assert_eq!(layer["spellingNormalizedMatches"], 0);
        let findings = layer["drifts"].as_array().unwrap();
        assert!(
            findings
                .iter()
                .any(|f| f["kind"] == "conditional-route-openapi")
        );
        assert!(
            !findings
                .iter()
                .any(|f| f["kind"] == "openapi-path-not-found-in-scanned-routes")
        );
    }
    fn mapping_project(root: &Path, routes: &[(&str, &str)]) {
        std::fs::write(root.join("package.json"), "{}").unwrap();
        std::fs::create_dir_all(root.join("models")).unwrap();
        std::fs::create_dir_all(root.join("src/routes")).unwrap();
        for table in ["user", "team"] {
            std::fs::write(
                root.join(format!("models/{table}.json")),
                json!({
                    "name": table, "columns": [{"name":"id"}, {"name":"email"}, {"name":"secret"}]
                })
                .to_string(),
            )
            .unwrap();
        }
        for (name, source) in routes {
            std::fs::write(root.join("src/routes").join(name), source).unwrap();
        }
    }

    #[tokio::test]
    async fn parsed_route_mapping_ignores_comments_strings_and_other_models() {
        let temp = ScopedTempDir::new("exact-map");
        mapping_project(
            temp.path(),
            &[(
                "user.rs",
                r#"
            schema_type!(User from crate::models::user::Model, pick = [id]);
            schema_type!(Team from crate::models::team::Model, omit = [secret]);
            // email secret crate::models::user::Column::Email
            const TEXT: &str = "crate::models::user::Model";
            fn unrelated() { let email_secret = 1; }
        "#,
            )],
        );
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["entity-route".into()],
        )
        .await
        .unwrap();
        let drifts = result["layers"]["entity-route"]["drifts"]
            .as_array()
            .unwrap();
        let columns = drifts
            .iter()
            .map(|d| (d["table"].as_str().unwrap(), d["column"].as_str().unwrap()))
            .collect::<BTreeSet<_>>();
        assert_eq!(
            columns,
            BTreeSet::from([("user", "email"), ("user", "secret"), ("team", "secret")])
        );
        assert!(
            drifts
                .iter()
                .all(|d| d["kind"] == "column-never-referenced-in-routes")
        );
    }

    #[tokio::test]
    async fn parsed_route_mapping_direct_references_are_table_scoped() {
        let temp = ScopedTempDir::new("direct-map");
        mapping_project(
            temp.path(),
            &[(
                "user.rs",
                r#"
            fn route() { crate::models::user::Column::Email; crate::models::team::Entity::find(); }
        "#,
            )],
        );
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["entity-route".into()],
        )
        .await
        .unwrap();
        let drifts = result["layers"]["entity-route"]["drifts"]
            .as_array()
            .unwrap();
        assert_eq!(drifts.len(), 2, "{result}");
        assert!(
            drifts
                .iter()
                .all(|d| d["table"] == "user" && d["column"] != "email")
        );
    }

    #[tokio::test]
    async fn unresolved_route_files_keep_separate_low_confidence_fallback() {
        let temp = ScopedTempDir::new("fallback-map");
        mapping_project(temp.path(), &[("user.rs", "fn route() { let id = 1; }")]);
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["entity-route".into()],
        )
        .await
        .unwrap();
        let drifts = result["layers"]["entity-route"]["drifts"]
            .as_array()
            .unwrap();
        assert_eq!(drifts.len(), 4);
        assert!(drifts.iter().all(|d| d["kind"]
            == "column-never-referenced-in-routes-text-fallback"
            && d["confidence"] == "low"));
    }

    #[test]
    fn extracts_all_literal_client_forms() {
        for source in [
            r#"<ApiForm api={api} method="post" path="missingOperation" />"#,
            r#"<ApiForm path='/users/{id}' method='patch' api={api} />"#,
            "import { missingOperation as localName, anotherOperation } from '@devup-api/fetch/server';",
            "pathSchemas.GET['missingOperation']",
            "queryClient.getQueryKey('GET', 'missingOperation')",
            "useSuspenseQuery('get', 'missingOperation')",
            "useInfiniteQuery('get', 'missingOperation')",
            "useQueries([['get', 'missingOperation', { nested: ['get', 'notACall'] }], ['post', 'anotherOperation']])",
        ] {
            let calls = extract_devup_api_calls(source);
            assert!(!calls.is_empty(), "unrecognised form: {source}");
            assert!(!calls.iter().any(|(_, id)| id == "notACall"));
        }
    }

    #[test]
    fn client_extraction_preserves_literals_and_ignores_lookalikes() {
        let source = r##"
            // pathSchemas.GET['comment']
            const example = "queryClient.getQueryKey('get', 'string')";
            <OtherForm method="post" path="notAForm" />;
            <ApiForm method="get" path="notAMutation" />;
            <ApiForm method="delete" path={dynamicPath} />;
            import { wrongModule } from 'another/server';
            pathSchemas.NOT_A_METHOD['wrongMethod'];
            longeruseQueries([['get', 'wrongFunction']]);
            queryClient.getQueryKey('GET', '/users/{userId}/');
            useQueries([['get', 'one'], ['post', 'two', { nested: ['get', 'notACall'] }]]);
        "##;
        let ids = extract_devup_api_calls(source)
            .into_iter()
            .map(|(_, id)| id)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            ids,
            BTreeSet::from(["/users/{userId}/".into(), "one".into(), "two".into()])
        );
    }

    #[tokio::test]
    async fn mixed_route_resolution_preserves_fallback_without_cross_model_leaks() {
        let temp = ScopedTempDir::new("mixed-map");
        mapping_project(
            temp.path(),
            &[
                (
                    "explicit.rs",
                    "schema_type!(User from crate::models::user::Model, pick = [id]); // secret",
                ),
                ("unresolved.rs", "fn handler() { let email = 1; }"),
            ],
        );
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["entity-route".into()],
        )
        .await
        .unwrap();
        let drifts = result["layers"]["entity-route"]["drifts"]
            .as_array()
            .unwrap();
        assert_eq!(drifts.len(), 3, "{result}");
        assert!(
            drifts
                .iter()
                .all(|d| d["kind"] == "column-never-referenced-in-routes-text-fallback")
        );
        assert!(
            drifts
                .iter()
                .any(|d| d["table"] == "user" && d["column"] == "secret")
        );
    }

    #[tokio::test]
    async fn schema_mapping_handles_omit_raw_identifiers_and_grouped_model_imports() {
        let temp = ScopedTempDir::new("schema-map");
        mapping_project(
            temp.path(),
            &[(
                "user.rs",
                r##"
            vespera::schema_type! {
                User from crate :: models :: user :: Model, omit = [r#secret]
            }
            use crate::models::team::{Entity, Model, Column};
            const EXAMPLE: &str = r#"crate::models::user::Model"#;
            /* nested /* crate::models::user::Model */ ignored */
        "##,
            )],
        );
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["entity-route".into()],
        )
        .await
        .unwrap();
        let drifts = result["layers"]["entity-route"]["drifts"]
            .as_array()
            .unwrap();
        assert_eq!(drifts.len(), 1, "{result}");
        assert_eq!(drifts[0]["table"], "user");
        assert_eq!(drifts[0]["column"], "secret");
        assert_eq!(drifts[0]["kind"], "column-never-referenced-in-routes");
    }

    #[test]
    fn incomplete_rust_syntax_keeps_unresolved_mapping_without_panicking() {
        for source in [
            "r#",
            "schema_type!(User from crate::models::user::Model, pick = [id]",
            "schema_type!(User from crate::models::user::Model, unsupported = [id]);",
        ] {
            let mapping = parse::route_mapping(source);
            assert!(mapping.unresolved, "{source}");
            assert!(!mapping.mentions("user", "secret"), "{source}");
        }
    }

    #[tokio::test]
    async fn new_client_forms_share_normalization_and_crud_tag_resolution() {
        let temp = ScopedTempDir::new("client-forms");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        std::fs::write(
            temp.path().join("openapi.json"),
            json!({"paths": {
                "/users/{user-id}": {"get": {"operationId":"getUser", "tags":["devup:user:one"]},
                    "post": {"operationId":"createUser", "tags":["devup:user:create"]},
                    "put": {"operationId":"editUser", "tags":["devup:user:edit"]},
                    "patch": {"operationId":"fixUser", "tags":["devup:user:fix"]}}
            }})
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            temp.path().join("page.tsx"),
            r#"
            <ApiForm api={api} method="post" path="/users/{userId}/" />
            pathSchemas.GET['/users/{userId}'];
            queryClient.getQueryKey('GET', '/users/{userId}');
            useQueries([['get', '/users/{userId}'], ['post', '/users/{userId}/missing']]);
            import { getUser as local, missingServerOperation } from '@devup-api/fetch/server';
            <ApiCrud config={crudConfigs.user} />
            useApiCrud({ config: crudConfigs.user });
            <ApiCrud config={crudConfigs.missing} />
        "#,
        )
        .unwrap();
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["openapi-client".into()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["openapi-client"];
        assert_eq!(layer["spellingNormalizedMatches"], 4, "{layer}");
        assert_eq!(layer["callsChecked"], 16, "{layer}");
        let drifts = layer["drifts"].as_array().unwrap();
        assert_eq!(drifts.len(), 3, "{layer}");
        assert!(
            drifts
                .iter()
                .any(|d| d["identifier"] == "/users/{userId}/missing")
        );
        assert!(
            drifts
                .iter()
                .any(|d| d["identifier"] == "missingServerOperation")
        );
        assert!(
            drifts
                .iter()
                .any(|d| d["kind"] == "client-crud-config-not-in-openapi")
        );
    }

    #[tokio::test]
    async fn each_new_client_form_reports_unknown_identifiers_verbatim() {
        let temp = ScopedTempDir::new("each-client-form");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        std::fs::write(
            temp.path().join("openapi.json"),
            r#"{"paths":{"/users/{user-id}":{"get":{"operationId":"getUser"}}}}"#,
        )
        .unwrap();
        for template in [
            "<ApiForm api={api} method=\"post\" path=\"IDENTIFIER\" />",
            "pathSchemas.GET['IDENTIFIER']",
            "queryClient.getQueryKey('GET', 'IDENTIFIER')",
            "useSuspenseQuery('get', 'IDENTIFIER')",
            "useInfiniteQuery('get', 'IDENTIFIER')",
            "useQueries([['get', 'IDENTIFIER']])",
        ] {
            for (identifier, expected_drifts, normalized) in [
                ("/users/{userId}/", 0, 1),
                ("getUser", 0, 0),
                ("/users/{otherId}", 1, 0),
                ("/users/{userId}/extra", 1, 0),
                ("missingOperation", 1, 0),
            ] {
                std::fs::write(
                    temp.path().join("page.tsx"),
                    template.replace("IDENTIFIER", identifier),
                )
                .unwrap();
                let result = run(
                    Some(&temp.path().to_string_lossy()),
                    &["openapi-client".into()],
                )
                .await
                .unwrap();
                let layer = &result["layers"]["openapi-client"];
                assert_eq!(layer["callsChecked"], 1, "{template}: {layer}");
                assert_eq!(
                    layer["spellingNormalizedMatches"], normalized,
                    "{template}: {layer}"
                );
                assert_eq!(
                    layer["drifts"].as_array().unwrap().len(),
                    expected_drifts,
                    "{template}: {layer}"
                );
                if expected_drifts > 0 {
                    assert_eq!(layer["drifts"][0]["identifier"], identifier);
                }
            }
        }
    }

    #[tokio::test]
    async fn crud_resolution_requires_a_recognized_devup_role() {
        let temp = ScopedTempDir::new("crud-tags");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        std::fs::write(temp.path().join("openapi.json"), json!({"paths": {
            "/users": {"get": {"tags":["devup:valid:one", "devup:wrong:list", "devup:extra:one:more", "ordinary"]}}
        }}).to_string()).unwrap();
        std::fs::write(
            temp.path().join("page.tsx"),
            r#"
            <ApiCrud config={crudConfigs.valid} />
            useApiCrud({ config: crudConfigs.wrong });
            <ApiCrud config={crudConfigs.extra} />
            <OtherCrud config={crudConfigs.ignored} />
            const unusedConfig = crudConfigs.unused;
        "#,
        )
        .unwrap();
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["openapi-client".into()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["openapi-client"];
        assert_eq!(layer["callsChecked"], 3, "{layer}");
        let names = layer["drifts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["identifier"].as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(names, BTreeSet::from(["wrong", "extra"]));
    }

    #[tokio::test]
    async fn every_drift_identifies_ownership_and_upstream_repair() {
        let temp = ScopedTempDir::new("ownership");
        mapping_project(
            temp.path(),
            &[("user.rs", "#[vespera::route(get)]\npub async fn user() {}")],
        );
        std::fs::write(
            temp.path().join("openapi.json"),
            r#"{"paths":{"/old":{"post":{}}}}"#,
        )
        .unwrap();
        std::fs::write(temp.path().join("page.ts"), "api.get('missing')").unwrap();
        let result = run(Some(&temp.path().to_string_lossy()), &[])
            .await
            .unwrap();
        for (name, layer) in result["layers"].as_object().unwrap() {
            let drifts = layer["drifts"].as_array().unwrap();
            assert!(!drifts.is_empty(), "{name}");
            for drift in drifts {
                let ownership = &drift["sourceOwnership"];
                assert!(ownership["humanAuthored"].is_array(), "{drift}");
                assert!(ownership["generated"].is_array(), "{drift}");
                let repair = ownership["repairDirection"]
                    .as_str()
                    .expect("repair direction");
                assert!(!repair.contains("edit openapi.json"));
                assert!(!repair.contains("edit the entity"));
                assert!(matches!(
                    drift["confidence"].as_str(),
                    Some("low" | "medium")
                ));
            }
        }
    }

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
    async fn all_layers_report_nested_only_inputs_without_using_them() {
        let temp = ScopedTempDir::new("stackdiff-nested-only");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let checkout = temp.path().join(".worktrees/stale");
        std::fs::create_dir_all(checkout.join("models")).unwrap();
        std::fs::create_dir_all(checkout.join("routes")).unwrap();
        std::fs::write(checkout.join("openapi.json"), "not valid JSON").unwrap();
        let result = run(Some(&temp.path().to_string_lossy()), &[])
            .await
            .unwrap();
        for (layer, paths) in [
            ("db-entity", vec![".worktrees/stale/models"]),
            ("entity-route", vec![".worktrees/stale/models"]),
            (
                "route-openapi",
                vec![".worktrees/stale/routes", ".worktrees/stale/openapi.json"],
            ),
            ("openapi-client", vec![".worktrees/stale/openapi.json"]),
        ] {
            let layer = &result["layers"][layer];
            assert_eq!(layer["checked"], false, "{layer}");
            assert_eq!(layer["drifts"], json!([]), "{layer}");
            let expected = paths
                .into_iter()
                .map(|path| {
                    json!({
                        "path": path, "reason": "nested-checkout-directory"
                    })
                })
                .collect::<Vec<_>>();
            assert_eq!(layer["excludedPaths"], json!(expected), "{layer}");
        }
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
