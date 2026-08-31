use devup_mcp_figma::{
    CompletenessState, Diagnostic, ErrorCode, RawNode, Snapshot, SnapshotChunk, UpstreamResult,
    merge_chunks, snapshot_chunk_from_result,
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

#[test]
fn audit_marks_missing_children_parent_mismatches_and_field_loss_partial() {
    let snapshot = merge_chunks(vec![
        serde_json::from_value::<SnapshotChunk>(json!({
            "fileKey": "file-key",
            "version": "1",
            "rootIds": ["1:1"],
            "nodes": [
                {
                    "id": "1:1",
                    "type": "FRAME",
                    "fields": {
                        "childrenIds": ["1:2", "1:3", "1:404"],
                        "childCount": 3
                    },
                    "extra": {},
                    "fieldErrors": {}
                },
                {
                    "id": "1:2",
                    "type": "FRAME",
                    "fields": {
                        "parentId": "1:1",
                        "childrenIds": [],
                        "visible": false
                    },
                    "extra": {
                        "fillGeometry": {"$truncated": "byte-budget", "byteLength": 9000}
                    },
                    "fieldErrors": {"restrictedGetter": "access denied"}
                },
                {
                    "id": "1:3",
                    "type": "INSTANCE",
                    "fields": {
                        "parentId": "9:9",
                        "childrenIds": []
                    },
                    "extra": {},
                    "fieldErrors": {}
                },
                {
                    "id": "1:9",
                    "type": "TEXT",
                    "fields": {"parentId": "8:8", "childrenIds": []},
                    "extra": {},
                    "fieldErrors": {}
                }
            ],
            "diagnostics": []
        }))
        .unwrap(),
    ])
    .unwrap();

    let audit = snapshot.audit();

    assert_eq!(audit.state, CompletenessState::Partial);
    assert_eq!(audit.root_count, 1);
    assert_eq!(audit.preserved_node_count, 4);
    assert_eq!(audit.reachable_node_count, 3);
    assert_eq!(audit.orphan_node_ids, vec!["1:9"]);
    assert_eq!(audit.declared_child_count, 3);
    assert_eq!(audit.exported_child_count, 2);
    assert_eq!(audit.missing_children.len(), 1);
    assert_eq!(audit.missing_children[0].parent_id, "1:1");
    assert_eq!(audit.missing_children[0].child_id, "1:404");
    assert_eq!(audit.parent_mismatches.len(), 1);
    assert_eq!(audit.parent_mismatches[0].parent_id, "1:1");
    assert_eq!(audit.parent_mismatches[0].child_id, "1:3");
    assert_eq!(
        audit.parent_mismatches[0].observed_parent_id.as_deref(),
        Some("9:9")
    );
    assert_eq!(audit.truncated_fields.len(), 1);
    assert_eq!(audit.truncated_fields[0].node_id, "1:2");
    assert_eq!(audit.truncated_fields[0].field, "fillGeometry");
    assert_eq!(audit.field_error_count, 1);
}

#[test]
fn audit_treats_hidden_and_expanded_instance_children_as_complete() {
    let snapshot = merge_chunks(vec![
        serde_json::from_value::<SnapshotChunk>(json!({
            "fileKey": "file-key",
            "version": "1",
            "rootIds": ["1:1"],
            "nodes": [
                {
                    "id": "1:1",
                    "type": "FRAME",
                    "fields": {"childrenIds": ["1:2"], "childCount": 1},
                    "extra": {},
                    "fieldErrors": {}
                },
                {
                    "id": "1:2",
                    "type": "INSTANCE",
                    "fields": {
                        "parentId": "1:1",
                        "childrenIds": ["1:3"],
                        "childCount": 1,
                        "visible": false
                    },
                    "extra": {},
                    "fieldErrors": {}
                },
                {
                    "id": "1:3",
                    "type": "TEXT",
                    "fields": {"parentId": "1:2", "childrenIds": [], "childCount": 0},
                    "extra": {},
                    "fieldErrors": {}
                }
            ],
            "diagnostics": []
        }))
        .unwrap(),
    ])
    .unwrap();

    let audit = snapshot.audit();

    assert_eq!(audit.state, CompletenessState::Complete);
    assert_eq!(audit.reachable_node_count, 3);
    assert_eq!(audit.declared_child_count, 2);
    assert_eq!(audit.exported_child_count, 2);
    assert!(audit.missing_children.is_empty());
    assert!(audit.parent_mismatches.is_empty());
    assert!(audit.orphan_node_ids.is_empty());
    assert!(audit.truncated_fields.is_empty());
}

#[test]
fn audit_fails_when_a_requested_root_is_missing() {
    let snapshot = Snapshot {
        file_key: "file-key".to_owned(),
        version: Some("1".to_owned()),
        roots: vec!["1:404".to_owned()],
        nodes: Default::default(),
        diagnostics: Vec::new(),
    };

    let audit = snapshot.audit();

    assert_eq!(audit.state, CompletenessState::Failed);
    assert_eq!(audit.missing_root_ids, vec!["1:404"]);
    assert_eq!(audit.reachable_node_count, 0);
}

#[test]
fn audit_reports_observed_child_count_mismatches() {
    let snapshot = merge_chunks(vec![
        serde_json::from_value::<SnapshotChunk>(json!({
            "fileKey": "file-key",
            "version": "1",
            "rootIds": ["1:1"],
            "nodes": [
                {
                    "id": "1:1",
                    "type": "FRAME",
                    "fields": {"childrenIds": ["1:2"], "childCount": 2},
                    "extra": {},
                    "fieldErrors": {}
                },
                {
                    "id": "1:2",
                    "type": "TEXT",
                    "fields": {"parentId": "1:1", "childrenIds": [], "childCount": 0},
                    "extra": {},
                    "fieldErrors": {}
                }
            ],
            "diagnostics": []
        }))
        .unwrap(),
    ])
    .unwrap();

    let audit = snapshot.audit();

    assert_eq!(audit.state, CompletenessState::Partial);
    assert_eq!(audit.child_count_mismatches.len(), 1);
    assert_eq!(audit.child_count_mismatches[0].node_id, "1:1");
    assert_eq!(audit.child_count_mismatches[0].declared_count, 2);
    assert_eq!(audit.child_count_mismatches[0].exported_count, 1);
}

#[test]
fn audit_preserves_declared_order_for_missing_children() {
    let snapshot = merge_chunks(vec![chunk(
        "1",
        json!({
            "id": "1:1",
            "type": "FRAME",
            "fields": {"childrenIds": ["1:missing-b", "1:missing-a"]},
            "extra": {},
            "fieldErrors": {}
        }),
    )])
    .unwrap();

    let audit = snapshot.audit();

    assert_eq!(
        audit
            .missing_children
            .iter()
            .map(|missing| missing.child_id.as_str())
            .collect::<Vec<_>>(),
        vec!["1:missing-b", "1:missing-a"]
    );
}

#[test]
fn structured_diagnostic_round_trips_safe_recovery_context() {
    let expected = json!({
        "code": "DEVUP_RESOURCE_UNRESOLVED",
        "message": "Figma resource fallback",
        "nodeId": "1:2",
        "severity": "warning",
        "property": "fills.color",
        "resourceKind": "variable",
        "resourceId": "VariableID:1:404",
        "fallback": "raw-value",
        "recoverable": true,
        "details": {"modeId": "1:0"}
    });

    let diagnostic: Diagnostic = serde_json::from_value(expected.clone()).unwrap();

    assert_eq!(serde_json::to_value(diagnostic).unwrap(), expected);
}
