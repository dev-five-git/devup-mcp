use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use devup_mcp::server::{
    artifacts::{ArtifactLimits, ArtifactRequestKey, ArtifactStore},
    delivery::{
        DeliveryMode, MAX_INLINE_OUTPUT_BYTES, MAX_INLINE_TOTAL_BYTES, ProjectedOutput,
        RESOURCE_CHUNK_BYTES, choose_delivery, choose_delivery_for_result,
    },
    output::{OutputPolicy, OutputTransaction},
    resources::{list_output_resources, read_output_resource, resource_templates},
};
use devup_mcp_figma::{
    CollectedPayload, CollectionRequest, CollectionScope, CollectionStats, FigmaTarget,
    PayloadCompleteness, ResourceScope, Snapshot, SourcePolicy,
};
use rmcp::model::ResourceContents;
use serde_json::json;

#[test]
fn delivery_boundaries_are_deterministic_and_strict() {
    let exactly = ProjectedOutput::text(
        "tsx",
        "text/typescript",
        vec![b'a'; MAX_INLINE_OUTPUT_BYTES],
    );
    assert!(
        !choose_delivery(DeliveryMode::Auto, &[exactly])
            .unwrap()
            .inline
    );

    let comfortably_below_serialized_limit =
        ProjectedOutput::text("tsx", "text/typescript", vec![b'a'; 100_000]);
    assert!(
        choose_delivery(DeliveryMode::Auto, &[comfortably_below_serialized_limit])
            .unwrap()
            .inline
    );

    let over = ProjectedOutput::text(
        "tsx",
        "text/typescript",
        vec![b'a'; MAX_INLINE_OUTPUT_BYTES + 1],
    );
    assert!(
        !choose_delivery(DeliveryMode::Auto, &[over.clone()])
            .unwrap()
            .inline
    );
    assert!(choose_delivery(DeliveryMode::Inline, &[over]).is_ok());

    let aggregate = (0..5)
        .map(|index| {
            ProjectedOutput::text(
                format!("part-{index}"),
                "application/json",
                vec![b'x'; MAX_INLINE_TOTAL_BYTES / 5 + 1],
            )
        })
        .collect::<Vec<_>>();
    assert!(
        !choose_delivery(DeliveryMode::Auto, &aggregate)
            .unwrap()
            .inline
    );
    assert!(choose_delivery(DeliveryMode::Inline, &aggregate).is_err());
    assert!(!choose_delivery(DeliveryMode::Resource, &[]).unwrap().inline);

    assert_eq!("auto".parse::<DeliveryMode>().unwrap(), DeliveryMode::Auto);
    assert_eq!(
        "inline".parse::<DeliveryMode>().unwrap(),
        DeliveryMode::Inline
    );
    assert_eq!(
        "resource".parse::<DeliveryMode>().unwrap(),
        DeliveryMode::Resource
    );
    assert!("other".parse::<DeliveryMode>().is_err());
}

#[test]
fn auto_delivery_accounts_for_json_escaping_base64_and_tool_result_duplication() {
    let quote_heavy = "\"".repeat(100_000);
    let text = ProjectedOutput::text("tsx", "text/typescript", quote_heavy.as_bytes().to_vec());
    assert!(text.bytes.len() < MAX_INLINE_OUTPUT_BYTES);
    assert!(
        !choose_delivery_for_result(DeliveryMode::Auto, &json!({"tsx": quote_heavy}), &[text],)
            .unwrap()
            .inline
    );

    let total_quote_heavy = "\"".repeat(300_000);
    let total_text = ProjectedOutput::text(
        "tsx",
        "text/typescript",
        total_quote_heavy.as_bytes().to_vec(),
    );
    assert!(
        choose_delivery_for_result(
            DeliveryMode::Inline,
            &json!({"tsx": total_quote_heavy}),
            &[total_text],
        )
        .is_err(),
        "explicit inline must reject a response whose serialized wire form exceeds 1 MiB"
    );

    let below = vec![0x5a; 90_000];
    let below_base64 = STANDARD.encode(&below);
    assert!(
        choose_delivery_for_result(
            DeliveryMode::Auto,
            &json!({"referencePng": {
                "mimeType": "image/png",
                "dataBase64": below_base64,
                "byteLength": below.len(),
                "sha256": "0".repeat(64)
            }}),
            &[ProjectedOutput::binary("reference.png", "image/png", below)],
        )
        .unwrap()
        .inline
    );

    let above = vec![0x5a; 100_000];
    let above_base64 = STANDARD.encode(&above);
    assert!(
        !choose_delivery_for_result(
            DeliveryMode::Auto,
            &json!({"referencePng": {
                "mimeType": "image/png",
                "dataBase64": above_base64,
                "byteLength": above.len(),
                "sha256": "0".repeat(64)
            }}),
            &[ProjectedOutput::binary("reference.png", "image/png", above)],
        )
        .unwrap()
        .inline
    );
}

