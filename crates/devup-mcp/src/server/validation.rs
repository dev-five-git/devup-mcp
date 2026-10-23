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

/// `status` reads the workspace and writes nothing. `install` writes the
/// vendored documents for the embedded skills; it can never install an
/// external one, because those bytes are not devup-mcp's to ship.
pub(crate) const SKILL_ACTIONS: [&str; 2] = ["status", "install"];

pub(crate) const COLLECTION_SCOPES: [&str; 3] = ["node", "page", "file"];

pub(crate) const ROOT_LAYOUTS: [&str; 2] = ["standalone", "embedded"];

pub(crate) const DELIVERY_MODES: [&str; 3] = ["auto", "inline", "resource"];

pub(crate) const SEARCH_MATCH_KINDS: [&str; 3] = ["exact", "normalized", "fuzzy"];

pub(crate) const ASSET_FORMATS: [&str; 5] = ["png", "jpg", "svg", "pdf", "webp"];

pub(crate) const PROJECT_CONTEXT_SCOPES: [&str; 5] = ["theme", "api", "db", "routes", "all"];

pub(crate) const STACK_DIFF_LAYERS: [&str; 4] = [
    "db-entity",
    "entity-route",
    "route-openapi",
    "openapi-client",
];

pub(crate) const EXPORT_OUTPUTS: [&str; 15] = [
    "outline",
    "imageAudit",
    "assetCssMap",
    "tsx",
    "componentTsx",
    "responsiveTsx",
    "pageScaffold",
    "devupJson",
    "rawSnapshot",
    "rawPayload",
    "sourceMap",
    "designFingerprints",
    "designChanges",
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
            "outline"
                | "tsx"
                | "componentTsx"
                | "responsiveTsx"
                | "pageScaffold"
                | "rawSnapshot"
                | "rawPayload"
                | "sourceMap"
                | "designFingerprints"
                | "designChanges"
                | "assetManifest"
                | "imageAudit"
                | "assetCssMap"
                | "referencePng"
        )
    });
    let theme_requested = outputs.iter().any(|output| output == "devupJson");
    let kind_compatible = match capabilities.kind {
        ArtifactKind::Design => true,
        ArtifactKind::ThemeOnly => theme_requested && !design_output_requested,
        // An index can return a selection for any code projection. Actual
        // generation still requires collected frame snapshots.
        ArtifactKind::SectionIndex => outputs.iter().any(|output| {
            matches!(
                output.as_str(),
                "tsx"
                    | "componentTsx"
                    | "responsiveTsx"
                    | "pageScaffold"
                    | "designFingerprints"
                    | "designChanges"
            )
        }),
        ArtifactKind::Measurement | ArtifactKind::Search | ArtifactKind::Explore => false,
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

    let missing = [
        (!kind_compatible).then_some("artifact-kind"),
        (!collection_compatible).then_some("collection-scope"),
        (!resources_compatible).then_some("file-resource-scope"),
        (!assets_compatible).then_some("asset-captures"),
        (!reference_png_compatible).then_some("referencePng"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    let target = &artifact.payload.target;
    let mut arguments = json!({
        "url": target.link(target.node_id.as_deref()),
        "outputs": outputs,
        "scope": requested_scope,
    });
    if let Some(selection) = &capabilities.section_selection {
        arguments["frameIds"] = json!(selection.frame_ids);
        arguments["allScreens"] = json!(selection.all_screens);
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
            },
            "recovery": {
                "mode": "recollect",
                "cause": "capture-missing",
                "missing": missing,
                "reprojectable": false,
                "requiresOriginalUrl": false,
            },
            "nextAction": {
                "tool": "devup_figma_export",
                "arguments": arguments,
                "omitArguments": ["artifactId"],
                "how": include_str!("text/validation-validate_artifact_projection-e8e39fca.md"),
            },
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
            return Err(DevupError::with_details(
                ErrorCode::DevupInvalidInput,
                format!(
                    include_str!("text/validation-validate_outputs-166587f3.md"),
                    output = output
                ),
                false,
                json!({"stage":"preflight","nextAction":{"tool":"devup_figma_export",
                    "arguments":{"debug":true},
                    "how":"Repeat the same call with debug: true. Pair rawSnapshot with sourceMap, and reuse cache.artifactId for later projections instead of re-collecting."}}),
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
            include_str!("text/validation-validate_asset_budget-915c1c5f.md"),
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
        let asset_id = match (
            request.asset_id.trim().is_empty(),
            request.node_id.as_deref(),
        ) {
            (false, None) => request.asset_id.clone(),
            (true, Some(id)) if !id.trim().is_empty() => format!("{id}:node"),
            _ => {
                return Err(DevupError::new(
                    ErrorCode::DevupInvalidInput,
                    "Supply exactly one of assetId or nodeId.",
                    false,
                ));
            }
        };
        if !request.scale.is_finite()
            || asset_id.trim().is_empty()
            || request.scale == 0.0
            || request.scale > 4.0
            || !seen.insert(asset_id.clone())
        {
            return Err(DevupError::new(
                ErrorCode::DevupInvalidInput,
                "An assetRequests ID, scale, or duplicate entry is invalid.",
                false,
            ));
        }
        let defaulted_source = request.format.is_none() && request.render_mode == "original-source";
        let format = match request.format.as_deref().unwrap_or(if defaulted_source { "auto" } else { "png" }) {
            "auto" if defaulted_source => AssetFormat::Auto,
            "png" => AssetFormat::Png,
            "webp" => AssetFormat::Webp,
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
        let render_mode = match request.render_mode.as_str() {
            "original-source" if matches!(format, AssetFormat::Svg | AssetFormat::Pdf) => {
                devup_mcp_figma::AssetRenderMode::Node
            }
            "original-source" => devup_mcp_figma::AssetRenderMode::OriginalSource,
            "visible" => devup_mcp_figma::AssetRenderMode::Visible,
            "node" => devup_mcp_figma::AssetRenderMode::Node,
            "fill" => devup_mcp_figma::AssetRenderMode::Fill,
            "auto" => devup_mcp_figma::AssetRenderMode::Auto,
            _ => {
                return Err(DevupError::new(
                    ErrorCode::DevupInvalidInput,
                    "renderMode must be original-source, fill, node, visible or auto.",
                    false,
                ));
            }
        };
        let quality = request.quality;
        devup_mcp_figma::validate_webp_options(format, quality, request.lossless)?;
        if render_mode == devup_mcp_figma::AssetRenderMode::Fill
            && (!matches!(
                format,
                AssetFormat::Png | AssetFormat::Jpg | AssetFormat::Webp
            ) || request.scale != 1.0)
        {
            return Err(DevupError::new(
                ErrorCode::DevupInvalidInput,
                include_str!("text/validation-parse_asset_requests-eecb2af1.md"),
                false,
            ));
        }
        if request
            .usage_aspect_ratio
            .is_some_and(|r| !r.is_finite() || r <= 0.0 || r > 10000.0)
        {
            return Err(DevupError::new(
                ErrorCode::DevupInvalidInput,
                "usageAspectRatio must be finite, positive and at most10000.",
                false,
            ));
        }
        devup_mcp_figma::validate_asset_options(
            format,
            render_mode,
            request.scale,
            request.max_width,
            request.srcset,
        )?;
        selections.push(AssetSelection {
            usage_aspect_ratio: request.usage_aspect_ratio,
            max_width: request.max_width,
            srcset: request.srcset,
            visible_scope_node_id: request.visible_scope_node_id.clone(),
            quality,
            lossless: request.lossless,
            render_mode,
            asset_id: asset_id.clone(),
            format,
            scale: request.scale,
        });
        if let Some(path) = &request.output_path {
            let path = if format == AssetFormat::Webp {
                std::path::Path::new(path)
                    .with_extension("webp")
                    .to_string_lossy()
                    .into_owned()
            } else {
                path.clone()
            };
            output_paths.insert(asset_id, path);
        }
    }
    Ok((selections, output_paths))
}

pub(super) fn validate_asset_render_outputs(
    requests: &[FigmaAssetRequestInput],
    outputs: &[String],
) -> Result<(), DevupError> {
    if requests.iter().any(|a| a.render_mode == "fill")
        && outputs.iter().any(|o| {
            matches!(
                o.as_str(),
                "tsx" | "componentTsx" | "responsiveTsx" | "pageScaffold"
            )
        })
    {
        return Err(DevupError::new(
            ErrorCode::DevupInvalidInput,
            "Legacy fill captures remain separate from generated code; use original-source for CSS projection or renderMode node.",
            false,
        ));
    }
    if requests.iter().any(|a| a.render_mode == "visible")
        && outputs.iter().any(|o| {
            matches!(
                o.as_str(),
                "tsx" | "componentTsx" | "responsiveTsx" | "pageScaffold"
            )
        })
    {
        return Err(DevupError::new(
            ErrorCode::DevupInvalidInput,
            include_str!("text/validation-validate_asset_render_outputs-12b6f4a5.md"),
            false,
        ));
    }
    Ok(())
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

pub(super) fn parse_width_policy(
    input: Option<&super::tools::WidthPolicyInput>,
) -> Result<Option<devup_mcp_devup_ui::codegen::width::WidthPolicy>, DevupError> {
    use devup_mcp_devup_ui::codegen::width::{Anchor, WidthPolicy};
    let Some(input) = input else {
        return Ok(None);
    };
    if input.root != "fluid" {
        return Err(DevupError::new(
            ErrorCode::DevupInvalidInput,
            "widthPolicy.root must be \"fluid\".",
            false,
        ));
    }
    let anchors = input
        .anchors
        .iter()
        .map(|(node_id, anchor)| {
            let anchor = match anchor.as_str() {
                "left" => Anchor::Left,
                "right" => Anchor::Right,
                "center" => Anchor::Center,
                "stretch" => Anchor::Stretch,
                "scale" => Anchor::Scale,
                other => {
                    return Err(DevupError::new(
                        ErrorCode::DevupInvalidInput,
                        format!(
                            "widthPolicy.anchors[{node_id}] must be left, right, center, stretch or scale, not {other}."
                        ),
                        false,
                    ));
                }
            };
            Ok((node_id.clone(), anchor))
        })
        .collect::<Result<_, _>>()?;
    Ok(Some(WidthPolicy { anchors }))
}

/// A policy may only name nodes the capture holds; a typo would otherwise be
/// an anchor that silently never applies.
pub(super) fn validate_width_policy_nodes(
    policy: Option<&devup_mcp_devup_ui::codegen::width::WidthPolicy>,
    snapshot: &devup_mcp_figma::Snapshot,
) -> Result<(), DevupError> {
    let unknown = policy
        .into_iter()
        .flat_map(|policy| policy.anchors.keys())
        .filter(|node_id| !snapshot.nodes.contains_key(*node_id))
        .collect::<Vec<_>>();
    if unknown.is_empty() {
        return Ok(());
    }
    Err(DevupError::with_details(
        ErrorCode::DevupInvalidInput,
        "widthPolicy.anchors names nodes this capture does not hold.",
        false,
        json!({"unknownNodeIds": unknown,
            "how": "Use node ids from widthBehavior.findings of this export."}),
    ))
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
            include_str!("text/validation-validate_export_budget-986a2783.md"),
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
    fn webp_options_are_validated_and_paths_use_the_actual_codec_extension() {
        let base = json!({"assetId":"sample:fills:0","format":"webp","quality":0,"lossless":true,"outputPath":"images/sample.png"});
        let parsed =
            parse_asset_requests(&[serde_json::from_value(base.clone()).unwrap()]).unwrap();
        assert_eq!(parsed.0[0].quality, Some(0));
        assert_eq!(parsed.0[0].lossless, Some(true));
        assert!(parsed.1.values().next().unwrap().ends_with(".webp"));
        for change in [
            json!({"quality":101}),
            json!({"format":"png","quality":80}),
            json!({"format":"jpg","lossless":false}),
        ] {
            let mut input = base.clone();
            for (key, value) in change.as_object().unwrap() {
                input[key] = value.clone();
            }
            assert!(parse_asset_requests(&[serde_json::from_value(input).unwrap()]).is_err());
        }
        for invalid in [json!(-1), json!(0.5), json!("80"), json!(256)] {
            let mut input = base.clone();
            input["quality"] = invalid;
            assert!(serde_json::from_value::<FigmaAssetRequestInput>(input).is_err());
        }
        let mut input = base;
        input["lossless"] = json!("true");
        assert!(serde_json::from_value::<FigmaAssetRequestInput>(input).is_err());
    }

    #[test]
    fn image_mode_input_preserves_default_and_refuses_unimplemented_settings() {
        let parse = |value: serde_json::Value| {
            parse_asset_requests(&[serde_json::from_value(value).unwrap()])
        };
        let base = json!({"assetId":"sample:fills:0"});
        assert_eq!(
            parse(base.clone()).unwrap().0[0].render_mode,
            devup_mcp_figma::AssetRenderMode::OriginalSource
        );
        let fill = json!({"assetId":"sample:fills:0","renderMode":"fill","format":"png","scale":1});
        assert_eq!(
            parse(fill.clone()).unwrap().0[0].render_mode,
            devup_mcp_figma::AssetRenderMode::Fill
        );
        let request = serde_json::from_value(fill.clone()).unwrap();
        assert!(
            validate_asset_render_outputs(
                std::slice::from_ref(&request),
                &["assetManifest".into()]
            )
            .is_ok()
        );
        for output in ["tsx", "componentTsx", "responsiveTsx", "pageScaffold"] {
            assert!(
                validate_asset_render_outputs(std::slice::from_ref(&request), &[output.into()])
                    .is_err()
            );
        }
        for (field, value) in [
            ("renderMode", json!("unknown")),
            ("format", json!("svg")),
            ("format", json!("pdf")),
            ("scale", json!(2)),
        ] {
            let mut input = fill.clone();
            input[field] = value;
            assert!(parse(input).is_err());
        }
    }

    #[test]
    fn original_source_defaults_fractional_node_and_resize_schema_are_real_inputs() {
        let parse =
            |v: serde_json::Value| parse_asset_requests(&[serde_json::from_value(v).unwrap()]);
        let (defaults, _) = parse(json!({"assetId":"picture:fills:0"})).unwrap();
        assert_eq!(defaults[0].scale, 2.0);
        assert_eq!(
            defaults[0].render_mode,
            devup_mcp_figma::AssetRenderMode::OriginalSource
        );
        let (node, _) = parse(json!({"nodeId":"card","renderMode":"node","scale":1.5})).unwrap();
        assert_eq!(node[0].asset_id, "card:node");
        assert_eq!(node[0].scale, 1.5);
        let (source,_)=parse(json!({"assetId":"picture:fills:0","format":"webp","maxWidth":300,"srcset":true,"usageAspectRatio":1.25})).unwrap();
        assert_eq!(source[0].max_width, Some(300));
        assert!(source[0].srcset);
        assert_eq!(source[0].usage_aspect_ratio, Some(1.25));
        for bad in [
            json!({"nodeId":"card","assetId":"other"}),
            json!({"assetId":"picture:fills:0","scale":-1}),
            json!({"assetId":"picture:fills:0","scale":0}),
            json!({"assetId":"picture:fills:0","scale":4.1}),
            json!({"assetId":"picture:fills:0","format":"webp","maxWidth":0}),
            json!({"assetId":"picture:fills:0","format":"webp","usageAspectRatio":-1}),
        ] {
            assert!(parse(bad).is_err());
        }
        let schema = serde_json::to_value(schemars::schema_for!(FigmaAssetRequestInput)).unwrap();
        assert_eq!(schema["properties"]["scale"]["type"], "number");
        assert_eq!(schema["properties"]["scale"]["default"], 2.0);
        assert!(
            schema["properties"]["renderMode"]["enum"]
                .as_array()
                .unwrap()
                .contains(&json!("visible"))
        );
        assert_eq!(
            schema["properties"]["renderMode"]["default"],
            "original-source"
        );
    }

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

    #[tokio::test]
    async fn p2_capability_refusal_names_what_is_missing_and_how_to_recollect() {
        use super::super::artifacts::{
            ArtifactRequestKey, ArtifactStore,
            w3_tests::{payload, request},
        };
        let artifact = ArtifactStore::default()
            .insert(ArtifactRequestKey::from_collection(&request()), payload())
            .await
            .unwrap();
        let error = validate_artifact_projection(
            &artifact,
            &["tsx".into(), "referencePng".into()],
            "node",
            &[],
        )
        .unwrap_err();
        assert_eq!(error.details["recovery"]["mode"], "recollect");
        assert_eq!(
            error.details["recovery"]["missing"],
            json!(["referencePng"])
        );
        let next = &error.details["nextAction"];
        assert_eq!(next["omitArguments"], json!(["artifactId"]));
        assert_eq!(
            devup_mcp_figma::FigmaTarget::parse(next["arguments"]["url"].as_str().unwrap())
                .unwrap(),
            request().target
        );
        assert!(validate_artifact_projection(&artifact, &["tsx".into()], "node", &[]).is_ok());
    }

    #[test]
    fn p3_public_mapping_rejects_outside_root_and_encodes_url_segments() {
        let root = std::env::temp_dir();
        let mapped = public_asset_url(&root, &root.join("icons").join("로고 #1.png")).unwrap();
        assert_eq!(mapped, "/icons/%EB%A1%9C%EA%B3%A0%20%231.png");
        assert!(public_asset_url(&root.join("public"), &root.join("private.png")).is_err());
    }
}

#[cfg(test)]
mod r30_tests {
    use super::*;

    #[test]
    fn omitted_source_format_is_deferred_and_explicit_formats_are_kept() {
        let parse = |v: serde_json::Value| {
            parse_asset_requests(&[serde_json::from_value(v).unwrap()]).unwrap()
        };
        let (defaulted, _) = parse(json!({"assetId":"picture:fills:0"}));
        assert_eq!(defaulted[0].format, AssetFormat::Auto);
        assert_eq!(defaulted[0].quality, None);
        let (png, _) = parse(json!({"assetId":"picture:fills:0","format":"png"}));
        assert_eq!(png[0].format, AssetFormat::Png);
        assert_eq!(png[0].quality, None);
        let (svg, _) = parse(json!({"nodeId":"icon","renderMode":"node","format":"svg"}));
        assert_eq!(svg[0].format, AssetFormat::Svg);
        let (node, _) = parse(json!({"nodeId":"card","renderMode":"node"}));
        assert_eq!(node[0].format, AssetFormat::Png);
    }
}
