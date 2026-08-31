use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_figma::{
    CollectionRequest, CollectionScope, CollectorSession, CollectorStep, FigmaTarget,
    LargeValueAssembler, LargeValueCursor, LargeValueDescriptor, LargeValueFragment, ReadToolCall,
    UpstreamResult,
};
use serde_json::json;

fn descriptor() -> LargeValueDescriptor {
    LargeValueDescriptor {
        node_id: "1:2".to_owned(),
        field: "characters".to_owned(),
        byte_length: 19,
        sha256: "5ab6efd34df9db2f30a0581487fd5d023fde8f658c3dfe9378dbed52332e11f8".to_owned(),
        cursor: LargeValueCursor {
            next_offset: 0,
            max_chunk_bytes: 8,
        },
    }
}

fn fragment(offset: usize, bytes: &[u8], complete: bool) -> LargeValueFragment {
    LargeValueFragment {
        file_key: "FileKey123".to_owned(),
        version: Some("v1".to_owned()),
        node_id: "1:2".to_owned(),
        field: "characters".to_owned(),
        offset,
        next_offset: offset + bytes.len(),
        byte_length: 19,
        sha256: descriptor().sha256,
        data_base64: STANDARD.encode(bytes),
        complete,
    }
}

#[test]
fn reassembles_out_of_order_fragments_and_accepts_identical_duplicates() {
    let literal = br#""hello large value""#;
    let mut assembler =
        LargeValueAssembler::new("FileKey123", Some("v1".to_owned()), descriptor()).unwrap();

    assembler.push(fragment(8, &literal[8..16], false)).unwrap();
    assembler.push(fragment(0, &literal[..8], false)).unwrap();
    assembler.push(fragment(0, &literal[..8], false)).unwrap();
    assembler.push(fragment(16, &literal[16..], true)).unwrap();

    assert_eq!(
        assembler.finish().unwrap(),
        serde_json::json!("hello large value")
    );
}

#[test]
fn rejects_conflicting_duplicate_missing_and_wrong_identity_fragments() {
    let literal = br#""hello large value""#;
    let mut conflicting =
        LargeValueAssembler::new("FileKey123", Some("v1".to_owned()), descriptor()).unwrap();
    conflicting.push(fragment(0, &literal[..8], false)).unwrap();
    assert!(conflicting.push(fragment(0, b"different", false)).is_err());

    let mut missing =
        LargeValueAssembler::new("FileKey123", Some("v1".to_owned()), descriptor()).unwrap();
    missing.push(fragment(0, &literal[..8], false)).unwrap();
    missing.push(fragment(16, &literal[16..], true)).unwrap();
    assert!(missing.finish().is_err());

    let mut wrong_identity = fragment(0, &literal[..8], false);
    wrong_identity.node_id = "9:9".to_owned();
    let mut assembler =
        LargeValueAssembler::new("FileKey123", Some("v1".to_owned()), descriptor()).unwrap();
    assert!(assembler.push(wrong_identity).is_err());
}

