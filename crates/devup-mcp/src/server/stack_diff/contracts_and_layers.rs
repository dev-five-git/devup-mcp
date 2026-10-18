
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use devup_mcp_figma::{DevupError, ErrorCode};
use serde_json::{Value, json};

#[path = "../stack_diff_parse.rs"]
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
            include_str!("../text/stack_diff-attach_source_ownership-758751d4.md")
        }
        "route-openapi" => {
            "Review the human-authored route handlers and merged app configuration, then rebuild so Vespera regenerates openapi.json and regenerate the downstream client."
        }
        "openapi-client" => {
            include_str!("../text/stack_diff-attach_source_ownership-0113946e.md")
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
                    "message": include_str!("../text/stack_diff-route_openapi_layer-4d0cfa16.md"),
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
            "message": include_str!("../text/stack_diff-route_openapi_layer-215d8b4d.md"),
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
            include_str!("../text/stack_diff-route_openapi_layer-3c2d71f8.md"),
            spelling_only_matches = spelling_only_matches
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
                "message": include_str!("../text/stack_diff-route_openapi_layer-70549f41.md"),
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