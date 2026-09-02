use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use devup_mcp::server::handoff::{
    Clock, HandoffLimits, HandoffStep, HandoffStore, PendingOperation,
};
use devup_mcp_figma::{
    CollectionRequest, CollectionScope, CollectorSession, ErrorCode, FigmaTarget,
};
use serde_json::{Value, json};

#[derive(Debug, Default)]
struct FakeClock(AtomicU64);

impl FakeClock {
    fn advance(&self, seconds: u64) {
        self.0.fetch_add(seconds, Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn now_epoch_seconds(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

fn collector() -> CollectorSession {
    let target =
        FigmaTarget::parse("https://www.figma.com/design/FileKey123/Fixture?node-id=1-2").unwrap();
    CollectorSession::new(CollectionRequest::new(target, CollectionScope::Node))
}

fn metadata_result() -> Value {
    json!({
        "structuredContent": {
            "devupMetadata": {
                "fileKey": "FileKey123",
                "version": "v1",
                "rootId": "1:2",
                "nodes": [{
                    "id": "1:2",
                    "type": "FRAME",
                    "childrenIds": [],
                    "descendantCount": 1
                }]
            }
        }
    })
}

fn snapshot_result() -> Value {
    json!({
        "fileKey": "FileKey123",
        "version": "v1",
        "rootIds": ["1:2"],
        "nodes": [{
            "id": "1:2",
            "type": "FRAME",
            "fields": {"name": "Synthetic", "childrenIds": []},
            "extra": {},
            "fieldErrors": {}
        }]
    })
}

fn limits() -> HandoffLimits {
    HandoffLimits {
        ttl: Duration::from_secs(600),
        max_sessions: 8,
        max_result_bytes: 1024,
        max_total_bytes: 4096,
    }
}

#[tokio::test]
async fn expires_sessions_after_ten_minutes() {
    let clock = Arc::new(FakeClock::default());
    let store = HandoffStore::with_clock(clock.clone(), limits());
    let id = store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();

    clock.advance(601);
    let error = store.next(&id).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupFigmaHandoffExpired);
    assert_eq!(error.details["reason"], "expired");
}

#[tokio::test]
async fn expired_session_remains_distinguishable_after_pruning() {
    let clock = Arc::new(FakeClock::default());
    let store = HandoffStore::with_clock(clock.clone(), limits());
    let expired_id = store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();

    clock.advance(601);
    store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();
    let error = store.next(&expired_id).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupFigmaHandoffExpired);
    assert!(error.retryable);
    assert_eq!(error.details["reason"], "expired");
}

#[tokio::test]
async fn enforces_session_and_payload_memory_limits() {
    let clock = Arc::new(FakeClock::default());
    let store = HandoffStore::with_clock(clock, limits());
    for _ in 0..8 {
        store
            .begin(PendingOperation::Collect, collector())
            .await
            .unwrap();
    }
    let error = store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupFigmaResponseTooLarge);

    let strict = HandoffStore::with_limits(HandoffLimits {
        max_result_bytes: 32,
        max_total_bytes: 64,
        ..limits()
    });
    let id = strict
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();
    let HandoffStep::NeedsFigma { calls, .. } = strict.next(&id).await.unwrap() else {
        panic!()
    };
    let error = strict
        .accept(&id, &calls[0].call_id, json!({"large": "x".repeat(80)}))
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupFigmaResponseTooLarge);
    let removed = strict.next(&id).await.unwrap_err();
    assert_eq!(removed.code, ErrorCode::DevupFigmaHandoffInvalid);
}

#[tokio::test]
async fn uses_opaque_ids_and_consumes_each_call_once() {
    let store = HandoffStore::with_limits(limits());
    let id = store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();
    assert_eq!(id.len(), 43);
    assert!(
        id.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    );

    let HandoffStep::NeedsFigma { calls, .. } = store.next(&id).await.unwrap() else {
        panic!()
    };
    assert_eq!(calls[0].call_id.len(), 43);
    store
        .accept(&id, &calls[0].call_id, metadata_result())
        .await
        .unwrap();
    let replay = store
        .accept(&id, &calls[0].call_id, metadata_result())
        .await
        .unwrap_err();
    assert_eq!(replay.code, ErrorCode::DevupFigmaHandoffInvalid);
    assert_eq!(replay.details["reason"], "consumed");
}

#[tokio::test]
async fn accepted_results_renew_the_lease_but_polling_does_not() {
    let clock = Arc::new(FakeClock::default());
    let store = HandoffStore::with_clock(clock.clone(), limits());
    let id = store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();
    let HandoffStep::NeedsFigma {
        calls,
        expires_at_epoch_seconds,
        ..
    } = store.next(&id).await.unwrap()
    else {
        panic!()
    };
    assert_eq!(expires_at_epoch_seconds, 600);

    clock.advance(590);
    store
        .accept(&id, &calls[0].call_id, metadata_result())
        .await
        .unwrap();
    let HandoffStep::NeedsFigma {
        expires_at_epoch_seconds,
        ..
    } = store.next(&id).await.unwrap()
    else {
        panic!()
    };
    assert_eq!(expires_at_epoch_seconds, 1_190);

    clock.advance(590);
    let HandoffStep::NeedsFigma {
        expires_at_epoch_seconds,
        ..
    } = store.next(&id).await.unwrap()
    else {
        panic!()
    };
    assert_eq!(expires_at_epoch_seconds, 1_190);
    clock.advance(11);
    assert_eq!(
        store.next(&id).await.unwrap_err().code,
        ErrorCode::DevupFigmaHandoffExpired
    );
}

