use devup_mcp_figma::{
    FigmaTarget, UpstreamResult, decode_fast_multi_snapshot, decode_fast_snapshot,
    decode_fast_theme,
};
use serde_json::{Value, json};

// No binary (PNG-chunked) transport exists any more — real-world hosts
// silently discarded the old image attachments, so it never actually worked
// end to end. Fast snapshots and fast themes are delivered as plain text
// only now; a node subtree that doesn't fit in one round is paginated across
// several text rounds instead (see the `paginated_*` tests below).

#[test]
fn valid_snapshot_envelope_round_trips_without_an_image() {
    let envelope = complete_envelope();
    let result = text_upstream_result(&envelope);

    let decoded = decode_fast_snapshot(&result, &target()).expect("valid text envelope");

    assert_eq!(decoded.snapshot.file_key, "fileKey123");
    assert_eq!(decoded.snapshot.root_ids, ["1:1"]);
    assert_eq!(decoded.snapshot.nodes.len(), 2);
    assert_eq!(
        decoded.resources.raw["variables"].as_array().unwrap().len(),
        1
    );
    assert_eq!(decoded.resources.raw["styles"].as_array().unwrap().len(), 1);
    assert_eq!(decoded.stats.raw_bytes, envelope.len());
    assert_eq!(decoded.stats.wire_bytes, envelope.len());
    assert_eq!(decoded.stats.chunk_count, 0);
    assert_eq!(decoded.stats.transport, "text");
}

#[test]
fn json_stringified_official_mcp_result_round_trips() {
    let target = target();
    let envelope = complete_envelope();
    let mut result = text_upstream_result(&envelope);
    result.raw = Value::String(result.raw.to_string());

    let decoded = decode_fast_snapshot(&result, &target)
        .expect("official handoff schema transports the MCP result as a JSON string");

    assert_eq!(decoded.snapshot.root_ids, ["1:1"]);
    assert_eq!(decoded.stats.transport, "text");
}

#[test]
fn oversized_stringified_upstream_result_is_rejected_before_json_decode() {
    let result = UpstreamResult {
        raw: Value::String(" ".repeat(16 * 1024 * 1024 + 1)),
    };

    let error = decode_fast_snapshot(&result, &target()).expect_err("oversized result string");

    assert_eq!(
        error.code,
        devup_mcp_figma::ErrorCode::DevupFigmaResponseTooLarge
    );
    assert_eq!(error.details["category"], "upstreamResultJson");
}

#[test]
fn a_text_envelope_over_the_15kb_safety_margin_is_rejected() {
    let envelope = mutate_envelope(|value| {
        value["snapshot"]["nodes"][1]["fields"]["characters"] = json!("x".repeat(20 * 1024));
    });
    let result = text_upstream_result(&envelope);

    let error = decode_fast_snapshot(&result, &target()).expect_err("oversized text envelope");

    assert_eq!(error.details["category"], "textEnvelope");
}

#[test]
fn missing_fast_envelope_text_is_rejected() {
    let result = UpstreamResult {
        raw: json!({"content": [{"type": "text", "text": "not an envelope"}]}),
    };

    let error = decode_fast_snapshot(&result, &target()).expect_err("no tagged envelope");

    assert_eq!(error.details["category"], "textEnvelopeMissing");
}

#[test]
fn duplicate_tagged_envelopes_are_rejected() {
    let envelope = complete_envelope();
    let text = std::str::from_utf8(&envelope).unwrap();
    let result = UpstreamResult {
        raw: json!({"content": [
            {"type": "text", "text": text},
            {"type": "text", "text": text}
        ]}),
    };

    let error = decode_fast_snapshot(&result, &target()).expect_err("duplicate envelope text");

    assert_eq!(error.details["category"], "textEnvelopeMultiplicity");
}

#[test]
fn valid_multi_root_envelope_requires_the_exact_ordered_root_set() {
    let envelope = mutate_envelope(|value| {
        value["source"]["rootId"] = json!("9:9");
        value["snapshot"]["rootIds"] = json!(["1:1", "1:2"]);
    });
    let result = text_upstream_result(&envelope);
    let section_target = FigmaTarget {
        node_id: Some("9:9".to_owned()),
        ..target()
    };

    let decoded = decode_fast_multi_snapshot(
        &result,
        &section_target,
        &["1:1".to_owned(), "1:2".to_owned()],
    )
    .expect("valid multi-root envelope");
    assert_eq!(decoded.snapshot.root_ids, ["1:1", "1:2"]);

    let error = decode_fast_multi_snapshot(
        &result,
        &section_target,
        &["1:2".to_owned(), "1:1".to_owned()],
    )
    .expect_err("ordered root mismatch");
    assert_eq!(error.details["category"], "targetMismatch");
}

