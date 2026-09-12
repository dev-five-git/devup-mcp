use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_figma::{DevupError, ErrorCode};
use rmcp::model::{
    ListResourceTemplatesResult, ListResourcesResult, MetaObject, ReadResourceResult, Resource,
    ResourceContents, ResourceTemplate,
};
use serde_json::Value;

use super::artifacts::{ArtifactStore, AttachedOutputManifest};
use super::guide;

const LIST_PAGE_SIZE: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResourceAddress {
    Manifest {
        artifact_id: String,
        output_id: String,
    },
    Chunk {
        artifact_id: String,
        output_id: String,
        index: usize,
    },
}

impl ResourceAddress {
    fn parse(uri: &str) -> Result<Self, DevupError> {
        let path = uri
            .strip_prefix("devup://artifact/")
            .ok_or_else(invalid_request)?;
        let parts = path.split('/').collect::<Vec<_>>();
        if parts.len() < 4
            || parts[1] != "outputs"
            || !valid_opaque(parts[0], 43)
            || !valid_opaque(parts[2], 22)
        {
            return Err(invalid_request());
        }
        match parts.as_slice() {
            [artifact_id, "outputs", output_id, "manifest"] => Ok(Self::Manifest {
                artifact_id: (*artifact_id).to_owned(),
                output_id: (*output_id).to_owned(),
            }),
            [artifact_id, "outputs", output_id, "chunks", index] => {
                let index = index.parse::<usize>().map_err(|_| invalid_request())?;
                Ok(Self::Chunk {
                    artifact_id: (*artifact_id).to_owned(),
                    output_id: (*output_id).to_owned(),
                    index,
                })
            }
            _ => Err(invalid_request()),
        }
    }
}

pub async fn list_output_resources(
    store: &ArtifactStore,
    cursor: Option<&str>,
) -> Result<ListResourcesResult, DevupError> {
    let offset = cursor
        .map(str::parse::<usize>)
        .transpose()
        .map_err(|_| invalid_request())?
        .unwrap_or(0);
    // Generated outputs come first and the static guide last. The order is not
    // cosmetic: a caller already holding a manifest link indexes into this list,
    // and `instructions` names the guide URI outright, so the guide loses
    // nothing by sitting at the end while the existing contract keeps its
    // positions.
    let mut listed = store
        .output_manifests()
        .await
        .iter()
        .map(manifest_resource)
        .collect::<Vec<_>>();
    listed.push(guide_resource());
    if offset > listed.len() {
        return Err(invalid_request());
    }
    let end = offset.saturating_add(LIST_PAGE_SIZE).min(listed.len());
    let mut result = ListResourcesResult::with_all_items(listed[offset..end].to_vec());
    result.next_cursor = (end < listed.len()).then(|| end.to_string());
    Ok(result)
}

/// The usage guide is a fixed resource rather than a template: there is one of
/// it, at one URI, and a template would imply parameters it does not have.
fn guide_resource() -> Resource {
    Resource::new(guide::GUIDE_URI, guide::GUIDE_NAME)
        .with_title(guide::GUIDE_TITLE)
        .with_description(guide::GUIDE_DESCRIPTION)
        .with_mime_type(guide::GUIDE_MIME_TYPE)
}

pub fn resource_templates() -> ListResourceTemplatesResult {
    ListResourceTemplatesResult::with_all_items(vec![
        ResourceTemplate::new(
            "devup://artifact/{artifactId}/outputs/{outputId}/manifest",
            "devup-output-manifest",
        )
        .with_title("Devup output manifest")
        .with_description("Bounded metadata for a generated Devup output")
        .with_mime_type("application/json"),
        ResourceTemplate::new(
            "devup://artifact/{artifactId}/outputs/{outputId}/chunks/{index}",
            "devup-output-chunk",
        )
        .with_title("Devup output chunk")
        .with_description("A bounded text or base64 binary chunk of a generated output"),
    ])
}

pub async fn read_output_resource(
    store: &ArtifactStore,
    uri: &str,
) -> Result<ReadResourceResult, DevupError> {
    // The guide is answered before the artifact address is parsed: it is not an
    // artifact, it never expires, and it must stay readable in a session that
    // has produced no outputs at all.
    if uri == guide::GUIDE_URI {
        return Ok(ReadResourceResult::new(vec![
            ResourceContents::text(guide::GUIDE, uri).with_mime_type(guide::GUIDE_MIME_TYPE),
        ]));
    }
    match ResourceAddress::parse(uri)? {
        ResourceAddress::Manifest {
            artifact_id,
            output_id,
        } => {
            let (manifest, _) = store
                .read_resource_output(&artifact_id, &output_id, None)
                .await?;
            let text = serde_json::to_string(&manifest).map_err(|error| {
                invalid_content(&format!("Cannot serialize the resource manifest: {error}"))
            })?;
            Ok(ReadResourceResult::new(vec![
                ResourceContents::text(text, uri).with_mime_type("application/json"),
            ]))
        }
        ResourceAddress::Chunk {
            artifact_id,
            output_id,
            index,
        } => {
            let (manifest, bytes) = store
                .read_resource_output(&artifact_id, &output_id, Some(index))
                .await?;
            let contents = if manifest.is_binary {
                ResourceContents::blob(STANDARD.encode(bytes), uri)
                    .with_mime_type(manifest.mime_type)
            } else {
                let text = String::from_utf8(bytes)
                    .map_err(|_| invalid_content("The text resource is not UTF-8."))?;
                ResourceContents::text(text, uri).with_mime_type(manifest.mime_type)
            };
            Ok(ReadResourceResult::new(vec![contents]))
        }
    }
}

