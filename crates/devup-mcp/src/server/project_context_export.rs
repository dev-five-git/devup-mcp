//! Bounded export context: existing UI evidence and environment key NAMES only.
use super::super::project_root::{find_matching_files, find_project_root};
use serde_json::{Value, json};
use std::{collections::BTreeSet, io::Read, path::Path};

pub(crate) fn export_context(project_root: &str) -> Value {
    let Some(root) = find_project_root(Path::new(project_root)) else {
        return json!({"ui":{"found":false},"envKeyNames":[],"reason":"Project root could not be resolved."});
    };
    let mut ui = super::ui::run(&root, None);
    // Read key names and routing configuration evidence; never return source values.
    let (files, excluded) = find_matching_files(
        &root,
        |p| {
            p.file_name().and_then(|s| s.to_str()).is_some_and(|s| {
                s.starts_with(".env")
                    || [
                        "next.config.js",
                        "next.config.mjs",
                        "next.config.ts",
                        "middleware.ts",
                        "middleware.js",
                        "proxy.ts",
                        "proxy.js",
                    ]
                    .contains(&s)
            })
        },
        usize::MAX,
    );
    let mut keys = BTreeSet::new();
    let mut diagnostics = vec![];
    let mut routing_config = false;
    for file in files.iter().take(32) {
        let mut source = String::new();
        let read =
            std::fs::File::open(file).and_then(|f| f.take(65_537).read_to_string(&mut source));
        if read.is_err() || source.len() > 65_536 {
            diagnostics.push(json!({"path":super::relative_display(&root,file),"reason":"Context file unreadable or exceeds 64 KiB; contents omitted."}));
            continue;
        }
        if !file
            .file_name()
            .is_some_and(|s| s.to_string_lossy().starts_with(".env"))
        {
            routing_config |= file
                .file_name()
                .is_some_and(|s| s.to_string_lossy().starts_with("middleware"))
                || file
                    .file_name()
                    .is_some_and(|s| s.to_string_lossy().starts_with("proxy"))
                || ["rewrites", "redirects", "basePath", "i18n"]
                    .iter()
                    .any(|s| source.contains(s));
            continue;
        }
        for line in source.lines() {
            let line = line.trim().strip_prefix("export ").unwrap_or(line.trim());
            if let Some((key, _)) = line.split_once('=') {
                let key = key.trim();
                if !key.is_empty()
                    && key.bytes().enumerate().all(|(i, b)| {
                        b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit())
                    })
                {
                    keys.insert(key.to_owned());
                }
            }
        }
    }
    ui["customRoutingUnverified"] =
        json!(routing_config || files.len() > 32 || !diagnostics.is_empty());
    let (supplementary, _) = find_matching_files(
        &root,
        |p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| ["route.ts", "route.js"].contains(&n))
                || p.components().any(|c| c.as_os_str() == "public")
        },
        usize::MAX,
    );
    let extra = supplementary
        .iter()
        .take(256)
        .filter_map(|p| {
            let relative = super::relative_display(&root, p);
            if let Some((_, asset)) = relative.split_once("public/") {
                return Some(json!(format!("/{asset}")));
            }
            let (_, rest) = relative.split_once("app/")?;
            let segments = rest.split('/').collect::<Vec<_>>();
            let dirs = &segments[..segments.len() - 1];
            if dirs.iter().any(|s| s.starts_with('_')) {
                return None;
            }
            Some(json!(format!(
                "/{}",
                dirs.iter()
                    .filter(|s| !s.starts_with('(') && !s.starts_with('@'))
                    .copied()
                    .collect::<Vec<_>>()
                    .join("/")
            )))
        })
        .collect::<Vec<_>>();
    ui["nonPageDestinations"] = json!(extra);
    ui["customRoutingUnverified"] =
        json!(ui["customRoutingUnverified"] == true || supplementary.len() > 256);
    let key_count = keys.len();
    json!({"ui":ui,"envKeyNames":keys.into_iter().take(200).collect::<Vec<_>>(),
        "envContext":{"valuesIncluded":false,"filesScanned":files.len().min(32),"truncated":files.len()>32 || key_count>200,"keyNamesOmitted":key_count.saturating_sub(200),"maxFiles":32,"maxFileBytes":65_536,"maxKeyNames":200,"excludedPaths":excluded,"diagnostics":diagnostics},
        "reuseGuidance":include_str!("text/project_context_export-export_context-12e0ec03.md")})
}

/// Added context is bounded independently and respects remaining inline space.
pub(crate) fn attach(result: &mut Value, root: &str) {
    let context = export_context(root);
    let suggestions =
        super::super::navigation::suggestions(&result["interactiveCandidates"], &context["ui"]);
    let mut added = json!({"projectContext":context,"routeSuggestions":suggestions});
    let wire_bytes = serde_json::to_vec(&super::super::tool_result(result.clone()))
        .map_or(usize::MAX, |v| v.len());
    let budget = 65_536usize.min(
        super::super::delivery::MAX_INLINE_TOTAL_BYTES
            // Review, skills, theme-validation and job metadata are attached
            // around this boundary and are also duplicated in MCP content.
            .saturating_sub(wire_bytes.saturating_add(32_768))
            / 3,
    );
    if budget < 2048 {
        added = json!({"projectContext":{"state":"deferred","reason":"Remaining inline response budget cannot hold UI context; call devup_project_context scope ui/routes with projectRoot.","projectRoot":root},"routeSuggestions":[]});
    } else {
        added["projectContext"]["exportContextLimits"] =
            json!({"truncated":false,"maxBytes":budget,"omitted":{}});
        while added.to_string().len() > budget {
            let pointers = [
                "/projectContext/ui/components",
                "/projectContext/ui/routes",
                "/projectContext/ui/diagnostics",
                "/projectContext/ui/excludedPaths",
                "/routeSuggestions",
            ];
            let path = pointers
                .into_iter()
                .filter(|p| {
                    added
                        .pointer(p)
                        .and_then(Value::as_array)
                        .is_some_and(|a| !a.is_empty())
                })
                .max_by_key(|p| added.pointer(p).unwrap().to_string().len());
            let Some(path) = path else { break };
            added
                .pointer_mut(path)
                .unwrap()
                .as_array_mut()
                .unwrap()
                .pop();
            added["projectContext"]["exportContextLimits"]["truncated"] = json!(true);
            added["projectContext"]["ui"]["truncation"]["truncated"] = json!(true);
            let omitted = added["projectContext"]["exportContextLimits"]["omitted"][path]
                .as_u64()
                .unwrap_or(0);
            added["projectContext"]["exportContextLimits"]["omitted"][path] = json!(omitted + 1);
        }
        if added.to_string().len() > budget {
            added = json!({"projectContext":{"state":"deferred","reason":"UI context exceeds export metadata cap; request ui/routes scope separately.","projectRoot":root},"routeSuggestions":[]});
        }
    }
    result
        .as_object_mut()
        .unwrap()
        .extend(added.as_object().unwrap().clone());
}
