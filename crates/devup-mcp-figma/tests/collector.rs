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

fn official_xml_metadata() -> UpstreamResult {
    UpstreamResult {
        raw: json!({
            "content": [
                {
                    "type": "text",
                    "text": "<frame id=\"1:2\" name=\"Synthetic Root\" x=\"0\" y=\"0\" width=\"320\" height=\"240\"><text id=\"1:3\" name=\"Synthetic Child\" x=\"8\" y=\"8\" width=\"100\" height=\"20\" /></frame>"
                },
                {
                    "type": "text",
                    "text": "Synthetic guidance that is not metadata"
                }
            ]
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

#[test]
fn variable_collection_uses_catalog_then_batched_resources() {
    let mut request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    request.include_variables = true;
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
    collector
        .accept(&snapshot_call.id, snapshot("1:2"))
        .unwrap();

    let CollectorStep::Call(catalog_call) = collector.advance().unwrap() else {
        panic!("variable/style catalog should follow the snapshot")
    };
    let catalog_code = catalog_call.call.arguments()["code"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(catalog_code.contains("getLocalVariableCollectionsAsync"));
    assert!(!catalog_code.contains("getLocalVariablesAsync"));
    let catalog = UpstreamResult {
        raw: json!({
            "content": [{
                "type": "text",
                "text": "{\"collections\":[{\"id\":\"c1\",\"name\":\"Synthetic\",\"defaultModeId\":\"m1\",\"modes\":[]}],\"variableIds\":[\"v1\"],\"styles\":[{\"id\":\"s1\",\"styleType\":\"TEXT\"}],\"localComplete\":true,\"usedRemoteComplete\":false}"
            }]
        }),
    };
    collector.accept(&catalog_call.id, catalog).unwrap();

    let CollectorStep::Call(batch_call) = collector.advance().unwrap() else {
        panic!("resource batch should follow the catalog")
    };
    let batch_code = batch_call.call.arguments()["code"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(batch_code.contains("getVariableByIdAsync"));
    assert!(batch_code.contains("getStyleByIdAsync"));
    let variable_batch = UpstreamResult {
        raw: json!({
            "content": [{
                "type": "text",
                "text": "{\"variables\":[{\"id\":\"v1\",\"name\":\"Synthetic Variable\"}],\"styles\":[]}"
            }]
        }),
    };
    collector.accept(&batch_call.id, variable_batch).unwrap();
    let CollectorStep::Call(style_call) = collector.advance().unwrap() else {
        panic!("style batch should be separate")
    };
    collector
        .accept(
            &style_call.id,
            UpstreamResult {
                raw: json!({
                    "variables": [],
                    "styles": [{"id": "s1", "name": "Synthetic Style", "styleType": "TEXT", "value": {}}]
                }),
            },
        )
        .unwrap();

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("collection should finish after variables")
    };
    let merged = &parts.variables.as_ref().unwrap().raw;
    assert_eq!(merged["collections"][0]["id"], "c1");
    assert_eq!(merged["variables"][0]["id"], "v1");
    assert_eq!(merged["styles"][0]["id"], "s1");
    assert_eq!(parts.styles.as_ref().unwrap().raw, *merged);
}

#[test]
fn accepts_the_official_xml_metadata_envelope_without_inventing_values() {
    let request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(metadata_call) = collector.advance().unwrap() else {
        panic!()
    };

    collector
        .accept(&metadata_call.id, official_xml_metadata())
        .unwrap();
    let CollectorStep::Call(snapshot_call) = collector.advance().unwrap() else {
        panic!()
    };
    assert_eq!(snapshot_call.expected_file_key, "FileKey123");
    assert_eq!(snapshot_call.expected_node_id.as_deref(), Some("1:2"));
}

#[test]
fn variable_batches_merge_in_catalog_order_when_results_arrive_out_of_order() {
    let mut request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    request.include_variables = true;
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
    collector
        .accept(&snapshot_call.id, snapshot("1:2"))
        .unwrap();
    let CollectorStep::Call(catalog_call) = collector.advance().unwrap() else {
        panic!()
    };
    let variable_ids = (0..2)
        .map(|index| format!("v{index:02}"))
        .collect::<Vec<_>>();
    collector
        .accept(
            &catalog_call.id,
            UpstreamResult {
                raw: json!({
                    "collections": [],
                    "variableIds": variable_ids,
                    "styles": [],
                    "localComplete": true,
                    "usedRemoteComplete": false
                }),
            },
        )
        .unwrap();
    let CollectorStep::Call(first) = collector.advance().unwrap() else {
        panic!()
    };
    let CollectorStep::Call(second) = collector.advance().unwrap() else {
        panic!()
    };
    collector
        .accept(
            &second.id,
            UpstreamResult {
                raw: json!({"variables": [{"id": "v01"}], "styles": []}),
            },
        )
        .unwrap();
    collector
        .accept(
            &first.id,
            UpstreamResult {
                raw: json!({
                    "variables": [{"id": "v00"}],
                    "styles": []
                }),
            },
        )
        .unwrap();
    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!()
    };
    let variables = parts.variables.unwrap();
    let ids = variables.raw["variables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|variable| variable["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ids.first().copied(), Some("v00"));
    assert_eq!(ids.last().copied(), Some("v01"));
}