fn manifest_resource(manifest: &AttachedOutputManifest) -> Resource {
    let mut meta = MetaObject::new();
    meta.0.insert(
        "payloadMimeType".to_owned(),
        Value::from(manifest.mime_type.clone()),
    );
    meta.0.insert(
        "payloadBytes".to_owned(),
        Value::from(manifest.raw_bytes as u64),
    );
    meta.0.insert(
        "payloadSha256".to_owned(),
        Value::from(manifest.sha256.clone()),
    );
    Resource::new(&manifest.manifest_uri, format!("devup-{}", manifest.name))
        .with_title(format!("Devup {} output", manifest.name))
        .with_description("Generated Devup output manifest")
        .with_mime_type("application/json")
        .with_meta(meta)
}

fn invalid_content(message: &str) -> DevupError {
    DevupError::new(ErrorCode::DevupFigmaHandoffInvalid, message, false)
}

fn valid_opaque(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn invalid_request() -> DevupError {
    DevupError::new(
        ErrorCode::DevupFigmaHandoffInvalid,
        "The resource URI or list cursor is invalid.",
        false,
    )
}

#[cfg(test)]
mod w3_tests {
    use super::*;
    use crate::server::{
        artifacts::{
            ArtifactLimits, ArtifactRequestKey,
            w3_tests::{Clock, payload, request},
        },
        delivery::{ProjectedOutput, RESOURCE_CHUNK_BYTES},
    };
    use std::{
        sync::{Arc, atomic::Ordering},
        time::Duration,
    };

    #[tokio::test]
    async fn w3_manifest_links_reconstruct_text_and_binary_without_uri_assembly() {
        for (binary, repeats, expected_chunks) in [
            (false, 0, 1),
            (false, 2, 1),
            (true, 2, 1),
            (false, RESOURCE_CHUNK_BYTES / 3 + 7, 2),
            (true, RESOURCE_CHUNK_BYTES + 7, 2),
        ] {
            let store = ArtifactStore::default();
            let artifact = store
                .insert(ArtifactRequestKey::from_collection(&request()), payload())
                .await
                .unwrap();
            let bytes = if binary {
                vec![0xff; repeats]
            } else {
                "가".repeat(repeats).into_bytes()
            };
            let output = if binary {
                ProjectedOutput::binary("asset", "application/octet-stream", bytes.clone())
            } else {
                ProjectedOutput::text("tsx", "text/plain", bytes.clone())
            };
            store
                .attach_outputs(&artifact.artifact_id, "projection", vec![output])
                .await
                .unwrap();
            let listed = list_output_resources(&store, None).await.unwrap();
            let result = read_output_resource(&store, &listed.resources[0].uri)
                .await
                .unwrap();
            let ResourceContents::TextResourceContents { text, .. } = &result.contents[0] else {
                panic!("manifest must be JSON text")
            };
            let manifest: Value = serde_json::from_str(text).unwrap();
            let uris = manifest["chunkUris"]
                .as_array()
                .expect("manifest must advertise content URIs");
            assert_eq!(uris.len(), expected_chunks);
            let mut reconstructed = Vec::new();
            for uri in uris {
                let chunk = read_output_resource(&store, uri.as_str().unwrap())
                    .await
                    .unwrap();
                match &chunk.contents[0] {
                    ResourceContents::TextResourceContents { text, .. } => {
                        reconstructed.extend_from_slice(text.as_bytes())
                    }
                    ResourceContents::BlobResourceContents { blob, .. } => {
                        reconstructed.extend(STANDARD.decode(blob).unwrap())
                    }
                    _ => panic!("unexpected resource content"),
                }
            }
            assert_eq!(reconstructed, bytes);
        }
    }

    #[tokio::test]
    async fn w3_invalid_missing_and_expired_resources_are_distinct() {
        let clock = Arc::new(Clock::default());
        let store = ArtifactStore::with_clock(
            clock.clone(),
            ArtifactLimits {
                ttl: Duration::from_secs(10),
                ..ArtifactLimits::default()
            },
        );
        let artifact = store
            .insert(ArtifactRequestKey::from_collection(&request()), payload())
            .await
            .unwrap();
        let manifests = store
            .attach_outputs(
                &artifact.artifact_id,
                "projection",
                vec![ProjectedOutput::text(
                    "tsx",
                    "text/plain",
                    b"hello".to_vec(),
                )],
            )
            .await
            .unwrap();
        let invalid = read_output_resource(&store, "bad-uri").await.unwrap_err();
        assert_eq!(invalid.code, ErrorCode::DevupFigmaHandoffInvalid);
        assert!(!invalid.retryable);
        let missing_uri = format!(
            "devup://artifact/{}/outputs/{}/manifest",
            artifact.artifact_id,
            "z".repeat(22)
        );
        let missing = read_output_resource(&store, &missing_uri)
            .await
            .unwrap_err();
        assert_eq!(missing.details["resourceKind"], "output");
        let unknown = format!(
            "devup://artifact/{}/outputs/{}/manifest",
            "z".repeat(43),
            "z".repeat(22)
        );
        let absent = read_output_resource(&store, &unknown).await.unwrap_err();
        assert_eq!(absent.details["resourceKind"], "artifact");
        let invalid_chunk = format!(
            "devup://artifact/{}/outputs/{}/chunks/9",
            artifact.artifact_id, manifests[0].output_id
        );
        let absent_chunk = read_output_resource(&store, &invalid_chunk)
            .await
            .unwrap_err();
        assert_eq!(absent_chunk.details["resourceKind"], "chunk");
        clock.0.store(10, Ordering::SeqCst);
        let expired = read_output_resource(&store, &manifests[0].manifest_uri)
            .await
            .unwrap_err();
        assert_ne!(missing.message, expired.message);
        assert_eq!(expired.code, ErrorCode::DevupFigmaHandoffExpired);
    }
}
