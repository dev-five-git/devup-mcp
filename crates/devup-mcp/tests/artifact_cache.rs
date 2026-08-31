use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use devup_mcp::server::artifacts::{
    ArtifactClock, ArtifactLimits, ArtifactRequestKey, ArtifactStore,
};
use devup_mcp_figma::{
    CollectedPayload, CollectionRequest, CollectionScope, CollectionStats, FigmaTarget,
    PayloadCompleteness, ResourceScope, Snapshot, SourcePolicy,
};
use serde_json::json;

#[derive(Debug, Default)]
struct FakeClock(AtomicU64);

impl FakeClock {
    fn set(&self, seconds: u64) {
        self.0.store(seconds, Ordering::SeqCst);
    }
}

impl ArtifactClock for FakeClock {
    fn now_epoch_seconds(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

fn limits() -> ArtifactLimits {
    ArtifactLimits {
        ttl: Duration::from_secs(10),
        max_entries: 2,
        max_entry_bytes: 64 * 1024,
        max_total_bytes: 64 * 1024,
    }
}

fn request(file_key: &str, node_id: &str) -> CollectionRequest {
    let mut request = CollectionRequest::new(
        FigmaTarget {
            file_key: file_key.to_owned(),
            node_id: Some(node_id.to_owned()),
            branch_key: None,
        },
        CollectionScope::Node,
    );
    request.resource_scope = ResourceScope::Used;
    request
}

fn payload(file_key: &str, node_id: &str, marker: &str) -> CollectedPayload {
    CollectedPayload {
        target: FigmaTarget {
            file_key: file_key.to_owned(),
            node_id: Some(node_id.to_owned()),
            branch_key: None,
        },
        scope: CollectionScope::Node,
        metadata: json!({"marker": marker}),
        snapshot: Snapshot {
            file_key: file_key.to_owned(),
            version: Some("1".to_owned()),
            roots: vec![node_id.to_owned()],
            nodes: BTreeMap::new(),
            diagnostics: Vec::new(),
        },
        variables: None,
        styles: None,
        completeness: PayloadCompleteness::ResolvedValuesOnly,
        source_version: Some("1".to_owned()),
        stats: CollectionStats::default(),
        assets: Vec::new(),
    }
}

#[tokio::test]
async fn reuses_same_request_until_expiry_and_refresh_bypasses_it() -> anyhow::Result<()> {
    let clock = Arc::new(FakeClock::default());
    clock.set(100);
    let store = ArtifactStore::with_clock(clock.clone(), limits());
    let key =
        ArtifactRequestKey::from_collection(&request("file-one", "1:2"), SourcePolicy::Direct);
    let calls = AtomicUsize::new(0);

    let first = store
        .get_or_acquire(key.clone(), false, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(payload("file-one", "1:2", "first"))
        })
        .await?;
    let hit = store
        .get_or_acquire(key.clone(), false, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(payload("file-one", "1:2", "unexpected"))
        })
        .await?;

    assert_eq!(first.artifact_id, hit.artifact_id);
    assert!(!first.cache_hit);
    assert!(hit.cache_hit);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let refreshed = store
        .get_or_acquire(key.clone(), true, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(payload("file-one", "1:2", "refresh"))
        })
        .await?;
    assert_ne!(refreshed.artifact_id, first.artifact_id);
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    clock.set(111);
    let expired = store
        .get_or_acquire(key, false, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(payload("file-one", "1:2", "expired"))
        })
        .await?;
    assert_ne!(expired.artifact_id, refreshed.artifact_id);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    Ok(())
}

#[tokio::test]
async fn evicts_lru_entries_by_count_and_aggregate_bytes() -> anyhow::Result<()> {
    let clock = Arc::new(FakeClock::default());
    clock.set(100);
    let mut bounded = limits();
    let sample_bytes = serde_json::to_vec(&payload("file-one", "1:1", "a"))?.len();
    bounded.max_total_bytes = sample_bytes * 2 + 16;
    let store = ArtifactStore::with_clock(clock, bounded);

    let one = store
        .insert(
            ArtifactRequestKey::from_collection(&request("file-one", "1:1"), SourcePolicy::Direct),
            payload("file-one", "1:1", "a"),
        )
        .await?;
    let two = store
        .insert(
            ArtifactRequestKey::from_collection(&request("file-two", "2:2"), SourcePolicy::Direct),
            payload("file-two", "2:2", "b"),
        )
        .await?;
    store.get(&one.artifact_id).await.expect("touch first");
    let three = store
        .insert(
            ArtifactRequestKey::from_collection(
                &request("file-three", "3:3"),
                SourcePolicy::Direct,
            ),
            payload("file-three", "3:3", "c"),
        )
        .await?;

    assert!(store.get(&one.artifact_id).await.is_some());
    assert!(store.get(&two.artifact_id).await.is_none());
    assert!(store.get(&three.artifact_id).await.is_some());
    let stats = store.stats().await;
    assert_eq!(stats.entry_count, 2);
    assert!(stats.total_bytes <= bounded.max_total_bytes);
    Ok(())
}

#[tokio::test]
async fn concurrent_same_key_requests_share_one_acquisition() -> anyhow::Result<()> {
    let store = ArtifactStore::with_limits(limits());
    let key =
        ArtifactRequestKey::from_collection(&request("file-one", "1:2"), SourcePolicy::Direct);
    let calls = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(tokio::sync::Barrier::new(3));

    let spawn = |store: ArtifactStore| {
        let key = key.clone();
        let calls = calls.clone();
        let barrier = barrier.clone();
        tokio::spawn(async move {
            barrier.wait().await;
            store
                .get_or_acquire(key, false, || async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    tokio::task::yield_now().await;
                    Ok(payload("file-one", "1:2", "shared"))
                })
                .await
        })
    };
    let left = spawn(store.clone());
    let right = spawn(store);
    barrier.wait().await;
    let (left, right) = tokio::try_join!(left, right)?;
    let left = left?;
    let right = right?;

    assert_eq!(left.artifact_id, right.artifact_id);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(left.cache_hit ^ right.cache_hit);
    Ok(())
}

#[tokio::test]
async fn safe_keys_and_stats_never_serialize_url_credentials_or_payloads() -> anyhow::Result<()> {
    let target = FigmaTarget::parse(
        "https://www.figma.com/design/file-one/Example?node-id=1-2&access_token=super-secret",
    )?;
    let key = ArtifactRequestKey::from_collection(
        &CollectionRequest::new(target, CollectionScope::Node),
        SourcePolicy::Auto,
    );
    let key_json = serde_json::to_string(&key)?;
    assert!(!key_json.contains("super-secret"));
    assert!(!key_json.contains("access_token"));

    let store = ArtifactStore::with_limits(limits());
    store
        .insert(key, payload("file-one", "1:2", "private payload value"))
        .await?;
    let stats_json = serde_json::to_string(&store.stats().await)?;
    assert!(!stats_json.contains("private payload value"));
    assert!(!stats_json.contains("super-secret"));
    Ok(())
}
