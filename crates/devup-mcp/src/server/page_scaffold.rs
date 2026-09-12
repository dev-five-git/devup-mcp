//! Deterministic page shell and screen placement; interaction ownership is not in Figma.
use crate::server::output::{OutputPolicy, OutputTarget};
use devup_mcp_figma::{AssetManifest, AssetStatus, DevupError, ErrorCode};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
// Object-form false schema keeps strict option parsing compatible with MCP clients.
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct PageScaffoldOptions {
    /// Caller-supplied App Router route, e.g. /settings/profile or / for the root page.
    pub route: String,
    /// Preview by default, including assets and other outputPaths in this call.
    #[serde(default)]
    pub write: bool,
    /// Explicitly permit replacing existing files using the export transaction's backups.
    #[serde(default)]
    pub overwrite: bool,
}

fn invalid(message: impl Into<String>) -> DevupError {
    DevupError::new(ErrorCode::DevupInvalidInput, message, false)
}

impl PageScaffoldOptions {
    fn route_path(&self) -> Result<&str, DevupError> {
        let route = self.route.trim_matches('/');
        if self.route.is_empty()
            || self.route.starts_with("//")
            || self.route.ends_with("//")
            || route.split('/').any(|s| {
                s == "."
                    || s == ".."
                    || (s.is_empty() && !route.is_empty())
                    || s.chars()
                        .any(|c| !(c.is_ascii_alphanumeric() || "-_.[]()@".contains(c)))
            })
        {
            return Err(invalid(
                "pageScaffold.route must be a caller-supplied route without traversal or unsafe path characters; use / for the root route.",
            ));
        }
        Ok(route)
    }
}

pub(crate) fn validate(
    options: Option<&PageScaffoldOptions>,
    outputs: &[String],
) -> Result<(), DevupError> {
    let requested = outputs.iter().any(|o| o == "pageScaffold");
    match (requested, options) {
        (true, None) => Err(invalid(
            "pageScaffold requires pageScaffold.route; the route is never inferred.",
        )),
        (false, Some(_)) => Err(invalid(
            "pageScaffold options require pageScaffold in outputs.",
        )),
        (_, Some(options)) => options.route_path().map(|_| ()),
        _ => Ok(()),
    }
}

fn key(result: &Map<String, Value>) -> Result<String, DevupError> {
    if let Some(frames) = result.get("frames").and_then(Value::as_array) {
        if frames.len() != 1 {
            return Err(invalid(
                "pageScaffold accepts one screen per caller-supplied route; request one frameId per call.",
            ));
        }
        Ok(format!(
            "frame:{}:pageScaffold",
            frames[0]["nodeId"].as_str().unwrap_or_default()
        ))
    } else {
        Ok("pageScaffold".into())
    }
}

fn project_root(
    key: &str,
    paths: &BTreeMap<String, String>,
    policy: &OutputPolicy,
) -> Result<PathBuf, DevupError> {
    // Resolve through the existing guard, including when the root does not exist yet.
    let root = Path::new(paths.get(key).map(String::as_str).unwrap_or("."));
    let probe = policy.resolve(&root.join("page-scaffold-root-check").to_string_lossy())?;
    Ok(probe
        .display_path()
        .parent()
        .expect("guarded target has parent")
        .to_path_buf())
}

pub(crate) fn prepare_assets(
    result: &Map<String, Value>,
    paths: &BTreeMap<String, String>,
    manifest: &AssetManifest,
    asset_paths: &mut BTreeMap<String, String>,
    public_root: &mut Option<PathBuf>,
    policy: &OutputPolicy,
) -> Result<(), DevupError> {
    let root = project_root(&key(result)?, paths, policy)?;
    let public = public_root.get_or_insert_with(|| root.join("public"));
    for asset in &manifest.assets {
        if asset.status == AssetStatus::Exported
            && let Some(path) = &asset.path
        {
            asset_paths
                .entry(asset.asset_id.clone())
                .or_insert_with(|| {
                    public
                        .join(path.trim_start_matches('/'))
                        .to_string_lossy()
                        .into_owned()
                });
        }
    }
    Ok(())
}

