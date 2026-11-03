use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::{Value, json};

fn project(resize: &str, max_lines: Value) -> devup_mcp_devup_ui::codegen::CodegenOutput {
    let snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"intrinsic-width", "roots":["frame"], "diagnostics":[],
        "nodes":{
            "frame":{"id":"frame","type":"FRAME","fields":{
                "width":360,"height":200,"layoutMode":"HORIZONTAL","childrenIds":["text"]}},
            "text":{"id":"text","type":"TEXT","fields":{
                "parentId":"frame","characters":"Regenerating Joints  ✦  Restoring Lives\nChondroSpheroid",
                "width":2400,"height":120,"textAutoResize":resize,"textTruncation":"DISABLED",
                "fontSize":80,"maxLines":max_lines}}
        }
    })).unwrap();
    generate_component(&snapshot, "frame", &CodegenOptions::default()).unwrap()
}

#[test]
fn auto_width_text_preserves_unwrapped_width_spaces_and_hard_breaks() {
    let output = project("WIDTH_AND_HEIGHT", Value::Null);
    assert!(output.tsx.contains("whiteSpace=\"pre\""), "{}", output.tsx);
    assert!(output.tsx.contains("<br />"), "{}", output.tsx);
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
            .any(|e| e.resolution == "derived-intrinsic-text-width"
                && e.property.as_deref() == Some("textAutoResize")
                && e.generated_property.as_deref() == Some("whiteSpace=\"pre\""))
    );
}

#[test]
fn fixed_width_text_retains_soft_wrapping_policy() {
    let output = project("HEIGHT", Value::Null);
    assert!(!output.tsx.contains("whiteSpace=\"pre\""));
}

#[test]
fn explicit_line_clamp_takes_precedence_over_auto_width() {
    let output = project("WIDTH_AND_HEIGHT", json!(1));
    assert!(output.tsx.contains("whiteSpace=\"nowrap\""));
    assert!(!output.tsx.contains("whiteSpace=\"pre\""));
}
