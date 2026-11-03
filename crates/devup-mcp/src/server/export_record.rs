//! What an export into a project wrote for each Figma node, kept so a
//! geometry mismatch can be told apart: code that no longer matches what was
//! exported (drift, fixed by restoring the export) from code that still
//! matches it and renders wrong anyway (a converter bug, reported with a
//! repro, never hand-tuned in silence).
//!
//! `.devup/exports/<root node>.json` keeps source-map identities, generated
//! code and design evidence outside application source. Source ordinals can
//! identify an unchanged generated file; they never prove runtime DOM identity.
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

fn source_elements(tsx: &str) -> Vec<Value> {
    let mut attributes = BTreeMap::<usize, Vec<String>>::new();
    for attribute in generated_attributes(tsx).into_iter().filter(|attribute| !is_identity(&attribute.name)) {
        attributes.entry(attribute.element).or_default().push(normalized(&attribute.source));
    }
    devup_mcp_devup_ui::provenance::attributes::opening_elements(tsx).into_iter().map(|opening| {
        let tag = tsx[opening.start + 1..opening.end].split(|c: char| c.is_whitespace() || c == '>' || c == '/').next().unwrap_or_default();
        json!({"elementIndex":opening.ordinal,"tag":tag,"attributes":attributes.remove(&opening.ordinal).unwrap_or_default()})
    }).collect()
}

fn mapped_elements(tsx: &str, response: &Value, field: &str) -> BTreeMap<String, Value> {
    let elements = source_elements(tsx);
    let tags = elements.iter().map(|entry| entry["tag"].clone()).collect::<Vec<_>>();
    let entries = response["outputResults"][field]["_sourceMap"].as_array().into_iter().flatten()
        .filter_map(|entry| Some((entry["nodeId"].as_str()?.to_owned(), entry["elementIndex"].as_u64()? as usize)))
        .chain(response["outputResults"][field]["sectionCoverage"]["elementMappings"].as_array().into_iter().flatten()
            .filter_map(|entry| Some((entry["nodeId"].as_str()?.to_owned(),entry["elementIndex"].as_u64()? as usize))))
        .chain(response["sourceIdentity"]["responsive"].as_array().into_iter().flatten()
            .filter(|_| field == "responsiveTsx")
            .flat_map(|entry| entry["ids"].as_array().into_iter().flatten().filter_map(|id| Some((id["nodeId"].as_str()?.to_owned(),entry["elementIndex"].as_u64()? as usize))))).collect::<Vec<_>>();
    let mut nodes = BTreeMap::new();
    for (node,index) in entries {
        let Some(element) = elements.get(index) else { continue; };
        let mut element = element.clone();
        element["sourceTags"] = json!(tags);
        nodes.entry(node).or_insert(element);
    }
    nodes
}

