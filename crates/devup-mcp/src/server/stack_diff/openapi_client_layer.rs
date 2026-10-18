

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
        object.insert(
            "notes".to_owned(),
            json!([format!(
                include_str!("../text/stack_diff-openapi_client_layer-a74a750f.md"),
                spelling_only_matches = spelling_only_matches
            )]),
        );
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
include!("tests.rs");
include!("tests_route_openapi_layer_flags_route_missing_from_spec.rs");
}
