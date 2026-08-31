use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component, normalize_component_name};
use devup_mcp_figma::{SnapshotChunk, merge_chunks};
use pretty_assertions::assert_eq;
use serde_json::json;

fn snapshot() -> devup_mcp_figma::Snapshot {
    let chunk: SnapshotChunk = serde_json::from_value(json!({
        "fileKey": "file-key",
        "version": "1",
        "rootIds": ["1:1"],
        "nodes": [
            {
                "id": "1:1", "type": "FRAME",
                "fields": {
                    "name": "[FR-026] 본연체", "childrenIds": ["1:2"],
                    "layoutMode": "HORIZONTAL", "inferredAutoLayout": {"layoutMode": "HORIZONTAL", "itemSpacing": 8},
                    "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                    "width": 320, "height": 80,
                    "itemSpacing": 8, "paddingTop": 16, "paddingRight": 16,
                    "paddingBottom": 16, "paddingLeft": 16,
                    "fills": [{"type": "SOLID", "color": {"r": 1, "g": 1, "b": 1}, "opacity": 1}]
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:2", "type": "TEXT",
                "fields": {
                    "name": "Label", "childrenIds": [], "characters": "안녕 <Devup>",
                    "textTruncation": "DISABLED",
                    "fontSize": 16, "lineHeight": {"unit": "PIXELS", "value": 24},
                    "devupTokens": {"fills": "primary"}
                },
                "extra": {}, "fieldErrors": {}
            }
        ],
        "diagnostics": []
    }))
    .expect("synthetic snapshot");
    merge_chunks(vec![chunk]).expect("snapshot")
}

#[test]
fn generates_deterministic_devup_ui_tsx() {
    let output = generate_component(
        &snapshot(),
        "1:1",
        &CodegenOptions {
            component_name: None,
            include_diagnostics: true,
            ..CodegenOptions::default()
        },
    )
    .expect("codegen");

    assert_eq!(
        output.tsx,
        concat!(
            "import { Flex, Text } from \"@devup-ui/react\";\n\n",
            "export function Fr026본연체() {\n",
            "  return (\n",
            "    <Flex bg=\"#FFF\" h=\"80px\" p=\"16px\" w=\"320px\">\n",
            "      <Text boxSize=\"100%\" color=\"$primary\" fontSize=\"16px\" lineHeight=\"24px\">\n",
            "        안녕 {\"<\"}Devup{\">\"}\n",
            "      </Text>\n",
            "    </Flex>\n",
            "  );\n",
            "}\n"
        )
    );
    assert_eq!(output.imports, vec!["Flex", "Text"]);
    assert_eq!(
        output.used_tokens.into_iter().collect::<Vec<_>>(),
        ["primary"]
    );
}

#[test]
fn normalizes_names_to_valid_typescript_identifiers() {
    assert_eq!(normalize_component_name("[FR-026] 본연체"), "Fr026본연체");
    assert_eq!(normalize_component_name("123"), "_123");
    assert_eq!(normalize_component_name("---"), "FigmaComponent");
}

#[test]
fn records_explicit_diagnostics_for_unsupported_visuals() {
    let mut snapshot = snapshot();
    let node = snapshot.nodes.get_mut("1:1").unwrap();
    node.fields.insert("isMask".into(), json!(true));
    node.fields
        .insert("layoutPositioning".into(), json!("ABSOLUTE"));
    node.fields
        .insert("effects".into(), json!([{"type": "BACKGROUND_BLUR"}]));

    let output = generate_component(&snapshot, "1:1", &CodegenOptions::default()).unwrap();
    let codes = output
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"DEVUP_CODEGEN_MASK_FALLBACK"));
    assert!(codes.contains(&"DEVUP_CODEGEN_ABSOLUTE_FALLBACK"));
    assert!(codes.contains(&"DEVUP_CODEGEN_EFFECT_FALLBACK"));
}

