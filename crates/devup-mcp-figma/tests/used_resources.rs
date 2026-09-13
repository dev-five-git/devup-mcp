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
    assert!(code.contains("getVariableCollectionByIdAsync"));
    assert!(code.contains("getStyleByIdAsync"));
    assert!(code.contains("unresolved"));
    assert!(!code.contains("getStyleConsumersAsync"));

    // 이 스크립트가 지켜야 하는 것은 "요청한 것만 돌려준다"이고, 그것은
    // 출력이 요청한 id 로만 만들어지는지로 확인한다.
    //
    // 전에는 `getLocalVariablesAsync` 를 부르지 않는지를 봤다. 그 호출을
    // 금지한 이유는 파일에 있는 것을 통째로 실어 응답이 커지는 것을 막는
    // 데 있었는데, 실제로 이 파일에서 재보니 개별 조회가 건당 21.5초를 쓰고
    // 끝내 null 을 내는 동안 지역 목록 한 번은 12ms 에 돌아왔다. 지금은 그
    // 목록을 색인으로만 쓰고 응답에는 요청한 id 의 결과만 담으므로, 막으려던
    // 것은 여전히 막혀 있다. 호출 이름이 아니라 그 사실을 검사한다.
    assert!(code.contains("usedVariableIds: resources.variableIds"));
    assert!(code.contains("usedStyleIds: resources.styles.map"));
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
                    "characters": "[1. Name]",
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

#[test]
fn variable_aliases_are_collected_from_any_lossless_snapshot_field() {
    let chunks = [SnapshotChunk {
        file_key: "file-key".to_owned(),
        version: None,
        root_ids: vec!["1:1".to_owned()],
        nodes: vec![node(
            "1:1",
            json!({
                "styledTextSegments": [{
                    "fontSize": {
                        "type": "VARIABLE_ALIAS",
                        "id": "VariableID:font-size"
                    }
                }]
            }),
        )],
        diagnostics: Vec::new(),
    }];

    let refs = collect_used_resource_refs(&chunks);

    assert_eq!(refs.variable_ids, ["VariableID:font-size"]);
    assert!(refs.occurrences.iter().any(|occurrence| {
        occurrence.field == "styledTextSegments[0].fontSize"
            && occurrence.resource_id == "VariableID:font-size"
            && occurrence.resource_kind == ResourceKind::Variable
    }));
}
