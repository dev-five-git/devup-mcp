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

/// The export outputs this server understands.
///
/// The JSON schema for `outputs` advertises this same constant, so a caller
/// can discover the set instead of learning it one rejection at a time, and
/// the published schema cannot drift from what is actually accepted.
pub(crate) const EXPORT_OUTPUTS: [&str; 7] = [
    "tsx",
    "componentTsx",
    "devupJson",
    "rawSnapshot",
    "sourceMap",
    "assetManifest",
    "referencePng",
];

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
            "tsx" | "componentTsx" | "rawSnapshot" | "sourceMap" | "assetManifest" | "referencePng"
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
        "The artifact capture capability does not cover the requested export scope.",
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
            "outputs must contain at least one entry.",
            false,
        ));
    }
    for output in outputs {
        if !EXPORT_OUTPUTS.contains(&output.as_str()) {
            return Err(DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                format!(
                    "Unsupported export output: {output}. Supported: {}.",
                    EXPORT_OUTPUTS.join(", ")
                ),
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

        _ => Err(DevupError::new(
            ErrorCode::DevupInvalidInput,
            "sourcePolicy must be auto or direct.",
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
            "At most 16 assets can be exported at once.",
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
                "An assetRequests ID, scale, or duplicate entry is invalid.",
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
                    "asset format must be png, jpg, svg, or pdf.",
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
            "scope must be node, page, or file.",
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
            "rootLayout must be standalone or embedded.",
            false,
        )),
    }
}
