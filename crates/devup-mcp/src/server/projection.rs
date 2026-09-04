use std::fmt::Write as _;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_devup_ui::{
    codegen::{CodegenOptions, generate_component},
    theme::{generate_devup_json, variable_snapshot_from_result},
};
use devup_mcp_figma::{
    AssetManifest, AssetStatus, CollectedPayload, CollectionStats, DevupError, ErrorCode,
    ExploreOptions, SearchOptions, TargetKind, classify_target, explore_snapshot, search_snapshot,
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
        OutputQuality, acquisition_quality, assets_quality, projection_quality, theme_quality,
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

pub(super) fn commit_single_output(
    policy: &OutputPolicy,
    path: Option<&str>,
    name: &str,
    contents: &[u8],
) -> Result<Option<String>, DevupError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let mut transaction = OutputTransaction::new();
    transaction.stage(name, policy.resolve(path)?, contents)?;
    Ok(transaction.commit()?.remove(name))
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
        PendingOperation::ToUi {
            component_name,
            include_diagnostics,
            root_layout,
            output_path,
            delivery,
        } => {
            let node_id = payload.target.node_id.as_deref().ok_or_else(|| {
                DevupError::new(
                    ErrorCode::DevupFigmaNodeNotFound,
                    "A UI conversion payload requires a node ID.",
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
                    ..CodegenOptions::default()
                }
                .with_payload_tokens(payload),
            )?;
            let quality = OutputQuality {
                acquisition: acquisition_quality(&completeness_report, false),
                projection: projection_quality(true, &output.diagnostics),
                theme: theme_quality(false, 0, 0),
                assets: assets_quality(false, &[], &[]),
            };
            let status = quality.status();
            let diagnostics = if include_diagnostics {
                output.diagnostics.clone()
            } else {
                Vec::new()
            };
            let projected_outputs = vec![ProjectedOutput::text(
                "tsx",
                "text/typescript",
                output.tsx.as_bytes().to_vec(),
            )];
            let mut result = json!({
                "status": status,
                "quality": quality,
                "tsx": output.tsx,
                "imports": output.imports,
                "usedTokens": output.used_tokens,
                "fidelity": output.fidelity_report,
                "diagnostics": diagnostics,
                "outputPath": null,
                "completeness": payload.completeness,
                "completenessReport": &completeness_report,
                "rootLayout": root_layout,
                "collection": collection,
                "cache": artifact_metadata(artifact),
                "source": {
                    "kind": source_kind,
                    "fileKey": payload.target.file_key,
                    "nodeId": node_id,
                    "version": payload.snapshot.version
                },
                "snapshot": {
                    "preservedNodeCount": payload.snapshot.nodes.len(),
                    "fieldErrorCount": payload.snapshot.nodes.values()
                        .map(|node| node.field_errors.len()).sum::<usize>()
                }
            });
            let attachment = apply_delivery(
                &mut result,
                delivery,
                artifact_store,
                artifact,
                projected_outputs,
            )
            .await?;
            let written_path = match commit_single_output(
                output_policy,
                output_path.as_deref(),
                "tsx",
                output.tsx.as_bytes(),
            ) {
                Ok(path) => path,
                Err(error) => {
                    rollback_delivery(artifact_store, artifact, attachment).await;
                    return Err(error);
                }
            };
            commit_delivery(attachment);
            result["outputPath"] = json!(written_path);
            if status == "complete" {
                // Unambiguous "this is the real, final answer" marker.
                // Without it, an agent repeatedly seeing `needs_figma`
                // intermediate steps has, in an observed real failure,
                // concluded the conversion was "probably done" and moved
                // on to hand-interpreting the raw node tree instead of
                // waiting for this response.
                result["deliverable"] = json!({
                    "kind": "devup-ui-tsx",
                    "isFinal": true,
                    "note": "This tsx is the final deliverable. Implement from this value."
                });
            }
            Ok(result)
        }
        PendingOperation::ToJson {
            scope,
            include_diagnostics,
            output_path,
            delivery,
        } => {
            let result = payload.variables.as_ref().ok_or_else(|| {
                DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "There is no Figma variable/style collection result.",
                    false,
                )
            })?;
            let variables = variable_snapshot_from_result(result)?;
            let output = generate_devup_json(&variables, parse_scope(&scope)?)?;
            let quality = OutputQuality {
                acquisition: acquisition_quality(&completeness_report, false),
                projection: projection_quality(false, &[]),
                theme: theme_quality(
                    true,
                    output.conflicts.len(),
                    output.unresolved_variables.len(),
                ),
                assets: assets_quality(false, &[], &[]),
            };
            let status = quality.status();
            let diagnostics = if include_diagnostics {
                output.diagnostics.clone()
            } else {
                Vec::new()
            };
            let projected_outputs = vec![ProjectedOutput::text(
                "devupJson",
                "application/json",
                output.json.as_bytes().to_vec(),
            )];
            let mut result = json!({
                "status": status,
                "quality": quality,
                "devupJson": output.json,
                "counts": output.counts,
                "completeness": output.completeness,
                "completenessReport": &completeness_report,
                "conflicts": output.conflicts,
                "unresolvedVariables": output.unresolved_variables,
                "diagnostics": diagnostics,
                "outputPath": null,
                "collection": collection,
                "cache": artifact_metadata(artifact),
                "source": {
                    "kind": source_kind,
                    "fileKey": payload.target.file_key,
                    "nodeId": payload.target.node_id,
                    "version": payload.snapshot.version
                }
            });
            let attachment = apply_delivery(
                &mut result,
                delivery,
                artifact_store,
                artifact,
                projected_outputs,
            )
            .await?;
            let written_path = match commit_single_output(
                output_policy,
                output_path.as_deref(),
                "devupJson",
                output.json.as_bytes(),
            ) {
                Ok(path) => path,
                Err(error) => {
                    rollback_delivery(artifact_store, artifact, attachment).await;
                    return Err(error);
                }
            };
            commit_delivery(attachment);
            result["outputPath"] = json!(written_path);
            Ok(result)
        }
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
            Ok(json!({
                "status": quality.status(),
                "quality": quality,
                "query": query,
                "count": matches.len(),
                "matches": matches,
                "completeness": payload.completeness,
                "completenessReport": &completeness_report,
                "collection": collection,
                "cache": artifact_metadata(artifact),
                "source": {
                    "kind": source_kind,
                    "fileKey": payload.target.file_key,
                    "version": payload.snapshot.version
                }
            }))
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
            Ok(json!({
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
                "completenessReport": &completeness_report,
                "collection": collection,
                "cache": artifact_metadata(artifact),
                "source": {
                    "kind": source_kind,
                    "fileKey": target.file_key,
                    "nodeId": target.node_id,
                    "version": payload.snapshot.version
                }
            }))
        }
        PendingOperation::Export {
            outputs,
            component_name,
            include_diagnostics,
            root_layout,
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
            result.insert("completeness".to_owned(), json!(payload.completeness));
            result.insert("completenessReport".to_owned(), json!(&completeness_report));
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
                    let mut frame = json!({
                        "nodeId": candidate.node.node_id,
                        "name": candidate.node.name,
                        "canonicalUrl": candidate.canonical_url,
                        "status": frame_quality.status(),
                        "quality": frame_quality,
                        "tsx": output.tsx,
                        "imports": output.imports,
                        "usedTokens": output.used_tokens,
                        "fidelity": output.fidelity_report,
                        "completenessReport": &completeness_report
                    });
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
                result.insert("imports".to_owned(), json!(output.imports));
                result.insert("usedTokens".to_owned(), json!(output.used_tokens));
                result.insert("fidelity".to_owned(), json!(output.fidelity_report));
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
                        ..CodegenOptions::default()
                    }
                    .with_payload_tokens(payload),
                )?;
                if output_paths.contains_key("componentTsx") {
                    pending_text_outputs.insert("componentTsx".to_owned(), output.tsx.clone());
                }
                result.insert("componentTsx".to_owned(), json!(output.tsx));
                result.insert("componentImports".to_owned(), json!(output.imports));
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
            let tsx_produced =
                section_tsx_projected || outputs.iter().any(|output| output == "tsx");
            if final_status == "complete" && tsx_produced {
                // Same unambiguous final-answer marker as devup_figma_to_ui
                // — see that branch's comment for why this exists. Checked
                // here (before `apply_delivery` may move `tsx`/each frame's
                // `tsx` into `resources`) so the marker reflects whether a
                // devup-ui TSX was actually produced, independent of how
                // large output routed it for delivery.
                result.insert(
                    "deliverable".to_owned(),
                    json!({
                        "kind": "devup-ui-tsx",
                        "isFinal": true,
                        "note": "This tsx is the final deliverable. Implement from this value."
                    }),
                );
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
            for (name, target, bytes) in planned_outputs {
                written_paths.insert(
                    name.clone(),
                    json!(target.display_path().to_string_lossy().into_owned()),
                );
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
