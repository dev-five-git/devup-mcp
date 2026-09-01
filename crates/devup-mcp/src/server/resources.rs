use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_figma::{DevupError, ErrorCode};
use rmcp::model::{
    ListResourceTemplatesResult, ListResourcesResult, ReadResourceResult, Resource,
    ResourceContents, ResourceTemplate,
};

use super::artifacts::{ArtifactStore, AttachedOutputManifest};

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
            .ok_or_else(not_found)?;
        let parts = path.split('/').collect::<Vec<_>>();
        if parts.len() < 4
            || parts[1] != "outputs"
            || !valid_opaque(parts[0], 43)
            || !valid_opaque(parts[2], 22)
        {
            return Err(not_found());
        }
        match parts.as_slice() {
            [artifact_id, "outputs", output_id, "manifest"] => Ok(Self::Manifest {
                artifact_id: (*artifact_id).to_owned(),
                output_id: (*output_id).to_owned(),
            }),
            [artifact_id, "outputs", output_id, "chunks", index] => {
                let index = index.parse::<usize>().map_err(|_| not_found())?;
                Ok(Self::Chunk {
                    artifact_id: (*artifact_id).to_owned(),
                    output_id: (*output_id).to_owned(),
                    index,
                })
            }
            _ => Err(not_found()),
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
        .map_err(|_| not_found())?
        .unwrap_or(0);
    let manifests = store.output_manifests().await;
    if offset > manifests.len() {
        return Err(not_found());
    }
    let end = offset.saturating_add(LIST_PAGE_SIZE).min(manifests.len());
    let resources = manifests[offset..end]
        .iter()
        .map(manifest_resource)
        .collect();
    let mut result = ListResourcesResult::with_all_items(resources);
    result.next_cursor = (end < manifests.len()).then(|| end.to_string());
    Ok(result)
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
    match ResourceAddress::parse(uri)? {
        ResourceAddress::Manifest {
            artifact_id,
            output_id,
        } => {
            let manifest = store
                .output_manifest(&artifact_id, &output_id)
                .await
                .ok_or_else(not_found)?;
            let text = serde_json::to_string(&manifest).map_err(|_| not_found())?;
            Ok(ReadResourceResult::new(vec![
                ResourceContents::text(text, uri).with_mime_type("application/json"),
            ]))
        }
        ResourceAddress::Chunk {
            artifact_id,
            output_id,
            index,
        } => {
            let manifest = store
                .output_manifest(&artifact_id, &output_id)
                .await
                .ok_or_else(not_found)?;
            let bytes = store
                .read_output_chunk(&artifact_id, &output_id, index)
                .await
                .ok_or_else(not_found)?;
            let contents = if manifest.is_binary {
                ResourceContents::blob(STANDARD.encode(bytes), uri)
                    .with_mime_type(manifest.mime_type)
            } else {
                let text = String::from_utf8(bytes).map_err(|_| not_found())?;
                ResourceContents::text(text, uri).with_mime_type(manifest.mime_type)
            };
            Ok(ReadResourceResult::new(vec![contents]))
        }
    }
}

fn manifest_resource(manifest: &AttachedOutputManifest) -> Resource {
    Resource::new(&manifest.manifest_uri, format!("devup-{}", manifest.name))
        .with_title(format!("Devup {} output", manifest.name))
        .with_description("Generated Devup output manifest")
        .with_mime_type("application/json")
        .with_size(manifest.raw_bytes as u64)
}

fn valid_opaque(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn not_found() -> DevupError {
    DevupError::new(
        ErrorCode::DevupFigmaHandoffExpired,
        "resource not found",
        true,
    )
}