#[tokio::test]
async fn attached_outputs_are_bounded_hashed_and_share_artifact_lifetime() -> anyhow::Result<()> {
    let store = ArtifactStore::with_limits(ArtifactLimits {
        ttl: Duration::from_secs(60),
        max_entries: 2,
        max_entry_bytes: 4 * 1024 * 1024,
        max_total_bytes: 8 * 1024 * 1024,
    });
    let request = request();
    let artifact = store
        .insert(
            ArtifactRequestKey::from_collection(&request, SourcePolicy::Direct),
            payload(),
        )
        .await?;
    let acquisition_hash = artifact.content_hash.clone();
    let bytes = vec![0x5a; RESOURCE_CHUNK_BYTES * 2 + 7];

    let attached = store
        .attach_outputs(
            &artifact.artifact_id,
            "projection-key",
            vec![
                ProjectedOutput::text("tsx", "text/typescript", b"hello".to_vec()),
                ProjectedOutput::binary("preview", "image/png", bytes.clone()),
            ],
        )
        .await?;

    assert_eq!(attached.len(), 2);
    assert_eq!(
        store.get(&artifact.artifact_id).await.unwrap().content_hash,
        acquisition_hash
    );
    let preview = attached.iter().find(|item| item.name == "preview").unwrap();
    assert_eq!(preview.raw_bytes, bytes.len());
    assert_eq!(preview.chunk_count, 3);
    assert!(!preview.output_id.contains(&request.target.file_key));
    let mut restored = Vec::new();
    for index in 0..preview.chunk_count {
        let chunk = store
            .read_output_chunk(&artifact.artifact_id, &preview.output_id, index)
            .await
            .expect("attached chunk");
        assert!(chunk.len() <= RESOURCE_CHUNK_BYTES);
        restored.extend_from_slice(&chunk);
    }
    assert_eq!(restored, bytes);

    let reused = store
        .attach_outputs(
            &artifact.artifact_id,
            "projection-key",
            vec![ProjectedOutput::text(
                "ignored",
                "text/plain",
                b"different".to_vec(),
            )],
        )
        .await?;
    assert_eq!(reused, attached);
    assert!(
        store
            .detach_projection(&artifact.artifact_id, "projection-key")
            .await
    );
    assert!(
        store
            .output_manifest(&artifact.artifact_id, &preview.output_id)
            .await
            .is_none()
    );
    assert!(
        !store
            .detach_projection(&artifact.artifact_id, "projection-key")
            .await
    );
    Ok(())
}