pub(crate) type PlannedOutput = (String, OutputTarget, Vec<u8>);

pub(crate) fn attach(
    result: &mut Map<String, Value>,
    options: &PageScaffoldOptions,
    paths: &BTreeMap<String, String>,
    manifest: Option<&AssetManifest>,
    asset_paths: &BTreeMap<String, String>,
    policy: &OutputPolicy,
    keep_tsx: bool,
) -> Result<(String, Vec<PlannedOutput>), DevupError> {
    let key = key(result)?;
    let root = project_root(&key, paths, policy)?;
    if options.write && !paths.contains_key(&key) {
        return Err(invalid(format!(
            "Writing pageScaffold requires outputPaths[{key:?}] as the project directory."
        )));
    }
    let route = options.route_path()?;
    let carrier = if key == "pageScaffold" {
        &mut *result
    } else {
        result.get_mut("frames").unwrap().as_array_mut().unwrap()[0]
            .as_object_mut()
            .unwrap()
    };
    let code = carrier.get("tsx").and_then(Value::as_str).ok_or_else(|| {
        invalid("pageScaffold has no generated screen; inspect projection failures.")
    })?;
    // The existing generator emits one named function. Do not split its JSX or infer hooks.
    let name = code
        .lines()
        .find_map(|line| line.strip_prefix("export function "))
        .and_then(|rest| rest.split_once('('))
        .map(|(name, _)| name)
        .ok_or_else(|| invalid("pageScaffold requires a generated named screen function."))?;
    let page_name = if name.ends_with("Page") {
        name.to_owned()
    } else {
        format!("{name}Page")
    };
    let page_name = if page_name == name {
        format!("{name}ShellPage")
    } else {
        page_name
    };
    let relative = Path::new("components/pages")
        .join(route)
        .join(format!("{name}.tsx"));
    let import = format!(
        "{}{}",
        "../".repeat(2 + route.split('/').filter(|s| !s.is_empty()).count()),
        relative
            .to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches(".tsx")
    );
    let page = format!(
        "import {{ {name} }} from \"{import}\";\n\nexport default function {page_name}() {{\n  return <{name} />;\n}}\n"
    );
    let mut files = Vec::new();
    let mut planned = Vec::new();
    for (suffix, path, content) in [
        (
            "page",
            root.join("src/app").join(route).join("page.tsx"),
            page,
        ),
        ("component", root.join(relative), code.to_owned()),
    ] {
        let target = policy.resolve(&path.to_string_lossy())?;
        files.push(json!({"path":target.display_path(),"content":content,"encoding":"utf8"}));
        planned.push((format!("{key}:{suffix}"), target, content.into_bytes()));
    }
    let mut unavailable = Vec::new();
    if let Some(manifest) = manifest {
        for asset in &manifest.assets {
            if asset.status != AssetStatus::Exported {
                unavailable.push(json!({"assetId":asset.asset_id,"path":asset.path,"status":asset.status,"decision":"Collect asset bytes before relying on this reference."}));
                continue;
            }
            let Some(path) = asset_paths.get(&asset.asset_id) else {
                continue;
            };
            let target = policy.resolve(path)?;
            let data = asset
                .data_base64
                .as_deref()
                .ok_or_else(|| invalid("Scaffold asset bytes are unavailable."))?;
            if let Some(existing) = files
                .iter()
                .find(|f| f["path"] == json!(target.display_path()))
            {
                if existing["content"] != data || existing["encoding"] != "base64" {
                    return Err(invalid(format!(
                        "Scaffold file collision: {}",
                        target.display_path().display()
                    )));
                }
                continue;
            }
            files.push(json!({"path":target.display_path(),"content":data,"encoding":"base64","publicUrl":asset.path}));
            // Reconciliation already validates the bytes; the existing asset loop stages them.
        }
    }
    carrier.insert("pageScaffold".into(), json!({"route":options.route,"projectRoot":root,"mode":if options.write {"write"} else {"preview"},
        "collisionPolicy":if options.overwrite {"replace-with-rollback"} else {"refuse-existing"},
        "rollbackPolicy":"Existing staged export transaction; reverse-order rollback restores originals and reports retained recovery backups on restoration failure.",
        "files":files,"unavailableAssets":unavailable,
        "unresolvedDecisions":[
            {"kind":"client-boundary","reason":"State, events and browser API ownership are not in Figma; a human or validator must choose any client boundary."},
            {"kind":"component-decomposition","reason":"Further component boundaries and reuse require application context; only the page shell and one screen were emitted."}
        ]}));
    if let Some(output) = carrier
        .get_mut("outputResults")
        .and_then(Value::as_object_mut)
    {
        if let Some(tsx) = output.get("tsx").cloned() {
            output.insert("pageScaffold".into(), tsx);
        }
        if !keep_tsx {
            output.remove("tsx");
        }
    }
    if !keep_tsx {
        carrier.remove("tsx");
    }
    if let Some(deliverable) = result.get_mut("deliverable") {
        deliverable["isFinal"] = json!(false);
        deliverable["note"] = json!(
            "The pageScaffold file set preserves the generated screen; settle unresolvedDecisions and unavailableAssets before integration."
        );
    }
    Ok((key, planned))
}

