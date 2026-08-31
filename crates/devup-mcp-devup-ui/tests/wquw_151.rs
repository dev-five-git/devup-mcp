use devup_mcp_devup_ui::codegen::{CodegenOptions, RootLayout, generate_component};
use devup_mcp_figma::{
    CollectedPayload, CollectionScope, CollectionStats, FigmaTarget, PayloadCompleteness, Snapshot,
    UpstreamResult,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActualFigmaFixture {
    source: FixtureSource,
    snapshot: Snapshot,
    resources: UpstreamResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureSource {
    file_key: String,
    node_id: String,
    capture: String,
    node_count: usize,
}

#[test]
fn actual_wquw_151_screen_preserves_children_tokens_and_typography() {
    let fixture: ActualFigmaFixture =
        serde_json::from_str(include_str!("fixtures/wquw-151-proofread.json"))
            .expect("actual WQUW-151 Figma fixture");
    assert_eq!(
        fixture.source.capture,
        "official Figma MCP use_figma read-only projection"
    );
    assert_eq!(fixture.snapshot.nodes.len(), fixture.source.node_count);
    assert_eq!(fixture.snapshot.nodes.len(), 144);

    let payload = CollectedPayload {
        target: FigmaTarget {
            file_key: fixture.source.file_key,
            node_id: Some(fixture.source.node_id.clone()),
            branch_key: None,
        },
        scope: CollectionScope::Node,
        metadata: Value::Null,
        snapshot: fixture.snapshot,
        variables: Some(fixture.resources.clone()),
        styles: Some(fixture.resources),
        completeness: PayloadCompleteness::UsedTokens,
        source_version: None,
        stats: CollectionStats::default(),
    };
    let output = generate_component(
        &payload.snapshot,
        &fixture.source.node_id,
        &CodegenOptions {
            include_diagnostics: true,
            inline_instances: true,
            ..CodegenOptions::default()
        }
        .with_payload_tokens(&payload),
    )
    .expect("WQUW-151 DevupUI output");

    assert!(output.tsx.contains("이야기가 글로"));
    assert!(output.tsx.contains("작은 시장, 큰 사랑"));
    assert!(
        output
            .tsx
            .contains("학교를 마치고 집으로 돌아오던 늦은 오후였다.")
    );
    assert!(output.tsx.contains("[1. 이름]"));
    for token in [
        "$backgroundLight",
        "$containerBackground",
        "$primary",
        "$text",
    ] {
        assert!(output.tsx.contains(token), "missing token {token}");
    }
    for typography in ["h3", "body", "bodySemibold"] {
        assert!(
            output.tsx.contains(&format!("typography=\"{typography}\"")),
            "missing typography {typography}"
        );
    }
    assert!(output.tsx.matches("<VStack").count() >= 10);
    assert!(output.tsx.matches("<Text").count() >= 20);
    assert!(output.tsx.contains("borderTop=\"solid 1px $border\""));
    assert_eq!(
        output.tsx.matches("border=\"solid 1px $border\"").count(),
        1,
        "the public-setting card keeps its border, while the footer uses only borderTop"
    );
    let source_entry = |node_id: &str, property: &str| {
        output
            .source_map
            .entries
            .iter()
            .find(|entry| {
                entry.node_id.as_deref() == Some(node_id)
                    && entry.property.as_deref() == Some(property)
            })
            .expect("WQUW-151 provenance entry")
    };
    let heading_typography = source_entry("3879:35520", "textStyleId");
    assert_eq!(
        heading_typography.style_id.as_deref(),
        Some("S:b2c215baee1a675e5d0bbe9e39c21bf78a39eabc,")
    );
    let nested_placeholder = output
        .source_map
        .entries
        .iter()
        .find(|entry| {
            entry.node_id.as_deref() == Some("3879:35555")
                && entry.property.as_deref() == Some("styledTextSegments")
                && entry.variable_id.as_deref() == Some("VariableID:98:3276")
        })
        .expect("nested placeholder color provenance");
    assert_eq!(nested_placeholder.resolution, "variable-token");
    let footer_border = source_entry("3879:35564", "strokes");
    assert_eq!(
        footer_border.variable_id.as_deref(),
        Some("VariableID:1:1001")
    );
    let source_map_json = serde_json::to_string(&output.source_map).unwrap();
    assert!(!source_map_json.contains("작은 시장"));
    assert!(!source_map_json.contains("[1. 이름]"));

    let embedded = generate_component(
        &payload.snapshot,
        &fixture.source.node_id,
        &CodegenOptions {
            include_diagnostics: true,
            inline_instances: true,
            root_layout: RootLayout::Embedded,
            ..CodegenOptions::default()
        }
        .with_payload_tokens(&payload),
    )
    .expect("embedded WQUW-151 DevupUI output");
    let embedded_root = component_root_opening(&embedded.tsx);
    assert!(!embedded_root.contains("h=\"740px\""));
    assert!(!embedded_root.contains("w=\"360px\""));
    assert!(!embedded_root.contains("pos=\"relative\""));
    assert!(embedded.tsx.contains("bottom=\"0px\""));
    assert!(embedded.tsx.contains("pos=\"absolute\""));

    insta::assert_snapshot!("wquw_151_proofread_devup_ui", output.tsx);
    insta::assert_snapshot!("wquw_151_proofread_devup_ui_embedded", embedded.tsx);
    insta::assert_json_snapshot!("wquw_151_proofread_used_tokens", output.used_tokens);
    insta::assert_json_snapshot!("wquw_151_proofread_diagnostics", json!(output.diagnostics));
    insta::assert_json_snapshot!("wquw_151_proofread_source_map", output.source_map);
}

fn component_root_opening(tsx: &str) -> &str {
    let start = tsx.find("    <").expect("component root");
    let end = tsx[start..].find('>').expect("root opening close") + start;
    &tsx[start..=end]
}
