use super::*;

pub(super) struct HiddenAssetOutputs<'a> {
    pub(super) payload: &'a CollectedPayload,
    pub(super) asset_names_per_node: bool,
    pub(super) result: Map<String, Value>,
    pub(super) failures: Vec<Value>,
    pub(super) pending_text_outputs: BTreeMap<String, String>,
}

type FilteredAssetOutputs =
    Result<(Map<String, Value>, Vec<Value>, BTreeMap<String, String>), DevupError>;

pub(super) fn exclude_hidden_asset_outputs(input: HiddenAssetOutputs<'_>) -> FilteredAssetOutputs {
    let HiddenAssetOutputs {
        payload,
        asset_names_per_node,
        mut result,
        mut failures,
        mut pending_text_outputs,
    } = input;
    // Reconcile every generated output, including reusable and responsive
    // code, against exclusions before inline/resource/file delivery.
    let mut assets = devup_mcp_figma::discover_asset_manifest(&payload.snapshot).assets;
    for captured in &payload.assets {
        if let Some(existing) = assets
            .iter_mut()
            .find(|asset| asset.asset_id == captured.asset_id)
        {
            *existing = captured.clone();
        } else {
            assets.push(captured.clone());
        }
    }
    let reference_paths = |asset: &devup_mcp_figma::AssetManifestEntry| {
        let generated = asset
            .field
            .strip_prefix("fills/")
            .and_then(|i| i.parse::<usize>().ok())
            .and_then(|i| {
                devup_mcp_devup_ui::codegen::image_fill_path(
                    &payload.snapshot,
                    &asset.node_id,
                    i,
                    asset_names_per_node,
                )
            })
            .or_else(|| {
                devup_mcp_devup_ui::codegen::asset_path(
                    &payload.snapshot,
                    &asset.node_id,
                    asset_names_per_node,
                )
            });
        generated
            .into_iter()
            .chain(asset.path.clone())
            .flat_map(|path| [host_safe_asset_path(&path), path])
            .collect::<Vec<_>>()
    };
    let available_paths = assets
        .iter()
        .filter(|asset| asset.error_code.as_deref() != Some("DEVUP_ASSET_NODE_HIDDEN"))
        .flat_map(&reference_paths)
        .collect::<std::collections::BTreeSet<_>>();
    for asset in assets
        .iter()
        .filter(|asset| asset.error_code.as_deref() == Some("DEVUP_ASSET_NODE_HIDDEN"))
    {
        let paths = reference_paths(asset)
            .into_iter()
            .filter(|path| !available_paths.contains(path))
            .collect::<Vec<_>>();
        let references = |code: &Value| {
            code.as_str()
                .is_some_and(|code| paths.iter().any(|path| code.contains(path)))
        };
        let source_node = payload.snapshot.nodes.get(&asset.node_id);
        let exclusion_details = json!({"referencePaths":paths,
        "assetNodeId":asset.node_id,
        "exclusionReason":source_node.and_then(devup_mcp_figma::asset_exclusion_reason),
        "visible":source_node.and_then(|n| n.typed_view().value("visible")),
        "opacity":source_node.and_then(|n| n.typed_view().value("opacity")),
        "absoluteRenderBounds":source_node.and_then(|n| n.typed_view().value("absoluteRenderBounds")),
        "policyConflict":source_node.is_some_and(|n| devup_mcp_figma::asset_exclusion_reason(n).is_none())});
        for field in ["tsx", "componentTsx", "responsiveTsx"] {
            if result.get(field).is_some_and(&references) {
                result.remove(field);
                pending_text_outputs.remove(field);
                failures.push(json!({"nodeId":payload.target.node_id,"assetId":asset.asset_id,"output":field,
                "errorCode":"DEVUP_EXCLUDED_ASSET_REFERENCED","stage":"projection","details":exclusion_details,
                "message":"Generated code references an excluded asset; this output was withheld."}));
            }
            if let Some(frames) = result.get_mut("frames").and_then(Value::as_array_mut) {
                for frame in frames {
                    if frame.get(field).is_some_and(&references) {
                        frame.as_object_mut().unwrap().remove(field);
                        frame["status"] = json!("partial");
                        frame["quality"]["projection"] = json!("lossy");
                        frame["quality"]["assets"] = json!("partial");
                        failures.push(json!({"nodeId":frame["nodeId"],"assetId":asset.asset_id,"output":field,
                        "errorCode":"DEVUP_EXCLUDED_ASSET_REFERENCED","stage":"projection","details":exclusion_details,
                        "message":"Generated code references an excluded asset; this output was withheld."}));
                    }
                }
            }
        }
    }
    Ok((result, failures, pending_text_outputs))
}
