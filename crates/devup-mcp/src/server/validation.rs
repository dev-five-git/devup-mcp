use devup_mcp_devup_ui::codegen::RootLayout;
use devup_mcp_figma::{
    AssetFormat, AssetSelection, CollectionScope, DevupError, ErrorCode, ResourceScope,
};
use serde_json::json;

use super::{
    artifacts::{ArtifactKind, ArtifactLookup},
    tools::FigmaAssetRequestInput,
};

/// The values each closed-set string input accepts.
///
/// Every one of these is advertised by the tool's JSON schema and consumed by
/// the parser below, so a caller discovers the set instead of learning it one
/// rejection at a time, and the published schema cannot drift from what is
/// actually accepted. `outputs` and the asset `format` were the only two
/// inputs that did this; the rest arrived as bare strings, which left an
/// agent to guess `scope` and `delivery` and find out by being refused.
pub(crate) const AUTH_ACTIONS: [&str; 5] = ["status", "login", "logout", "configure", "doctor"];

pub(crate) const COLLECTION_SCOPES: [&str; 3] = ["node", "page", "file"];

pub(crate) const ROOT_LAYOUTS: [&str; 2] = ["standalone", "embedded"];

pub(crate) const DELIVERY_MODES: [&str; 3] = ["auto", "inline", "resource"];

pub(crate) const SEARCH_MATCH_KINDS: [&str; 3] = ["exact", "normalized", "fuzzy"];

pub(crate) const ASSET_FORMATS: [&str; 4] = ["png", "jpg", "svg", "pdf"];

pub(crate) const PROJECT_CONTEXT_SCOPES: [&str; 4] = ["theme", "api", "db", "all"];

pub(crate) const STACK_DIFF_LAYERS: [&str; 4] = [
    "db-entity",
    "entity-route",
    "route-openapi",
    "openapi-client",
];

pub(crate) const EXPORT_OUTPUTS: [&str; 9] = [
    "tsx",
    "componentTsx",
    "responsiveTsx",
    "devupJson",
    "rawSnapshot",
    "rawPayload",
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
            "tsx"
                | "componentTsx"
                | "responsiveTsx"
                | "rawSnapshot"
                | "rawPayload"
                | "sourceMap"
                | "assetManifest"
                | "referencePng"
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

/// The two outputs that answer a question about the generator rather than
/// contributing to the screen.
///
/// They are the design as it was collected, in raw form. Nothing that
/// implements a screen needs them - the tsx already carries what they carry,
/// measured across ten real captured screens at 100% of nodes, text,
/// typography, assets and layout. What they are for is the other question:
/// the UI looks wrong, and someone has to decide whether the generator is at
/// fault or the design says so. Answering that means reading the design
/// beside the code, which is exactly this.
///
/// Left in the ordinary `outputs` list they were requested as a matter of
/// course - the server's own instructions used to say to take `rawSnapshot`
/// every time, which on one measured screen spent about eight bytes for
/// every one of code. `debug` is the door: closed for implementation, open
/// when a defect is being adjudicated.
pub(crate) const DIAGNOSIS_OUTPUTS: [&str; 2] = ["rawSnapshot", "rawPayload"];

pub(super) fn validate_outputs(outputs: &[String], debug: bool) -> Result<(), DevupError> {
    if outputs.is_empty() {
        return Err(DevupError::new(
            ErrorCode::DevupInvalidInput,
            "outputs must contain at least one entry.",
            false,
        ));
    }
    for output in outputs {
        if !EXPORT_OUTPUTS.contains(&output.as_str()) {
            return Err(DevupError::new(
                ErrorCode::DevupInvalidInput,
                format!(
                    "Unsupported export output: {output}. Supported: {}.",
                    EXPORT_OUTPUTS.join(", ")
                ),
                false,
            ));
        }
        if !debug && DIAGNOSIS_OUTPUTS.contains(&output.as_str()) {
            return Err(DevupError::new(
                ErrorCode::DevupInvalidInput,
                format!(
                    "{output} is the collected design in raw form, for deciding whether a \
                     screen that looks wrong is the generator's fault or the design's. It is \
                     not needed to implement anything - the tsx already carries what it \
                     carries - and requesting it by habit is most of the response. Pass \
                     debug: true to read it."
                ),
                false,
            ));
        }
    }
    Ok(())
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
            ErrorCode::DevupInvalidInput,
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
                ErrorCode::DevupInvalidInput,
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
                    ErrorCode::DevupInvalidInput,
                    format!("asset format must be one of: {}.", ASSET_FORMATS.join(", ")),
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

// A bad `scope` or `rootLayout` used to be reported as DEVUP_THEME_CONFLICT,
// which names a real condition - two collections defining one token - and has
// nothing to do with a misspelled argument. Both are DEVUP_INVALID_INPUT now,
// so `is_caller_mistake` can route them to INVALID_PARAMS without dragging
// genuine theme conflicts along with them.
pub(super) fn parse_collection_scope(scope: &str) -> Result<CollectionScope, DevupError> {
    match scope {
        "node" => Ok(CollectionScope::Node),
        "page" => Ok(CollectionScope::Page),
        "file" => Ok(CollectionScope::File),
        _ => Err(DevupError::new(
            ErrorCode::DevupInvalidInput,
            format!("scope must be one of: {}.", COLLECTION_SCOPES.join(", ")),
            false,
        )),
    }
}

pub(super) fn parse_root_layout(root_layout: &str) -> Result<RootLayout, DevupError> {
    match root_layout {
        "standalone" => Ok(RootLayout::Standalone),
        "embedded" => Ok(RootLayout::Embedded),
        _ => Err(DevupError::new(
            ErrorCode::DevupInvalidInput,
            format!("rootLayout must be one of: {}.", ROOT_LAYOUTS.join(", ")),
            false,
        )),
    }
}
