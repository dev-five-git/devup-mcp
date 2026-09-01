use std::fmt::Write as _;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_figma::{DevupError, ErrorCode};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::{
    artifacts::{ArtifactLookup, ArtifactStore},
    delivery::{DeliveryMode, ProjectedOutput, choose_delivery},
    format_epoch_rfc3339,
    output::{OutputPolicy, OutputTransaction},
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
                    "referencePng resource에 base64 data가 없습니다.",
                    false,
                )
            })?;
        let bytes = STANDARD.decode(data.as_bytes()).map_err(|_| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "referencePng resource의 base64가 올바르지 않습니다.",
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
            format!("resource output을 JSON으로 직렬화할 수 없습니다: {error}"),
            false,
        )
    })
}

pub(super) struct DeliveryAttachment {
    projection_key: String,
    created: bool,
}

pub(super) async fn apply_delivery(
    result: &mut Value,
    mode: DeliveryMode,
    artifact_store: &ArtifactStore,
    artifact: &ArtifactLookup,
    outputs: Vec<ProjectedOutput>,
) -> Result<Option<DeliveryAttachment>, DevupError> {
    if outputs.is_empty() || choose_delivery(mode, &outputs)?.inline {
        return Ok(None);
    }
    let projection_key = projection_key(&outputs);
    let (manifests, created) = artifact_store
        .attach_outputs_transactional(&artifact.artifact_id, &projection_key, outputs)
        .await?;
    let result = result.as_object_mut().ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaHandoffInvalid,
            "resource delivery 결과가 JSON object가 아닙니다.",
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
    Ok(Some(DeliveryAttachment {
        projection_key,
        created,
    }))
}

pub(super) async fn rollback_delivery(
    artifact_store: &ArtifactStore,
    artifact: &ArtifactLookup,
    attachment: Option<DeliveryAttachment>,
) {
    if let Some(attachment) = attachment
        && attachment.created
    {
        artifact_store
            .detach_projection(&artifact.artifact_id, &attachment.projection_key)
            .await;
    }
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
    json!({
        "artifactId": artifact.artifact_id,
        "contentHash": artifact.content_hash,
        "cacheHit": artifact.cache_hit,
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