#[test]
fn schema_target_and_resource_integrity_are_validated() {
    let unsupported = mutate_envelope(|value| value["schemaVersion"] = Value::from(2));
    assert_category(
        text_upstream_result(&unsupported),
        &target(),
        "schemaVersion",
    );

    let wrong_target = FigmaTarget {
        file_key: "otherFileKey".to_owned(),
        ..target()
    };
    assert_category(
        text_upstream_result(&complete_envelope()),
        &wrong_target,
        "targetMismatch",
    );

    let duplicate_node = mutate_envelope(|value| {
        let duplicate = value["snapshot"]["nodes"][1].clone();
        value["snapshot"]["nodes"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
    });
    assert_category(
        text_upstream_result(&duplicate_node),
        &target(),
        "duplicateNode",
    );

    let missing_resource = mutate_envelope(|value| {
        value["resources"]["variables"] = json!([]);
    });
    assert_category(
        text_upstream_result(&missing_resource),
        &target(),
        "resourceMissing",
    );

    // Corrupt the utf8Bytes counter *after* finalization (finalize_envelope's
    // convergence loop would otherwise just recompute a correct value).
    let mut bad_utf8_count: Value = serde_json::from_slice(&complete_envelope()).unwrap();
    bad_utf8_count["integrity"]["utf8Bytes"] = json!(1);
    let bad_utf8_bytes = serde_json::to_vec(&bad_utf8_count).unwrap();
    assert_category(
        text_upstream_result(&bad_utf8_bytes),
        &target(),
        "utf8Bytes",
    );
}

#[test]
fn a_complete_single_page_envelope_still_requires_full_child_containment() {
    // No cursor marker at all: treated as a single complete page, so a
    // dangling child (referencing a node that was never sent) is rejected
    // exactly like the pre-pagination behavior.
    let dangling_child = mutate_envelope(|value| {
        value["snapshot"]["nodes"][0]["fields"]["childrenIds"][0] = Value::from("9:9");
    });
    assert_category(
        text_upstream_result(&dangling_child),
        &target(),
        "danglingChild",
    );
}

#[test]
fn a_final_page_with_an_explicit_cursor_still_requires_full_child_containment() {
    let dangling_child = mutate_envelope(|value| {
        value["snapshot"]["nodes"][0]["fields"]["childrenIds"][0] = Value::from("9:9");
        push_cursor_marker(value, 0, 2, true, 2);
    });
    assert_category(
        text_upstream_result(&dangling_child),
        &target(),
        "danglingChild",
    );
}

#[test]
fn a_non_final_page_may_reference_children_that_have_not_arrived_yet() {
    // node "1:2" (the second real node) is deliberately left out of this
    // page; the root's childrenIds still references it. Because the page
    // reports `complete: false`, this is expected — the child is assumed to
    // arrive in a later round — and must not be rejected as dangling.
    let first_page = mutate_envelope(|value| {
        let nodes = value["snapshot"]["nodes"].as_array_mut().unwrap();
        nodes.truncate(1);
        value["integrity"]["nodeCount"] = json!(1);
        // No boundVariables/textStyleId left in this page, so no resources
        // are referenced by it.
        value["snapshot"]["nodes"][0]["fields"]
            .as_object_mut()
            .unwrap()
            .remove("boundVariables");
        value["integrity"]["variableRefCount"] = json!(0);
        value["integrity"]["styleRefCount"] = json!(0);
        value["resources"]["variables"] = json!([]);
        value["resources"]["styles"] = json!([]);
        push_cursor_marker(value, 0, 1, false, 2);
    });
    let result = text_upstream_result(&first_page);

    let decoded = decode_fast_snapshot(&result, &target()).expect("valid first page");
    assert_eq!(decoded.snapshot.nodes.len(), 2); // real node + cursor marker
}

#[test]
fn a_first_page_that_omits_the_root_is_still_rejected() {
    // The root must always be present on the first page (BFS visits it at
    // index 0); a first page (offset == 0) that omits it is a real error.
    let missing_root = mutate_envelope(|value| {
        let nodes = value["snapshot"]["nodes"].as_array_mut().unwrap();
        nodes.remove(0);
        value["integrity"]["nodeCount"] = json!(1);
        value["integrity"]["variableRefCount"] = json!(0);
        value["integrity"]["styleRefCount"] = json!(1);
        value["resources"]["variables"] = json!([]);
        push_cursor_marker(value, 0, 1, false, 2);
    });
    assert_category(text_upstream_result(&missing_root), &target(), "nodeCount");
}

#[test]
fn a_continuation_page_may_omit_the_root_that_a_prior_page_already_sent() {
    let second_page = mutate_envelope(|value| {
        let nodes = value["snapshot"]["nodes"].as_array_mut().unwrap();
        nodes.remove(0);
        value["integrity"]["nodeCount"] = json!(1);
        value["integrity"]["variableRefCount"] = json!(0);
        value["integrity"]["styleRefCount"] = json!(1);
        value["resources"]["variables"] = json!([]);
        push_cursor_marker(value, 1, 2, true, 2);
    });
    let result = text_upstream_result(&second_page);

    let decoded = decode_fast_snapshot(&result, &target()).expect("valid continuation page");
    assert_eq!(decoded.snapshot.nodes.len(), 2); // real node + cursor marker
}

#[test]
fn a_cursor_marker_missing_offset_is_rejected() {
    // Regression: the script once emitted the marker without `offset`, so
    // every real fast snapshot failed `peek_page_cursor` and silently fell
    // back to legacy cursor collection.
    let bad = mutate_envelope(|value| {
        push_cursor_marker(value, 0, 2, true, 2);
        value["snapshot"]["nodes"][2]["fields"]
            .as_object_mut()
            .unwrap()
            .remove("offset");
    });
    assert_category(text_upstream_result(&bad), &target(), "cursorShape");
}

#[test]
fn duplicate_cursor_markers_are_rejected() {
    let bad = mutate_envelope(|value| {
        push_cursor_marker(value, 0, 2, true, 2);
        push_cursor_marker(value, 0, 2, true, 2);
        value["integrity"]["nodeCount"] = json!(4);
    });
    assert_category(text_upstream_result(&bad), &target(), "cursorMultiplicity");
}

#[test]
fn valid_fast_theme_envelope_round_trips_and_validates_counts() {
    let envelope = theme_envelope();
    let result = theme_text_upstream_result(&envelope);

    let decoded = decode_fast_theme(&result, "fileKey123").expect("valid fast theme");

    assert_eq!(decoded.source_version, Some("v42".to_owned()));
    assert_eq!(
        decoded.resources.raw["collections"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        decoded.resources.raw["variables"].as_array().unwrap().len(),
        1
    );
    assert_eq!(decoded.resources.raw["styles"].as_array().unwrap().len(), 1);
    assert_eq!(decoded.resources.raw["localComplete"], true);
    assert_eq!(decoded.stats.raw_bytes, envelope.len());
    assert_eq!(decoded.stats.transport, "text");

    let bad = mutate_theme_envelope(|value| value["integrity"]["variableCount"] = json!(2));
    let error = decode_fast_theme(&theme_text_upstream_result(&bad), "fileKey123")
        .expect_err("count mismatch");
    assert_eq!(error.details["category"], "variableCount");
}

#[test]
fn json_stringified_fast_theme_result_round_trips() {
    let envelope = theme_envelope();
    let mut result = theme_text_upstream_result(&envelope);
    result.raw = Value::String(result.raw.to_string());

    let decoded =
        decode_fast_theme(&result, "fileKey123").expect("stringified official theme envelope");

    assert_eq!(decoded.stats.transport, "text");
    assert_eq!(decoded.resources.raw["localComplete"], true);
}

fn target() -> FigmaTarget {
    FigmaTarget {
        file_key: "fileKey123".to_owned(),
        node_id: Some("1:1".to_owned()),
        branch_key: None,
    }
}

fn complete_envelope() -> Vec<u8> {
    finalize_envelope(json!({
        "kind": "devupFastSnapshotEnvelope",
        "schemaVersion": 1,
        "source": {
            "fileKey": "fileKey123",
            "rootId": "1:1"
        },
        "snapshot": {
            "fileKey": "fileKey123",
            "version": null,
            "rootIds": ["1:1"],
            "nodes": [
                {
                    "id": "1:1",
                    "type": "FRAME",
                    "fields": {
                        "childrenIds": ["1:2"],
                        "boundVariables": {
                            "fills": {"type": "VARIABLE_ALIAS", "id": "VariableID:1:1"}
                        }
                    }
                },
                {
                    "id": "1:2",
                    "type": "TEXT",
                    "fields": {
                        "textStyleId": "S:style1",
                        "characters": "테스트"
                    }
                }
            ],
            "diagnostics": []
        },
        "resources": {
            "collections": [],
            "variables": [{"id": "VariableID:1:1", "name": "color/text"}],
            "styles": [{"id": "S:style1", "name": "body", "styleType": "TEXT"}],
            "usedRemoteVariables": [],
            "localComplete": false,
            "usedRemoteComplete": true,
            "unresolved": []
        },
        "integrity": {
            "nodeCount": 2,
            "variableRefCount": 1,
            "styleRefCount": 1,
            "utf8Bytes": 0
        }
    }))
}

fn theme_envelope() -> Vec<u8> {
    finalize_theme_envelope(theme_envelope_value())
}

fn theme_envelope_value() -> Value {
    json!({
        "kind": "devupFastThemeEnvelope",
        "schemaVersion": 1,
        "source": {"fileKey": "fileKey123", "version": "v42"},
        "resources": {
            "collections": [{"id": "c", "name": "Theme"}],
            "variables": [{"id": "v", "name": "primary"}],
            "styles": [{"id": "s", "name": "body", "styleType": "TEXT"}],
            "usedRemoteVariables": [],
            "usedVariableIds": ["v"],
            "usedStyleIds": ["s"],
            "localComplete": true,
            "usedRemoteComplete": true,
            "unresolved": []
        },
        "integrity": {
            "collectionCount": 1,
            "variableCount": 1,
            "styleCount": 1,
            "unresolvedCount": 0,
            "utf8Bytes": 0
        }
    })
}

fn text_upstream_result(envelope: &[u8]) -> UpstreamResult {
    UpstreamResult {
        raw: json!({
            "content": [{
                "type": "text",
                "text": std::str::from_utf8(envelope).unwrap()
            }]
        }),
    }
}

fn theme_text_upstream_result(envelope: &[u8]) -> UpstreamResult {
    text_upstream_result(envelope)
}

/// Appends the `__DEVUP_SNAPSHOT_CURSOR__` marker node every fast snapshot
/// script emits, mirroring the shape `take_snapshot_cursor` parses, and
/// updates `integrity.nodeCount` to include it (matching real script output,
/// which always counts the marker in the same `nodes` array it serializes).
fn push_cursor_marker(
    value: &mut Value,
    offset: u64,
    next_offset: u64,
    complete: bool,
    total_nodes: u64,
) {
    let nodes = value["snapshot"]["nodes"].as_array_mut().unwrap();
    let real_node_count = nodes.len() as u64;
    nodes.push(json!({
        "id": "__DEVUP_SNAPSHOT_CURSOR__",
        "type": "DEVUP_INTERNAL",
        "fields": {
            "offset": offset,
            "nextOffset": next_offset,
            "complete": complete,
            "totalNodes": total_nodes
        },
        "extra": {},
        "fieldErrors": {}
    }));
    value["integrity"]["nodeCount"] = json!(real_node_count + 1);
}

fn mutate_envelope(mutate: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(&complete_envelope()).unwrap();
    mutate(&mut value);
    finalize_envelope(value)
}

fn mutate_theme_envelope(mutate: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut value = theme_envelope_value();
    mutate(&mut value);
    finalize_theme_envelope(value)
}

fn finalize_envelope(value: Value) -> Vec<u8> {
    finalize_utf8_bytes(value)
}

fn finalize_theme_envelope(value: Value) -> Vec<u8> {
    finalize_utf8_bytes(value)
}

fn finalize_utf8_bytes(mut value: Value) -> Vec<u8> {
    for _ in 0..8 {
        let bytes = serde_json::to_vec(&value).unwrap();
        let length = bytes.len() as u64;
        if value["integrity"]["utf8Bytes"] == length {
            return bytes;
        }
        value["integrity"]["utf8Bytes"] = Value::from(length);
    }
    panic!("utf8Bytes did not converge");
}

fn assert_category(result: UpstreamResult, target: &FigmaTarget, expected: &str) {
    let error = decode_fast_snapshot(&result, target).expect_err(expected);
    assert_eq!(error.details["category"], expected);
}
