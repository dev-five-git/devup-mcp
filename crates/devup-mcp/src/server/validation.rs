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
        // An index can return a selection for any code projection. Actual
        // generation still requires collected frame snapshots.
        ArtifactKind::SectionIndex => outputs
            .iter()
            .any(|output| matches!(output.as_str(), "tsx" | "componentTsx" | "responsiveTsx")),
        ArtifactKind::Search | ArtifactKind::Explore => false,
    };
    if capabilities.kind == ArtifactKind::SectionIndex && !kind_compatible {
        return Err(DevupError::with_details(
            ErrorCode::DevupInvalidInput,
            "These outputs need collected screen data; this artifact is only a Section candidate index. Pass frameIds or allScreens:true to collect screens, then request the outputs again.",
            false,
            json!({"outputs":outputs,"artifactId":artifact.artifact_id,"capabilities":capabilities}),
        ));
    }
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

pub(super) const MAX_ASSET_COUNT: usize = 6;
pub(super) const RECOMMENDED_ASSET_COUNT: usize = 3;

pub(super) fn validate_asset_budget(requests: &[FigmaAssetRequestInput]) -> Result<(), DevupError> {
    if requests.len() > MAX_ASSET_COUNT {
        return Err(DevupError::with_details(
            ErrorCode::DevupInvalidInput,
            "Export exceeds the asset batch budget. Split assetRequests into calls of 1–3 assets (maximum 6); retain each format, scale and outputPath. Poll or resume an existing assetJob instead of restarting it.",
            false,
            json!({"requestedAssetCount":requests.len(),"maxAssetCount":MAX_ASSET_COUNT,
                "recommendedBatchSize":RECOMMENDED_ASSET_COUNT,
                "recommendedAssetRequests":&requests[..RECOMMENDED_ASSET_COUNT],
                "remainingAssetRequests":&requests[RECOMMENDED_ASSET_COUNT..],
                "clientTimeoutSeconds":300,
                "nextAction":{"type":"split_asset_requests","tool":"devup_figma_export",
                    "preserveOtherArguments":true,"replaceAssetRequests":&requests[..RECOMMENDED_ASSET_COUNT]}}),
        ));
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
    validate_asset_budget(requests)?;
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

/// MCP classification is exhaustive here because the shared Figma enum also
/// serves collectors, where unsupported snapshots can mean corrupt data.
/// Keep internal acquisition failures distinct from editable tool arguments.
pub(super) fn is_caller_mistake(error: &DevupError) -> bool {
    match error.code {
        ErrorCode::DevupInvalidInput
        | ErrorCode::DevupFigmaNodeNotFound
        | ErrorCode::DevupFigmaUnsupportedFile
        | ErrorCode::DevupProjectRootNotFound
        | ErrorCode::DevupFigmaHandoffInvalid => true,
        // output.rs currently uses CodegenFailed for both validation and I/O.
        // Only its explicit parameter refusals are caller mistakes.
        ErrorCode::DevupCodegenFailed => matches!(
            error.message.as_str(),
            "outputPath must be a file path."
                | "outputPath is outside the allowed root."
                | "outputPath contains an unsafe file name."
                | "outputPath cannot escape the allowed root."
                | "A symlink or junction in an outputPath ancestor is not allowed."
                | "An outputPath ancestor is not a directory."
                | "Two or more outputs cannot use the same file path."
        ),
        ErrorCode::DevupSnapshotUnsupported => matches!(
            error.message.as_str(),
            "Using assetRequests requires assetManifest in outputs."
                | "referencePng can only be collected for a single Figma link target."
                | "The Section Frame collection scope must be node."
                | "frameIds and allScreens cannot be used together."
                | "frameIds and allScreens can only be used on a Section artifact."
                | "frameIds contains a duplicate node."
        ),
        ErrorCode::DevupAuthRequired
        | ErrorCode::DevupAuthCallbackTimeout
        | ErrorCode::DevupAuthStateMismatch
        | ErrorCode::DevupFigmaCallbackPortInUse
        | ErrorCode::DevupFigmaPermissionDenied
        | ErrorCode::DevupFigmaRateLimited
        | ErrorCode::DevupFigmaDirectUnavailable
        | ErrorCode::DevupFigmaCatalogRejected
        | ErrorCode::DevupFigmaHandoffExpired
        | ErrorCode::DevupFigmaResponseTooLarge
        | ErrorCode::DevupFigmaVersionChanged
        | ErrorCode::DevupThemeConflict => false,
    }
}

pub(super) fn validate_export_budget(
    frame_ids: &[String],
    outputs: &[String],
) -> Result<(), DevupError> {
    let output_count = outputs
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        .max(1);
    let batch_size = (12 / output_count).clamp(1, 6);
    if frame_ids.len() > batch_size {
        let recommended = batch_size.min(3);
        return Err(DevupError::with_details(
            ErrorCode::DevupInvalidInput,
            "Export exceeds the batch budget for a 300-second client timeout. Split frameIds into smaller calls; use 1–3 frames per call and reuse artifactId when its capture covers the selected frames.",
            false,
            json!({"requestedFrameCount":frame_ids.len(),"outputCount":output_count,
                "frameOutputUnits":frame_ids.len().saturating_mul(output_count),
                "maxFrameOutputUnits":12,"maxFrameCount":6,"recommendedBatchSize":recommended,
                "recommendedFrameIds":&frame_ids[..recommended],"remainingFrameIds":&frame_ids[recommended..],
                "estimatedSecondsPerFrame":[15,60],"estimateKind":"planning heuristic; complexity, paging and throttling can exceed this",
                "clientTimeoutSeconds":300}),
        ));
    }
    Ok(())
}

pub(super) fn validate_public_root(
    root: Option<&str>,
) -> Result<Option<std::path::PathBuf>, DevupError> {
    let Some(root) = root else {
        return Ok(None);
    };
    let invalid = || {
        DevupError::new(
            ErrorCode::DevupInvalidInput,
            "assetPublicRoot must be an existing absolute directory served at URL /.",
            false,
        )
    };
    if !std::path::Path::new(root).is_absolute() {
        return Err(invalid());
    }
    let root = dunce::canonicalize(root).map_err(|_| invalid())?;
    if !root.is_dir() {
        return Err(invalid());
    }
    Ok(Some(root))
}

pub(super) fn public_asset_url(
    root: &std::path::Path,
    target: &std::path::Path,
) -> Result<String, DevupError> {
    let relative = target.strip_prefix(root).map_err(|_| {
        DevupError::new(
            ErrorCode::DevupInvalidInput,
            "Asset outputPath must be inside assetPublicRoot.",
            false,
        )
    })?;
    let mut url = String::new();
    for component in relative.components() {
        let std::path::Component::Normal(segment) = component else {
            return Err(DevupError::new(
                ErrorCode::DevupInvalidInput,
                "Invalid public asset path.",
                false,
            ));
        };
        url.push('/');
        for byte in segment.to_string_lossy().as_bytes() {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(byte) {
                url.push(char::from(*byte));
            } else {
                use std::fmt::Write as _;
                write!(&mut url, "%{byte:02X}").expect("writing to String");
            }
        }
    }
    Ok(url)
}

#[cfg(test)]
mod p3_tests {
    use super::*;

    #[test]
    fn p3_error_variants_are_audited_at_the_mcp_boundary() {
        use ErrorCode::*;
        for (code, expected) in [
            (DevupAuthRequired, false),
            (DevupAuthCallbackTimeout, false),
            (DevupAuthStateMismatch, false),
            (DevupFigmaCallbackPortInUse, false),
            (DevupFigmaPermissionDenied, false),
            (DevupFigmaRateLimited, false),
            (DevupFigmaDirectUnavailable, false),
            (DevupFigmaCatalogRejected, false),
            (DevupFigmaHandoffExpired, false),
            (DevupFigmaHandoffInvalid, true),
            (DevupFigmaNodeNotFound, true),
            (DevupFigmaUnsupportedFile, true),
            (DevupFigmaResponseTooLarge, false),
            (DevupFigmaVersionChanged, false),
            (DevupSnapshotUnsupported, false),
            (DevupCodegenFailed, false),
            (DevupThemeConflict, false),
            (DevupInvalidInput, true),
            (DevupProjectRootNotFound, true),
        ] {
            assert_eq!(
                is_caller_mistake(&DevupError::new(code, "internal detail", false)),
                expected,
                "{code:?}"
            );
        }
        for message in [
            "Using assetRequests requires assetManifest in outputs.",
            "referencePng can only be collected for a single Figma link target.",
            "The Section Frame collection scope must be node.",
            "frameIds and allScreens cannot be used together.",
            "frameIds and allScreens can only be used on a Section artifact.",
            "frameIds contains a duplicate node.",
        ] {
            assert!(is_caller_mistake(&DevupError::new(
                DevupSnapshotUnsupported,
                message,
                false
            )));
        }
        for message in [
            "The exported asset binary base64 is invalid.",
            "The exported asset length or hash does not match.",
        ] {
            assert!(!is_caller_mistake(&DevupError::new(
                DevupSnapshotUnsupported,
                message,
                false
            )));
        }
        for message in [
            "outputPath must be a file path.",
            "outputPath is outside the allowed root.",
            "outputPath contains an unsafe file name.",
            "outputPath cannot escape the allowed root.",
            "A symlink or junction in an outputPath ancestor is not allowed.",
            "An outputPath ancestor is not a directory.",
            "Two or more outputs cannot use the same file path.",
        ] {
            assert!(is_caller_mistake(&DevupError::new(
                DevupCodegenFailed,
                message,
                false
            )));
        }
        assert!(!is_caller_mistake(&DevupError::new(
            DevupCodegenFailed,
            "Cannot create the output staging file: disk full",
            false
        )));
    }

    #[test]
    fn r3_asset_budget_splits_sixteen_requests_without_losing_paths() {
        let requests: Vec<FigmaAssetRequestInput> = (0..16)
            .map(|i| {
                serde_json::from_value(json!({
            "assetId":format!("1:{i}:node"),"outputPath":format!("icons/{i}.svg"),"format":"svg"
        })).unwrap()
            })
            .collect();
        let error = parse_asset_requests(&requests).unwrap_err();
        assert_eq!(error.details["maxAssetCount"], 6);
        assert_eq!(error.details["recommendedBatchSize"], 3);
        assert_eq!(
            error.details["recommendedAssetRequests"][0]["outputPath"],
            "icons/0.svg"
        );
        assert_eq!(
            error.details["remainingAssetRequests"]
                .as_array()
                .unwrap()
                .len(),
            13
        );
        for batch in requests.chunks(3) {
            assert_eq!(parse_asset_requests(batch).unwrap().0.len(), batch.len());
        }
    }

    #[test]
    fn p3_budget_counts_frames_times_distinct_outputs() {
        let frames = (1..=6).map(|i| format!("1:{i}")).collect::<Vec<_>>();
        assert!(validate_export_budget(&frames, &["tsx".into(), "assetManifest".into()]).is_ok());
        let error = validate_export_budget(
            &frames,
            &["tsx".into(), "devupJson".into(), "assetManifest".into()],
        )
        .unwrap_err();
        assert_eq!(error.details["recommendedBatchSize"], 3);
        assert_eq!(error.details["maxFrameCount"], 6);
        assert_eq!(error.details["frameOutputUnits"], 18);
        assert_eq!(
            error.details["remainingFrameIds"],
            json!(["1:4", "1:5", "1:6"])
        );
        assert!(validate_export_budget(&frames, &["tsx".into(), "tsx".into()]).is_ok());
    }

    #[test]
    fn p3_public_mapping_rejects_outside_root_and_encodes_url_segments() {
        let root = std::env::temp_dir();
        let mapped = public_asset_url(&root, &root.join("icons").join("로고 #1.png")).unwrap();
        assert_eq!(mapped, "/icons/%EB%A1%9C%EA%B3%A0%20%231.png");
        assert!(public_asset_url(&root.join("public"), &root.join("private.png")).is_err());
    }
}
