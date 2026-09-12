//! Opt-in UI reuse inventory. Uses the shared authoritative filesystem scan,
//! parses fresh source, then resolves static module edges without executing code.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use oxc_span::SourceType;
use serde_json::{Value, json};

use super::super::project_root::{display_path, find_matching_files};
use super::relative_display;

#[path = "project_context_ui_parse.rs"]
mod syntax;

const MAX_COMPONENTS: usize = 200;
const MAX_BYTES: usize = 131_072;
const MAX_SOURCE_BYTES: u64 = 2_097_152;

struct Module {
    path: PathBuf,
    owner: PathBuf,
    evidence: syntax::Evidence,
}

fn source_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("tsx" | "jsx" | "ts" | "js" | "mjs" | "mts")
    )
}

fn read_source(path: &Path) -> Result<String, String> {
    let mut source = String::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_SOURCE_BYTES + 1)
        .read_to_string(&mut source)
        .map_err(|e| e.to_string())?;
    if source.len() as u64 > MAX_SOURCE_BYTES {
        return Err("maxSourceBytes exceeded; file was not parsed".to_owned());
    }
    Ok(source)
}

fn owner(root: &Path, file: &Path) -> PathBuf {
    for directory in file.parent().into_iter().flat_map(Path::ancestors) {
        if directory == root {
            return root.to_path_buf();
        }
        if directory.join("package.json").is_file()
            || directory.join("devup.json").is_file()
            || directory.join("app").is_dir()
            || directory.join("src/app").is_dir()
        {
            // src/app is an application directory, not a separate package.
            if directory.file_name().is_some_and(|n| n == "src") {
                continue;
            }
            return directory.to_path_buf();
        }
    }
    root.to_path_buf()
}

fn authority(root: &Path, owner: &Path, value: &mut Value) {
    value["authority"] = json!(if owner == root {
        "project-root"
    } else {
        "package-local"
    });
    value["appliesTo"] = json!(if owner == root {
        ".".to_owned()
    } else {
        relative_display(root, owner)
    });
}