#[tokio::test]
async fn resource_protocol_lists_manifests_and_round_trips_chunks() -> anyhow::Result<()> {
    let store = ArtifactStore::with_limits(ArtifactLimits {
        ttl: Duration::from_secs(60),
        max_entries: 2,
        max_entry_bytes: 4 * 1024 * 1024,
        max_total_bytes: 8 * 1024 * 1024,
    });
    let artifact = store
        .insert(
            ArtifactRequestKey::from_collection(&request(), SourcePolicy::Direct),
            payload(),
        )
        .await?;
    let original = "가나다".repeat(100_000).into_bytes();
    let attached = store
        .attach_outputs(
            &artifact.artifact_id,
            "unicode",
            vec![ProjectedOutput::text(
                "tsx",
                "text/typescript",
                original.clone(),
            )],
        )
        .await?;
    let manifest = &attached[0];

    let listed = list_output_resources(&store, None).await?;
    assert_eq!(listed.resources.len(), 1);
    assert_eq!(listed.resources[0].uri, manifest.manifest_uri);
    assert_eq!(
        listed.resources[0].mime_type.as_deref(),
        Some("application/json")
    );
    assert_eq!(listed.resources[0].size, None);
    assert_eq!(
        listed.resources[0]
            .meta
            .as_ref()
            .and_then(|meta| meta.0.get("payloadMimeType"))
            .and_then(serde_json::Value::as_str),
        Some("text/typescript")
    );
    assert!(listed.next_cursor.is_none());
    assert_eq!(resource_templates().resource_templates.len(), 2);

    let manifest_read = read_output_resource(&store, &manifest.manifest_uri).await?;
    let ResourceContents::TextResourceContents { text, .. } = &manifest_read.contents[0] else {
        panic!("manifest must be JSON text")
    };
    let value: serde_json::Value = serde_json::from_str(text.as_str())?;
    assert_eq!(value["sha256"], manifest.sha256);
    assert_eq!(value["rawBytes"], original.len());

    let mut restored = Vec::new();
    for index in 0..manifest.chunk_count {
        let uri = format!(
            "devup://artifact/{}/outputs/{}/chunks/{index}",
            artifact.artifact_id, manifest.output_id
        );
        let read = read_output_resource(&store, &uri).await?;
        let ResourceContents::TextResourceContents { text, .. } = &read.contents[0] else {
            panic!("text chunk expected")
        };
        restored.extend_from_slice(text.as_bytes());
    }
    assert_eq!(restored, original);
    assert!(
        read_output_resource(
            &store,
            "devup://artifact/not-an-id/outputs/not-an-id/chunks/0"
        )
        .await
        .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn reserved_resources_stay_invisible_until_publication() -> anyhow::Result<()> {
    let store = ArtifactStore::with_limits(ArtifactLimits {
        ttl: Duration::from_secs(60),
        max_entries: 2,
        max_entry_bytes: 4 * 1024 * 1024,
        max_total_bytes: 8 * 1024 * 1024,
    });
    let artifact = store
        .insert(
            ArtifactRequestKey::from_collection(&request(), SourcePolicy::Direct),
            payload(),
        )
        .await?;
    let root = unique_temp_dir("combined-publication")?;
    let policy = OutputPolicy::from_roots(vec![root.clone()])?;
    let mut transaction = OutputTransaction::new();
    transaction.stage("tsx", policy.resolve("Component.tsx")?, b"reserved")?;
    let reservation = store
        .reserve_outputs(
            &artifact.artifact_id,
            "invisible",
            vec![ProjectedOutput::text(
                "tsx",
                "text/typescript",
                b"reserved".to_vec(),
            )],
        )
        .await?;
    let manifest_uri = reservation.manifests()[0].manifest_uri.clone();
    let listing_store = store.clone();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let mut listing = tokio::spawn(async move {
        let _ = started_tx.send(());
        list_output_resources(&listing_store, None).await
    });
    let reading_store = store.clone();
    let reading_uri = manifest_uri.clone();
    let (reading_started_tx, reading_started_rx) = tokio::sync::oneshot::channel();
    let mut reading = tokio::spawn(async move {
        let _ = reading_started_tx.send(());
        read_output_resource(&reading_store, &reading_uri).await
    });

    started_rx.await?;
    reading_started_rx.await?;
    assert!(
        tokio::time::timeout(Duration::from_millis(25), &mut listing)
            .await
            .is_err(),
        "resources/list must not observe an in-between reservation"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), &mut reading)
            .await
            .is_err(),
        "resources/read must not observe an in-between reservation"
    );
    transaction.commit()?;
    reservation.commit();

    assert_eq!(listing.await??.resources.len(), 1);
    assert!(reading.await?.is_ok());
    assert_eq!(fs::read(root.join("Component.tsx"))?, b"reserved");

    drop(policy);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[tokio::test]
async fn failed_file_commit_does_not_publish_or_evict_lru_resources() -> anyhow::Result<()> {
    let sample_bytes = serde_json::to_vec(&payload())?.len();
    let store = ArtifactStore::with_limits(ArtifactLimits {
        ttl: Duration::from_secs(60),
        max_entries: 2,
        max_entry_bytes: sample_bytes * 2,
        max_total_bytes: sample_bytes * 2 + 32,
    });
    let target = store
        .insert(
            ArtifactRequestKey::from_collection(&request(), SourcePolicy::Direct),
            payload(),
        )
        .await?;
    let mut unrelated_request = request();
    unrelated_request.target.file_key = "UnrelatedFile".to_owned();
    let mut unrelated_payload = payload();
    unrelated_payload.target.file_key = "UnrelatedFile".to_owned();
    unrelated_payload.snapshot.file_key = "UnrelatedFile".to_owned();
    let unrelated = store
        .insert(
            ArtifactRequestKey::from_collection(&unrelated_request, SourcePolicy::Direct),
            unrelated_payload,
        )
        .await?;
    let reservation = store
        .reserve_outputs(
            &target.artifact_id,
            "will-roll-back",
            vec![ProjectedOutput::text(
                "tsx",
                "text/typescript",
                vec![b'x'; 128],
            )],
        )
        .await?;
    let manifest_uri = reservation.manifests()[0].manifest_uri.clone();

    let root = unique_temp_dir("combined-rollback")?;
    let policy = OutputPolicy::from_roots(vec![root.clone()])?;
    let mut transaction = OutputTransaction::new();
    transaction.stage("tsx", policy.resolve("Component.tsx")?, b"new")?;
    fs::create_dir(root.join("Component.tsx"))?;
    assert!(transaction.commit().is_err());
    reservation.rollback();

    assert!(store.get(&unrelated.artifact_id).await.is_some());
    assert!(read_output_resource(&store, &manifest_uri).await.is_err());
    assert!(
        list_output_resources(&store, None)
            .await?
            .resources
            .is_empty()
    );

    drop(policy);
    fs::remove_dir_all(root)?;
    Ok(())
}

fn unique_temp_dir(label: &str) -> anyhow::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "devup-mcp-{label}-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn request() -> CollectionRequest {
    let mut request = CollectionRequest::new(
        FigmaTarget {
            file_key: "FileKey123".to_owned(),
            node_id: Some("1:1".to_owned()),
            branch_key: None,
        },
        CollectionScope::Node,
    );
    request.resource_scope = ResourceScope::Used;
    request
}

fn payload() -> CollectedPayload {
    CollectedPayload {
        target: request().target,
        scope: CollectionScope::Node,
        metadata: json!({}),
        snapshot: Snapshot {
            file_key: "FileKey123".to_owned(),
            version: None,
            roots: vec!["1:1".to_owned()],
            nodes: BTreeMap::new(),
            diagnostics: Vec::new(),
        },
        variables: None,
        styles: None,
        completeness: PayloadCompleteness::ResolvedValuesOnly,
        source_version: None,
        stats: CollectionStats::default(),
        assets: Vec::new(),
        reference_png: None,
    }
}
