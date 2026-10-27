//! What an export into a project wrote for each Figma node, kept so a
//! geometry mismatch can be told apart: code that no longer matches what was
//! exported (drift, fixed by restoring the export) from code that still
//! matches it and renders wrong anyway (a converter bug, reported with a
//! repro, never hand-tuned in silence).
//!
//! `.devup/exports/<root node>.json` holds, for every element the export
//! marked with `data-figma-node-id(s)`, its tag, its attributes as written
//! and the source map entries of its node.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use devup_mcp_devup_ui::provenance::attributes::generated_attributes;
use devup_mcp_figma::DevupError;
use serde_json::{Value, json};

use super::output::{OutputPolicy, OutputTransaction};


pub(super) const EXPORTS_DIR: &str = ".devup/exports";
pub(super) const CONVERTER_BUGS_DIR: &str = ".devup/converter-bugs";
const RECORD_KIND: &str = "devup-export-record";
const MAX_RECORD_BYTES: u64 = 4 * 1024 * 1024;

/// Props that decide an element's box. Adding one the export did not write
/// is hand-tuning layout, the drift the audit refuses.
const LAYOUT_PROPS: &[&str] = &[
    "w",
    "h",
    "width",
    "height",
    "minW",
    "maxW",
    "minH",
    "maxH",
    "minWidth",
    "maxWidth",
    "minHeight",
    "maxHeight",
    "boxSize",
    "p",
    "px",
    "py",
    "pt",
    "pr",
    "pb",
    "pl",
    "padding",
    "paddingX",
    "paddingY",
    "paddingTop",
    "paddingRight",
    "paddingBottom",
    "paddingLeft",
    "m",
    "mx",
    "my",
    "mt",
    "mr",
    "mb",
    "ml",
    "margin",
    "marginTop",
    "marginRight",
    "marginBottom",
    "marginLeft",
    "gap",
    "rowGap",
    "columnGap",
    "flex",
    "flexDir",
    "flexDirection",
    "flexWrap",
    "flexGrow",
    "flexShrink",
    "flexBasis",
    "justifyContent",
    "justifyItems",
    "alignItems",
    "alignSelf",
    "alignContent",
    "display",
    "pos",
    "position",
    "top",
    "right",
    "bottom",
    "left",
    "inset",
    "aspectRatio",
    "gridTemplateColumns",
    "gridTemplateRows",
    "gridColumn",
    "gridRow",
    "order",
    "fontSize",
    "lineHeight",
    "letterSpacing",
    "fontWeight",
    "fontFamily",
    "typography",
    "overflow",
    "overflowX",
    "overflowY",
    "objectFit",
    "transform",
];

