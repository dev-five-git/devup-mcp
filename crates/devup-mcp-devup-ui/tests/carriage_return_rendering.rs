use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;

#[test]
fn captured_transit_controls_remain_one_line_and_keep_raw_provenance() {
    // Captured 2026-11-02. Figma native reference-company-location PNGs paint
    // these transit suffixes on the same line, separated by a space. Their
    // captured single-line boxes are 24px desktop and 23px mobile.
    let snapshot: Snapshot = serde_json::from_str(include_str!(
        "fixtures/chondrogen_transit_carriage_returns.json"
    ))
    .unwrap();
    let original = snapshot.clone();
    for id in &snapshot.roots {
        let raw = snapshot.nodes[id]
            .typed_view()
            .string("characters")
            .unwrap();
        assert!(raw.contains('\r') && !raw.contains('\n'));
        let height = snapshot.nodes[id].typed_view().number("height").unwrap();
        assert!([23.0, 24.0].contains(&height));
        let output = generate_component(&snapshot, id, &CodegenOptions::default()).unwrap();
        assert!(!output.tsx.contains("<br"), "{}", output.tsx);
        assert!(
            output.tsx.contains(&raw.replace('\r', " ")),
            "{}",
            output.tsx
        );
        assert!(output.tsx.contains("whiteSpace=\"pre\""), "{}", output.tsx);
        assert!(
            output.fidelity_report.text.complete(),
            "{:?}",
            output.fidelity_report.text
        );
        assert!(
            output
                .source_map
                .entries
                .iter()
                .any(|entry| entry.node_id.as_deref() == Some(id)
                    && entry.property.as_deref() == Some("characters")
                    && entry.generated_range.is_some())
        );
    }
    assert_eq!(
        serde_json::to_value(snapshot).unwrap(),
        serde_json::to_value(original).unwrap()
    );
}

#[test]
fn captured_component_text_defaults_use_render_controls_without_mutating_source() {
    let mut snapshot: Snapshot = serde_json::from_value(json!({"fileKey":"cr-default","roots":["set"],"diagnostics":[],"nodes":{
        "set":{"id":"set","type":"COMPONENT_SET","fields":{"name":"Transit","childrenIds":["base"],"componentPropertyDefinitions":{"size":{"type":"VARIANT","defaultValue":"small","variantOptions":["small"]},"value#1":{"type":"TEXT","defaultValue":"First\rsecond"}}}},
        "base":{"id":"base","type":"COMPONENT","fields":{"parentId":"set","name":"Default","variantProperties":{"size":"small"},"layoutMode":"VERTICAL","childrenIds":["text"]}},
        "text":{"id":"text","type":"TEXT","fields":{"parentId":"base","name":"Value","characters":"First\rsecond","textAutoResize":"WIDTH_AND_HEIGHT","componentPropertyReferences":{"characters":"value#1"}}}
    }})).unwrap();
    let output = generate_component(&snapshot, "set", &CodegenOptions::default()).unwrap();
    assert!(
        output.tsx.contains("value = \"First second\""),
        "{}",
        output.tsx
    );
    assert!(!output.tsx.contains("\\r"), "{}", output.tsx);
    assert_eq!(snapshot.nodes["text"].fields["characters"], "First\rsecond");
    snapshot
        .nodes
        .get_mut("text")
        .unwrap()
        .fields
        .remove("componentPropertyReferences");
    let synthetic = generate_component(&snapshot, "set", &CodegenOptions::default()).unwrap();
    assert!(
        synthetic.tsx.contains("value2 = \"First second\""),
        "{}",
        synthetic.tsx
    );
}