#[test]
fn collector_resolves_every_descriptor_before_completing_the_snapshot() {
    let target =
        FigmaTarget::parse("https://www.figma.com/design/FileKey123/Fixture?node-id=1-2").unwrap();
    let mut collector =
        CollectorSession::new(CollectionRequest::new(target, CollectionScope::Node));
    let CollectorStep::Call(metadata) = collector.advance().unwrap() else {
        panic!("metadata call")
    };
    collector
        .accept(
            &metadata.id,
            UpstreamResult {
                raw: json!({"structuredContent":{"devupMetadata":{
                    "fileKey":"FileKey123","version":"v1","rootId":"1:2",
                    "nodes":[{"id":"1:2","type":"TEXT","childrenIds":[],"descendantCount":0}]
                }}}),
            },
        )
        .unwrap();
    let CollectorStep::Call(snapshot_call) = collector.advance().unwrap() else {
        panic!("snapshot call")
    };
    collector
        .accept(
            &snapshot_call.id,
            UpstreamResult {
                raw: json!({
                    "fileKey":"FileKey123","version":"v1","rootIds":["1:2"],
                    "nodes":[
                        {"id":"1:2","type":"TEXT","fields":{"characters":{"$largeValue":descriptor()}},"extra":{},"fieldErrors":{}},
                        {"id":"__DEVUP_SNAPSHOT_CURSOR__","type":"DEVUP_INTERNAL","fields":{"nextOffset":1,"complete":true,"totalNodes":1},"extra":{},"fieldErrors":{}}
                    ],"diagnostics":[]
                }),
            },
        )
        .unwrap();

    let literal = br#""hello large value""#;
    for (offset, end) in [(0, 8), (8, 16), (16, 19)] {
        let step = collector.advance().unwrap();
        let CollectorStep::Call(call) = step else {
            panic!("large value continuation call, got {step:?}")
        };
        let ReadToolCall::LargeValue { options, .. } = call.call else {
            panic!("large value read")
        };
        assert_eq!(options.offset, offset);
        collector
            .accept(
                &call.id,
                UpstreamResult {
                    raw: serde_json::to_value(fragment(offset, &literal[offset..end], end == 19))
                        .unwrap(),
                },
            )
            .unwrap();
    }

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("complete snapshot")
    };
    assert_eq!(
        parts.snapshot_chunks[0].nodes[0].fields["characters"],
        json!("hello large value")
    );
}

#[test]
fn collector_marks_large_value_as_unsupported_when_upstream_rejects_continuation() {
    let target =
        FigmaTarget::parse("https://www.figma.com/design/FileKey123/Fixture?node-id=1-2").unwrap();
    let mut collector =
        CollectorSession::new(CollectionRequest::new(target, CollectionScope::Node));
    let CollectorStep::Call(metadata) = collector.advance().unwrap() else {
        panic!("metadata call")
    };
    collector
        .accept(
            &metadata.id,
            UpstreamResult {
                raw: json!({"structuredContent":{"devupMetadata":{
                    "fileKey":"FileKey123","version":"v1","rootId":"1:2",
                    "nodes":[{"id":"1:2","type":"TEXT","childrenIds":[],"descendantCount":0}]
                }}}),
            },
        )
        .unwrap();
    let CollectorStep::Call(snapshot_call) = collector.advance().unwrap() else {
        panic!("snapshot call")
    };
    collector
        .accept(
            &snapshot_call.id,
            UpstreamResult {
                raw: json!({
                    "fileKey":"FileKey123","version":"v1","rootIds":["1:2"],
                    "nodes":[
                        {"id":"1:2","type":"TEXT","fields":{"characters":{"$largeValue":descriptor()}},"extra":{},"fieldErrors":{}},
                        {"id":"__DEVUP_SNAPSHOT_CURSOR__","type":"DEVUP_INTERNAL","fields":{"nextOffset":1,"complete":true,"totalNodes":1},"extra":{},"fieldErrors":{}}
                    ],"diagnostics":[]
                }),
            },
        )
        .unwrap();

    let CollectorStep::Call(continuation) = collector.advance().unwrap() else {
        panic!("large value continuation call")
    };
    assert!(
        collector
            .reject(
                &continuation.id,
                &devup_mcp_figma::DevupError::new(
                    devup_mcp_figma::ErrorCode::DevupSnapshotUnsupported,
                    "upstream does not support field getter",
                    false,
                ),
            )
            .unwrap()
    );

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("complete snapshot")
    };
    let node = &parts.snapshot_chunks[0].nodes[0];
    assert_eq!(
        node.fields["characters"],
        json!({"$truncated":"unsupported-by-upstream","byteLength":19})
    );
    assert_eq!(
        node.field_errors.get("characters").map(String::as_str),
        Some("DEVUP_FIELD_UNSUPPORTED_BY_UPSTREAM")
    );
    assert!(
        parts.snapshot_chunks[0]
            .diagnostics
            .iter()
            .any(|diagnostic| {
                diagnostic.code == "DEVUP_FIELD_UNSUPPORTED_BY_UPSTREAM"
                    && diagnostic.node_id.as_deref() == Some("1:2")
            })
    );
}