#[test]
fn nested_text_style_uses_typography() {
    let chunk: SnapshotChunk = serde_json::from_value(json!({
        "fileKey": "file-key",
        "version": "1",
        "rootIds": ["2:1"],
        "nodes": [{
            "id": "2:1",
            "type": "TEXT",
            "fields": {
                "name": "Mixed story",
                "childrenIds": [],
                "characters": "우리 [1. 이름] 왔어?\n다음 줄",
                "textTruncation": "DISABLED",
                "fills": {"$unsupported": "symbol"},
                "styledTextSegments": [
                    {
                        "characters": "우리 ",
                        "start": 0,
                        "end": 3,
                        "textStyleId": "S:body",
                        "fontName": {"family": "Pretendard", "style": "Regular"},
                        "fontSize": 16,
                        "fontWeight": 400,
                        "fills": [{
                            "type": "SOLID",
                            "color": {"r": 0, "g": 0, "b": 0},
                            "boundVariables": {"color": {"type": "VARIABLE_ALIAS", "id": "VariableID:text"}}
                        }]
                    },
                    {
                        "characters": "[1. 이름]",
                        "start": 3,
                        "end": 10,
                        "textStyleId": "S:bodySemibold",
                        "fontName": {"family": "Pretendard", "style": "SemiBold"},
                        "fontSize": 16,
                        "fontWeight": 600,
                        "fills": [{
                            "type": "SOLID",
                            "color": {"r": 0.2, "g": 0.4, "b": 1},
                            "boundVariables": {"color": {"type": "VARIABLE_ALIAS", "id": "VariableID:primaryLight"}}
                        }]
                    },
                    {
                        "characters": " 왔어?\n다음 줄",
                        "start": 10,
                        "end": 20,
                        "textStyleId": "S:body",
                        "fontName": {"family": "Pretendard", "style": "Regular"},
                        "fontSize": 16,
                        "fontWeight": 400,
                        "fills": [{
                            "type": "SOLID",
                            "color": {"r": 0, "g": 0, "b": 0},
                            "boundVariables": {"color": {"type": "VARIABLE_ALIAS", "id": "VariableID:text"}}
                        }]
                    }
                ]
            },
            "extra": {},
            "fieldErrors": {}
        }],
        "diagnostics": []
    }))
    .unwrap();
    let snapshot = merge_chunks(vec![chunk]).unwrap();
    let options = CodegenOptions {
        text_style_tokens: [
            ("S:body".to_owned(), "body".to_owned()),
            ("S:bodySemibold".to_owned(), "bodySemibold".to_owned()),
        ]
        .into_iter()
        .collect(),
        variable_tokens: [
            ("VariableID:text".to_owned(), "text".to_owned()),
            (
                "VariableID:primaryLight".to_owned(),
                "primaryLight".to_owned(),
            ),
        ]
        .into_iter()
        .collect(),
        ..CodegenOptions::default()
    };

    let output = generate_component(&snapshot, "2:1", &options).unwrap();

    assert!(output.tsx.contains("typography=\"body\""));
    assert!(output.tsx.contains("color=\"$text\""));
    assert!(output.tsx.contains("typography=\"bodySemibold\""));
    assert!(output.tsx.contains("color=\"$primaryLight\""));
    assert!(output.used_tokens.contains("text"));
    assert!(output.used_tokens.contains("primaryLight"));
    assert!(output.tsx.contains("{\" \"}왔어?<br />다음 줄"));
    assert!(!output.tsx.contains("fontSize=\"16px\""));
    assert!(!output.tsx.contains("fontWeight=\"600\""));
}

#[test]
fn standalone_instance_inlining_uses_resolved_children_not_component_props() {
    let chunk: SnapshotChunk = serde_json::from_value(json!({
        "fileKey": "file-key",
        "version": "1",
        "rootIds": ["3:1"],
        "nodes": [
            {
                "id": "3:1", "type": "FRAME",
                "fields": {"name": "Screen", "childrenIds": ["3:2"], "layoutMode": "VERTICAL"},
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "3:2", "type": "INSTANCE",
                "fields": {
                    "name": "SectionTitle", "childrenIds": ["3:3"], "layoutMode": "HORIZONTAL",
                    "componentPropertyReferences": {"visible": "Visible#1:2"}
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "3:3", "type": "TEXT",
                "fields": {"name": "Resolved label", "childrenIds": [], "characters": "실제 자식"},
                "extra": {}, "fieldErrors": {}
            }
        ],
        "diagnostics": []
    }))
    .unwrap();
    let snapshot = merge_chunks(vec![chunk]).unwrap();

    let output = generate_component(
        &snapshot,
        "3:1",
        &CodegenOptions {
            inline_instances: true,
            ..CodegenOptions::default()
        },
    )
    .unwrap();

    assert!(output.tsx.contains("실제 자식"));
    assert!(!output.tsx.contains("Visible"));
    assert!(!output.tsx.contains("<SectionTitle"));
}
