use std::collections::BTreeMap;
use std::fmt::Write as _;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_devup_ui::{
    codegen::{
        CodegenOptions, generate_component, normalize_component_name, responsive::merge_breakpoints,
    },
    theme::{generate_devup_json, variable_snapshot_from_result},
};
use devup_mcp_figma::{
    AssetManifest, AssetStatus, CollectedPayload, CollectionStats, DevupError, Diagnostic,
    DiagnosticSeverity, ErrorCode, ExploreOptions, SearchOptions, TargetKind, classify_target,
    explore_snapshot, search_snapshot,
};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::{
    artifacts::{ArtifactLookup, ArtifactStore, OutputReservation},
    delivery::{DeliveryMode, ProjectedOutput, choose_delivery_for_result},
    format_epoch_rfc3339,
    operation::PendingOperation,
    output::{OutputPolicy, OutputTransaction},
    parse_scope,
    quality::{
        AcquisitionQuality, OutputQuality, ProjectionQuality, acquisition_quality, assets_quality,
        projection_quality, theme_quality,
    },
    section_candidate_as_explore, section_index_from_payload,
};

pub(super) fn projected_outputs_from_result(
    result: &Map<String, Value>,
) -> Result<Vec<ProjectedOutput>, DevupError> {
    let mut outputs = Vec::new();
    for field in ["tsx", "componentTsx", "responsiveTsx"] {
        if let Some(tsx) = result.get(field).and_then(Value::as_str) {
            outputs.push(ProjectedOutput::text(
                field,
                "text/typescript",
                tsx.as_bytes().to_vec(),
            ));
        }
    }
    if let Some(devup_json) = result.get("devupJson").and_then(Value::as_str) {
        outputs.push(ProjectedOutput::text(
            "devupJson",
            "application/json",
            devup_json.as_bytes().to_vec(),
        ));
    }
    if let Some(reference) = result.get("referencePng") {
        let data = reference
            .get("dataBase64")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "The referencePng resource has no base64 data.",
                    false,
                )
            })?;
        let bytes = STANDARD.decode(data.as_bytes()).map_err(|_| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "The referencePng resource base64 is invalid.",
                false,
            )
        })?;
        outputs.push(ProjectedOutput::binary("reference.png", "image/png", bytes));
    }
    for (field, name) in [
        ("rawSnapshot", "raw-snapshot.json"),
        ("rawPayload", "raw-payload.json"),
        ("sourceMap", "source-map.json"),
        ("assetManifest", "asset-manifest.json"),
    ] {
        if let Some(value) = result.get(field) {
            outputs.push(ProjectedOutput::text(
                name,
                "application/json",
                encode_projected_json(value)?,
            ));
        }
    }
    if let Some(frames) = result.get("frames").and_then(Value::as_array) {
        for (index, frame) in frames.iter().enumerate() {
            for field in ["tsx", "componentTsx"] {
                if let Some(tsx) = frame.get(field).and_then(Value::as_str) {
                    let name = if field == "tsx" {
                        format!("frame-{}.tsx", index + 1)
                    } else {
                        format!("frame-{}.component.tsx", index + 1)
                    };
                    outputs.push(ProjectedOutput::text(
                        name,
                        "text/typescript",
                        tsx.as_bytes().to_vec(),
                    ));
                }
            }
            if let Some(source_map) = frame.get("sourceMap") {
                outputs.push(ProjectedOutput::text(
                    format!("frame-{}.source-map.json", index + 1),
                    "application/json",
                    encode_projected_json(source_map)?,
                ));
            }
        }
    }
    Ok(outputs)
}

fn encode_projected_json(value: &Value) -> Result<Vec<u8>, DevupError> {
    serde_json::to_vec(value).map_err(|error| {
        DevupError::new(
            ErrorCode::DevupSnapshotUnsupported,
            format!("Cannot serialize the resource output to JSON: {error}"),
            false,
        )
    })
}

pub(super) struct DeliveryAttachment {
    reservation: OutputReservation,
}

pub(super) async fn apply_delivery(
    result: &mut Value,
    mode: DeliveryMode,
    artifact_store: &ArtifactStore,
    artifact: &ArtifactLookup,
    mut outputs: Vec<ProjectedOutput>,
) -> Result<Option<DeliveryAttachment>, DevupError> {
    if outputs.is_empty() || choose_delivery_for_result(mode, result, &outputs)?.inline {
        return Ok(None);
    }
    let projection_key = projection_key(&outputs);
    materialize_asset_resource_references(result, &mut outputs, &artifact.artifact_id)?;
    let reservation = artifact_store
        .reserve_outputs(&artifact.artifact_id, &projection_key, outputs)
        .await?;
    let manifests = reservation.manifests().to_vec();
    let result = result.as_object_mut().ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaHandoffInvalid,
            "The resource delivery result is not a JSON object.",
            false,
        )
    })?;
    for field in [
        "tsx",
        "componentTsx",
        "responsiveTsx",
        "devupJson",
        "rawSnapshot",
        "rawPayload",
        "sourceMap",
        "assetManifest",
        "referencePng",
    ] {
        result.remove(field);
    }
    if let Some(frames) = result.get_mut("frames").and_then(Value::as_array_mut) {
        for frame in frames {
            if let Some(frame) = frame.as_object_mut() {
                frame.remove("tsx");
                frame.remove("componentTsx");
                frame.remove("sourceMap");
            }
        }
    }
    if let Some(deliverable) = result.get_mut("deliverable") {
        deliverable["note"] = json!(
            "The final TSX deliverables are in resources. Read the linked resources to implement them."
        );
    }
    result.insert(
        "resources".to_owned(),
        Value::Array(
            manifests
                .into_iter()
                .map(|manifest| {
                    json!({
                        "type": "resource_link",
                        "uri": manifest.manifest_uri,
                        "name": manifest.name,
                        "mimeType": manifest.mime_type,
                        "size": manifest.raw_bytes,
                        "contentHash": manifest.sha256,
                        "expiresAt": format_epoch_rfc3339(manifest.expires_at_epoch_seconds)
                    })
                })
                .collect(),
        ),
    );
    Ok(Some(DeliveryAttachment { reservation }))
}

pub(super) async fn rollback_delivery(
    _artifact_store: &ArtifactStore,
    _artifact: &ArtifactLookup,
    attachment: Option<DeliveryAttachment>,
) {
    if let Some(attachment) = attachment {
        attachment.reservation.rollback();
    }
}

pub(super) fn commit_delivery(attachment: Option<DeliveryAttachment>) {
    if let Some(attachment) = attachment {
        attachment.reservation.commit();
    }
}

fn materialize_asset_resource_references(
    result: &mut Value,
    outputs: &mut [ProjectedOutput],
    artifact_id: &str,
) -> Result<(), DevupError> {
    let Some(assets) = result
        .get_mut("assetManifest")
        .and_then(|manifest| manifest.get_mut("assets"))
        .and_then(Value::as_array_mut)
    else {
        return Ok(());
    };
    for output in outputs.iter().filter(|output| output.asset_id.is_some()) {
        let asset_id = output.asset_id.as_deref().unwrap_or_default();
        let asset = assets
            .iter_mut()
            .find(|asset| asset.get("assetId").and_then(Value::as_str) == Some(asset_id))
            .and_then(Value::as_object_mut)
            .ok_or_else(|| {
                DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "No manifest entry matches this asset resource.",
                    false,
                )
            })?;
        asset.remove("dataBase64");
        asset.insert(
            "resource".to_owned(),
            json!({
                "uri": output.manifest_uri(artifact_id),
                "mimeType": output.mime_type,
                "byteLength": output.bytes.len(),
                "sha256": sha256_hex(&output.bytes)
            }),
        );
    }
    if let Some(manifest_output) = outputs
        .iter_mut()
        .find(|output| output.name == "asset-manifest.json")
    {
        manifest_output.bytes =
            encode_projected_json(result.get("assetManifest").ok_or_else(|| {
                DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "The asset manifest resource is missing.",
                    false,
                )
            })?)?;
    }
    Ok(())
}

fn projected_asset_outputs(manifest: &AssetManifest) -> Result<Vec<ProjectedOutput>, DevupError> {
    let mut outputs = Vec::new();
    for (index, asset) in manifest.assets.iter().enumerate() {
        if asset.status != AssetStatus::Exported {
            continue;
        }
        let data = asset.data_base64.as_deref().ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "The exported asset binary is not in the artifact.",
                false,
            )
        })?;
        let bytes = STANDARD.decode(data.as_bytes()).map_err(|_| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "The exported asset binary base64 is invalid.",
                false,
            )
        })?;
        let mime_type = asset.mime_type.as_deref().ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "The exported asset has no MIME type.",
                false,
            )
        })?;
        let expected_hash = asset.sha256.as_deref().ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "The exported asset has no hash.",
                false,
            )
        })?;
        if asset.byte_length != Some(bytes.len()) || expected_hash != sha256_hex(&bytes) {
            return Err(DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "The exported asset length or hash does not match.",
                false,
            ));
        }
        let extension = asset
            .format
            .map_or("bin", devup_mcp_figma::AssetFormat::extension);
        outputs.push(ProjectedOutput::asset(
            format!("asset-{}.{extension}", index + 1),
            mime_type,
            bytes,
            &asset.asset_id,
        ));
    }
    Ok(outputs)
}

/// Bound generated filenames, retaining a hash when transliteration would lose
/// identity. The generated folders are short; user-chosen output roots remain
/// subject to OutputPolicy and the host filesystem's own path limits.
fn host_safe_file_name(name: &str) -> String {
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name, ""));
    let mut slug = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    slug = slug.trim_matches('-').to_owned();
    let upper = slug.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && matches!(upper.as_bytes()[3], b'1'..=b'9'));
    let needs_hash =
        !name.is_ascii() || slug != stem || slug.len() > 120 || slug.is_empty() || reserved;
    if slug.is_empty() || reserved {
        slug.insert_str(0, "asset-");
    }
    if needs_hash {
        slug.truncate(100);
    }
    if needs_hash {
        slug.push('-');
        slug.push_str(&sha256_hex(name.as_bytes())[..12]);
    }
    let extension = extension
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(10)
        .collect::<String>();
    if !extension.is_empty() {
        slug.push('.');
        slug.push_str(&extension);
    }
    slug
}

/// `/icons/ic:round-arrow-left.svg` as `/icons/ic-round-arrow-left-b041d24e668d.svg`. Only
/// the file name is touched; the folder the code looks in is left alone.
fn host_safe_asset_path(path: &str) -> String {
    for prefix in ["/icons/", "/images/"] {
        if let Some(name) = path.strip_prefix(prefix) {
            return format!("{prefix}{}", host_safe_file_name(name));
        }
    }
    match path.rsplit_once('/') {
        Some((directory, name)) => format!("{directory}/{}", host_safe_file_name(name)),
        None => host_safe_file_name(path),
    }
}

