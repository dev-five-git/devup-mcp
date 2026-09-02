use devup_mcp_devup_ui::codegen::RootLayout;
use devup_mcp_figma::{
    AssetFormat, AssetSelection, CollectionScope, DevupError, ErrorCode, ResourceScope,
    SourcePolicy,
};
use serde_json::json;

use super::{
    artifacts::{ArtifactKind, ArtifactLookup},
    tools::FigmaAssetRequestInput,
};

pub(super) fn validate_artifact_projection(
    artifact: &ArtifactLookup,
    outputs: &[String],
    requested_scope: &str,
    requested_assets: &[AssetSelection],
) -> Result<(), DevupError> {
    let requested_scope = parse_collection_scope(requested_scope)?;
    let capabilities = &artifact.capabilities;
    let design_output_requested = outputs.iter().any(|output| {
        matches!(
            output.as_str(),
            "tsx" | "rawSnapshot" | "sourceMap" | "assetManifest" | "referencePng"
        )
    });
    let theme_requested = outputs.iter().any(|output| output == "devupJson");
    let kind_compatible = match capabilities.kind {
        ArtifactKind::Design => true,
        ArtifactKind::ThemeOnly => theme_requested && !design_output_requested,
        ArtifactKind::SectionIndex => outputs.iter().any(|output| output == "tsx"),
        ArtifactKind::Search | ArtifactKind::Explore => false,
    };
    let collection_compatible = collection_scope_rank(requested_scope)
        <= collection_scope_rank(capabilities.collection_scope);
    let resources_compatible = !theme_requested
        || match requested_scope {
            CollectionScope::File => capabilities.resource_scope == ResourceScope::File,
            CollectionScope::Node | CollectionScope::Page => {
                capabilities.resource_scope != ResourceScope::None
            }
        };
    let assets_compatible = capabilities.supports_asset_captures(requested_assets);
    let reference_png_compatible =
        !outputs.iter().any(|output| output == "referencePng") || capabilities.reference_png;

    if kind_compatible
        && collection_compatible
        && resources_compatible
        && assets_compatible
        && reference_png_compatible
    {
        return Ok(());
    }

    Err(DevupError::with_details(
        ErrorCode::DevupFigmaHandoffInvalid,
        "artifact capture capability가 요청한 export 범위를 충족하지 않습니다.",
        false,
        json!({
            "capabilities": capabilities,
            "requested": {
                "outputs": outputs,
                "collectionScope": requested_scope,
                "assetCaptureCount": requested_assets.len()
            }
        }),
    ))
}

fn collection_scope_rank(scope: CollectionScope) -> u8 {
    match scope {
        CollectionScope::Node => 0,
        CollectionScope::Page => 1,
        CollectionScope::File => 2,
    }
}

pub(super) fn validate_outputs(outputs: &[String]) -> Result<(), DevupError> {
    if outputs.is_empty() {
        return Err(DevupError::new(
            ErrorCode::DevupSnapshotUnsupported,
            "outputs는 하나 이상이어야 합니다.",
            false,
        ));
    }
    for output in outputs {
        if !matches!(
            output.as_str(),
            "tsx" | "devupJson" | "rawSnapshot" | "sourceMap" | "assetManifest" | "referencePng"
        ) {
            return Err(DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                format!("지원하지 않는 export output입니다: {output}"),
                false,
            ));
        }
    }
    Ok(())
}

pub(super) fn parse_source_policy(policy: &str) -> Result<SourcePolicy, DevupError> {
    match policy {
        "auto" => Ok(SourcePolicy::Auto),
        "direct" => Ok(SourcePolicy::Direct),
        "host" => Ok(SourcePolicy::Host),
        _ => Err(DevupError::new(
            ErrorCode::DevupFigmaHostRequired,
            "sourcePolicy는 auto, direct 또는 host여야 합니다.",
            false,
        )),
    }
}

pub(super) fn parse_asset_requests(
    requests: &[FigmaAssetRequestInput],
) -> Result<
    (
        Vec<AssetSelection>,
        std::collections::BTreeMap<String, String>,
    ),
    DevupError,
> {
    if requests.len() > 16 {
        return Err(DevupError::new(
            ErrorCode::DevupSnapshotUnsupported,
            "한 번에 export할 asset은 16개 이하여야 합니다.",
            false,
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut selections = Vec::with_capacity(requests.len());
    let mut output_paths = std::collections::BTreeMap::new();
    for request in requests {
        if request.asset_id.trim().is_empty()
            || request.scale == 0
            || request.scale > 4
            || !seen.insert(request.asset_id.as_str())
        {
            return Err(DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "assetRequests의 ID, scale 또는 중복 값이 올바르지 않습니다.",
                false,
            ));
        }
        let format = match request.format.as_str() {
            "png" => AssetFormat::Png,
            "jpg" | "jpeg" => AssetFormat::Jpg,
            "svg" => AssetFormat::Svg,
            "pdf" => AssetFormat::Pdf,
            _ => {
                return Err(DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "asset format은 png, jpg, svg 또는 pdf여야 합니다.",
                    false,
                ));
            }
        };
        selections.push(AssetSelection {
            asset_id: request.asset_id.clone(),
            format,
            scale: request.scale,
        });
        if let Some(path) = &request.output_path {
            output_paths.insert(request.asset_id.clone(), path.clone());
        }
    }
    Ok((selections, output_paths))
}

pub(super) fn parse_collection_scope(scope: &str) -> Result<CollectionScope, DevupError> {
    match scope {
        "node" => Ok(CollectionScope::Node),
        "page" => Ok(CollectionScope::Page),
        "file" => Ok(CollectionScope::File),
        _ => Err(DevupError::new(
            ErrorCode::DevupThemeConflict,
            "scope는 node, page 또는 file이어야 합니다.",
            false,
        )),
    }
}

pub(super) fn parse_root_layout(root_layout: &str) -> Result<RootLayout, DevupError> {
    match root_layout {
        "standalone" => Ok(RootLayout::Standalone),
        "embedded" => Ok(RootLayout::Embedded),
        _ => Err(DevupError::new(
            ErrorCode::DevupThemeConflict,
            "rootLayout은 standalone 또는 embedded여야 합니다.",
            false,
        )),
    }
}
