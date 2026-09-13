use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;

fn text(characters: &str, resize: &str) -> Snapshot {
    serde_json::from_value(json!({
        "fileKey":"test", "version":"1", "roots":["text"], "diagnostics":[],
        "nodes":{"text":{"id":"text","type":"TEXT","fields":{
            "characters":characters,"styledTextSegments":[{"characters":characters}],
            "textAutoResize":resize,"textTruncation":"DISABLED",
            "width":197,"textAlignHorizontal":"CENTER"
        },"extra":{},"fieldErrors":{}}}
    }))
    .unwrap()
}

#[test]
fn explicit_break_spaces_survive_at_fixed_inline_width() {
    for width in [197, 523] {
        for characters in [
            "First line \nSecond",
            "First \r\nSecond",
            "First \u{2028}Second",
            "First\u{2029} Second",
            "첫 줄 \n둘째 줄",
        ] {
            let mut snapshot = text(characters, "HEIGHT");
            snapshot
                .nodes
                .get_mut("text")
                .unwrap()
                .fields
                .insert("width".into(), json!(width));
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
            if characters.contains('첫') {
                assert!(output.tsx.contains("wordBreak=\"keep-all\""));
            }
        }
    }
}

#[test]
fn preserving_break_spaces_has_derived_character_provenance() {
    let snapshot = text("First \nSecond", "HEIGHT");
    let output = generate_component(&snapshot, "text", &CodegenOptions::default()).unwrap();
    assert!(
        output
            .source_map
            .entries
            .iter()
            .any(
                |entry| entry.generated_property.as_deref() == Some("whiteSpace=\"pre-wrap\"")
                    && entry.property.as_deref() == Some("characters")
                    && entry.resolution == "derived-hard-break-whitespace"
            ),
        "{:#?}",
        output.source_map
    );
}

#[test]
fn segment_boundary_spaces_use_segment_provenance() {
    let mut snapshot = text("", "HEIGHT");
    let fields = &mut snapshot.nodes.get_mut("text").unwrap().fields;
    fields.remove("characters");
    fields.insert(
        "styledTextSegments".into(),
        json!([
            {"characters":"First "}, {"characters":"\nSecond"}
        ]),
    );
    let output = generate_component(&snapshot, "text", &CodegenOptions::default()).unwrap();
    assert!(
        output.tsx.contains("whiteSpace=\"pre-wrap\""),
        "{}",
        output.tsx
    );
    assert!(
        output
            .source_map
            .entries
            .iter()
            .any(
                |entry| entry.generated_property.as_deref() == Some("whiteSpace=\"pre-wrap\"")
                    && entry.property.as_deref() == Some("styledTextSegments")
                    && entry.resolution == "derived-hard-break-whitespace"
            )
    );
}

#[test]
fn unrelated_whitespace_and_constrained_text_keep_existing_policy() {
    for characters in [
        "Single trailing ",
        " leading",
        "Two  spaces",
        "First\nSecond",
    ] {
        let output = generate_component(
            &text(characters, "HEIGHT"),
            "text",
            &CodegenOptions::default(),
        )
        .unwrap();
        assert!(!output.tsx.contains("whiteSpace=\"pre-wrap\""));
    }
    for max_lines in [1, 2] {
        let mut snapshot = text("First \nSecond", "HEIGHT");
        snapshot
            .nodes
            .get_mut("text")
            .unwrap()
            .fields
            .insert("maxLines".into(), json!(max_lines));
        let output = generate_component(&snapshot, "text", &CodegenOptions::default()).unwrap();
        assert!(!output.tsx.contains("whiteSpace=\"pre-wrap\""));
        assert!(
            output
                .diagnostics
                .iter()
                .any(|d| d.code == "DEVUP_CODEGEN_TEXT_WHITESPACE_COLLAPSE")
        );
    }
    for resize in ["NONE", "WIDTH_AND_HEIGHT", ""] {
        let output = generate_component(
            &text("First \nSecond", resize),
            "text",
            &CodegenOptions::default(),
        )
        .unwrap();
        assert!(!output.tsx.contains("whiteSpace=\"pre-wrap\""));
    }
    for list in [false, true] {
        let mut snapshot = text("First \nSecond", "HEIGHT");
        if list {
            snapshot.nodes.get_mut("text").unwrap().fields.insert(
                "styledTextSegments".into(),
                json!([
                    {"characters":"First \nSecond", "listOptions":{"type":"UNORDERED"}}
                ]),
            );
        } else {
            snapshot
                .nodes
                .get_mut("text")
                .unwrap()
                .fields
                .insert("characters".into(), json!("First\t \nSecond"));
        }
        let output = generate_component(&snapshot, "text", &CodegenOptions::default()).unwrap();
        assert!(!output.tsx.contains("whiteSpace=\"pre-wrap\""));
        assert!(
            output
                .diagnostics
                .iter()
                .any(|d| d.code == "DEVUP_CODEGEN_TEXT_WHITESPACE_COLLAPSE")
        );
    }
}