fn source_evidence(response: &Value) -> Value {
    let outputs = response["outputResults"].as_object().into_iter().flat_map(|outputs| outputs.iter())
        .filter(|(_, output)| output["_sourceMap"].is_array() || output["sectionCoverage"].is_object())
        .map(|(field, output)| (field.clone(), json!({"sourceMap":output["_sourceMap"],"sectionCoverage":output["sectionCoverage"]})))
        .collect::<serde_json::Map<_, _>>();
    json!({"sourceMap":response["sourceMap"],"outputs":outputs})
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
    for field in ["responsiveTsx", "tsx", "componentTsx"] {
        if let Some(tsx) = response[field].as_str() {
            let mut found = elements(tsx);
            found.extend(mapped_elements(tsx, response, field));
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
    for entry in ["tsx", "componentTsx", "responsiveTsx"].into_iter().flat_map(|field| response["outputResults"][field]["_sourceMap"].as_array().into_iter().flatten())
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
        "schemaVersion": 2,
        "fileKey": file_key,
        "rootNodeId": root_node_id,
        "output": output,
        "nodes": nodes,
        "responsive": response["figmaNodeIds"]["responsive"],
        "sourceIdentity": response["sourceIdentity"],
        "instanceOverrides": response["instanceOverrides"],
        "interactionHints": response["interactionHints"],
        "behaviorPlan": response["behaviorPlan"],
        "developmentContract": response["developmentContract"],
        "overlayAnchors": response["overlayAnchors"],
        "sourceEvidence": source_evidence(response),
        "frames": response["frames"].as_array().into_iter().flatten().map(|frame| json!({
            "nodeId":frame["nodeId"],"sourceEvidence":source_evidence(frame),"behaviorPlan":frame["behaviorPlan"],
            "generatedSource":frame["tsx"],"componentSource":frame["componentTsx"],
            "instanceOverrides":frame["instanceOverrides"],"interactionHints":frame["interactionHints"],
            "overlayAnchors":frame["overlayAnchors"],"sourceIdentity":frame["sourceIdentity"]
        })).collect::<Vec<_>>(),
        "frameIds": frame_ids,
        "artifactId": response["artifactId"],
        "generatedStyles": generated.map(style_elements),
        "generatedSource": generated,
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
        if let Some(element) = elements(&text).remove(node_id) {
            code = Some((source.clone(), element));
            break;
        }
        if let Some(index) = exported["elementIndex"].as_u64() {
            let current = source_elements(&text);
            let tags = current.iter().map(|entry| entry["tag"].clone()).collect::<Vec<_>>();
            // A changed tree or an ambiguous multi-file extraction invalidates ordinal matching.
            if sources.len() == 1 && record["generatedSource"].as_str() == Some(text.as_str()) && exported["sourceTags"] == json!(tags)
                && let Some(element) = current.get(index as usize) {
                code = Some((source.clone(), element.clone()));
                break;
            }
        }
    }
    let Some((file, element)) = code else {
        return Cause::Unknown(format!(
            "No unambiguous source-map/file mapping identifies {node_id}; refresh the sidecar after restructuring or component extraction. DOM or JSX order alone is not identity proof."
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
        "file": "Report it to devup-mcp with this file. Record any local correction in this sidecar; keep application source free of export metadata comments.",
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
include!("export_record/semantic_drift.rs");

#[cfg(test)]
mod pure_record_tests {
    use super::*;

    #[test]
    fn pure_exports_retain_node_and_override_evidence_outside_source() {
        let root = std::env::temp_dir().join(format!("devup-pure-sidecar-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&root).unwrap();
        let code = "export function Screen() { return <Box h=\"40px\"><Text>Content</Text></Box>; }";
        let response = json!({"tsx":code,"outputResults":{"tsx":{"_sourceMap":[{"nodeId":"frame","elementIndex":0}]}},"instanceOverrides":[{"instanceNodeId":"instance","generatedProps":["flex=\"1\""]}],"interactionHints":[{"nodeId":"action","targetResolved":false}],"frames":[{"nodeId":"other-width","tsx":code,"sourceMap":{"entries":[{"nodeId":"nested","elementIndex":1}]}}]});
        let policy = OutputPolicy::from_roots(vec![root.clone()]).unwrap();
        let path = write(&policy, &root.to_string_lossy(), "file", "frame", &response).unwrap().unwrap();
        let record: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(record["nodes"]["frame"]["elementIndex"], 0);
        assert_eq!(record["nodes"]["frame"]["tag"], "Box");
        assert_eq!(record["instanceOverrides"], response["instanceOverrides"]);
        assert_eq!(record["interactionHints"], response["interactionHints"]);
        assert_eq!(record["sourceEvidence"]["outputs"]["tsx"]["sourceMap"], response["outputResults"]["tsx"]["_sourceMap"]);
        assert_eq!(record["frames"][0]["sourceEvidence"]["sourceMap"], response["frames"][0]["sourceMap"]);
        std::fs::write(root.join("Screen.tsx"), code).unwrap();
        assert!(matches!(geometry_cause(&root, "frame", &["Screen.tsx".into()]), Cause::ConverterBug { .. }));
        std::fs::write(root.join("Screen.tsx"), code.replace("40px", "80px")).unwrap();
        assert!(matches!(geometry_cause(&root, "frame", &["Screen.tsx".into()]), Cause::Unknown(_)));
        drop(policy);
        std::fs::remove_dir_all(root).unwrap();
    }
}
