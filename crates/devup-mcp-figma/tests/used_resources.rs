use devup_mcp_figma::{
    RawNode, ReadToolCall, ResourceBatch, ResourceKind, ResourceScope, ResourceStyleRef,
    SnapshotChunk, collect_used_resource_refs,
};
use serde_json::{Map, json};

fn node(id: &str, fields: serde_json::Value) -> RawNode {
    RawNode {
        id: id.to_owned(),
        node_type: "TEXT".to_owned(),
        fields: fields.as_object().cloned().unwrap_or_else(Map::new),
        extra: Map::new(),
        field_errors: Default::default(),
    }
}

#[test]
fn exact_id_script_fetches_used_resources_without_style_consumers() {
    let call = ReadToolCall::used_resources(
        "FileKey123",
        "3879:35518",
        ResourceBatch {
            variable_ids: vec!["VariableID:12:34".to_owned()],
            styles: vec![ResourceStyleRef {
                id: "S:text".to_owned(),
                style_type: "TEXT".to_owned(),
                consumer_start: None,
                consumer_end: None,
            }],
        },
    );
    let code = call.arguments()["code"].as_str().unwrap().to_owned();

    assert!(code.contains("getVariableByIdAsync"));
    assert!(code.contains("getStyleByIdAsync"));
    assert!(code.contains("unresolved"));
    assert!(!code.contains("getLocalVariablesAsync"));
    assert!(!code.contains("getStyleConsumersAsync"));
}

#[test]
fn scanner_collects_bound_variables_and_every_supported_style_field() {
    let chunks = vec![SnapshotChunk {
        file_key: "FileKey123".to_owned(),
        version: Some("v1".to_owned()),
        root_ids: vec!["3879:35518".to_owned()],
        nodes: vec![node(
            "3879:35518",
            json!({
                "id": "ordinary-node-id-must-not-be-collected",
                "boundVariables": {
                    "fills": [
                        {"type": "VARIABLE_ALIAS", "id": "VariableID:56:78"},
                        {"type": "VARIABLE_ALIAS", "id": "VariableID:12:34"}
                    ],
                    "visible": {"type": "VARIABLE_ALIAS", "id": "VariableID:12:34"},
                    "ignored": {"type": "SOMETHING_ELSE", "id": "VariableID:99:99"}
                },
                "textStyleId": "S:text",
                "fillStyleId": "S:fill",
                "strokeStyleId": "S:stroke",
                "effectStyleId": "S:effect",
                "gridStyleId": "S:grid",
                "backgroundStyleId": "S:background",
                "styledTextSegments": [{
                    "characters": "[1. 이름]",
                    "textStyleId": "S:text-emphasis",
                    "boundVariables": {
                        "fills": [{"type": "VARIABLE_ALIAS", "id": "VariableID:90:12"}]
                    }
                }]
            }),
        )],
        diagnostics: Vec::new(),
    }];

    let refs = collect_used_resource_refs(&chunks);

    assert_eq!(
        refs.variable_ids,
        vec![
            "VariableID:12:34".to_owned(),
            "VariableID:56:78".to_owned(),
            "VariableID:90:12".to_owned(),
        ]
    );
    assert_eq!(
        refs.styles
            .iter()
            .map(|style| (style.id.as_str(), style.style_type.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("S:background", "PAINT"),
            ("S:effect", "EFFECT"),
            ("S:fill", "PAINT"),
            ("S:grid", "GRID"),
            ("S:stroke", "PAINT"),
            ("S:text", "TEXT"),
            ("S:text-emphasis", "TEXT"),
        ]
    );
    assert!(refs.occurrences.iter().any(|occurrence| {
        occurrence.node_id == "3879:35518"
            && occurrence.field == "styledTextSegments[0].boundVariables.fills[0]"
            && occurrence.resource_id == "VariableID:90:12"
            && occurrence.resource_kind == ResourceKind::Variable
    }));
}

#[test]
fn scanner_is_deterministic_and_ignores_unbound_ids_and_mixed_sentinels() {
    let chunks = vec![SnapshotChunk {
        file_key: "FileKey123".to_owned(),
        version: None,
        root_ids: vec!["2:2".to_owned(), "1:1".to_owned()],
        nodes: vec![
            node(
                "2:2",
                json!({
                    "componentProperties": {"id": "VariableID:not-bound"},
                    "textStyleId": "figma.mixed",
                    "fillStyleId": null,
                    "boundVariables": {
                        "fills": [{"type": "VARIABLE_ALIAS", "id": "VariableID:2:2"}]
                    }
                }),
            ),
            node(
                "1:1",
                json!({
                    "boundVariables": {
                        "fills": [{"type": "VARIABLE_ALIAS", "id": "VariableID:1:1"}]
                    },
                    "textStyleId": "S:body"
                }),
            ),
        ],
        diagnostics: Vec::new(),
    }];

    let first = collect_used_resource_refs(&chunks);
    let mut reversed = chunks;
    reversed[0].nodes.reverse();
    let second = collect_used_resource_refs(&reversed);

    assert_eq!(first, second);
    assert_eq!(first.variable_ids, ["VariableID:1:1", "VariableID:2:2"]);
    assert_eq!(first.styles.len(), 1);
    assert_eq!(ResourceScope::default(), ResourceScope::None);
}