/// A file set has one requested outputPaths key, with its directory as the value.
pub(crate) fn committed_paths(result: &Map<String, Value>, written: &mut Map<String, Value>) {
    let Ok(key) = key(result) else {
        return;
    };
    let scaffold = if key == "pageScaffold" {
        &result["pageScaffold"]
    } else {
        &result["frames"][0]["pageScaffold"]
    };
    if written.remove(&format!("{key}:page")).is_some() {
        written.remove(&format!("{key}:component"));
        written.insert(key, scaffold["projectRoot"].clone());
    }
}

/// A scaffold key targets a directory. Report file-shaped targets in the same
/// diagnostics channel as unsupported outputPaths, without staging any outputs.
pub(crate) fn directory_refusal(
    result: &Map<String, Value>,
    paths: &BTreeMap<String, String>,
    policy: &OutputPolicy,
) -> Result<Option<Value>, DevupError> {
    let key = key(result)?;
    let Some(requested) = paths.get(&key) else {
        return Ok(None);
    };
    let file_target = match project_root(&key, paths, policy) {
        Ok(root) => root
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| {
                ["tsx", "ts", "jsx", "js", "json"]
                    .iter()
                    .any(|known| ext.eq_ignore_ascii_case(known))
            }),
        // The existing path guard already identified an existing file ancestor.
        Err(error) if error.message == "An outputPath ancestor is not a directory." => true,
        Err(error) => return Err(error),
    };
    if !file_target {
        return Ok(None);
    }
    let diagnostic = json!({"code":"DEVUP_OUTPUT_PATH_EXPECTED_DIRECTORY","key":key,
        "path":requested,"expectedPathKind":"directory","written":false,
        "message":"pageScaffold outputPaths takes a project directory, not a TSX/JS/JSON file path. No files were written."});
    Ok(Some(json!({"status":"failed","outputPaths":{},
        "outputPathResults":{"supportedKeys":[key],"pathKinds":{key:"directory"},"diagnostics":[diagnostic]},
        "failures":[{"output":"pageScaffold","errorCode":"DEVUP_OUTPUT_PATH_EXPECTED_DIRECTORY",
            "message":"Supply a project directory for the scaffold outputPaths key; the file set was withheld."}]})))
}

/// Section candidates are alternatives for the supplied route, never a batch of pages.
pub(crate) fn selection_guidance(result: &mut Map<String, Value>) {
    let Some(next) = result.get_mut("nextAction").and_then(Value::as_object_mut) else {
        return;
    };
    next.remove("batches");
    next.insert("maxFramesPerCall".into(), json!(1));
    next.insert("how".into(), json!("Choose exactly one screen candidate for the caller-supplied route. The example selects one candidate; do not apply this route to every screen."));
    let args = &mut next.get_mut("example").expect("selection example")["arguments"];
    let Some(id) = args["frameIds"][0].as_str().map(str::to_owned) else {
        return;
    };
    if let Some(paths) = args["outputPaths"].as_object_mut()
        && let Some(root) = paths.remove("pageScaffold")
    {
        paths.insert(format!("frame:{id}:pageScaffold"), root);
    }
}