fn normalized(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

#[derive(Default)]
struct Aliases {
    base: PathBuf,
    paths: BTreeMap<String, Vec<String>>,
}

/// JSONC accepts comments and trailing commas, both common in tsconfig. Mask
/// only outside strings so URL/path text and escaped quotes remain unchanged.
fn json_config(source: &str) -> Result<Value, String> {
    let mut bytes = source.as_bytes().to_vec();
    let mut quoted = false;
    let mut i = 0;
    while i < bytes.len() {
        if quoted {
            match bytes[i] {
                b'\\' => i += 1,
                b'"' => quoted = false,
                _ => {}
            }
        } else if bytes[i] == b'"' {
            quoted = true;
        } else if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                bytes[i] = b' ';
                i += 1;
            }
            continue;
        } else if bytes[i..].starts_with(b"/*") {
            bytes[i] = b' ';
            bytes[i + 1] = b' ';
            i += 2;
            while i + 1 < bytes.len() && !bytes[i..].starts_with(b"*/") {
                bytes[i] = b' ';
                i += 1;
            }
            if i + 1 >= bytes.len() {
                return Err("Unterminated JSONC comment".to_owned());
            }
            bytes[i] = b' ';
            bytes[i + 1] = b' ';
            i += 2;
            continue;
        }
        i += 1;
    }
    quoted = false;
    i = 0;
    while i < bytes.len() {
        if quoted {
            match bytes[i] {
                b'\\' => i += 1,
                b'"' => quoted = false,
                _ => {}
            }
        } else if bytes[i] == b'"' {
            quoted = true;
        } else if bytes[i] == b','
            && bytes[i + 1..]
                .iter()
                .find(|b| !b.is_ascii_whitespace())
                .is_some_and(|b| matches!(b, b'}' | b']'))
        {
            bytes[i] = b' ';
        }
        i += 1;
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

fn aliases(
    root: &Path,
    modules: &[Module],
    diagnostics: &mut Vec<Value>,
) -> BTreeMap<PathBuf, Aliases> {
    let mut result = BTreeMap::new();
    for module in modules {
        if result.contains_key(&module.owner) {
            continue;
        }
        let mut config = Aliases {
            base: module.owner.clone(),
            ..Default::default()
        };
        for name in ["tsconfig.json", "jsconfig.json"] {
            let path = module.owner.join(name);
            if !path.is_file() {
                continue;
            }
            match read_source(&path).and_then(|s| json_config(&s)) {
                Ok(value) => {
                    let options = &value["compilerOptions"];
                    if let Some(base) = options["baseUrl"].as_str() { config.base = normalized(&module.owner.join(base)); }
                    if let Some(paths) = options["paths"].as_object() {
                        for (alias, targets) in paths {
                            config.paths.insert(alias.clone(), targets.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect());
                        }
                    }
                    if value.get("extends").is_some() {
                        diagnostics.push(json!({"path":relative_display(root,&path),"kind":"config-unresolved","reason":"Inherited tsconfig paths are not evaluated; unresolved import edges are reported."}));
                    }
                }
                Err(reason) => diagnostics.push(json!({"path":relative_display(root,&path),"kind":"config-unresolved","reason":reason})),
            }
            break;
        }
        result.insert(module.owner.clone(), config);
    }
    result
}

fn resolve(
    source: &str,
    importer: &Module,
    aliases: &BTreeMap<PathBuf, Aliases>,
    indices: &BTreeMap<PathBuf, usize>,
) -> Option<usize> {
    let mut bases = vec![];
    if source.starts_with('.') {
        bases.push(normalized(&importer.path.parent()?.join(source)));
    } else if let Some(config) = aliases.get(&importer.owner) {
        for (pattern, targets) in &config.paths {
            let capture = if let Some((prefix, suffix)) = pattern.split_once('*') {
                source
                    .strip_prefix(prefix)
                    .and_then(|s| s.strip_suffix(suffix))
            } else {
                (source == pattern).then_some("")
            };
            if let Some(capture) = capture {
                for target in targets {
                    bases.push(normalized(&config.base.join(target.replace('*', capture))));
                }
            }
        }
        bases.push(normalized(&config.base.join(source)));
    }
    for base in bases {
        if let Some(index) = indices.get(&base) {
            return Some(*index);
        }
        // TS projects often use .js in imports of .ts files.
        if matches!(
            base.extension().and_then(|e| e.to_str()),
            Some("js" | "jsx" | "mjs")
        ) {
            for extension in ["ts", "tsx", "mts"] {
                if let Some(index) = indices.get(&base.with_extension(extension)) {
                    return Some(*index);
                }
            }
        }
        for extension in ["tsx", "ts", "jsx", "js", "mts", "mjs"] {
            let appended = PathBuf::from(format!("{}.{}", base.display(), extension));
            for candidate in [appended, base.join(format!("index.{extension}"))] {
                if let Some(index) = indices.get(&candidate) {
                    return Some(*index);
                }
            }
        }
    }
    None
}

fn route(root: &Path, module: &Module, paths: &[PathBuf]) -> Option<Value> {
    if !matches!(
        module.path.file_name()?.to_str()?,
        "page.tsx" | "page.jsx" | "page.js" | "page.ts"
    ) {
        return None;
    }
    let app = if module.path.starts_with(module.owner.join("src/app")) {
        module.owner.join("src/app")
    } else {
        module.owner.join("app")
    };
    let relative = module.path.parent()?.strip_prefix(&app).ok()?;
    let segments = relative
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if segments.iter().any(|s| s.starts_with('_')) {
        return None;
    }
    let groups = segments
        .iter()
        .filter(|s| s.starts_with('(') && s.ends_with(')'))
        .cloned()
        .collect::<Vec<_>>();
    let slots = segments
        .iter()
        .filter(|s| s.starts_with('@'))
        .cloned()
        .collect::<Vec<_>>();
    let route_segments = segments
        .iter()
        .filter(|s| !groups.contains(s) && !slots.contains(s))
        .cloned()
        .collect::<Vec<_>>();
    let route = format!("/{}", route_segments.join("/"));
    let components_root = app
        .parent()?
        .join("components/pages")
        .join(route_segments.join("/"));
    let components = paths
        .iter()
        .filter(|p| p.starts_with(&components_root))
        .map(|p| relative_display(root, p))
        .collect::<Vec<_>>();
    let mut value = json!({"route":route,"page":relative_display(root,&module.path),"routeGroups":groups,"parallelSlots":slots,"components":components});
    authority(root, &module.owner, &mut value);
    Some(value)
}

/// Resolve one requested exported symbol through barrels. Visits (module,name)
/// pairs so cyclic barrels terminate and unrelated exports do not become usages.
fn origins(
    index: usize,
    name: &str,
    modules: &[Module],
    resolved_reexports: &[Vec<(String, String, usize)>],
    visited: &mut BTreeSet<(usize, String)>,
) -> BTreeSet<usize> {
    if !visited.insert((index, name.to_owned())) {
        return BTreeSet::new();
    }
    let mut result = BTreeSet::new();
    let evidence = &modules[index].evidence;
    if (name == "*" && !evidence.local_exports.is_empty()) || evidence.local_exports.contains(name)
    {
        result.insert(index);
    }
    for (exported, imported, target) in &resolved_reexports[index] {
        if exported == "*" && name == "default" {
            continue;
        }
        if exported == name || exported == "*" || name == "*" {
            let wanted = if exported == "*" && name != "*" {
                name
            } else {
                imported
            };
            result.extend(origins(
                *target,
                wanted,
                modules,
                resolved_reexports,
                visited,
            ));
        }
    }
    if evidence.reexports.iter().any(|r| {
        matches!(
            r.source.as_str(),
            "@devup-ui/components" | "@devup-ui/react"
        ) && (r.exported == name || r.exported == "*" || name == "*")
    }) {
        result.insert(index);
    }
    result
}

fn bound(mut response: Value) -> Value {
    let mut caps = vec![];
    let count = response["components"].as_array().unwrap().len();
    if count > MAX_COMPONENTS {
        response["components"]
            .as_array_mut()
            .unwrap()
            .truncate(MAX_COMPONENTS);
        caps.push("maxComponents");
        response["truncation"]["omitted"]["components"] = json!(count - MAX_COMPONENTS);
    }
    response["truncation"]["capsHit"] = json!(caps);
    response["truncation"]["truncated"] = json!(!caps.is_empty());
    while serde_json::to_vec(&response).expect("JSON value").len() > MAX_BYTES {
        if !caps.contains(&"maxBytes") {
            caps.push("maxBytes");
        }
        response["truncation"]["capsHit"] = json!(caps);
        response["truncation"]["truncated"] = json!(true);
        let mut removed = false;
        for key in ["components", "routes", "diagnostics", "excludedPaths"] {
            if response[key]
                .as_array_mut()
                .is_some_and(|a| a.pop().is_some())
            {
                let count = response["truncation"]["omitted"][key].as_u64().unwrap_or(0);
                response["truncation"]["omitted"][key] = json!(count + 1);
                removed = true;
                break;
            }
        }
        if !removed {
            break;
        } // Fixed metadata is far below MAX_BYTES.
    }
    response
}

pub(super) fn run(root: &Path, filter: Option<&str>) -> Value {
    // No arbitrary depth cutoff: the iterative shared scanner never follows
    // symlinks and always prunes build directories and nested checkout authority.
    let (files, excluded) = find_matching_files(root, source_file, usize::MAX);
    let mut modules = vec![];
    let mut diagnostics = vec![];
    for file in &files {
        let source_type = SourceType::from_path(file).unwrap_or(SourceType::tsx());
        let package = owner(root, file);
        match read_source(file).and_then(|source| syntax::parse(&source, source_type)) {
            Ok(evidence) => modules.push(Module {
                path: file.clone(),
                owner: package,
                evidence,
            }),
            Err(reason) => {
                let mut diagnostic = json!({"path":relative_display(root,file),"kind":"parse-error","state":"unparsed","reason":reason});
                authority(root, &package, &mut diagnostic);
                diagnostics.push(diagnostic);
            }
        }
    }
    let unparsed = diagnostics.len();
    let indices = modules
        .iter()
        .enumerate()
        .map(|(i, m)| (normalized(&m.path), i))
        .collect::<BTreeMap<_, _>>();
    let aliases = aliases(root, &modules, &mut diagnostics);
    let mut reexports = vec![vec![]; modules.len()];
    let mut edges = vec![BTreeSet::new(); modules.len()];
    let mut imported_by = vec![BTreeSet::new(); modules.len()];
    for (i, module) in modules.iter().enumerate() {
        for export in &module.evidence.reexports {
            if let Some(target) = resolve(&export.source, module, &aliases, &indices) {
                reexports[i].push((export.exported.clone(), export.imported.clone(), target));
            }
        }
    }
    for (i, module) in modules.iter().enumerate() {
        for import in &module.evidence.imports {
            if let Some(target) = resolve(&import.source, module, &aliases, &indices) {
                edges[i].insert(target);
                if modules[target]
                    .evidence
                    .component
                    .as_ref()
                    .is_some_and(|c| {
                        c["exports"].as_array().is_some_and(|exports| {
                            import.names.iter().any(|name| {
                                name == "*"
                                    || exports.iter().any(|export| export == name || export == "*")
                            })
                        })
                    })
                {
                    imported_by[target].insert(i);
                }
                for name in &import.names {
                    for origin in origins(target, name, &modules, &reexports, &mut BTreeSet::new())
                    {
                        imported_by[origin].insert(i);
                    }
                }
            } else if import.source.starts_with('.')
                || import.source.starts_with("@/")
                || aliases.get(&module.owner).is_some_and(|a| {
                    a.paths.keys().any(|key| {
                        import
                            .source
                            .starts_with(key.split('*').next().unwrap_or(key))
                    })
                })
            {
                diagnostics.push(json!({"path":relative_display(root,&module.path),"kind":"import-unresolved","source":import.source,"reason":"No authoritative scanned module resolved for this static import."}));
            }
        }
    }
    let component_paths = modules
        .iter()
        .filter(|m| m.evidence.component.is_some())
        .map(|m| m.path.clone())
        .collect::<Vec<_>>();
    let mut routes = vec![];
    let mut memberships = vec![BTreeSet::new(); modules.len()];
    for (index, module) in modules.iter().enumerate() {
        if let Some(route) = route(root, module, &component_paths) {
            let route_index = routes.len();
            let mut pending = vec![index];
            let app = if module.path.starts_with(module.owner.join("src/app")) {
                module.owner.join("src/app")
            } else {
                module.owner.join("app")
            };
            // Layouts/templates belong to every descendant page in their app.
            for (i, candidate) in modules.iter().enumerate() {
                if candidate.owner == module.owner
                    && candidate.path.starts_with(&app)
                    && candidate
                        .path
                        .parent()
                        .is_some_and(|p| module.path.starts_with(p))
                    && matches!(
                        candidate.path.file_stem().and_then(|s| s.to_str()),
                        Some("layout" | "template")
                    )
                {
                    pending.push(i);
                }
            }
            while let Some(i) = pending.pop() {
                if memberships[i].insert(route_index) {
                    pending.extend(edges[i].iter().copied());
                }
            }
            routes.push(route);
        }
    }
    let mut components = vec![];
    for (i, module) in modules.iter().enumerate() {
        let Some(mut component) = module.evidence.component.clone() else {
            continue;
        };
        component["path"] = json!(relative_display(root, &module.path));
        authority(root, &module.owner, &mut component);
        component["importedBy"] = json!(imported_by[i].iter().map(|index| {
            let importer = &modules[*index];
            let mut site = json!({"path":relative_display(root,&importer.path),
                "routes":memberships[*index].iter().filter_map(|i|routes[*i]["route"].as_str()).collect::<BTreeSet<_>>(),
                "routePages":memberships[*index].iter().map(|i|routes[*i]["page"].clone()).collect::<Vec<_>>()});
            authority(root,&importer.owner,&mut site);
            site
        }).collect::<Vec<_>>());
        components.push(component);
    }
    let total_components = components.len();
    let total_routes = routes.len();
    let owners = modules.iter().map(|m| &m.owner).collect::<BTreeSet<_>>();
    let matches =
        |value: &Value| filter.is_none_or(|f| value.as_str().is_some_and(|s| s.contains(f)));
    components.retain(|c| {
        matches(&c["path"])
            || ["exports", "localNames"]
                .iter()
                .any(|key| c[*key].as_array().is_some_and(|a| a.iter().any(matches)))
    });
    routes.retain(|r| matches(&r["route"]) || matches(&r["page"]));
    let matched_components = components.len();
    let matched_routes = routes.len();
    let mut response = json!({
        "found":!files.is_empty(),"scope":"ui","projectRoot":display_path(root),
        "components":components,"routes":routes,"excludedPaths":excluded,"diagnostics":diagnostics,
        "scannedFiles":files.len(),"unparsedFiles":unparsed,"componentCountExact":unparsed == 0,
        "totalComponents":total_components,"matchedComponents":matched_components,"totalRoutes":total_routes,"matchedRoutes":matched_routes,
        "totalExcludedPaths":excluded.len(),"totalDiagnostics":diagnostics.len(),
        "limits":{"maxComponents":MAX_COMPONENTS,"maxBytes":MAX_BYTES,"maxSourceBytes":MAX_SOURCE_BYTES,"byteEncoding":"UTF-8 compact JSON scope payload, before the MCP transport envelope and server identity","componentCountUnit":"file entries with one or more component exports"},
        "truncation":{"truncated":false,"capsHit":[],"omitted":{"components":0,"routes":0,"diagnostics":0,"excludedPaths":0}},
        "evidenceNotes":["Syntactic evidence, not a TypeScript type check: uppercase exported functions, component factories, classes and reexports are candidates. Inspect unresolved props before reuse.",
            "client records the file's use client directive, not transitive client-bundle membership.",
            "importedBy includes static import/reexport sites resolved through named barrels; routes describe static module reachability, not proof a component renders. Dynamic imports, runtime conditions and package export maps are not evaluated.",
            "Unparsed files may contain additional components; totalComponents counts parsed candidates only. Check diagnostics and truncation before concluding no reusable component exists."]
    });
    if owners.len() > 1 {
        response["authorityNote"] = json!(
            "Multiple application/package authorities exist. No entry governs the whole project; use each entry's appliesTo and each import site's routePages."
        );
    }
    if files.is_empty() {
        response["guardrail"] = json!({"action":"stop-and-report","message":"No authoritative UI source found. Check excludedPaths before assuming no reusable components exist."});
    }
    bound(response)
}