/// Every asset the generated code points at, renamed the way its bytes will
/// be written. The code holds each path inside a string or a `url(...)`, so a
/// name runs to the first quote or closing parenthesis - spaces and dots
/// belong to it, as they do in `Frame 269.png`.
fn with_host_safe_asset_paths(code: &str) -> String {
    let mut safe = String::with_capacity(code.len());
    let mut rest = code;
    loop {
        let Some((at, prefix)) = ["/icons/", "/images/"]
            .into_iter()
            .filter_map(|prefix| rest.find(prefix).map(|at| (at, prefix)))
            .min_by_key(|(at, _)| *at)
        else {
            safe.push_str(rest);
            return safe;
        };
        let after = at + prefix.len();
        safe.push_str(&rest[..after]);
        let end = rest[after..]
            .find(['"', '\'', ')'])
            .map_or(rest.len(), |offset| after + offset);
        safe.push_str(&host_safe_file_name(&rest[after..end]));
        rest = &rest[end..];
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Compare the generated text, removing only known asset references in codegen's
/// src and CSS URL forms. Literal text, layout, imports and component names stay
/// significant. Separate segments avoid collisions with a placeholder string.
fn generated_structure(code: &str, paths: &[String]) -> Vec<String> {
    let mut ranges = Vec::new();
    for attribute in ["src", "bg", "backgroundImage", "maskImage"] {
        let opening = format!("{attribute}=\"");
        for (at, _) in code.match_indices(&opening) {
            // An attribute name must start at a word boundary, not inside an
            // unrelated prop. Codegen quotes literal text containing quotes.
            if !code[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace)
            {
                continue;
            }
            let start = at + opening.len();
            let Some(end) = code[start..].find('"') else {
                continue;
            };
            let value = &code[start..start + end];
            for path in paths {
                if attribute == "src" {
                    if value == path {
                        ranges.push((start, start + path.len()));
                    }
                    continue;
                }
                // Preserve all paint layers and positions while recognizing
                // every image, including URLs after gradients or other images.
                for (prefix, suffix) in [("url(", ")"), ("url('", "')")] {
                    let reference = format!("{prefix}{path}{suffix}");
                    for (offset, _) in value.match_indices(&reference) {
                        let asset_start = start + offset + prefix.len();
                        ranges.push((asset_start, asset_start + path.len()));
                    }
                }
            }
        }
    }
    ranges.sort_unstable();
    ranges.dedup();
    let mut segments = Vec::new();
    let mut previous = 0;
    for (start, end) in ranges {
        if start < previous {
            continue;
        }
        segments.push(code[previous..start].to_owned());
        previous = end;
    }
    segments.push(code[previous..].to_owned());
    segments
}

fn annotate_identical_frames(frames: &mut [Value], payload: &CollectedPayload, per_node: bool) {
    let paths = devup_mcp_figma::discover_asset_manifest(&payload.snapshot)
        .assets
        .iter()
        .filter_map(|asset| {
            match asset
                .field
                .strip_prefix("fills/")
                .and_then(|n| n.parse::<usize>().ok())
            {
                Some(index) => devup_mcp_devup_ui::codegen::image_fill_path(
                    &payload.snapshot,
                    &asset.node_id,
                    index,
                    per_node,
                ),
                None => devup_mcp_devup_ui::codegen::asset_path(
                    &payload.snapshot,
                    &asset.node_id,
                    per_node,
                ),
            }
        })
        .map(|path| host_safe_asset_path(&path))
        .collect::<Vec<_>>();
    let mut seen = BTreeMap::new();
    for frame in frames {
        // Fidelity warnings may make a frame partial without changing the
        // generated text. Compare exactly the outputs that exist; the key and
        // comparedOutputs prevent missing outputs from implying equivalence.
        let generated = ["tsx", "componentTsx"]
            .into_iter()
            .filter_map(|field| frame[field].as_str().map(|code| (field, code.to_owned())))
            .collect::<Vec<_>>();
        if generated.is_empty() {
            continue;
        }
        let key = generated
            .iter()
            .map(|(field, code)| (*field, generated_structure(code, &paths)))
            .collect::<Vec<_>>();
        if let Some((node_id, original)) = seen.get(&key) {
            frame["structurallyIdenticalTo"] = json!(node_id);
            frame["differsOnly"] = if original == &generated {
                json!([])
            } else {
                json!(["assetReferences"])
            };
            frame["comparedOutputs"] = json!(
                generated
                    .iter()
                    .map(|(field, _)| *field)
                    .collect::<Vec<_>>()
            );
        } else {
            seen.insert(key, (frame["nodeId"].clone(), generated));
        }
    }
}

/// This describes collection, even when the caller did not request a manifest.
/// Missing evidence is never reported as proof that the design has no assets.
fn asset_summary(
    payload: &CollectedPayload,
    artifact: &ArtifactLookup,
    roots: &[String],
    manifest_requested: bool,
) -> Value {
    let mut snapshot = payload.snapshot.clone();
    snapshot.roots = roots.to_vec();
    let mut reachable = std::collections::BTreeSet::new();
    let mut pending = roots.to_vec();
    while let Some(id) = pending.pop() {
        if !reachable.insert(id.clone()) {
            continue;
        }
        if let Some(node) = snapshot.nodes.get(&id) {
            pending.extend(node.typed_view().child_ids().map(str::to_owned));
        }
    }
    snapshot.nodes.retain(|id, _| reachable.contains(id));
    let mut assets = devup_mcp_figma::discover_asset_manifest(&snapshot).assets;
    for captured in payload
        .assets
        .iter()
        .filter(|asset| reachable.contains(&asset.node_id))
    {
        if let Some(existing) = assets
            .iter_mut()
            .find(|asset| asset.asset_id == captured.asset_id)
        {
            *existing = captured.clone();
        } else {
            assets.push(captured.clone());
        }
    }
    let index_only = artifact.capabilities.kind == super::artifacts::ArtifactKind::SectionIndex;
    let incomplete = index_only
        || snapshot.audit().state != devup_mcp_figma::CompletenessState::Complete
        || snapshot
            .nodes
            .values()
            .any(|node| node.typed_view().bool("projectionTruncated") == Some(true));
    let collected = assets
        .iter()
        .filter(|asset| asset.status == AssetStatus::Exported)
        .count();
    let unavailable = assets.iter().filter(|asset| asset.status != AssetStatus::Exported).map(|asset| {
        let reason = match asset.error_code.as_deref() {
            Some("DEVUP_ASSET_NODE_HIDDEN") => "hidden-node",
            Some(_) => "export-failed",
            None if artifact.capabilities.asset_capture_count == 0 => "not-requested",
            None => "capture-not-in-artifact",
        };
        json!({"assetId":asset.asset_id,"nodeId":asset.node_id,"reason":reason,"errorCode":asset.error_code})
    }).collect::<Vec<_>>();
    let status = if assets.is_empty() {
        if incomplete { "unknown" } else { "none" }
    } else if collected == 0 {
        "not-collected"
    } else if collected < assets.len() || incomplete {
        "partial"
    } else {
        "collected"
    };
    json!({"status":status,"scopeRootIds":roots,
        "discovery":if incomplete { "incomplete" } else { "complete" },
        "discoveryReason":if index_only { Some("index-only") } else if incomplete { Some("snapshot-incomplete") } else { None },
        "discoveredCount":assets.len(),"collectedCount":collected,
        "manifestIncluded":manifest_requested,"unavailable":unavailable})
}

fn projection_key(outputs: &[ProjectedOutput]) -> String {
    let mut hasher = Sha256::new();
    for output in outputs {
        hasher.update(output.name.as_bytes());
        hasher.update([0]);
        hasher.update(output.mime_type.as_bytes());
        hasher.update([u8::from(output.is_binary)]);
        hasher.update((output.bytes.len() as u64).to_le_bytes());
        hasher.update(&output.bytes);
    }
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

pub(super) fn artifact_metadata(artifact: &ArtifactLookup) -> Value {
    let origin_collection = &artifact.payload.stats;
    json!({
        "artifactId": artifact.artifact_id,
        "contentHash": artifact.content_hash,
        "cacheHit": artifact.cache_hit,
        "reuseKind": artifact.reuse_kind,
        "ageSeconds": artifact.age_seconds,
        "remainingTtlSeconds": artifact.remaining_ttl_seconds,
        "avoidedFigmaToolCalls": if artifact.cache_hit { origin_collection.figma_tool_calls } else { 0 },
        "originCollection": origin_collection,
        "capabilities": artifact.capabilities,
        "sizeBytes": artifact.size_bytes,
        "acquiredAt": format_epoch_rfc3339(artifact.created_at_epoch_seconds),
        "expiresAt": format_epoch_rfc3339(artifact.expires_at_epoch_seconds)
    })
}

/// Adds `completenessReport` only when it has something to say.
///
/// `quality.acquisition` already grades the capture, and on a clean one the
/// report underneath it is six empty arrays and a row of counters that agree
/// with the grade. Measured on one export it was 360 of the 1,911 bytes every
/// response carried before this, none of it actionable. It is attached
/// whenever the capture is not clean, and whenever the caller asked for
/// diagnostics and therefore wants the detail regardless.
/// Adds `fidelity` whenever the report has something `quality` does not say.
///
/// Keyed on the report itself, not on `quality.projection`. That was the
/// first attempt and it hid a real defect: `projection_quality` is computed
/// from diagnostics alone, so a coverage shortfall that raises no diagnostic
/// leaves the grade reading `exact` and took the report away with it.
/// Measured on a real 50-node screen, `variables` was 19 of 20 - a `$primary`
/// binding frozen into an exported SVG - while `quality.projection` said
/// `exact`, so the response went from showing the shortfall to hiding it.
///
/// `strict_compatible` is the same predicate `strict: true` refuses on, which
/// keeps the two from disagreeing about whether anything was lost. On the ten
/// real captured screens it is true - nothing sent - for every screen with no
/// shortfall and no approximation, which is what this is for.
fn attach_fidelity(
    response: &mut Value,
    report: &devup_mcp_devup_ui::provenance::FidelityReport,
    include_diagnostics: bool,
) {
    if include_diagnostics || !report.strict_compatible() {
        response["fidelity"] = json!(report);
    }
}

fn attach_completeness_report(
    response: &mut Value,
    quality: OutputQuality,
    report: &devup_mcp_figma::PayloadCompletenessReport,
    include_diagnostics: bool,
) {
    let clean = matches!(
        quality.acquisition,
        AcquisitionQuality::Complete | AcquisitionQuality::ExpectedProjection
    );
    if include_diagnostics || !clean {
        response["completenessReport"] = json!(report);
    }
}

fn unavailable_output(payload: &CollectedPayload, output: &str, reason: &str) -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupInvalidInput,
        format!("{output} cannot be produced. {reason}"),
        false,
        json!({"output":output,"nodeId":payload.target.node_id,"snapshotRootIds":payload.snapshot.roots,
            "collectedTargetIds":payload.snapshot.roots}),
    )
}

fn projection_failure(
    payload: &CollectedPayload,
    node_id: &str,
    output: &str,
    error: &DevupError,
) -> Value {
    let mut details = error.details.as_object().cloned().unwrap_or_default();
    details.insert("nodeId".into(), json!(node_id));
    details.insert("snapshotRootIds".into(), json!(payload.snapshot.roots));
    details.insert("collectedTargetIds".into(), json!(payload.snapshot.roots));
    json!({"nodeId":node_id,"output":output,"errorCode":error.code,"message":error.message,
        "retryable":error.retryable,"details":details})
}

fn generate_collected_component(
    payload: &CollectedPayload,
    node_id: &str,
    options: &CodegenOptions,
) -> Result<devup_mcp_devup_ui::codegen::CodegenOutput, DevupError> {
    if !options.inline_instances {
        let mut pending = vec![node_id];
        let mut visited = std::collections::BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let Some(node) = payload.snapshot.nodes.get(id) else {
                continue;
            };
            let view = node.typed_view();
            if view.bool("visible") == Some(false) {
                continue;
            }
            if node.node_type == "INSTANCE" {
                let component_id = view
                    .string("mainComponentId")
                    .or_else(|| view.string("componentId"))
                    .or_else(|| view.string("mainComponent"))
                    .or_else(|| {
                        view.value("mainComponent")
                            .and_then(|value| value.get("id"))
                            .and_then(Value::as_str)
                    });
                if let Some(component_id) = component_id
                    && !payload.snapshot.nodes.contains_key(component_id)
                {
                    return Err(DevupError::with_details(
                        ErrorCode::DevupFigmaNodeNotFound,
                        "componentTsx needs a component master outside this snapshot. Re-collect the component master, or request tsx to inline the instance.",
                        false,
                        json!({"componentId":component_id,"instanceNodeId":id}),
                    ));
                }
            }
            pending.extend(view.child_ids());
        }
    }
    generate_component(&payload.snapshot, node_id, options)
}

