use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;

fn source(text: &str) -> Snapshot {
    serde_json::from_value(json!({"fileKey":"wrap-spaces","roots":["text"],"nodes":{"text":{"id":"text","type":"TEXT","fields":{"characters":text,"textAutoResize":"HEIGHT","layoutSizingHorizontal":"FIXED","width":220,"fontSize":16,"textTruncation":"DISABLED"}}},"diagnostics":[]})).unwrap()
}

#[test]
fn fixed_inline_text_keeps_repeated_and_edge_spaces_while_soft_wrapping() {
    for text in [
        "Company  ·   Address",
        " leading space",
        "trailing space ",
        "A wrapped  phrase whose words retain their captured spaces",
    ] {
        let snapshot = source(text);
        let output = generate_component(&snapshot, "text", &CodegenOptions::default()).unwrap();
        assert!(
            output.tsx.contains("whiteSpace=\"pre-wrap\""),
            "{}",
            output.tsx
        );
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.code == "DEVUP_CODEGEN_TEXT_WHITESPACE_COLLAPSE")
        );
        assert!(
            output
                .source_map
                .entries
                .iter()
                .any(
                    |e| e.generated_property.as_deref() == Some("whiteSpace=\"pre-wrap\"")
                        && e.property.as_deref() == Some("characters")
                        && e.resolution == "derived-wrapped-ascii-whitespace"
                )
        );
    }
}

#[test]
fn captured_desktop_and_mobile_footer_copy_preserves_actual_ascii_spacing() {
    let snapshot: Snapshot = serde_json::from_str(include_str!(
        "fixtures/chondrogen_footer_ascii_spacing.json"
    ))
    .unwrap();
    for id in &snapshot.roots {
        assert!(
            snapshot.nodes[id]
                .typed_view()
                .string("characters")
                .unwrap()
                .contains("  ")
        );
        let output = generate_component(&snapshot, id, &CodegenOptions::default()).unwrap();
        assert!(
            output.tsx.contains("whiteSpace=\"pre-wrap\""),
            "{}",
            output.tsx
        );
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.code == "DEVUP_CODEGEN_TEXT_WHITESPACE_COLLAPSE")
        );
        assert!(
            output
                .source_map
                .entries
                .iter()
                .any(|e| e.node_id.as_deref() == Some(id)
                    && e.resolution == "derived-wrapped-ascii-whitespace")
        );
    }
}

#[test]
fn ordinary_single_spaces_and_unproven_widths_keep_existing_policy() {
    for (case, mut snapshot) in [
        source("A normally wrapped phrase"),
        source("Repeated  spaces"),
        source("Repeated  spaces"),
    ]
    .into_iter()
    .enumerate()
    {
        let f = &mut snapshot.nodes.get_mut("text").unwrap().fields;
        if case == 1 {
            f.remove("width");
        } else if case == 2 {
            f.insert("layoutSizingHorizontal".into(), json!("HUG"));
        }
        let output = generate_component(&snapshot, "text", &CodegenOptions::default()).unwrap();
        assert!(
            !output.tsx.contains("whiteSpace=\"pre-wrap\""),
            "{}",
            output.tsx
        );
    }
}

#[test]
fn clamp_list_and_tab_cases_keep_existing_loss_reporting() {
    for (key, value) in [
        ("maxLines", json!(1)),
        ("maxLines", json!(2)),
        (
            "styledTextSegments",
            json!([{"characters":"A  B","listOptions":{"type":"UNORDERED"}}]),
        ),
        ("characters", json!("A\tB")),
    ] {
        let mut snapshot = source("A  B");
        snapshot
            .nodes
            .get_mut("text")
            .unwrap()
            .fields
            .insert(key.into(), value);
        let output = generate_component(&snapshot, "text", &CodegenOptions::default()).unwrap();
        assert!(
            !output.tsx.contains("whiteSpace=\"pre-wrap\""),
            "{}",
            output.tsx
        );
        assert!(
            output
                .diagnostics
                .iter()
                .any(|d| d.code == "DEVUP_CODEGEN_TEXT_WHITESPACE_COLLAPSE")
        );
        if key == "maxLines" && snapshot.nodes["text"].fields[key] == 1 {
            assert!(output.tsx.contains("whiteSpace=\"nowrap\""));
        }
    }
}
