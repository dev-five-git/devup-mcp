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
                    "layoutMode": "HORIZONTAL", "width": 320, "height": 80,
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
        },
    )
    .expect("codegen");

    assert_eq!(
        output.tsx,
        concat!(
            "import { Flex, Text } from \"@devup-ui/react\";\n\n",
            "export function Fr026본연체() {\n",
            "  return (\n",
            "    <Flex w=\"320px\" h=\"80px\" flexDir=\"row\" gap=\"8px\" p=\"16px\" bg=\"#ffffff\">\n",
            "      <Text color=\"$primary\" fontSize=\"16px\" lineHeight=\"24px\">안녕 &lt;Devup&gt;</Text>\n",
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