pub(super) async fn complete_operation(
    operation: PendingOperation,
    payload: &CollectedPayload,
    source_kind: &str,
    artifact: &ArtifactLookup,
    output_policy: &OutputPolicy,
    artifact_store: &ArtifactStore,
) -> Result<Value, DevupError> {
    let collection = if artifact.cache_hit {
        CollectionStats::default()
    } else {
        payload.stats.clone()
    };
    let completeness_report = payload.completeness_report();
    match operation {
        PendingOperation::Search {
            query,
            node_types,
            match_kind,
            limit,
        } => {
            let matches = search_snapshot(
                &payload.snapshot,
                &payload.target,
                &SearchOptions {
                    query: query.clone(),
                    node_types,
                    match_kind,
                    limit,
                },
            )?;
            let quality = OutputQuality {
                acquisition: acquisition_quality(&completeness_report, true),
                projection: projection_quality(false, &[]),
                theme: theme_quality(false, 0, 0),
                assets: assets_quality(false, &[], &[]),
            };
            let mut response = json!({
                "status": quality.status(),
                "quality": quality,
                "query": query,
                "count": matches.len(),
                "matches": matches,
                "completeness": payload.completeness,
                "collection": collection,
                "cache": artifact_metadata(artifact),
                "source": {
                    "kind": source_kind,
                    "fileKey": payload.target.file_key,
                    "version": payload.snapshot.version
                }
            });
            attach_completeness_report(&mut response, quality, &completeness_report, false);
            Ok(response)
        }
        PendingOperation::Explore { limit, target } => {
            let result = explore_snapshot(&payload.snapshot, &target, &ExploreOptions { limit })?;
            let count = result.candidates.len();
            let quality = OutputQuality {
                acquisition: acquisition_quality(&completeness_report, true),
                projection: projection_quality(false, &[]),
                theme: theme_quality(false, 0, 0),
                assets: assets_quality(false, &[], &[]),
            };
            let mut response = json!({
                "status": quality.status(),
                "quality": quality,
                "targetKind": result.target_kind,
                "anchor": result.anchor,
                "group": result.group,
                "count": count,
                "candidates": result.candidates,
                "truncated": result.truncated,
                "diagnostics": payload.snapshot.diagnostics,
                "completeness": payload.completeness,
                "collection": collection,
                "cache": artifact_metadata(artifact),
                "source": {
                    "kind": source_kind,
                    "fileKey": target.file_key,
                    "nodeId": target.node_id,
                    "version": payload.snapshot.version
                }
            });
            attach_completeness_report(&mut response, quality, &completeness_report, false);
            Ok(response)
        }
        PendingOperation::Export {
            outputs,
            component_name,
            include_diagnostics,
            root_layout,
            asset_names_per_node,
            scope,
            strict,
            output_paths,
            frame_ids,
            all_screens,
            asset_captures,
            asset_output_paths,
            delivery,
        } => {
            if outputs.iter().any(|output| output == "sourceMap")
                && !outputs
                    .iter()
                    .any(|output| matches!(output.as_str(), "tsx" | "componentTsx" | "devupJson"))
            {
                return Err(unavailable_output(
                    payload,
                    "sourceMap",
                    "A source map needs a generated output. Include tsx, componentTsx or devupJson in outputs.",
                ));
            }
            let mut result = Map::new();
            // Not restated by quality: this grades how far token resolution
            // reached - whether external library variables were covered - where
            // quality.theme only says whether what was resolved conflicts.
            result.insert("completeness".to_owned(), json!(payload.completeness));
            result.insert("collection".to_owned(), json!(collection));
            result.insert("cache".to_owned(), artifact_metadata(artifact));
            let mut failures = payload
                .failures
                .iter()
                .map(|failure| json!(failure))
                .collect::<Vec<_>>();
            let mut warnings = Vec::new();
            for asset in payload
                .assets
                .iter()
                .filter(|asset| asset.status == AssetStatus::Failed)
            {
                let failure = json!({"assetId":asset.asset_id,"nodeId":asset.node_id,
                    "errorCode":asset.error_code,"message":"Asset export failed. Re-collect this asset from its URL.",
                    "retryable":true});
                if asset.error_code.as_deref() == Some("DEVUP_ASSET_NODE_HIDDEN") {
                    warnings.push(failure);
                } else {
                    failures.push(failure);
                }
            }
            result.insert("failures".to_owned(), json!(failures));
            if !warnings.is_empty() {
                result.insert("warnings".to_owned(), json!(warnings));
            }
            result.insert(
                "source".to_owned(),
                json!({
                    "kind": source_kind,
                    "fileKey": payload.target.file_key,
                    "nodeId": payload.target.node_id,
                    "version": payload.snapshot.version
                }),
            );
            let payload_section_index = section_index_from_payload(payload);
            let target_kind = if payload_section_index.is_some() {
                TargetKind::Section
            } else {
                classify_target(&payload.snapshot, &payload.target)
            };
            result.insert("targetKind".to_owned(), json!(target_kind));
            let manifest_requested = outputs.iter().any(|output| output == "assetManifest");
            result.insert(
                "assetSummary".to_owned(),
                asset_summary(
                    payload,
                    artifact,
                    &payload.snapshot.roots,
                    manifest_requested,
                ),
            );

            if !frame_ids.is_empty() && all_screens {
                return Err(DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "frameIds and allScreens cannot be used together.",
                    false,
                ));
            }
            if target_kind != TargetKind::Section && (!frame_ids.is_empty() || all_screens) {
                return Err(DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "frameIds and allScreens can only be used on a Section artifact.",
                    false,
                ));
            }

            let mut candidates_truncated = false;
            let section_candidates = if target_kind == TargetKind::Section
                && outputs.iter().any(|output| {
                    matches!(output.as_str(), "tsx" | "componentTsx" | "responsiveTsx")
                }) {
                Some(if let Some(index) = &payload_section_index {
                    candidates_truncated = index.truncated;
                    index
                        .candidates
                        .iter()
                        .map(|candidate| {
                            let mut explored = section_candidate_as_explore(candidate);
                            explored.node.kind =
                                devup_mcp_figma::classify_explore_node(&explored.node);
                            explored.selection_reasons = if explored.node.is_screen_candidate() {
                                vec!["screen-like".into(), "inside-section".into()]
                            } else {
                                vec!["explicit-selection-only".into(), "inside-section".into()]
                            };
                            explored
                        })
                        .collect()
                } else {
                    let explored = explore_snapshot(
                        &payload.snapshot,
                        &payload.target,
                        &ExploreOptions { limit: 100 },
                    )?;
                    candidates_truncated = explored.candidates_truncated;
                    explored.candidates
                })
            } else {
                None
            };
            if let Some(candidates) = &section_candidates
                && frame_ids.is_empty()
                && !all_screens
            {
                // Both candidate producers report actual list truncation.
                // Reaching the limit alone does not mean candidates were omitted.
                let truncated = candidates_truncated;
                let (screens, explicit_candidates): (Vec<_>, Vec<_>) = candidates
                    .iter()
                    .partition(|candidate| candidate.node.is_screen_candidate());
                let quality = OutputQuality {
                    acquisition: acquisition_quality(&completeness_report, false),
                    projection: projection_quality(false, &[]),
                    theme: theme_quality(false, 0, 0),
                    assets: assets_quality(false, &[], &[]),
                };
                result.insert("status".to_owned(), json!("selection_required"));
                if let Some(summary) = result.get_mut("assetSummary") {
                    summary["manifestIncluded"] = json!(false);
                }
                result.insert("quality".to_owned(), json!(quality));
                result.insert(
                    "selection".to_owned(),
                    json!({
                        "kind": "screen-frame",
                        "status": if truncated { "partial" } else { "complete" },
                        "count": screens.len(),
                        "candidates": screens,
                        "explicitCandidates": explicit_candidates,
                        "truncated": truncated
                    }),
                );
                result.insert(
                    "nextAction".to_owned(),
                    json!({
                        "why": "This is a Section candidate list. Screen artifacts have not been exported yet.",
                        "how": "Review selection.candidates using name, nodeType and textPreview. Use allScreens:true for these automatic screen candidates in a complete list. Nonstandard cases and notes are in selection.explicitCandidates; select them with frameIds or their canonicalUrl.",
                        "doNot": "Do not try to collect the whole Section at once."
                    }),
                );
                // An example built from this call's own artifact and a real
                // candidate, so the next step is a call to run rather than a
                // shape to assemble.
                if let Some(candidate) = screens.first().or_else(|| explicit_candidates.first()) {
                    result
                        .get_mut("nextAction")
                        .expect("nextAction was inserted")["example"] = json!({
                        "tool": "devup_figma_export",
                        "arguments": {
                            "artifactId": artifact.artifact_id,
                            "frameIds": [candidate.node.node_id],
                            "outputs": outputs,
                            "delivery": "resource"
                        }
                    });
                }
                return Ok(Value::Object(result));
            }

            let mut written_paths = Map::new();
            let mut section_tsx_projected = false;
            let mut selected_projection_roots = None;
            let mut tsx_source_map = None;
            let mut devup_json_source_map = None;
            let mut projection_diagnostics = Vec::new();
            let mut fidelity_reports = Vec::new();
            let mut theme_conflict_count = 0;
            let mut theme_unresolved_count = 0;
            let mut pending_text_outputs = std::collections::BTreeMap::new();
            let mut pending_binary_outputs = std::collections::BTreeMap::new();
            let mut pending_asset_manifest = None;
            let mut asset_resource_outputs = Vec::new();
            if let Some(candidates) = section_candidates {
                let by_id = candidates
                    .iter()
                    .map(|candidate| (candidate.node.node_id.as_str(), candidate))
                    .collect::<std::collections::BTreeMap<_, _>>();
                let failed_ids = payload
                    .failures
                    .iter()
                    .map(|failure| failure.node_id.as_str())
                    .collect::<std::collections::BTreeSet<_>>();
                let selected = if all_screens {
                    candidates
                        .iter()
                        .filter(|candidate| candidate.node.is_screen_candidate())
                        .filter(|candidate| !failed_ids.contains(candidate.node.node_id.as_str()))
                        .collect::<Vec<_>>()
                } else {
                    let requested = frame_ids
                        .iter()
                        .map(String::as_str)
                        .collect::<std::collections::BTreeSet<_>>();
                    if requested.len() != frame_ids.len() {
                        return Err(DevupError::new(
                            ErrorCode::DevupSnapshotUnsupported,
                            "frameIds contains a duplicate node.",
                            false,
                        ));
                    }
                    if let Some(node_id) = requested
                        .iter()
                        .find(|node_id| !by_id.contains_key(**node_id))
                    {
                        return Err(DevupError::new(
                            ErrorCode::DevupFigmaNodeNotFound,
                            format!(
                                "Not a screen frame inside the Section, or it does not exist: {node_id}"
                            ),
                            false,
                        ));
                    }
                    candidates
                        .iter()
                        .filter(|candidate| {
                            requested.contains(candidate.node.node_id.as_str())
                                && !failed_ids.contains(candidate.node.node_id.as_str())
                        })
                        .collect::<Vec<_>>()
                };
                if selected.is_empty() && failed_ids.is_empty() {
                    return Err(unavailable_output(
                        payload,
                        "Section outputs",
                        "No screen frames match this selection. Use explicit frameIds for a nonstandard frame, or explore another Section.",
                    ));
                }
                selected_projection_roots = Some(
                    selected
                        .iter()
                        .map(|candidate| candidate.node.node_id.clone())
                        .collect::<Vec<_>>(),
                );
                const MAX_SECTION_FRAMES: usize = 6;
                if selected.len() > MAX_SECTION_FRAMES {
                    return Err(DevupError::with_details(
                        ErrorCode::DevupInvalidInput,
                        "This Section projection exceeds the batch budget. Request at most 6 frameIds per call; reuse this artifactId for subsequent batches.",
                        false,
                        json!({"artifactId":artifact.artifact_id,"requestedFrameCount":selected.len(),
                            "recommendedBatchSize":MAX_SECTION_FRAMES,
                            "recommendedFrameIds":selected.iter().take(MAX_SECTION_FRAMES).map(|c| &c.node.node_id).collect::<Vec<_>>(),
                            "remainingFrameIds":selected.iter().skip(MAX_SECTION_FRAMES).map(|c| &c.node.node_id).collect::<Vec<_>>() }),
                    ));
                }
                let mut frames = Vec::with_capacity(selected.len());
                for (index, candidate) in selected.iter().enumerate() {
                    let frame_component_name = component_name.as_ref().map(|name| {
                        if selected.len() == 1 {
                            name.clone()
                        } else {
                            format!("{name}{}", index + 1)
                        }
                    });
                    let mut frame = json!({"nodeId":candidate.node.node_id,"name":candidate.node.name,
                        "canonicalUrl":candidate.canonical_url});
                    let before_failures = failures.len();
                    let mut frame_diagnostics = Vec::new();
                    for field in ["tsx", "componentTsx"] {
                        if !outputs.iter().any(|output| output == field) {
                            continue;
                        }
                        let output = generate_collected_component(
                            payload,
                            &candidate.node.node_id,
                            &CodegenOptions {
                                component_name: frame_component_name.clone(),
                                include_diagnostics,
                                inline_instances: field == "tsx",
                                root_layout,
                                asset_names_per_node,
                                ..CodegenOptions::default()
                            }
                            .with_payload_tokens(payload),
                        );
                        let output = match output {
                            Ok(output) => output,
                            Err(error) => {
                                failures.push(projection_failure(
                                    payload,
                                    &candidate.node.node_id,
                                    field,
                                    &error,
                                ));
                                continue;
                            }
                        };
                        frame_diagnostics.extend(output.diagnostics.iter().cloned());
                        fidelity_reports.push(output.fidelity_report.clone());
                        frame[field] = json!(with_host_safe_asset_paths(&output.tsx));
                        attach_fidelity(&mut frame, &output.fidelity_report, include_diagnostics);
                        if outputs.iter().any(|output| output == "sourceMap") {
                            frame["sourceMap"] = json!({"version":output.source_map.version,
                                "entries":output.source_map.entries,"source":{"fileKey":payload.target.file_key,
                                "rootNodeId":candidate.node.node_id,"sourceVersion":payload.source_version}});
                        }
                    }
                    projection_diagnostics.extend(frame_diagnostics.iter().cloned());
                    let mut frame_quality = OutputQuality {
                        acquisition: acquisition_quality(&completeness_report, false),
                        projection: projection_quality(true, &frame_diagnostics),
                        theme: theme_quality(false, 0, 0),
                        assets: assets_quality(false, &[], &[]),
                    };
                    if failures.len() > before_failures {
                        frame_quality.projection =
                            if frame.get("tsx").is_some() || frame.get("componentTsx").is_some() {
                                ProjectionQuality::Lossy
                            } else {
                                ProjectionQuality::Failed
                            };
                    }
                    frame["quality"] = json!(frame_quality);
                    frame["status"] = json!(if failures.len() > before_failures {
                        "partial"
                    } else {
                        frame_quality.status()
                    });
                    frame["assetSummary"] = asset_summary(
                        payload,
                        artifact,
                        std::slice::from_ref(&candidate.node.node_id),
                        manifest_requested,
                    );
                    attach_completeness_report(
                        &mut frame,
                        frame_quality,
                        &completeness_report,
                        include_diagnostics,
                    );
                    if include_diagnostics {
                        frame["diagnostics"] = json!(frame_diagnostics);
                    }
                    frames.push(frame);
                }
                annotate_identical_frames(&mut frames, payload, asset_names_per_node);
                if let Some(roots) = &selected_projection_roots {
                    result.insert(
                        "assetSummary".to_owned(),
                        asset_summary(payload, artifact, roots, manifest_requested),
                    );
                }
                if outputs
                    .iter()
                    .any(|output| matches!(output.as_str(), "tsx" | "componentTsx"))
                {
                    result.insert("frames".to_owned(), Value::Array(frames));
                }
                section_tsx_projected = true;
            }

            let mut responsive_snapshot = std::borrow::Cow::Borrowed(&payload.snapshot);
            if let Some(roots) = selected_projection_roots {
                responsive_snapshot.to_mut().roots = roots;
            }
            let component_name_for_components = component_name.clone();
            // A screen captured with its other widths is convertible as one
            // tree. It is offered whenever those widths are present rather than
            // only on request, because a caller asking for a screen that has
            // them almost always wants the responsive form and cannot know from
            // the node id alone whether it exists.
            if outputs
                .iter()
                .any(|output| output == "tsx" || output == "responsiveTsx")
                && let Some(merged) = merge_breakpoints(
                    &responsive_snapshot,
                    &CodegenOptions {
                        component_name: component_name_for_components.clone(),
                        include_diagnostics,
                        inline_instances: false,
                        root_layout,
                        asset_names_per_node,
                        ..CodegenOptions::default()
                    }
                    .with_payload_tokens(payload),
                )?
                && (outputs.iter().any(|output| output == "responsiveTsx")
                    || !merged.slots.windows(2).any(|pair| pair[0] == pair[1]))
            {
                // Named as the plugin names it: after the Section the widths
                // sit in, with `Page` on the end — `AboutPage` for a Section
                // called `about`. The Section is outside the collected
                // subtree, so each width carries its name as `parentName`.
                // A caller's own name wins; without one and without a
                // Section name the page is `ResponsivePage`.
                let section_name = payload
                    .snapshot
                    .roots
                    .iter()
                    .filter_map(|root| payload.snapshot.nodes.get(root))
                    .find_map(|root| root.typed_view().string("parentName").map(str::to_owned));
                let name = component_name_for_components.clone().map_or_else(
                    || {
                        section_name.map_or_else(
                            || "ResponsivePage".to_owned(),
                            |section| format!("{}Page", normalize_component_name(&section)),
                        )
                    },
                    |name| normalize_component_name(&name),
                );
                if merged
                    .slots
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    != merged.slots.len()
                {
                    return Err(unavailable_output(
                        payload,
                        "responsiveTsx",
                        "Two frames occupy the same breakpoint slot. Request tsx separately, or select one frame per breakpoint slot.",
                    ));
                }
                let module = merged.module(&name);
                if output_paths.contains_key("responsiveTsx") {
                    pending_text_outputs.insert("responsiveTsx".to_owned(), module.clone());
                }
                result.insert("responsiveTsx".to_owned(), json!(module));
                result.insert("responsiveSlots".to_owned(), json!(merged.slots));
                if !merged.unrepresented.is_empty() {
                    projection_diagnostics.push(Diagnostic {
                        code:"DEVUP_CODEGEN_RESPONSIVE_LOSS".into(),
                        message:"Some breakpoint content could not be represented. See responsiveUnrepresented.".into(),
                        fidelity_impact:Some(devup_mcp_figma::FidelityImpact::Lossy),
                        severity:Some(DiagnosticSeverity::Warning),
                        ..Diagnostic::default()
                    });
                    result.insert(
                        "responsiveUnrepresented".to_owned(),
                        json!(
                            merged
                                .unrepresented
                                .iter()
                                .map(|note| json!({
                                    "nodeId": note.node_id,
                                    "detail": note.detail
                                }))
                                .collect::<Vec<_>>()
                        ),
                    );
                }
            }

            if outputs.iter().any(|output| output == "responsiveTsx")
                && !result.contains_key("responsiveTsx")
            {
                return Err(unavailable_output(
                    payload,
                    "responsiveTsx",
                    "The snapshot has no mergeable breakpoint frames. Request tsx for individual screens, or re-collect a screen with its breakpoint siblings.",
                ));
            }

            if outputs.iter().any(|output| output == "tsx") && !section_tsx_projected {
                let node_id = payload.target.node_id.as_deref().ok_or_else(|| {
                    DevupError::new(
                        ErrorCode::DevupFigmaNodeNotFound,
                        "A TSX export payload requires a node ID.",
                        false,
                    )
                })?;
                let output = generate_component(
                    &payload.snapshot,
                    node_id,
                    &CodegenOptions {
                        component_name,
                        include_diagnostics,
                        inline_instances: true,
                        root_layout,
                        asset_names_per_node,
                        ..CodegenOptions::default()
                    }
                    .with_payload_tokens(payload),
                )?;
                projection_diagnostics.extend(output.diagnostics.iter().cloned());
                fidelity_reports.push(output.fidelity_report.clone());
                tsx_source_map = Some(output.source_map.clone());
                if output_paths.contains_key("tsx") {
                    pending_text_outputs.insert("tsx".to_owned(), output.tsx.clone());
                }
                result.insert("tsx".to_owned(), json!(output.tsx));
                if include_diagnostics {
                    result.insert("diagnostics".to_owned(), json!(&output.diagnostics));
                }
            }

            if outputs.iter().any(|output| output == "componentTsx") && !section_tsx_projected {
                let node_id = payload.target.node_id.as_deref().ok_or_else(|| {
                    DevupError::new(
                        ErrorCode::DevupFigmaNodeNotFound,
                        "A component TSX export payload requires a node ID.",
                        false,
                    )
                })?;
                let output = generate_collected_component(
                    payload,
                    node_id,
                    &CodegenOptions {
                        component_name: component_name_for_components.clone(),
                        include_diagnostics: false,
                        inline_instances: false,
                        root_layout,
                        asset_names_per_node,
                        ..CodegenOptions::default()
                    }
                    .with_payload_tokens(payload),
                );
                match output {
                    Ok(output) => {
                        projection_diagnostics.extend(output.diagnostics.iter().cloned());
                        fidelity_reports.push(output.fidelity_report.clone());
                        if tsx_source_map.is_none() {
                            tsx_source_map = Some(output.source_map.clone());
                        }
                        if output_paths.contains_key("componentTsx") {
                            pending_text_outputs
                                .insert("componentTsx".to_owned(), output.tsx.clone());
                        }
                        result.insert("componentTsx".to_owned(), json!(output.tsx));
                    }
                    Err(error) => {
                        failures.push(projection_failure(payload, node_id, "componentTsx", &error))
                    }
                }
            }

            if outputs.iter().any(|output| output == "devupJson") {
                let variables = payload.variables.as_ref().ok_or_else(|| unavailable_output(payload, "devupJson",
                    "No variable/style resources were collected. Re-collect from the URL with devupJson in outputs and the desired scope."))?;
                let variables = variable_snapshot_from_result(variables)?;
                let output = generate_devup_json(&variables, parse_scope(&scope)?)?;
                theme_conflict_count = output.conflicts.len();
                theme_unresolved_count = output.unresolved_variables.len();
                devup_json_source_map = Some(output.source_map.clone());
                if output_paths.contains_key("devupJson") {
                    pending_text_outputs.insert("devupJson".to_owned(), output.json.clone());
                }
                result.insert("devupJson".to_owned(), json!(output.json));
                result.insert("themeCounts".to_owned(), json!(output.counts));
                result.insert("themeCompleteness".to_owned(), json!(output.completeness));
                result.insert("conflicts".to_owned(), json!(output.conflicts));
                result.insert(
                    "unresolvedVariables".to_owned(),
                    json!(output.unresolved_variables),
                );
                if include_diagnostics && !result.contains_key("diagnostics") {
                    result.insert("diagnostics".to_owned(), json!(output.diagnostics));
                }
            }

            if outputs.iter().any(|output| output == "rawSnapshot") {
                let raw = serde_json::to_value(&payload.snapshot).map_err(|error| {
                    DevupError::new(
                        ErrorCode::DevupSnapshotUnsupported,
                        format!("Cannot serialize the raw snapshot: {error}"),
                        false,
                    )
                })?;
                if output_paths.contains_key("rawSnapshot") {
                    pending_text_outputs.insert(
                        "rawSnapshot".to_owned(),
                        serde_json::to_string_pretty(&raw).unwrap_or_default(),
                    );
                }
                result.insert("rawSnapshot".to_owned(), raw);
            }

            // The whole collection, not only its node tree. A snapshot kept on
            // its own can be converted, but not the way the server converts
            // it: the token names come from the variables and styles collected
            // beside it, and without them a `$gray200` fill comes out as the
            // tail of its variable ID and a `typography="h4"` as five font
            // props. Captures kept as fixtures need the resources too, so
            // this writes them. The reference PNG is left out — it is large,
            // binary, and has its own output.
            if outputs.iter().any(|output| output == "rawPayload") {
                let mut without_png = payload.clone();
                without_png.reference_png = None;
                let raw = serde_json::to_value(&without_png).map_err(|error| {
                    DevupError::new(
                        ErrorCode::DevupSnapshotUnsupported,
                        format!("Cannot serialize the raw payload: {error}"),
                        false,
                    )
                })?;
                if output_paths.contains_key("rawPayload") {
                    pending_text_outputs.insert(
                        "rawPayload".to_owned(),
                        serde_json::to_string_pretty(&raw).unwrap_or_default(),
                    );
                }
                result.insert("rawPayload".to_owned(), raw);
            }

            if outputs.iter().any(|output| output == "sourceMap") && !section_tsx_projected {
                let source_map = json!({
                    "version": 1,
                    "tsx": tsx_source_map.map(|source_map| source_map.entries).unwrap_or_default(),
                    "devupJson": devup_json_source_map
                        .map(|source_map| source_map.entries)
                        .unwrap_or_default(),
                    "source": {
                        "fileKey": payload.target.file_key,
                        "rootNodeId": payload.target.node_id,
                        "sourceVersion": payload.source_version
                    }
                });
                if output_paths.contains_key("sourceMap") {
                    pending_text_outputs.insert(
                        "sourceMap".to_owned(),
                        serde_json::to_string_pretty(&source_map).unwrap_or_default(),
                    );
                }
                result.insert("sourceMap".to_owned(), source_map);
            }

            if outputs.iter().any(|output| output == "referencePng") {
                let reference = payload.reference_png.as_ref().ok_or_else(|| unavailable_output(payload, "referencePng",
                    "No reference PNG was collected. Re-collect a single screen URL with referencePng in outputs."))?;
                let bytes = STANDARD
                    .decode(reference.data_base64.as_bytes())
                    .map_err(|_| {
                        DevupError::new(
                            ErrorCode::DevupSnapshotUnsupported,
                            "The artifact reference PNG base64 is invalid.",
                            false,
                        )
                    })?;
                if bytes.len() != reference.byte_length
                    || Sha256::digest(&bytes)
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                        != reference.sha256
                {
                    return Err(DevupError::new(
                        ErrorCode::DevupSnapshotUnsupported,
                        "The artifact reference PNG length or hash does not match.",
                        false,
                    ));
                }
                if output_paths.contains_key("referencePng") {
                    pending_binary_outputs.insert("referencePng".to_owned(), bytes);
                }
                result.insert("referencePng".to_owned(), json!(reference));
            }

            if outputs.iter().any(|output| output == "assetManifest") {
                let mut manifest = devup_mcp_figma::discover_asset_manifest(&payload.snapshot);
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
                manifest.assets.retain(|asset| {
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
                    if asset.path.is_none() {
                        // An image fill is named from the node it is painted
                        // on and which fill it is, which holds for a layout
                        // box carrying a photograph as much as for a node
                        // drawn entirely from a file. Anything else is named
                        // after the node itself.
                        asset.path = match asset
                            .field
                            .strip_prefix("fills/")
                            .and_then(|index| index.parse::<usize>().ok())
                        {
                            Some(fill_index) => devup_mcp_devup_ui::codegen::image_fill_path(
                                &payload.snapshot,
                                &asset.node_id,
                                fill_index,
                                asset_names_per_node,
                            ),
                            None => devup_mcp_devup_ui::codegen::asset_path(
                                &payload.snapshot,
                                &asset.node_id,
                                asset_names_per_node,
                            ),
                        };
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
                for capture in &asset_captures {
                    if !payload.assets.iter().any(|asset| {
                        asset.asset_id == capture.asset_id
                            && asset.format == Some(capture.format)
                            && asset.scale == Some(capture.scale)
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
            let mut quality = OutputQuality {
                acquisition: acquisition_quality(&completeness_report, false),
                projection: projection_quality(
                    outputs.iter().any(|output| {
                        matches!(output.as_str(), "tsx" | "componentTsx" | "responsiveTsx")
                    }),
                    &projection_diagnostics,
                ),
                theme: theme_quality(
                    outputs.iter().any(|output| output == "devupJson"),
                    theme_conflict_count,
                    theme_unresolved_count,
                ),
                assets: assets_quality(
                    outputs.iter().any(|output| output == "assetManifest"),
                    &asset_captures
                        .iter()
                        .map(|capture| capture.asset_id.clone())
                        .collect::<Vec<_>>(),
                    &payload.assets,
                ),
            };
            if !warnings.is_empty()
                && !failures
                    .iter()
                    .any(|failure| failure.get("assetId").is_some())
            {
                let visible_requests = asset_captures
                    .iter()
                    .filter(|capture| {
                        !payload.assets.iter().any(|asset| {
                            asset.asset_id == capture.asset_id
                                && asset.error_code.as_deref() == Some("DEVUP_ASSET_NODE_HIDDEN")
                        })
                    })
                    .map(|capture| capture.asset_id.clone())
                    .collect::<Vec<_>>();
                quality.assets = assets_quality(
                    outputs.iter().any(|output| output == "assetManifest"),
                    &visible_requests,
                    &payload.assets,
                );
            }
            if failures
                .iter()
                .any(|failure| failure.get("assetId").is_some())
            {
                quality.assets = super::quality::AssetsQuality::Partial;
            }
            if failures
                .iter()
                .any(|failure| failure.get("output").is_some())
            {
                quality.projection = super::quality::ProjectionQuality::Lossy;
            }
            if !warnings.is_empty() {
                result.insert("warnings".to_owned(), json!(warnings));
            }
            if target_kind == TargetKind::Section && !failures.is_empty() {
                let failed_frames = failures
                    .iter()
                    .filter(|failure| failure.get("assetId").is_none())
                    .filter_map(|failure| failure["nodeId"].as_str())
                    .collect::<std::collections::BTreeSet<_>>();
                if !failed_frames.is_empty() {
                    let mut arguments = json!({"url":format!("https://www.figma.com/design/{}/?node-id={}",
                        payload.target.file_key, payload.target.node_id.as_deref().unwrap_or_default().replace(':', "-")),
                        "frameIds":failed_frames,"outputs":outputs,"scope":scope,"delivery":delivery,
                        "rootLayout":match root_layout { devup_mcp_devup_ui::codegen::RootLayout::Standalone => "standalone", devup_mcp_devup_ui::codegen::RootLayout::Embedded => "embedded" },
                        "assetNamesPerNode":asset_names_per_node,"strict":strict,"includeDiagnostics":include_diagnostics});
                    if let Some(branch) = &payload.target.branch_key {
                        arguments["url"] = json!(format!(
                            "https://www.figma.com/branch/{}/{}/?node-id={}",
                            payload.target.file_key,
                            branch,
                            payload
                                .target
                                .node_id
                                .as_deref()
                                .unwrap_or_default()
                                .replace(':', "-")
                        ));
                    }
                    if let Some(name) = &component_name_for_components {
                        arguments["componentName"] = json!(name);
                    }
                    result.insert("resume".into(), json!({"tool":"devup_figma_export", "arguments":arguments,
                        "note":"Retry only failed frames. Missing collected nodes require a fresh URL acquisition; component master failures may require collecting the master or requesting tsx."}));
                }
            }
            result.insert("failures".to_owned(), json!(failures));
            let fidelity_violation = fidelity_reports
                .iter()
                .any(|report| !report.strict_compatible());
            if strict && (quality.strict_violation() || fidelity_violation || !failures.is_empty())
            {
                return Err(DevupError::with_details(
                    ErrorCode::DevupSnapshotUnsupported,
                    format!(
                        "strict export only allows exact/complete output: status={}, quality={}",
                        quality.status(),
                        serde_json::to_string(&quality).unwrap_or_default()
                    ),
                    false,
                    json!({
                        "quality": quality,
                        "fidelity": fidelity_reports,
                        "failures": failures,
                        "completenessReport": completeness_report
                    }),
                ));
            }
            let final_status = if failures.is_empty() {
                quality.status()
            } else {
                "partial"
            };
            result.insert("status".to_owned(), json!(final_status));
            result.insert("quality".to_owned(), json!(quality));
            // An unambiguous "this is the answer, implement from it" marker.
            // It was removed once on the reasoning that the `needs_figma`
            // handoff it guarded against is gone, so it only restated
            // `status`. A consumer reported relying on it, which settles it:
            // `status: "complete"` says the run went well, and this says
            // which value is the deliverable. 109 bytes for that is cheap.
            //
            // Checked before `apply_delivery` may move `tsx` into
            // `resources`, so it reflects whether a devup-ui TSX was
            // produced rather than how it was routed for delivery.
            let tsx_produced = ["tsx", "componentTsx", "responsiveTsx"]
                .iter()
                .any(|field| result.get(*field).is_some_and(Value::is_string))
                || result
                    .get("frames")
                    .and_then(Value::as_array)
                    .is_some_and(|frames| {
                        frames.iter().any(|frame| {
                            frame.get("tsx").is_some_and(Value::is_string)
                                || frame.get("componentTsx").is_some_and(Value::is_string)
                        })
                    });
            if final_status == "complete" && tsx_produced {
                result.insert(
                    "deliverable".to_owned(),
                    json!({
                        "kind": "devup-ui-tsx",
                        "isFinal": true,
                        "note": "This tsx is the final deliverable. Implement from this value."
                    }),
                );
            }
            // Only the whole-node tsx is missing its fidelity report at this
            // point; each Section frame already carries its own.
            if !section_tsx_projected && let Some(report) = fidelity_reports.first() {
                let mut carrier = Value::Object(Map::new());
                attach_fidelity(&mut carrier, report, include_diagnostics);
                if let Some(fidelity) = carrier.get("fidelity") {
                    result.insert("fidelity".to_owned(), fidelity.clone());
                }
            }
            let mut carrier = Value::Object(Map::new());
            attach_completeness_report(
                &mut carrier,
                quality,
                &completeness_report,
                include_diagnostics,
            );
            if let Some(report) = carrier.get("completenessReport") {
                result.insert("completenessReport".to_owned(), report.clone());
            }
            // The code and the bytes have to name an asset alike. Renaming
            // here, once both are assembled, keeps the two in step while the
            // code generator itself stays byte-for-byte the plugin's.
            for output in ["tsx", "responsiveTsx", "componentTsx"] {
                if let Some(Value::String(code)) = result.get_mut(output) {
                    let safe = with_host_safe_asset_paths(code.as_str());
                    if safe != *code {
                        *code = safe;
                    }
                }
                if let Some(code) = pending_text_outputs.get_mut(output) {
                    let safe = with_host_safe_asset_paths(code.as_str());
                    if safe != *code {
                        *code = safe;
                    }
                }
            }
            let mut planned_outputs = Vec::new();
            for (output, contents) in pending_text_outputs {
                if let Some(path) = output_paths.get(&output) {
                    planned_outputs.push((
                        output,
                        output_policy.resolve(path)?,
                        contents.into_bytes(),
                    ));
                }
            }
            for (output, bytes) in pending_binary_outputs {
                if let Some(path) = output_paths.get(&output) {
                    planned_outputs.push((output, output_policy.resolve(path)?, bytes));
                }
            }
            if let Some(manifest) = pending_asset_manifest.as_ref() {
                for asset in &manifest.assets {
                    if asset.status != AssetStatus::Exported {
                        continue;
                    }
                    let Some(path) = asset_output_paths.get(&asset.asset_id) else {
                        continue;
                    };
                    let data = asset.data_base64.as_deref().ok_or_else(|| {
                        DevupError::new(
                            ErrorCode::DevupSnapshotUnsupported,
                            "The exported asset binary is not in the artifact.",
                            false,
                        )
                    })?;
                    let bytes = STANDARD.decode(data.as_bytes()).map_err(|_| {
                        DevupError::new(
                            ErrorCode::DevupSnapshotUnsupported,
                            "The exported asset binary base64 is invalid.",
                            false,
                        )
                    })?;
                    planned_outputs.push((
                        format!("asset:{}", asset.asset_id),
                        output_policy.resolve(path)?,
                        bytes,
                    ));
                }
            }
            let mut transaction = OutputTransaction::new();
            // Two nodes can be one picture - a logo drawn at three sizes
            // shares a file, as the plugin has it - so several assets resolve
            // to one path. That is one write, not a collision. Only differing
            // bytes under one name are a mistake, and that is worth refusing.
            // Two different drawings under one layer name land here too: the
            // plugin names an asset after its layer, and a snapshot cannot
            // tell two drawings of one name apart - only the exported bytes
            // can. The first is written and the rest are reported, so the
            // code still points at a file that exists and the caller learns
            // which names hide more than one picture.
            let mut staged_content: BTreeMap<String, String> = BTreeMap::new();
            let mut shared_names: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (name, target, bytes) in planned_outputs {
                let path = target.display_path().to_string_lossy().into_owned();
                written_paths.insert(name.clone(), json!(path.clone()));
                let fingerprint = sha256_hex(&bytes);
                match staged_content.get(&path) {
                    Some(staged) if *staged == fingerprint => continue,
                    Some(_) => {
                        let asset = name.strip_prefix("asset:").unwrap_or(&name).to_owned();
                        shared_names.entry(path).or_default().push(asset);
                        continue;
                    }
                    None => {
                        staged_content.insert(path, fingerprint);
                    }
                }
                transaction.stage(name, target, &bytes)?;
            }
            if let Some(mut manifest) = pending_asset_manifest {
                asset_resource_outputs = projected_asset_outputs(&manifest)?;
                for asset in &mut manifest.assets {
                    if asset.status != AssetStatus::Exported {
                        continue;
                    }
                    let output_name = format!("asset:{}", asset.asset_id);
                    let Some(path) = written_paths.get(&output_name).and_then(Value::as_str) else {
                        continue;
                    };
                    asset.output_path = Some(path.to_owned());
                    asset.data_base64 = None;
                }
                for (path, others) in &shared_names {
                    manifest.diagnostics.push(Diagnostic {
                        code: "DEVUP_ASSET_NAME_SHARED".to_owned(),
                        message: format!(
                            "{} further drawing(s) claim the file {path}; the first is written. \
                             An asset is named after its layer, as the plugin names it, so two \
                             drawings a designer named alike cannot both be written.",
                            others.len()
                        ),
                        severity: Some(DiagnosticSeverity::Warning),
                        resource_kind: Some("asset".to_owned()),
                        details: Some(json!({ "outputPath": path, "notWritten": others })),
                        ..Diagnostic::default()
                    });
                }
                let mut manifest = json!(manifest);
                if let Some(assets) = manifest["assets"].as_array_mut() {
                    for asset in assets {
                        let name = asset["nodeId"]
                            .as_str()
                            .and_then(|id| payload.snapshot.nodes.get(id))
                            .and_then(|node| node.typed_view().name());
                        if let Some(name) = name {
                            asset["layerName"] = json!(name);
                        }
                    }
                }
                result.insert("assetManifest".to_owned(), manifest);
            }
            result.insert("outputPaths".to_owned(), Value::Object(written_paths));
            let projected_outputs = projected_outputs_from_result(&result)?;
            let mut projected_outputs = projected_outputs;
            projected_outputs.extend(asset_resource_outputs);
            let mut result = Value::Object(result);
            let attachment = apply_delivery(
                &mut result,
                delivery,
                artifact_store,
                artifact,
                projected_outputs,
            )
            .await?;
            if let Err(error) = transaction.commit() {
                rollback_delivery(artifact_store, artifact, attachment).await;
                return Err(error);
            }
            commit_delivery(attachment);
            Ok(result)
        }
        PendingOperation::Collect | PendingOperation::Artifact { .. } => Err(DevupError::new(
            ErrorCode::DevupFigmaHandoffInvalid,
            "An internal collect operation cannot be completed from an MCP artifact.",
            false,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{host_safe_asset_path, with_host_safe_asset_paths};

    #[test]
    fn a_layer_name_no_file_system_takes_is_renamed_wherever_the_code_points_at_it() {
        let code = concat!(
            "<Box maskImage=\"url(/icons/ic:round-arrow-left.svg)\" />\n",
            "<Image src=\"/images/Frame 269.png\" />\n",
            "<Box maskImage=\"url('/icons/Frame 287.svg')\" />\n",
            "<Box maskImage=\"url(/icons/grommet-icons:language.svg)\" />\n",
        );
        assert_eq!(
            with_host_safe_asset_paths(code),
            concat!(
                "<Box maskImage=\"url(/icons/ic-round-arrow-left-b041d24e668d.svg)\" />\n",
                "<Image src=\"/images/Frame-269-3e2af8571e3f.png\" />\n",
                "<Box maskImage=\"url('/icons/Frame-287-cf31d759843d.svg')\" />\n",
                "<Box maskImage=\"url(/icons/grommet-icons-language-d201f62b16f5.svg)\" />\n",
            )
        );
    }

    #[test]
    fn the_written_path_is_renamed_the_same_way_the_code_is() {
        assert_eq!(
            host_safe_asset_path("/icons/grommet-icons:language.svg"),
            "/icons/grommet-icons-language-d201f62b16f5.svg"
        );
        assert_eq!(
            host_safe_asset_path("/images/Frame 269.png"),
            "/images/Frame-269-3e2af8571e3f.png"
        );
    }
}

#[cfg(test)]
mod w1_regressions {
    use super::super::artifacts::ArtifactRequestKey;
    use super::*;
    use devup_mcp_figma::{CollectionRequest, CollectionScope};

    fn payload() -> CollectedPayload {
        serde_json::from_value(json!({
            "target": {"fileKey":"W1Fixture", "nodeId":"1:1", "branchKey":null},
            "scope":"node", "metadata":{}, "variables":null, "styles":null,
            "completeness":"resolved-values-only", "sourceVersion":null,
            "snapshot": {"fileKey":"W1Fixture", "version":null, "roots":["1:1"],
                "nodes":{"1:1":{"id":"1:1","type":"FRAME","fields":{
                    "name":"Screen", "x":0,"y":0,"width":390,"height":800,
                    "visible":true,"childrenIds":[]}}},"diagnostics":[]}
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn p1_frames_report_generated_asset_only_equivalence() {
        let mut data = section(3);
        for i in 1..=3 {
            let id = format!("1:{i}");
            let node = data.snapshot.nodes.get_mut(&id).unwrap();
            node.fields.insert(
                "fills".into(),
                json!([{"type":"IMAGE", "imageHash":format!("image{i}"), "scaleMode":"FILL"}]),
            );
        }
        data.snapshot
            .nodes
            .get_mut("1:3")
            .unwrap()
            .fields
            .insert("childrenIds".into(), json!(["3:1"]));
        data.snapshot.nodes.insert(
            "3:1".into(),
            serde_json::from_value(json!({
                "id":"3:1", "type":"TEXT", "fields":{"name":"Copy", "parentId":"1:3",
                    "characters":"Different copy", "fontSize":16, "width":100, "height":24}
            }))
            .unwrap(),
        );
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["frames"][1]["structurallyIdenticalTo"], "1:1");
        assert_eq!(
            result["frames"][1]["differsOnly"],
            json!(["assetReferences"])
        );
        assert!(result["frames"][2].get("structurallyIdenticalTo").is_none());
    }

    #[tokio::test]
    async fn p1_asset_summary_distinguishes_none_uncollected_and_hidden() {
        let result = project(payload(), operation(&["tsx"])).await.unwrap();
        assert_eq!(result["assetSummary"]["status"], "none");
        let mut data = payload();
        data.snapshot.nodes.get_mut("1:1").unwrap().node_type = "VECTOR".into();
        let result = project(data.clone(), operation(&["assetManifest"]))
            .await
            .unwrap();
        assert_eq!(result["assetSummary"]["status"], "not-collected");
        assert_eq!(
            result["assetSummary"]["unavailable"][0]["reason"],
            "not-requested"
        );
        data.snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert("visible".into(), json!(false));
        let result = project(data, operation(&["assetManifest"])).await.unwrap();
        assert_eq!(result["assetSummary"]["status"], "not-collected");
        assert_eq!(
            result["assetSummary"]["unavailable"][0]["reason"],
            "hidden-node"
        );
    }

    #[tokio::test]
    async fn p1_incomplete_asset_discovery_never_claims_no_assets() {
        let mut data = payload();
        data.snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert("childrenIds".into(), json!(["missing"]));
        let result = project(data, operation(&["assetManifest"])).await.unwrap();
        assert_eq!(result["assetSummary"]["status"], "unknown");
        assert_eq!(result["assetSummary"]["discovery"], "incomplete");
    }

    #[tokio::test]
    async fn p1_selection_separates_explicit_cases_from_automatic_screens() {
        let mut data = section(2);
        data.metadata["sectionIndex"]["candidates"][1]["bounds"]["width"] = json!(150);
        data.metadata["sectionIndex"]["candidates"][1]["bounds"]["height"] = json!(150);
        let result = project(data.clone(), operation(&["tsx"])).await.unwrap();
        assert_eq!(result["selection"]["count"], 1);
        assert_eq!(
            result["selection"]["explicitCandidates"][0]["node"]["nodeId"],
            "1:2"
        );
        assert_eq!(
            result["selection"]["explicitCandidates"][0]["node"]["kind"],
            "annotation"
        );
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export { frame_ids, .. } = &mut op {
            *frame_ids = vec!["1:2".into()];
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["frames"][0]["nodeId"], "1:2");
    }

    #[test]
    fn p1_asset_comparison_preserves_literal_text_and_unknown_paths() {
        let paths = vec!["/icons/a.svg".into(), "/icons/b.svg".into()];
        assert_eq!(
            generated_structure("<Box maskImage=\"url('/icons/a.svg')\" />", &paths),
            generated_structure("<Box maskImage=\"url('/icons/b.svg')\" />", &paths)
        );
        assert_ne!(
            generated_structure("<Text>{\"url(/icons/a.svg)\"}</Text>", &paths),
            generated_structure("<Text>{\"url(/icons/b.svg)\"}</Text>", &paths)
        );
        assert_ne!(
            generated_structure("<Image src=\"/icons/a.svg\" />", &paths),
            generated_structure("<Image src=\"/icons/unknown.svg\" />", &paths)
        );
    }

    #[test]
    fn p1_layered_backgrounds_compare_every_asset_reference() {
        let paths = vec!["/images/a.png".into(), "/images/b.png".into()];
        let first = "<Box bg=\"linear-gradient(red, blue), url('/images/a.png') center/cover, url(/images/b.png)\" />";
        let second = "<Box bg=\"linear-gradient(red, blue), url('/images/b.png') center/cover, url(/images/a.png)\" />";
        assert_eq!(
            generated_structure(first, &paths),
            generated_structure(second, &paths)
        );
        assert_ne!(
            generated_structure(first, &paths),
            generated_structure(&second.replace("red", "green"), &paths)
        );
    }

    #[tokio::test]
    async fn p1_selection_does_not_claim_to_include_an_unproduced_manifest() {
        let result = project(section(2), operation(&["tsx", "assetManifest"]))
            .await
            .unwrap();
        assert_eq!(result["status"], "selection_required");
        assert!(result.get("assetManifest").is_none());
        assert_eq!(result["assetSummary"]["manifestIncluded"], false);
    }

    #[test]
    fn p1_comparison_scopes_partial_frames_to_their_generated_outputs() {
        let mut frames = vec![
            json!({"nodeId":"1", "status":"complete", "tsx":"code", "componentTsx":"component"}),
            json!({"nodeId":"2", "status":"partial", "tsx":"code"}),
            json!({"nodeId":"3", "status":"partial", "tsx":"code", "componentTsx":"component"}),
            json!({"nodeId":"4", "status":"partial"}),
        ];
        annotate_identical_frames(&mut frames, &payload(), true);
        assert!(frames[1].get("structurallyIdenticalTo").is_none());
        assert_eq!(frames[2]["structurallyIdenticalTo"], "1");
        assert_eq!(frames[2]["comparedOutputs"], json!(["tsx", "componentTsx"]));
        assert!(frames[3].get("structurallyIdenticalTo").is_none());
    }

    #[tokio::test]
    async fn p1_identical_hints_and_asset_summary_survive_resource_delivery() {
        let mut op = operation(&["tsx", "componentTsx"]);
        if let PendingOperation::Export {
            all_screens,
            delivery,
            ..
        } = &mut op
        {
            *all_screens = true;
            *delivery = DeliveryMode::Resource;
        }
        let result = project(section(2), op).await.unwrap();
        assert!(result["frames"][0].get("tsx").is_none());
        assert_eq!(result["frames"][1]["structurallyIdenticalTo"], "1:1");
        assert_eq!(result["frames"][1]["differsOnly"], json!([]));
        assert_eq!(
            result["frames"][1]["comparedOutputs"],
            json!(["tsx", "componentTsx"])
        );
        assert!(result["assetSummary"].is_object());
        assert!(result["frames"][0]["assetSummary"].is_object());
    }

    #[tokio::test]
    async fn p1_collected_assets_are_distinct_from_delivered_manifest() {
        let mut data = payload();
        data.snapshot.nodes.get_mut("1:1").unwrap().node_type = "VECTOR".into();
        data.assets.push(
            devup_mcp_figma::exported_asset_from_bytes(
                &devup_mcp_figma::AssetRequest {
                    asset_id: "1:1:node".into(),
                    node_id: "1:1".into(),
                    field: "node".into(),
                    image_hash: None,
                    format: devup_mcp_figma::AssetFormat::Svg,
                    scale: 1,
                },
                b"<svg/>",
            )
            .unwrap(),
        );
        let result = project(data, operation(&["tsx"])).await.unwrap();
        assert_eq!(result["assetSummary"]["status"], "collected");
        assert_eq!(result["assetSummary"]["collectedCount"], 1);
        assert_eq!(result["assetSummary"]["manifestIncluded"], false);
        assert!(result.get("assetManifest").is_none());
    }

    fn section(count: usize) -> CollectedPayload {
        let mut payload = payload();
        let template = payload.snapshot.nodes["1:1"].clone();
        payload.target.node_id = Some("0:1".into());
        payload.snapshot.nodes.clear();
        payload.snapshot.roots.clear();
        let mut candidates = Vec::new();
        for i in 1..=count {
            let id = format!("1:{i}");
            let mut node = template.clone();
            node.id = id.clone();
            payload.snapshot.nodes.insert(id.clone(), node);
            payload.snapshot.roots.push(id.clone());
            candidates.push(json!({"nodeId":id,"name":"Screen","nodeType":"FRAME",
                "textPreview":"","visible":true,"bounds":{"x":0,"y":0,"width":390,"height":800},
                "parentId":"0:1","breadcrumb":[],"directChildCount":0,
                "subtreeNodeCount":1,"estimatedSerializedBytes":500,"selectionReasons":["inside-section"],
                "canonicalUrl":format!("https://www.figma.com/design/W1Fixture/Test?node-id=1-{i}")}));
        }
        payload.metadata = json!({"sectionIndex":{"fileKey":"W1Fixture","sourceVersion":null,
            "section":{"nodeId":"0:1","name":"Section","bounds":{"x":0,"y":0,"width":2000,"height":2000}},
            "candidates":candidates,"truncated":false}});
        payload
    }

    fn operation(outputs: &[&str]) -> PendingOperation {
        PendingOperation::Export {
            outputs: outputs.iter().map(|s| (*s).into()).collect(),
            component_name: None,
            include_diagnostics: true,
            root_layout: Default::default(),
            asset_names_per_node: true,
            scope: "node".into(),
            strict: false,
            output_paths: BTreeMap::new(),
            frame_ids: vec![],
            all_screens: false,
            asset_captures: vec![],
            asset_output_paths: BTreeMap::new(),
            delivery: DeliveryMode::Inline,
        }
    }

    fn section_without_index(count: usize, projection_truncated: bool) -> CollectedPayload {
        let mut data = section(count);
        data.metadata = json!({});
        let mut root = data.snapshot.nodes["1:1"].clone();
        root.id = "0:1".into();
        root.node_type = "SECTION".into();
        root.fields = serde_json::from_value(json!({
            "name":"Section", "visible":true, "x":0, "y":0,
            "width":2000, "height":2000,
            "childrenIds":data.snapshot.roots,
            "projectionTruncated":projection_truncated
        }))
        .unwrap();
        for node in data.snapshot.nodes.values_mut() {
            node.fields.insert("parentId".into(), json!("0:1"));
        }
        data.snapshot.nodes.insert(root.id.clone(), root);
        data.snapshot.roots = vec!["0:1".into()];
        data
    }

    #[tokio::test]
    async fn w1_selection_exactly_100_candidates_is_complete() {
        let result = project(section_without_index(100, false), operation(&["tsx"]))
            .await
            .unwrap();
        assert_eq!(result["status"], "selection_required");
        assert_eq!(result["selection"]["count"], 100);
        assert_eq!(result["selection"]["truncated"], false);
        assert_eq!(result["selection"]["status"], "complete");
    }

    #[tokio::test]
    async fn w1_selection_over_100_candidates_is_partial() {
        let result = project(section_without_index(101, false), operation(&["tsx"]))
            .await
            .unwrap();
        assert_eq!(result["selection"]["count"], 100);
        assert_eq!(result["selection"]["truncated"], true);
        assert_eq!(result["selection"]["status"], "partial");
    }

    #[tokio::test]
    async fn w1_selection_separates_projection_and_candidate_truncation() {
        let data = section_without_index(100, true);
        let explored =
            explore_snapshot(&data.snapshot, &data.target, &ExploreOptions { limit: 100 }).unwrap();
        assert!(explored.truncated);
        assert!(!explored.candidates_truncated);
        let result = project(data, operation(&["tsx"])).await.unwrap();
        assert_eq!(result["selection"]["truncated"], false);
        assert_eq!(result["selection"]["status"], "complete");
    }

    async fn project(
        payload: CollectedPayload,
        operation: PendingOperation,
    ) -> Result<Value, DevupError> {
        let store = ArtifactStore::default();
        let key = ArtifactRequestKey::from_collection(&CollectionRequest::new(
            payload.target.clone(),
            CollectionScope::Node,
        ));
        let artifact = store.insert(key, payload).await.unwrap();
        complete_operation(
            operation,
            &artifact.payload,
            "fixture",
            &artifact,
            &OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap(),
            &store,
        )
        .await
    }

    #[tokio::test]
    async fn w1_component_tsx_projects_selected_frames_without_section_root() {
        let mut op = operation(&["componentTsx"]);
        if let PendingOperation::Export { frame_ids, .. } = &mut op {
            *frame_ids = vec!["1:1".into()];
        }
        let result = project(section(2), op).await.unwrap();
        assert_eq!(result["frames"].as_array().unwrap().len(), 1);
        assert!(
            result["frames"][0]["componentTsx"]
                .as_str()
                .unwrap()
                .contains("export function")
        );
        assert_ne!(result["quality"]["projection"], "not-requested");
    }

    #[tokio::test]
    async fn w1_unavailable_responsive_output_is_actionable_even_with_tsx_and_strict() {
        for outputs in [vec!["responsiveTsx"], vec!["tsx", "responsiveTsx"]] {
            for strict_value in [false, true] {
                let mut op = operation(&outputs);
                if let PendingOperation::Export {
                    all_screens,
                    strict,
                    ..
                } = &mut op
                {
                    *all_screens = true;
                    *strict = strict_value;
                }
                let error = project(section(2), op).await.unwrap_err();
                assert_eq!(error.code, ErrorCode::DevupInvalidInput);
                assert!(error.message.contains("responsiveTsx"));
                assert!(error.message.contains("tsx"));
                assert!(error.details.is_object());
            }
        }
    }

    #[tokio::test]
    async fn w1_asset_failure_is_partial_and_strict_refuses_it() {
        let mut data = payload();
        data.assets.push(serde_json::from_value(json!({"assetId":"1:1:fills:0","nodeId":"1:1",
            "field":"fills/0","sourceKind":"image","status":"failed","errorCode":"DEVUP_ASSET_EXPORT_FAILED"})).unwrap());
        let result = project(data.clone(), operation(&["assetManifest"]))
            .await
            .unwrap();
        assert_eq!(result["status"], "partial");
        assert_eq!(result["failures"][0]["assetId"], "1:1:fills:0");
        assert_ne!(result["quality"]["assets"], "complete");
        let mut strict_op = operation(&["assetManifest"]);
        if let PendingOperation::Export { strict, .. } = &mut strict_op {
            *strict = true;
        }
        assert!(project(data, strict_op).await.is_err());
    }

    #[tokio::test]
    async fn w1_frame_failure_preserves_other_frames_and_context() {
        let mut data = section(2);
        data.snapshot.nodes.remove("1:2");
        data.snapshot.roots.retain(|id| id != "1:2");
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["status"], "partial");
        assert!(result["frames"][0]["tsx"].is_string());
        assert_eq!(result["failures"][0]["nodeId"], "1:2");
        assert_eq!(result["frames"][1]["quality"]["projection"], "failed");
        assert_eq!(
            result["failures"][0]["details"]["snapshotRootIds"],
            json!(["1:1"])
        );
    }

    #[tokio::test]
    async fn w1_all_screens_uses_explore_classification() {
        let mut data = section(3);
        data.metadata["sectionIndex"]["candidates"][1]["bounds"] =
            json!({"x":0,"y":0,"width":900,"height":100});
        data.metadata["sectionIndex"]["candidates"][2]["bounds"] =
            json!({"x":0,"y":0,"width":100,"height":100});
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["frames"].as_array().unwrap().len(), 1);
        assert_eq!(result["frames"][0]["nodeId"], "1:1");
    }

    #[tokio::test]
    async fn w1_large_section_has_actionable_batch_refusal() {
        let mut op = operation(&["tsx", "assetManifest"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let error = project(section(14), op).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
        assert!(error.details["recommendedFrameIds"].is_array());
    }

    #[tokio::test]
    async fn w1_resource_delivery_moves_all_code_and_updates_note() {
        let store = ArtifactStore::default();
        let data = payload();
        let key = ArtifactRequestKey::from_collection(&CollectionRequest::new(
            data.target.clone(),
            CollectionScope::Node,
        ));
        let artifact = store.insert(key, data).await.unwrap();
        let mut result = json!({"tsx":"screen", "responsiveTsx":"responsive", "componentTsx":"component",
            "frames":[{"componentTsx":"frame-component"}],
            "deliverable":{"note":"Implement from this value."}});
        let outputs = projected_outputs_from_result(result.as_object().unwrap()).unwrap();
        let attachment = apply_delivery(
            &mut result,
            DeliveryMode::Resource,
            &store,
            &artifact,
            outputs,
        )
        .await
        .unwrap();
        commit_delivery(attachment);
        assert!(result.get("componentTsx").is_none());
        assert!(result.get("responsiveTsx").is_none());
        assert!(result["frames"][0].get("componentTsx").is_none());
        assert_eq!(result["resources"].as_array().unwrap().len(), 4);
        assert!(
            result["deliverable"]["note"]
                .as_str()
                .unwrap()
                .contains("resources")
        );
    }

    #[test]
    fn w1_asset_paths_are_ascii_bounded_and_windows_safe() {
        assert_eq!(
            host_safe_asset_path("/images/Frame 269.png"),
            "/images/Frame-269-3e2af8571e3f.png"
        );
        for name in [
            "한글.svg".to_owned(),
            "CON.svg".to_owned(),
            format!("{}.png", "a".repeat(300)),
        ] {
            let path = host_safe_asset_path(&format!("/icons/{name}"));
            assert!(path.is_ascii());
            assert!(path.len() < 200);
            assert_ne!(path.to_uppercase(), "/ICONS/CON.SVG");
            assert_eq!(host_safe_asset_path(&path), path);
        }
        assert_ne!(
            host_safe_asset_path("/icons/한글.svg"),
            host_safe_asset_path("/icons/한국.svg")
        );
    }

    #[test]
    fn w1_fixture_asset_paths_are_normalized_without_changing_other_code() {
        let code = include_str!("../../../../fixtures/plugin-answers/notice/pure.tsx");
        let safe = with_host_safe_asset_paths(code);
        assert!(!safe.contains("/icons/Property 1="));
        assert_eq!(with_host_safe_asset_paths(&safe), safe);
    }
    #[tokio::test]
    async fn w1_index_accepts_component_projection_selection() {
        let store = ArtifactStore::default();
        let data = section(2);
        let key = ArtifactRequestKey::from_collection(&CollectionRequest::new(
            data.target.clone(),
            CollectionScope::Node,
        ));
        let artifact = store.insert(key, data).await.unwrap();
        for output in ["componentTsx", "responsiveTsx"] {
            super::super::validation::validate_artifact_projection(
                &artifact,
                &[output.into()],
                "node",
                &[],
            )
            .unwrap();
        }
    }

    #[tokio::test]
    async fn w1_hidden_asset_is_warning_and_requested_failure_is_partial() {
        let mut data = payload();
        data.assets.push(
            serde_json::from_value(json!({"assetId":"1:1:fills:0","nodeId":"1:1",
            "field":"fills/0","sourceKind":"image","status":"failed","format":"png","scale":1,
            "errorCode":"DEVUP_ASSET_NODE_HIDDEN"}))
            .unwrap(),
        );
        let mut op = operation(&["assetManifest"]);
        if let PendingOperation::Export { asset_captures, .. } = &mut op {
            asset_captures.push(devup_mcp_figma::AssetSelection {
                asset_id: "1:1:fills:0".into(),
                format: devup_mcp_figma::AssetFormat::Png,
                scale: 1,
            });
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["failures"], json!([]));
        assert_eq!(result["warnings"][0]["assetId"], "1:1:fills:0");
        assert_eq!(result["status"], "complete");
    }

    #[tokio::test]
    async fn w1_manifest_preserves_layer_name_and_section_paths_match() {
        let mut data = section(1);
        data.snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert("name".into(), json!("BI - 아이콘"));
        data.snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert("childrenIds".into(), json!(["2:1"]));
        data.snapshot.nodes.insert(
            "2:1".into(),
            serde_json::from_value(json!({"id":"2:1","type":"VECTOR",
            "fields":{"name":"BI - 아이콘", "parentId":"1:1","width":24,"height":24,"visible":true,
                "fills":[{"type":"SOLID","color":{"r":0,"g":0,"b":0}}]}}))
            .unwrap(),
        );
        let mut op = operation(&["tsx", "assetManifest"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(data, op).await.unwrap();
        let asset = &result["assetManifest"]["assets"][0];
        assert_eq!(asset["layerName"], "BI - 아이콘");
        let path = asset["path"].as_str().unwrap();
        assert!(path.is_ascii());
        assert!(!path.contains(' '));
        assert!(result["frames"][0]["tsx"].as_str().unwrap().contains(path));
    }

    #[tokio::test]
    async fn w1_missing_master_is_partial_with_component_id() {
        let mut data = section(1);
        data.snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert("childrenIds".into(), json!(["2:1"]));
        data.snapshot.nodes.insert(
            "2:1".into(),
            serde_json::from_value(json!({"id":"2:1","type":"INSTANCE",
            "fields":{"name":"Button","parentId":"1:1","componentId":"9:9","visible":true}}))
            .unwrap(),
        );
        let mut op = operation(&["tsx", "componentTsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["status"], "partial");
        assert!(result["frames"][0]["tsx"].is_string());
        assert_eq!(result["failures"][0]["details"]["componentId"], "9:9");
        assert_eq!(result["failures"][0]["details"]["instanceNodeId"], "2:1");
    }

    #[tokio::test]
    async fn w1_source_map_without_generator_is_actionable() {
        let error = project(payload(), operation(&["sourceMap"]))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
        assert!(error.message.contains("tsx"));
    }

    fn responsive_payload() -> CollectedPayload {
        let mut data = section(2);
        for (id, name, width) in [("1:1", "mobile", 390), ("1:2", "desktop", 1440)] {
            let node = data.snapshot.nodes.get_mut(id).unwrap();
            node.fields.insert("name".into(), json!(name));
            node.fields.insert("width".into(), json!(width));
        }
        data
    }

    #[tokio::test]
    async fn w1_responsive_does_not_merge_unselected_frames() {
        let mut op = operation(&["responsiveTsx"]);
        if let PendingOperation::Export { frame_ids, .. } = &mut op {
            *frame_ids = vec!["1:1".into()];
        }
        let result = project(responsive_payload(), op).await;
        assert!(
            result.is_err(),
            "one selected width cannot be a responsive merge"
        );
    }

    #[tokio::test]
    async fn w1_responsive_success_has_code_resource_and_no_empty_frames() {
        let mut op = operation(&["responsiveTsx"]);
        if let PendingOperation::Export {
            all_screens,
            delivery,
            ..
        } = &mut op
        {
            *all_screens = true;
            *delivery = DeliveryMode::Resource;
        }
        let result = project(responsive_payload(), op).await.unwrap();
        assert_ne!(result["quality"]["projection"], "not-requested");
        assert!(
            result.get("frames").is_none(),
            "no per-frame outputs were requested"
        );
        assert!(
            result["resources"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["name"] == "responsiveTsx")
        );
    }

    #[tokio::test]
    async fn w1_empty_all_screens_is_actionable() {
        let mut data = section(1);
        data.metadata["sectionIndex"]["candidates"][0]["bounds"] =
            json!({"x":0,"y":0,"width":100,"height":100});
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let error = project(data, op).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
        assert!(error.message.contains("frameIds"));
    }

    #[tokio::test]
    async fn w1_partial_frames_provide_retry_arguments() {
        let mut data = section(2);
        data.snapshot.nodes.remove("1:2");
        data.snapshot.roots.retain(|id| id != "1:2");
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["resume"]["arguments"]["frameIds"], json!(["1:2"]));
        assert_eq!(result["resume"]["arguments"]["outputs"], json!(["tsx"]));
        assert!(result["resume"]["arguments"]["url"].is_string());
    }

    #[tokio::test]
    async fn w1_discovered_hidden_assets_are_warnings_not_failed_deliverables() {
        let mut data = payload();
        let node = data.snapshot.nodes.get_mut("1:1").unwrap();
        node.node_type = "VECTOR".into();
        node.fields.insert("visible".into(), json!(false));
        let result = project(data, operation(&["assetManifest"])).await.unwrap();
        assert_eq!(result["status"], "complete");
        assert_eq!(result["assetManifest"]["assets"], json!([]));
        assert_eq!(
            result["warnings"][0]["errorCode"],
            "DEVUP_ASSET_NODE_HIDDEN"
        );
    }

    #[tokio::test]
    async fn w1_index_unavailable_output_explains_collection_step() {
        let store = ArtifactStore::default();
        let data = section(2);
        let key = ArtifactRequestKey::from_collection(&CollectionRequest::new(
            data.target.clone(),
            CollectionScope::Node,
        ));
        let artifact = store.insert(key, data).await.unwrap();
        let error = super::super::validation::validate_artifact_projection(
            &artifact,
            &["assetManifest".into()],
            "node",
            &[],
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
        assert!(error.message.contains("frameIds"));
    }
    #[test]
    fn w1_slash_in_layer_name_matches_manifest_and_code() {
        let original = "/icons/한글 Foo/Bar-1-2.svg";
        let path = host_safe_asset_path(original);
        let code = with_host_safe_asset_paths(&format!("<Box maskImage=\"url('{original}')\" />"));
        assert!(path.is_ascii());
        assert!(!path.contains(' '));
        assert_eq!(path.matches('/').count(), 2);
        assert!(code.contains(&path));
    }

    #[tokio::test]
    async fn w1_captured_main_component_id_is_reported_when_missing() {
        let mut data = section(1);
        data.snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert("childrenIds".into(), json!(["2:1"]));
        data.snapshot.nodes.insert(
            "2:1".into(),
            serde_json::from_value(json!({"id":"2:1","type":"INSTANCE",
            "fields":{"name":"Button","parentId":"1:1","mainComponentId":"9:9","visible":true}}))
            .unwrap(),
        );
        let mut op = operation(&["componentTsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["status"], "partial");
        assert_eq!(result["failures"][0]["details"]["componentId"], "9:9");
    }

    #[tokio::test]
    async fn w1_responsive_text_loss_is_partial_and_strict_rejects() {
        let mut data = responsive_payload();
        for (root, id, text) in [
            ("1:1", "2:1", "Mobile only"),
            ("1:2", "2:2", "Desktop only"),
        ] {
            data.snapshot
                .nodes
                .get_mut(root)
                .unwrap()
                .fields
                .insert("childrenIds".into(), json!([id]));
            data.snapshot.nodes.insert(id.into(), serde_json::from_value(json!({"id":id,"type":"TEXT",
                "fields":{"name":"Label","parentId":root,"characters":text,"visible":true,"fontSize":16}})).unwrap());
        }
        let mut op = operation(&["responsiveTsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(data.clone(), op.clone()).await.unwrap();
        assert!(
            result["responsiveUnrepresented"]
                .as_array()
                .is_some_and(|notes| !notes.is_empty())
        );
        assert_eq!(result["status"], "partial");
        assert_eq!(result["quality"]["projection"], "lossy");
        if let PendingOperation::Export { strict, .. } = &mut op {
            *strict = true;
        }
        assert!(project(data, op).await.is_err());
    }

    #[tokio::test]
    async fn w1_responsive_slot_collision_is_actionable() {
        let mut data = responsive_payload();
        data.snapshot
            .nodes
            .get_mut("1:2")
            .unwrap()
            .fields
            .insert("width".into(), json!(400));
        let mut op = operation(&["responsiveTsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let error = project(data, op).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
        assert!(error.message.contains("breakpoint"));
    }

    #[tokio::test]
    async fn w1_missing_collected_outputs_explain_how_to_recollect() {
        for output in ["devupJson", "referencePng"] {
            let error = project(payload(), operation(&[output])).await.unwrap_err();
            assert_eq!(error.code, ErrorCode::DevupInvalidInput);
            assert!(error.message.contains(output));
            assert!(error.details["snapshotRootIds"].is_array());
        }
    }
    #[tokio::test]
    async fn w1_optional_responsive_collision_keeps_requested_tsx() {
        let mut data = responsive_payload();
        data.snapshot
            .nodes
            .get_mut("1:2")
            .unwrap()
            .fields
            .insert("width".into(), json!(400));
        let mut op = operation(&["tsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["frames"].as_array().unwrap().len(), 2);
        assert!(result["frames"][0]["tsx"].is_string());
        assert!(result.get("responsiveTsx").is_none());
    }

    #[test]
    fn w1_ascii_slugs_do_not_merge_distinct_variant_assets() {
        assert_ne!(
            host_safe_asset_path("/icons/Icons=a b.svg"),
            host_safe_asset_path("/icons/Icons=a-b.svg")
        );
        assert_ne!(
            host_safe_asset_path("/icons/Foo Bar.svg"),
            host_safe_asset_path("/icons/Foo-Bar.svg")
        );
    }
}
