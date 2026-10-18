use super::*;

pub(super) struct AssetManifestPreparation<'a> {
    pub(super) payload: &'a CollectedPayload,
    pub(super) outputs: &'a Vec<String>,
    pub(super) asset_names_per_node: bool,
    pub(super) asset_captures: &'a Vec<devup_mcp_figma::AssetSelection>,
    pub(super) web_ready_images: bool,
    pub(super) warnings: Vec<Value>,
    pub(super) scaffold_requested: bool,
    pub(super) manifest_requested: bool,
    pub(super) pending_asset_manifest: Option<AssetManifest>,
}

type PreparedAssetManifest = Result<(Option<AssetManifest>, Vec<Value>), DevupError>;

pub(super) fn prepare_asset_manifest(input: AssetManifestPreparation<'_>) -> PreparedAssetManifest {
    let AssetManifestPreparation {
        payload,
        outputs,
        asset_names_per_node,
        asset_captures,
        web_ready_images,
        mut warnings,
        scaffold_requested,
        manifest_requested,
        mut pending_asset_manifest,
    } = input;
    let reconcile_captured_assets = payload
        .assets
        .iter()
        .any(|asset| asset.status == AssetStatus::Exported)
        && outputs.iter().any(|output| {
            matches!(
                output.as_str(),
                "tsx" | "componentTsx" | "responsiveTsx" | "pageScaffold"
            )
        });
    if manifest_requested
        || reconcile_captured_assets
        || scaffold_requested
        || outputs.iter().any(|o| o == "assetCssMap")
    {
        let mut manifest =
            asset_policy::discovery(&payload.snapshot, &payload.assets, web_ready_images);
        for exported in &payload.assets {
            if let Some(existing) = manifest
                .assets
                .iter_mut()
                .find(|asset| asset.asset_id == exported.asset_id)
            {
                *existing = exported.clone();
            } else {
                manifest.assets.push(exported.clone());
            }
        }
        devup_mcp_figma::annotate_assets(&payload.snapshot, &mut manifest.assets);
        manifest.assets.retain(|asset| {
            if web_ready_images {
                return true;
            }
            if asset.error_code.as_deref() != Some("DEVUP_ASSET_NODE_HIDDEN") {
                return true;
            }
            if !warnings
                .iter()
                .any(|warning| warning["assetId"] == asset.asset_id)
            {
                warnings.push(json!({"assetId":asset.asset_id,"nodeId":asset.node_id,
                "errorCode":asset.error_code,"severity":"warning",
                "message":"Hidden asset excluded from deliverables."}));
            }
            false
        });
        manifest
            .assets
            .sort_by(|left, right| left.asset_id.cmp(&right.asset_id));
        // Where the generated code refers to each asset, so a
        // consumer can write the bytes there without re-deriving the
        // name - which, for two icons of one layer name, it would get
        // wrong.
        for asset in &mut manifest.assets {
            if asset.status == AssetStatus::Failed {
                asset.path = None;
                asset.output_path = None;
                continue;
            }
            if asset.path.is_none() {
                asset.path =
                    asset_policy::canonical_path(&payload.snapshot, asset, asset_names_per_node);
            }
        }
        // Where the bytes will be written, if a layer name is one a
        // file system refuses. The code that points at them is
        // renamed the same way once every output is assembled.
        for asset in &mut manifest.assets {
            if let Some(path) = asset.path.as_deref() {
                let safe = host_safe_asset_path(path);
                if safe != path {
                    asset.path = Some(safe);
                }
            }
        }
        for capture in asset_captures {
            if !payload.assets.iter().any(|asset| {
                asset.asset_id == capture.asset_id
                    && asset.format == Some(capture.format)
                    && asset.scale == Some(capture.scale)
                    && asset.render_mode == capture.render_mode
                    && asset.quality == capture.quality
                    && asset.lossless == capture.lossless
                    && asset.usage_aspect_ratio == capture.usage_aspect_ratio
                    && asset.max_width == capture.max_width
                    && asset.srcset == capture.srcset
                    && capture
                        .visible_scope_node_id
                        .as_ref()
                        .is_none_or(|id| asset.visible_scope_node_id.as_ref() == Some(id))
                    && matches!(asset.status, AssetStatus::Exported | AssetStatus::Failed)
            }) {
                return Err(DevupError::new(
                    ErrorCode::DevupFigmaHandoffInvalid,
                    format!(
                        "The exact requested asset export is not in the artifact. Re-collect it from the URL: {}",
                        capture.asset_id
                    ),
                    false,
                ));
            }
        }
        manifest.diagnostics = payload
            .snapshot
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.resource_kind.as_deref() == Some("asset"))
            .cloned()
            .collect();
        pending_asset_manifest = Some(manifest);
    }
    Ok((pending_asset_manifest, warnings))
}