fn file_name(node_id: &str) -> String {
    node_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn normalized(source: &str) -> String {
    source.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_identity(name: &str) -> bool {
    name == "data-figma-node-id" || name == "data-figma-node-ids"
}

/// Each element of `tsx` that names a Figma node: node id -> its tag and
/// attributes (without the identity attributes). An element of a merged
/// page is listed under the node of every width that draws it.
pub(super) fn elements(tsx: &str) -> BTreeMap<String, Value> {
    let mut by_element = BTreeMap::<usize, (String, Vec<(String, String)>)>::new();
    for attribute in generated_attributes(tsx) {
        by_element
            .entry(attribute.element)
            .or_insert_with(|| (attribute.tag.clone(), Vec::new()))
            .1
            .push((attribute.name, attribute.source));
    }
    let value_of = |source: &str| {
        source
            .split_once('=')
            .map(|(_, value)| value.trim_matches(|c| c == '"' || c == '\'').to_owned())
    };
    let mut out = BTreeMap::new();
    for (tag, attributes) in by_element.values() {
        let mut ids = Vec::new();
        for (name, source) in attributes {
            match name.as_str() {
                "data-figma-node-id" => ids.extend(value_of(source)),
                "data-figma-node-ids" => ids.extend(value_of(source).into_iter().flat_map(|map| {
                    map.split_whitespace()
                        .filter_map(|pair| pair.split_once(':').map(|(_, id)| id.to_owned()))
                        .collect::<Vec<_>>()
                })),
                _ => {}
            }
        }
        let written = attributes
            .iter()
            .filter(|(name, _)| !is_identity(name))
            .map(|(_, source)| normalized(source))
            .collect::<Vec<_>>();
        for id in ids {
            out.entry(id)
                .or_insert_with(|| json!({"tag": tag, "attributes": written}));
        }
    }
    out
}

/// Writes the record of one export into a project. Returns where.
pub(super) fn write(
    policy: &OutputPolicy,
    project_root: &str,
    file_key: &str,
    root_node_id: &str,
    response: &Value,
) -> Result<Option<String>, DevupError> {
    let mut nodes = BTreeMap::new();
    let mut output = None;
    for field in ["responsiveTsx", "tsx"] {
        if let Some(tsx) = response[field].as_str() {
            let found = elements(tsx);
            if !found.is_empty() && output.is_none() {
                output = Some(field);
            }
            for (id, element) in found {
                nodes.entry(id).or_insert(element);
            }
        }
    }
    let generated = response["responsiveTsx"].as_str().or_else(|| response["tsx"].as_str()).or_else(|| response["componentTsx"].as_str());
    if generated.is_none() && nodes.is_empty() && response["frames"].as_array().is_none_or(|frames| frames.is_empty()) {
        return Ok(None);
    }
    let output = if response["responsiveTsx"].is_string() { Some("responsiveTsx") }
        else if response["tsx"].is_string() { Some("tsx") }
        else if response["componentTsx"].is_string() { Some("componentTsx") } else { output };
    let mut frame_ids = response["responsiveEvidence"]["frames"].as_array().into_iter().flatten()
        .chain(response["frames"].as_array().into_iter().flatten())
        .filter_map(|frame| frame["nodeId"].as_str()).map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    if frame_ids.is_empty() && !root_node_id.is_empty() { frame_ids.insert(root_node_id.to_owned()); }
    let mut source_map = BTreeMap::<String, Vec<Value>>::new();
    for entry in response["outputResults"]["tsx"]["_sourceMap"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if let Some(node) = entry["nodeId"].as_str()
            && nodes.contains_key(node)
        {
            source_map
                .entry(node.to_owned())
                .or_default()
                .push(entry.clone());
        }
    }
    for (id, entries) in source_map {
        if let Some(node) = nodes.get_mut(&id) {
            node["sourceMap"] = json!(entries);
        }
    }
    let record = json!({
        "kind": RECORD_KIND,
        "schemaVersion": 1,
        "fileKey": file_key,
        "rootNodeId": root_node_id,
        "output": output,
        "nodes": nodes,
        "responsive": response["figmaNodeIds"]["responsive"],
        "frameIds": frame_ids,
        "artifactId": response["artifactId"],
        "generatedStyles": generated.map(style_elements),
    });
    let path = Path::new(project_root)
        .join(EXPORTS_DIR)
        .join(format!("{}.json", file_name(frame_ids.iter().next().map_or(root_node_id, String::as_str))));
    let target = policy.resolve(&path.to_string_lossy())?;
    let display = target.display_path().display().to_string();
    let bytes = serde_json::to_vec_pretty(&record).map_err(|error| {
        DevupError::new(
            devup_mcp_figma::ErrorCode::DevupInvalidInput,
            error.to_string(),
            false,
        )
    })?;
    let mut transaction = OutputTransaction::new();
    transaction.stage("exportRecord", target, &bytes)?;
    transaction.commit()?;
    Ok(Some(display))
}

/// Every export record in the project.
pub(super) fn records(project_root: &Path) -> Vec<Value> {
    let Ok(entries) = std::fs::read_dir(project_root.join(EXPORTS_DIR)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter(|entry| {
            entry
                .metadata()
                .is_ok_and(|meta| meta.len() <= MAX_RECORD_BYTES)
        })
        .filter_map(|entry| std::fs::read(entry.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .filter(|record| record["kind"] == RECORD_KIND)
        .collect()
}

/// Why an element's box differs from its Figma frame.
pub(super) enum Cause {
    /// Its code no longer writes what the export wrote.
    Drift {
        file: String,
        missing: Vec<String>,
        added: Vec<String>,
    },
    /// Its code is the export's, so the converter produced the wrong box.
    ConverterBug {
        file: String,
        record: Value,
        exported: Value,
    },
    /// No export record or no code element to compare.
    Unknown(String),
}

/// Compares the element marked with `node_id` in the project's sources with
/// what the export wrote for it. An exported attribute missing or changed,
/// or a layout prop the export did not write, is drift; extra behaviour
/// (handlers, hover styles, links) is not.
pub(super) fn geometry_cause(project_root: &Path, node_id: &str, sources: &[String]) -> Cause {
    let records = records(project_root);
    let Some((record, exported)) = records.iter().find_map(|record| {
        record["nodes"]
            .get(node_id)
            .map(|exported| (record.clone(), exported.clone()))
    }) else {
        return Cause::Unknown(format!(
            "No export record names {node_id}; export the frame with projectRoot so a mismatch can be told apart as drift or a converter bug."
        ));
    };
    let mut code = None;
    for source in sources {
        let path: PathBuf = project_root.join(source);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !text.contains(node_id) {
            continue;
        }
        if let Some(element) = elements(&text).remove(node_id) {
            code = Some((source.clone(), element));
            break;
        }
    }
    let Some((file, element)) = code else {
        return Cause::Unknown(format!(
            "No source file marks an element with data-figma-node-id {node_id}; the exported element was removed or renamed."
        ));
    };
    let attributes = |value: &Value| {
        value["attributes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let wrote = attributes(&exported);
    let has = attributes(&element);
    let name = |source: &str| source.split('=').next().unwrap_or(source).trim().to_owned();
    let missing = wrote
        .iter()
        .filter(|attribute| !has.contains(attribute))
        .cloned()
        .collect::<Vec<_>>();
    let wrote_names = wrote
        .iter()
        .map(|attribute| name(attribute))
        .collect::<Vec<_>>();
    let added = has
        .iter()
        .filter(|attribute| {
            let name = name(attribute);
            LAYOUT_PROPS.contains(&name.as_str()) && !wrote_names.contains(&name)
        })
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() && added.is_empty() {
        Cause::ConverterBug {
            file,
            record,
            exported,
        }
    } else {
        Cause::Drift {
            file,
            missing,
            added,
        }
    }
}

/// The repro a converter bug is filed with, written to
/// `.devup/converter-bugs/<node>.json`.
#[allow(clippy::too_many_arguments)]
pub(super) fn write_repro(
    policy: &OutputPolicy,
    project_root: &str,
    node_id: &str,
    width: &Value,
    finding: &Value,
    file: &str,
    record: &Value,
    exported: &Value,
) -> Result<(String, Value), DevupError> {
    let repro = json!({
        "kind": "devup-converter-bug",
        "fileKey": record["fileKey"],
        "frameNodeId": record["rootNodeId"],
        "nodeId": node_id,
        "viewportWidth": width,
        "figmaBox": finding["evidence"]["measured"]["expected"],
        "domBox": finding["evidence"]["measured"]["actual"],
        "mismatch": finding["reason"],
        "generatedTag": exported["tag"],
        "generatedProps": exported["attributes"],
        "sourceMap": exported["sourceMap"],
        "codeFile": file,
        "page": finding["routePath"],
        "screenshot": finding["evidence"]["screenshot"],
        "why": include_str!("text/export_record-write_repro-ab60ad0a.md"),
        "file": "Report it to devup-mcp with this file. A local override may finish the site, but it must carry a comment naming this file.",
    });
    let path = Path::new(project_root)
        .join(CONVERTER_BUGS_DIR)
        .join(format!("{}.json", file_name(node_id)));
    let target = policy.resolve(&path.to_string_lossy())?;
    let display = target.display_path().display().to_string();
    let bytes = serde_json::to_vec_pretty(&repro).map_err(|error| {
        DevupError::new(
            devup_mcp_figma::ErrorCode::DevupInvalidInput,
            error.to_string(),
            false,
        )
    })?;
    let mut transaction = OutputTransaction::new();
    transaction.stage("converterBug", target, &bytes)?;
    transaction.commit()?;
    Ok((display, repro))
}

include!("export_record/generated_drift.rs");
