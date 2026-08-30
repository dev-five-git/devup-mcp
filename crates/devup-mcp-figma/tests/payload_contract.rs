use devup_mcp_figma::{
    CollectedParts, CollectedPayload, CollectionScope, FigmaTarget, PayloadCompleteness,
    PayloadStructure, SnapshotChunk, UpstreamResult,
};
use serde_json::json;

fn synthetic_parts() -> CollectedParts {
    CollectedParts {
        target: FigmaTarget::parse("https://www.figma.com/design/FileKey123/Fixture?node-id=1-2")
            .unwrap(),
        scope: CollectionScope::Node,
        metadata: json!({
            "name": "PRIVATE_METADATA_VALUE",
            "nodes": [{"id": "1:2", "type": "FRAME"}]
        }),
        snapshot_chunks: vec![
            serde_json::from_value::<SnapshotChunk>(json!({
                "fileKey": "FileKey123",
                "version": "v1",
                "rootIds": ["1:2"],
                "nodes": [{
                    "id": "1:2",
                    "type": "FRAME",
                    "fields": {
                        "name": "PRIVATE_NODE_VALUE",
                        "futureField": {"nested": 42},
                        "childrenIds": []
                    },
                    "extra": {"futureEnumerable": true},
                    "fieldErrors": {"restrictedGetter": "access denied"}
                }]
            }))
            .unwrap(),
        ],
        variables: Some(UpstreamResult {
            raw: json!({"variables": [{"id": "v1", "name": "PRIVATE_VARIABLE"}]}),
        }),
        styles: Some(UpstreamResult {
            raw: json!({"styles": [{"id": "s1", "name": "PRIVATE_STYLE"}]}),
        }),
        source_version: Some("v1".to_owned()),
    }
}

#[test]
fn collected_payload_round_trips_without_losing_unknown_fields() {
    let payload = CollectedPayload::try_from(synthetic_parts()).unwrap();
    assert_eq!(payload.completeness, PayloadCompleteness::UsedTokens);

    let encoded = serde_json::to_value(&payload).unwrap();
    let decoded: CollectedPayload = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, payload);
    assert_eq!(
        decoded.snapshot.nodes["1:2"].fields["futureField"]["nested"],
        42
    );
    assert_eq!(
        decoded.snapshot.nodes["1:2"].extra["futureEnumerable"],
        true
    );
}

#[test]
fn structure_report_contains_shapes_but_no_design_values() {
    let payload = CollectedPayload::try_from(synthetic_parts()).unwrap();
    let report = PayloadStructure::from_payload(&payload);
    let serialized = serde_json::to_string(&report).unwrap();

    assert!(serialized.contains("futureField"));
    assert!(serialized.contains("restrictedGetter"));
    assert!(serialized.contains("schemaHash"));
    for private in [
        "PRIVATE_METADATA_VALUE",
        "PRIVATE_NODE_VALUE",
        "PRIVATE_VARIABLE",
        "PRIVATE_STYLE",
        "access denied",
    ] {
        assert!(!serialized.contains(private), "leaked {private}");
    }
}
