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

/// Builds the content-only XML shape exposed when an MCP client drops
/// `structuredContent`, optionally with Figma's fixed `get_metadata` reminder.
fn xml_metadata_result(append_tail: bool) -> Value {
    let xml = r#"<frame id="1:2" name="Fixture"></frame>"#;
    let text = if append_tail {
        format!(
            "{xml}\n\nIMPORTANT: After you call this tool, you MUST call get_design_context if trying to implement the design, since this tool only returns metadata. If you do not call get_design_context, the agent will not be able to implement the design."
        )
    } else {
        xml.to_owned()
    };
    json!({"content": [{"type": "text", "text": text}]})
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

/// Reproduces WQUW-156: opencode preserved only the XML text plus Figma's
/// reminder, then submitted that `get_metadata` result for the next
/// `use_figma` call; the boundary must identify the wrong tool explicitly.
#[tokio::test]
async fn wquw_156_wrong_tool_result_reports_tool_mismatch_after_text_only_metadata() {
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
        .accept(&id, &calls[0].call_id, xml_metadata_result(true))
        .await
        .unwrap();

    let HandoffStep::NeedsFigma { calls, .. } = store.next(&id).await.unwrap() else {
        panic!()
    };
    assert_eq!(calls[0].tool, "use_figma");

    let error = store
        .accept(&id, &calls[0].call_id, xml_metadata_result(true))
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupFigmaHandoffInvalid);
    assert_eq!(error.details["reason"], "tool_mismatch");
    assert_eq!(error.details["requested"]["tool"], "use_figma");
}

/// Locks in the pre-existing XML fallback when the official reminder is not
/// present, so reminder normalization cannot regress ordinary text-only hosts.
#[tokio::test]
async fn content_only_xml_metadata_without_figma_reminder_advances_to_use_figma() {
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
        .accept(&id, &calls[0].call_id, xml_metadata_result(false))
        .await
        .unwrap();

    let HandoffStep::NeedsFigma { calls, .. } = store.next(&id).await.unwrap() else {
        panic!()
    };
    assert_eq!(calls[0].tool, "use_figma");
}

/// A mismatch rejection must preserve the exact pending call so the host can
/// retry with the requested tool's raw result instead of restarting collection.
#[tokio::test]
async fn tool_mismatch_rejection_keeps_use_figma_call_pending_for_corrected_result() {
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
    assert_eq!(calls[0].tool, "use_figma");

    let error = store
        .accept(&id, &calls[0].call_id, xml_metadata_result(true))
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupFigmaHandoffInvalid);
    assert_eq!(error.details["reason"], "tool_mismatch");
    assert_eq!(error.details["requested"]["tool"], "use_figma");

    store
        .accept(&id, &calls[0].call_id, snapshot_result())
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

/// The exact incident this fix addresses: an agent whose host flattens the
/// official Figma MCP `get_metadata` response down to a bare string (no
/// envelope at all — see `handoff.rs`'s `normalize_handoff_result` doc
/// comment) submits that string directly. It must succeed without the
/// agent inventing a `{"content":[...]}"` wrapper by hand.
#[tokio::test]
async fn accept_promotes_a_bare_non_json_string_to_a_content_envelope() {
    let store = HandoffStore::with_limits(limits());
    let id = store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();
    let HandoffStep::NeedsFigma { calls, .. } = store.next(&id).await.unwrap() else {
        panic!()
    };
    assert_eq!(calls[0].tool, "get_metadata");

    // Bare XML text, not JSON, not wrapped — exactly what a host that
    // flattens tool results to plain text would hand the agent.
    let bare_xml = "<frame id=\"1:2\" name=\"Synthetic Root\" x=\"0\" y=\"0\" width=\"320\" height=\"240\"><text id=\"1:3\" name=\"Synthetic Child\" x=\"8\" y=\"8\" width=\"100\" height=\"20\" /></frame>".to_owned();
    store
        .accept(&id, &calls[0].call_id, Value::String(bare_xml))
        .await
        .unwrap();

    let HandoffStep::NeedsFigma { calls, .. } = store.next(&id).await.unwrap() else {
        panic!()
    };
    assert_eq!(calls[0].tool, "use_figma");
}

