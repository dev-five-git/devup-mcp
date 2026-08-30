use devup_mcp_figma::{
    CollectionRequest, CollectionScope, CollectorSession, CollectorStep, ErrorCode, FigmaTarget,
    UpstreamResult,
};
use serde_json::json;

fn target(node_id: &str) -> FigmaTarget {
    FigmaTarget::parse(&format!(
        "https://www.figma.com/design/FileKey123/Fixture?node-id={}",
        node_id.replace(':', "-")
    ))
    .unwrap()
}

fn metadata(node_type: &str, children: &[&str], descendant_count: usize) -> UpstreamResult {
    UpstreamResult {
        raw: json!({
            "structuredContent": {
                "devupMetadata": {
                    "fileKey": "FileKey123",
                    "version": "v1",
                    "rootId": "1:2",
                    "nodes": [{
                        "id": "1:2",
                        "type": node_type,
                        "childrenIds": children,
                        "descendantCount": descendant_count
                    }]
                }
            }
        }),
    }
}

fn snapshot(root_id: &str) -> UpstreamResult {
    UpstreamResult {
        raw: json!({
            "fileKey": "FileKey123",
            "version": "v1",
            "rootIds": [root_id],
            "nodes": [{
                "id": root_id,
                "type": "FRAME",
                "fields": {"name": "Synthetic"},
                "extra": {},
                "fieldErrors": {}
            }]
        }),
    }
}

#[test]
fn node_collection_advances_from_metadata_to_snapshot_to_complete() {
    let request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    let mut collector = CollectorSession::new(request);

    let CollectorStep::Call(metadata_call) = collector.advance().unwrap() else {
        panic!("metadata call expected")
    };
    assert_eq!(metadata_call.call.tool_name(), "get_metadata");
    collector
        .accept(&metadata_call.id, metadata("FRAME", &[], 1))
        .unwrap();

    let CollectorStep::Call(snapshot_call) = collector.advance().unwrap() else {
        panic!("snapshot call expected")
    };
    assert_eq!(snapshot_call.call.tool_name(), "use_figma");
    assert_eq!(snapshot_call.expected_node_id.as_deref(), Some("1:2"));
    collector
        .accept(&snapshot_call.id, snapshot("1:2"))
        .unwrap();

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("collection should be complete")
    };
    assert_eq!(parts.target.node_id.as_deref(), Some("1:2"));
    assert_eq!(parts.source_version.as_deref(), Some("v1"));
    assert_eq!(parts.snapshot_chunks.len(), 1);
}

#[test]
fn large_page_is_split_in_declared_child_order() {
    let request = CollectionRequest::new(target("1:2"), CollectionScope::Page);
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(call) = collector.advance().unwrap() else {
        panic!()
    };
    collector
        .accept(&call.id, metadata("PAGE", &["1:3", "1:4"], 500))
        .unwrap();

    let CollectorStep::Call(first) = collector.advance().unwrap() else {
        panic!()
    };
    let CollectorStep::Call(second) = collector.advance().unwrap() else {
        panic!()
    };
    assert_eq!(first.expected_node_id.as_deref(), Some("1:3"));
    assert_eq!(second.expected_node_id.as_deref(), Some("1:4"));

    collector.accept(&second.id, snapshot("1:4")).unwrap();
    collector.accept(&first.id, snapshot("1:3")).unwrap();
    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!()
    };
    let root_order = parts
        .snapshot_chunks
        .iter()
        .flat_map(|chunk| chunk.root_ids.iter().map(String::as_str))
        .collect::<Vec<_>>();
    assert_eq!(root_order, ["1:3", "1:4"]);
}

#[test]
fn rejects_snapshot_from_a_different_file_version() {
    let request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(metadata_call) = collector.advance().unwrap() else {
        panic!()
    };
    collector
        .accept(&metadata_call.id, metadata("FRAME", &[], 1))
        .unwrap();
    let CollectorStep::Call(snapshot_call) = collector.advance().unwrap() else {
        panic!()
    };
    let mut wrong_version = snapshot("1:2");
    wrong_version.raw["version"] = json!("v2");

    let error = collector
        .accept(&snapshot_call.id, wrong_version)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupFigmaVersionChanged);
}

#[test]
fn exposes_at_most_four_in_flight_calls() {
    let request = CollectionRequest::new(target("1:2"), CollectionScope::Page);
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(metadata_call) = collector.advance().unwrap() else {
        panic!()
    };
    collector
        .accept(
            &metadata_call.id,
            metadata("PAGE", &["1:3", "1:4", "1:5", "1:6", "1:7"], 500),
        )
        .unwrap();

    let mut issued = Vec::new();
    for _ in 0..4 {
        let CollectorStep::Call(call) = collector.advance().unwrap() else {
            panic!("four calls should be available")
        };
        issued.push(call);
    }
    assert!(matches!(
        collector.advance().unwrap(),
        CollectorStep::AwaitingResults
    ));

    collector.accept(&issued[0].id, snapshot("1:3")).unwrap();
    let CollectorStep::Call(fifth) = collector.advance().unwrap() else {
        panic!("the fifth call should be released after one result")
    };
    assert_eq!(fifth.expected_node_id.as_deref(), Some("1:7"));
}

#[test]
fn rejects_unknown_or_replayed_call_ids() {
    let request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(call) = collector.advance().unwrap() else {
        panic!()
    };

    let unknown = collector
        .accept("unknown", metadata("FRAME", &[], 1))
        .unwrap_err();
    assert_eq!(unknown.code, ErrorCode::DevupFigmaHandoffInvalid);

    collector
        .accept(&call.id, metadata("FRAME", &[], 1))
        .unwrap();
    let replayed = collector
        .accept(&call.id, metadata("FRAME", &[], 1))
        .unwrap_err();
    assert_eq!(replayed.code, ErrorCode::DevupFigmaHandoffInvalid);
}
