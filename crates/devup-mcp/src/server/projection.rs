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
    if let Some(tsx) = result.get("tsx").and_then(Value::as_str) {
        outputs.push(ProjectedOutput::text(
            "tsx",
            "text/typescript",
            tsx.as_bytes().to_vec(),
        ));
    }
    if let Some(tsx) = result.get("componentTsx").and_then(Value::as_str) {
        outputs.push(ProjectedOutput::text(
            "componentTsx",
            "text/typescript",
            tsx.as_bytes().to_vec(),
        ));
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
            if let Some(tsx) = frame.get("tsx").and_then(Value::as_str) {
                outputs.push(ProjectedOutput::text(
                    format!("frame-{}.tsx", index + 1),
                    "text/typescript",
                    tsx.as_bytes().to_vec(),
                ));
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
                frame.remove("sourceMap");
            }
        }
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

/// The file name a host file system will take. A designer names a layer for
/// the eye - `grommet-icons:language`, `ic:round-arrow-left` - and the plugin
/// writes that name verbatim; Windows cannot create it. The characters no
/// file system takes become dashes. A name that is already writable - nearly
/// every one - passes through untouched, so what the server delivers keeps
/// the plugin's own name wherever that name is a file at all.
fn host_safe_file_name(name: &str) -> String {
    name.chars()
        .map(|character| match character {
            ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\\' => '-',
            other if other.is_control() => '-',
            other => other,
        })
        .collect()
}