#[tokio::test]
async fn invalid_call_id_does_not_destroy_the_session() {
    let store = HandoffStore::with_limits(limits());
    let id = store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();
    let HandoffStep::NeedsFigma { calls, .. } = store.next(&id).await.unwrap() else {
        panic!()
    };

    let error = store
        .accept(&id, "unknown-call-id", metadata_result())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupFigmaHandoffInvalid);

    store
        .accept(&id, &calls[0].call_id, metadata_result())
        .await
        .unwrap();
    let HandoffStep::NeedsFigma { calls, .. } = store.next(&id).await.unwrap() else {
        panic!()
    };
    assert_eq!(calls[0].tool, "use_figma");
}

#[tokio::test]
async fn collector_rejection_keeps_the_call_pending_for_a_corrected_result() {
    let store = HandoffStore::with_limits(limits());
    let id = store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();
    let HandoffStep::NeedsFigma { calls, .. } = store.next(&id).await.unwrap() else {
        panic!()
    };

    store
        .accept(&id, &calls[0].call_id, json!({"malformed": true}))
        .await
        .unwrap_err();
    store
        .accept(&id, &calls[0].call_id, metadata_result())
        .await
        .unwrap();
    let HandoffStep::NeedsFigma { calls, .. } = store.next(&id).await.unwrap() else {
        panic!()
    };
    assert_eq!(calls[0].tool, "use_figma");
}

#[tokio::test]
async fn removes_the_session_after_collection_completes() {
    let store = HandoffStore::with_limits(limits());
    let id = store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();
    let HandoffStep::NeedsFigma { calls, .. } = store.next(&id).await.unwrap() else {
        panic!()
    };
    store
        .accept(&id, &calls[0].call_id, metadata_result())
        .await
        .unwrap();
    let HandoffStep::NeedsFigma { calls, .. } = store.next(&id).await.unwrap() else {
        panic!()
    };
    store
        .accept(&id, &calls[0].call_id, snapshot_result())
        .await
        .unwrap();
    let HandoffStep::Complete { parts, operation } = store.next(&id).await.unwrap() else {
        panic!()
    };
    assert_eq!(operation, PendingOperation::Collect);
    assert_eq!(parts.snapshot_chunks.len(), 1);

    let removed = store.next(&id).await.unwrap_err();
    assert_eq!(removed.code, ErrorCode::DevupFigmaHandoffInvalid);
}

#[tokio::test]
async fn stringified_tool_results_are_normalized_at_the_handoff_boundary() {
    let store = HandoffStore::with_limits(limits());
    let id = store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();

    let HandoffStep::NeedsFigma { calls, .. } = store.next(&id).await.unwrap() else {
        panic!()
    };
    assert_eq!(calls[0].tool, "get_metadata");
    store
        .accept(
            &id,
            &calls[0].call_id,
            Value::String(metadata_result().to_string()),
        )
        .await
        .unwrap();

    let HandoffStep::NeedsFigma { calls, .. } = store.next(&id).await.unwrap() else {
        panic!()
    };
    assert_eq!(calls[0].tool, "use_figma");
    store
        .accept(
            &id,
            &calls[0].call_id,
            Value::String(snapshot_result().to_string()),
        )
        .await
        .unwrap();

    let HandoffStep::Complete { parts, .. } = store.next(&id).await.unwrap() else {
        panic!()
    };
    assert_eq!(parts.snapshot_chunks.len(), 1);
}

#[tokio::test]
async fn rejects_cross_session_calls_and_concurrent_replays() {
    let store = HandoffStore::with_limits(limits());
    let first_id = store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();
    let second_id = store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();
    let HandoffStep::NeedsFigma { calls, .. } = store.next(&first_id).await.unwrap() else {
        panic!()
    };
    let cross_session = store
        .accept(&second_id, &calls[0].call_id, metadata_result())
        .await
        .unwrap_err();
    assert_eq!(cross_session.code, ErrorCode::DevupFigmaHandoffInvalid);

    let call_id = calls[0].call_id.clone();
    let left = {
        let store = store.clone();
        let session_id = first_id.clone();
        let call_id = call_id.clone();
        tokio::spawn(async move { store.accept(&session_id, &call_id, metadata_result()).await })
    };
    let right = {
        let store = store.clone();
        let session_id = first_id.clone();
        tokio::spawn(async move { store.accept(&session_id, &call_id, metadata_result()).await })
    };
    let results = [left.await.unwrap(), right.await.unwrap()];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .next()
            .unwrap()
            .code,
        ErrorCode::DevupFigmaHandoffInvalid
    );
}

#[tokio::test]
async fn enforces_the_aggregate_limit_across_sessions() {
    let payload = metadata_result();
    let encoded_len = serde_json::to_vec(&payload).unwrap().len();
    let store = HandoffStore::with_limits(HandoffLimits {
        max_result_bytes: encoded_len + 1,
        max_total_bytes: encoded_len * 2 - 1,
        ..limits()
    });
    let first_id = store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();
    let second_id = store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();
    let HandoffStep::NeedsFigma { calls: first, .. } = store.next(&first_id).await.unwrap() else {
        panic!()
    };
    let HandoffStep::NeedsFigma { calls: second, .. } = store.next(&second_id).await.unwrap()
    else {
        panic!()
    };
    store
        .accept(&first_id, &first[0].call_id, payload.clone())
        .await
        .unwrap();
    let error = store
        .accept(&second_id, &second[0].call_id, payload)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupFigmaResponseTooLarge);
}