/// The "그대로 통과시키되" half of the normalization contract: a `content`
/// array with usable text but no `structuredContent` must NOT be rejected.
/// Every real extraction path in this codebase's collector already
/// tolerates this shape by design (XML-text metadata, JSON-in-text
/// snapshots, ...); rejecting it here would be a regression.
#[tokio::test]
async fn accept_passes_through_content_only_result_when_text_is_usable() {
    let store = HandoffStore::with_limits(limits());
    let id = store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();
    let HandoffStep::NeedsFigma { calls, .. } = store.next(&id).await.unwrap() else {
        panic!()
    };

    let content_only = json!({
        "content": [{
            "type": "text",
            "text": "<frame id=\"1:2\" name=\"Synthetic Root\" x=\"0\" y=\"0\" width=\"320\" height=\"240\"><text id=\"1:3\" name=\"Synthetic Child\" x=\"8\" y=\"8\" width=\"100\" height=\"20\" /></frame>"
        }]
    });
    store
        .accept(&id, &calls[0].call_id, content_only)
        .await
        .unwrap();
}

/// `structuredContent` presence always exempts a result from the
/// no-usable-content rejection, regardless of what (if anything) is in
/// `content` alongside it.
#[tokio::test]
async fn accept_passes_through_structured_content_even_with_an_empty_content_array() {
    let store = HandoffStore::with_limits(limits());
    let id = store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();
    let HandoffStep::NeedsFigma { calls, .. } = store.next(&id).await.unwrap() else {
        panic!()
    };

    let mut with_empty_content = metadata_result();
    with_empty_content["content"] = json!([]);
    store
        .accept(&id, &calls[0].call_id, with_empty_content)
        .await
        .unwrap();
}

/// The one case this fix does reject: a `content` array with nothing
/// usable in it and no `structuredContent` either. Every reported field
/// must be exactly the brief's `expectedSchema`/`receivedShape` contract,
/// and `receivedShape` must never leak a value — only key names and
/// content-block `type`s.
#[tokio::test]
async fn accept_rejects_empty_content_with_a_schema_shaped_error_that_leaks_no_values() {
    let store = HandoffStore::with_limits(limits());
    let id = store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();
    let HandoffStep::NeedsFigma { calls, .. } = store.next(&id).await.unwrap() else {
        panic!()
    };

    let error = store
        .accept(&id, &calls[0].call_id, json!({"content": []}))
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupFigmaHandoffInvalid);
    assert_eq!(error.details["reason"], "missing_structured_content");
    assert_eq!(
        error.details["expectedSchema"]["content"][0]["type"],
        "text"
    );
    assert!(
        error.details["expectedSchema"]["structuredContent"]["devupMetadata"]
            .as_str()
            .unwrap()
            .contains("필수")
    );
    assert_eq!(
        error.details["receivedShape"]["topLevelKeys"],
        json!(["content"])
    );
    assert_eq!(error.details["receivedShape"]["contentTypes"], json!([]));
    assert!(
        !error.details["howToFix"].as_str().unwrap().is_empty(),
        "must tell the agent what to do next, not just that it failed"
    );
    assert!(error.details["doNot"].as_str().unwrap().contains("추측"));
}

/// Non-empty but still unusable content (an image block with no `data`, a
/// whitespace-only text block) is rejected the same way, and the block
/// `type`s are reported — but never the (here, absent) `data`/`text`
/// values themselves.
#[tokio::test]
async fn accept_rejects_content_with_no_usable_items_reporting_types_not_values() {
    let store = HandoffStore::with_limits(limits());
    let id = store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();
    let HandoffStep::NeedsFigma { calls, .. } = store.next(&id).await.unwrap() else {
        panic!()
    };

    let error = store
        .accept(
            &id,
            &calls[0].call_id,
            json!({"content": [{"type": "image"}, {"type": "text", "text": "   "}]}),
        )
        .await
        .unwrap_err();
    assert_eq!(error.details["reason"], "missing_structured_content");
    assert_eq!(
        error.details["receivedShape"]["contentTypes"],
        json!(["image", "text"])
    );
    // The (absent) design/binary values must never appear in the error.
    let rendered = error.details.to_string();
    assert!(!rendered.contains("\"data\""));
    assert!(!rendered.contains("\"text\":\"   \""));
}

/// An empty string, once promoted, carries no usable text — it must be
/// rejected rather than silently accepted as "successful but empty".
#[tokio::test]
async fn accept_rejects_a_bare_empty_string() {
    let store = HandoffStore::with_limits(limits());
    let id = store
        .begin(PendingOperation::Collect, collector())
        .await
        .unwrap();
    let HandoffStep::NeedsFigma { calls, .. } = store.next(&id).await.unwrap() else {
        panic!()
    };

    let error = store
        .accept(&id, &calls[0].call_id, Value::String(String::new()))
        .await
        .unwrap_err();
    assert_eq!(error.details["reason"], "missing_structured_content");
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