/// `/icons/ic:round-arrow-left.svg` as `/icons/ic-round-arrow-left.svg`. Only
/// the file name is touched; the folder the code looks in is left alone.
fn host_safe_asset_path(path: &str) -> String {
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
/// Adds `fidelity` only when the conversion was not exact.
///
/// `quality.projection` is the signal a caller acts on; `fidelity` is the
/// drill-down beneath it, at 437 measured bytes, repeated once per screen on
/// a Section export. Tying it to that same grade keeps the two from
/// disagreeing: an exact conversion sends the grade alone, and anything less
/// sends the axes that explain it.
///
/// Deliberately not keyed on `strict_compatible`, which also fails on a
/// coverage shortfall that changes nothing about the output - that would put
/// the report back on almost every response while `quality` still read
/// `exact`. `strict: true` keeps using `strict_compatible` to refuse, and
/// returns the same report in the error.
fn attach_fidelity(
    response: &mut Value,
    report: &devup_mcp_devup_ui::provenance::FidelityReport,
    projection: ProjectionQuality,
    include_diagnostics: bool,
) {
    if include_diagnostics || projection != ProjectionQuality::Exact {
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
            let mut result = Map::new();
            // Not restated by quality: this grades how far token resolution
            // reached - whether external library variables were covered - where
            // quality.theme only says whether what was resolved conflicts.
            result.insert("completeness".to_owned(), json!(payload.completeness));
            result.insert("collection".to_owned(), json!(collection));
            result.insert("cache".to_owned(), artifact_metadata(artifact));
            result.insert("failures".to_owned(), json!(&payload.failures));
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

            let section_candidates = if target_kind == TargetKind::Section
                && outputs.iter().any(|output| output == "tsx")
            {
                Some(if let Some(index) = &payload_section_index {
                    index
                        .candidates
                        .iter()
                        .map(section_candidate_as_explore)
                        .collect()
                } else {
                    explore_snapshot(
                        &payload.snapshot,
                        &payload.target,
                        &ExploreOptions { limit: 100 },
                    )?
                    .candidates
                })
            } else {
                None
            };
            if let Some(candidates) = &section_candidates
                && frame_ids.is_empty()
                && !all_screens
            {
                let quality = OutputQuality {
                    acquisition: acquisition_quality(&completeness_report, false),
                    projection: projection_quality(false, &[]),
                    theme: theme_quality(false, 0, 0),
                    assets: assets_quality(false, &[], &[]),
                };
                result.insert("status".to_owned(), json!("selection_required"));
                result.insert("quality".to_owned(), json!(quality));
                result.insert(
                    "selection".to_owned(),
                    json!({
                        "kind": "screen-frame",
                        "candidates": candidates,
                        "truncated": candidates.len() == 100
                    }),
                );
                result.insert(
                    "nextAction".to_owned(),
                    json!({
                        "why": "This link is a Section and holds several screens inside. Collecting them all at once exceeds the size limit.",
                        "how": "Call again with the target screen's canonicalUrl from screens[], or use allScreens:true if you need every screen.",
                        "doNot": "Do not try to collect the whole Section at once."
                    }),
                );
                return Ok(Value::Object(result));
            }

            let mut written_paths = Map::new();
            let mut section_tsx_projected = false;
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
                let mut frames = Vec::with_capacity(selected.len());
                for (index, candidate) in selected.into_iter().enumerate() {
                    let frame_component_name = component_name.as_ref().map(|name| {
                        if frame_ids.len() <= 1 && !all_screens {
                            name.clone()
                        } else {
                            format!("{name}{}", index + 1)
                        }
                    });
                    let output = generate_component(
                        &payload.snapshot,
                        &candidate.node.node_id,
                        &CodegenOptions {
                            component_name: frame_component_name,
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
                    let frame_quality = OutputQuality {
                        acquisition: acquisition_quality(&completeness_report, false),
                        projection: projection_quality(true, &output.diagnostics),
                        theme: theme_quality(false, 0, 0),
                        assets: assets_quality(false, &[], &[]),
                    };
                    let source_map = json!({
                        "version": output.source_map.version,
                        "entries": output.source_map.entries,
                        "source": {
                            "fileKey": payload.target.file_key,
                            "rootNodeId": candidate.node.node_id,
                            "sourceVersion": payload.source_version
                        }
                    });
                    // Every key here is paid for once per screen, so on
                    // allScreens the redundant ones multiply. `imports` and
                    // `usedTokens` both restate what the tsx beside them
                    // already spells out - its import line and its `$token`s.
                    let mut frame = json!({
                        "nodeId": candidate.node.node_id,
                        "name": candidate.node.name,
                        "canonicalUrl": candidate.canonical_url,
                        "status": frame_quality.status(),
                        "quality": frame_quality,
                        "tsx": output.tsx
                    });
                    attach_fidelity(
                        &mut frame,
                        &output.fidelity_report,
                        frame_quality.projection,
                        include_diagnostics,
                    );
                    attach_completeness_report(
                        &mut frame,
                        frame_quality,
                        &completeness_report,
                        include_diagnostics,
                    );
                    if outputs.iter().any(|output| output == "sourceMap") {
                        frame["sourceMap"] = source_map;
                    }
                    if include_diagnostics {
                        frame["diagnostics"] = json!(output.diagnostics);
                    }
                    frames.push(frame);
                }
                result.insert("frames".to_owned(), Value::Array(frames));
                section_tsx_projected = true;
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
                    &payload.snapshot,
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
                let module = merged.module(&name);
                if output_paths.contains_key("responsiveTsx") {
                    pending_text_outputs.insert("responsiveTsx".to_owned(), module.clone());
                }
                result.insert("responsiveTsx".to_owned(), json!(module));
                result.insert("responsiveSlots".to_owned(), json!(merged.slots));
                if !merged.unrepresented.is_empty() {
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

            if outputs.iter().any(|output| output == "componentTsx") {
                let node_id = payload.target.node_id.as_deref().ok_or_else(|| {
                    DevupError::new(
                        ErrorCode::DevupFigmaNodeNotFound,
                        "A component TSX export payload requires a node ID.",
                        false,
                    )
                })?;
                let output = generate_component(
                    &payload.snapshot,
                    node_id,
                    &CodegenOptions {
                        component_name: component_name_for_components,
                        include_diagnostics: false,
                        inline_instances: false,
                        root_layout,
                        asset_names_per_node,
                        ..CodegenOptions::default()
                    }
                    .with_payload_tokens(payload),
                )?;
                if output_paths.contains_key("componentTsx") {
                    pending_text_outputs.insert("componentTsx".to_owned(), output.tsx.clone());
                }
                result.insert("componentTsx".to_owned(), json!(output.tsx));
            }

            if outputs.iter().any(|output| output == "devupJson") {
                let variables = payload.variables.as_ref().ok_or_else(|| {
                    DevupError::new(
                        ErrorCode::DevupSnapshotUnsupported,
                        "There is no Figma variable/style collection result.",
                        false,
                    )
                })?;
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
                let reference = payload.reference_png.as_ref().ok_or_else(|| {
                    DevupError::new(
                        ErrorCode::DevupFigmaHandoffInvalid,
                        "The requested reference PNG is not in the artifact. Re-collect it from the URL.",
                        false,
                    )
                })?;
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
                            && asset.status == AssetStatus::Exported
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
            let quality = OutputQuality {
                acquisition: acquisition_quality(&completeness_report, false),
                projection: projection_quality(
                    outputs.iter().any(|output| output == "tsx"),
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
            let fidelity_violation = fidelity_reports
                .iter()
                .any(|report| !report.strict_compatible());
            if strict && (quality.strict_violation() || fidelity_violation) {
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
                        "completenessReport": completeness_report
                    }),
                ));
            }
            let final_status = quality.status();
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
            let tsx_produced =
                section_tsx_projected || outputs.iter().any(|output| output == "tsx");
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
                attach_fidelity(
                    &mut carrier,
                    report,
                    quality.projection,
                    include_diagnostics,
                );
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
                result.insert("assetManifest".to_owned(), json!(manifest));
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
                "<Box maskImage=\"url(/icons/ic-round-arrow-left.svg)\" />\n",
                "<Image src=\"/images/Frame 269.png\" />\n",
                "<Box maskImage=\"url('/icons/Frame 287.svg')\" />\n",
                "<Box maskImage=\"url(/icons/grommet-icons-language.svg)\" />\n",
            )
        );
    }

    #[test]
    fn the_written_path_is_renamed_the_same_way_the_code_is() {
        assert_eq!(
            host_safe_asset_path("/icons/grommet-icons:language.svg"),
            "/icons/grommet-icons-language.svg"
        );
        assert_eq!(
            host_safe_asset_path("/images/Frame 269.png"),
            "/images/Frame 269.png"
        );
    }
}
