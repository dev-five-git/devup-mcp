use devup_mcp_figma::{
    ErrorCode, RawNode, Snapshot, SnapshotChunk, UpstreamResult, merge_chunks,
    snapshot_chunk_from_result,
};
use serde_json::json;

fn chunk(version: &str, node: serde_json::Value) -> SnapshotChunk {
    serde_json::from_value(json!({
        "fileKey": "file-key",
        "version": version,
        "rootIds": ["1:1"],
        "nodes": [node],
        "diagnostics": []
    }))
    .expect("valid synthetic snapshot chunk")
}

#[test]
fn preserves_known_unknown_and_failed_fields_without_loss() {
    let node: RawNode = serde_json::from_value(json!({
        "id": "1:1",
        "type": "FRAME",
        "fields": {
            "name": "Root",
            "layoutMode": "HORIZONTAL",
            "futurePluginApiField": {"nested": [1, true, null]}
        },
        "extra": {"runtimeOnlyField": "kept"},
        "fieldErrors": {"inaccessibleGetter": "getter failed"}
    }))
    .expect("raw node");

    let round_trip = serde_json::to_value(&node).expect("serialize raw node");
    assert_eq!(
        round_trip["fields"]["futurePluginApiField"]["nested"],
        json!([1, true, null])
    );
    assert_eq!(round_trip["extra"]["runtimeOnlyField"], "kept");
    assert_eq!(
        round_trip["fieldErrors"]["inaccessibleGetter"],
        "getter failed"
    );
}

#[test]
fn merges_chunks_by_node_id_and_keeps_child_order() {
    let root = json!({
        "id": "1:1",
        "type": "FRAME",
        "fields": {"childrenIds": ["1:3", "1:2"]},
        "extra": {},
        "fieldErrors": {}
    });
    let child = json!({
        "id": "1:2",
        "type": "TEXT",
        "fields": {"characters": "hello"},
        "extra": {},
        "fieldErrors": {}
    });

    let snapshot = merge_chunks(vec![chunk("42", root.clone()), chunk("42", child)])
        .expect("matching chunks merge");

    assert_eq!(snapshot.roots, vec!["1:1"]);
    assert_eq!(snapshot.nodes.len(), 2);
    assert_eq!(
        snapshot.nodes["1:1"].fields["childrenIds"],
        json!(["1:3", "1:2"])
    );

    let deduplicated = merge_chunks(vec![chunk("42", root.clone()), chunk("42", root)])
        .expect("identical duplicate is safe");
    assert_eq!(deduplicated.nodes.len(), 1);
}

#[test]
fn rejects_cross_version_and_conflicting_chunks() {
    let first = json!({
        "id": "1:1", "type": "FRAME", "fields": {"name": "before"},
        "extra": {}, "fieldErrors": {}
    });
    let after = json!({
        "id": "1:1", "type": "FRAME", "fields": {"name": "after"},
        "extra": {}, "fieldErrors": {}
    });

    let version_error = merge_chunks(vec![chunk("1", first.clone()), chunk("2", after.clone())])
        .expect_err("versions must not mix");
    assert_eq!(version_error.code, ErrorCode::DevupFigmaVersionChanged);

    let conflict = merge_chunks(vec![chunk("1", first), chunk("1", after)])
        .expect_err("same id with different data is unsafe");
    assert_eq!(conflict.code, ErrorCode::DevupSnapshotUnsupported);
}

#[test]
fn typed_views_are_additive_over_the_raw_snapshot() {
    let snapshot: Snapshot = merge_chunks(vec![chunk(
        "1",
        json!({
            "id": "1:1",
            "type": "FRAME",
            "fields": {
                "name": "Row",
                "layoutMode": "HORIZONTAL",
                "width": 320,
                "futureField": 99
            },
            "extra": {},
            "fieldErrors": {}
        }),
    )])
    .expect("snapshot");
    let view = snapshot.nodes["1:1"].typed_view();

    assert_eq!(view.name(), Some("Row"));
    assert_eq!(view.string("layoutMode"), Some("HORIZONTAL"));
    assert_eq!(view.number("width"), Some(320.0));
    assert_eq!(snapshot.nodes["1:1"].fields["futureField"], 99);
}

#[test]
fn extracts_snapshot_from_structured_or_text_mcp_results() {
    let payload = json!({
        "fileKey": "file-key", "version": null, "rootIds": ["1:1"],
        "nodes": [{"id": "1:1", "type": "FRAME", "fields": {}, "extra": {}, "fieldErrors": {}}],
        "diagnostics": []
    });
    let structured = UpstreamResult {
        raw: json!({"structuredContent": {"result": payload.clone()}, "content": []}),
    };
    let text = UpstreamResult {
        raw: json!({"content": [{"type": "text", "text": serde_json::to_string(&payload).unwrap()}]}),
    };

    assert_eq!(
        snapshot_chunk_from_result(&structured).unwrap().nodes[0].id,
        "1:1"
    );
    assert_eq!(
        snapshot_chunk_from_result(&text).unwrap().file_key,
        "file-key"
    );
}
