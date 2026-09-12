use devup_mcp_devup_ui::{
    codegen::{CodegenOptions, generate_component},
    theme::{ThemeScope, VariableSnapshot, generate_devup_json},
};
use devup_mcp_figma::Snapshot;
use serde_json::{Value, json};

fn styled_snapshot(segment: Value) -> Snapshot {
    serde_json::from_value(json!({
        "fileKey":"test", "version":"1", "roots":["t"], "diagnostics":[],
        "nodes":{"t":{"id":"t","type":"TEXT","fields":{
            "characters":"Heading", "styledTextSegments":[segment]
        },"extra":{},"fieldErrors":{}}}
    }))
    .unwrap()
}

#[test]
fn reused_token_gets_resolved_size_and_advance_together() {
    let options = CodegenOptions {
        text_style_tokens: [("s".into(), "heading".into())].into(),
        ..CodegenOptions::default()
    };
    let snapshot = styled_snapshot(json!({"characters":"Heading", "textStyleId":"s",
        "fontSize":52,"lineHeight":{"unit":"PERCENT","value":130}}));
    let output = generate_component(&snapshot, "t", &options).unwrap();
    assert!(output.tsx.contains("fontSize=\"52px\""), "{}", output.tsx);
    assert!(output.tsx.contains("lineHeight=\"68px\""), "{}", output.tsx);
}

#[test]
fn rich_text_advance_has_segment_provenance() {
    let mut snapshot = styled_snapshot(json!({"characters":"Long default heading", "fontSize":38,
        "lineHeight":{"unit":"PERCENT","value":130}}));
    snapshot.nodes.get_mut("t").unwrap().fields.insert("styledTextSegments".into(), json!([
        {"characters":"Long default heading", "fontSize":38,"lineHeight":{"unit":"PERCENT","value":130}},
        {"characters":"small", "fontSize":18,"lineHeight":{"unit":"PERCENT","value":130}}
    ]));
    let output = generate_component(&snapshot, "t", &CodegenOptions::default()).unwrap();
    assert!(output.tsx.contains("lineHeight=\"23px\""), "{}", output.tsx);
    assert!(
        output
            .source_map
            .entries
            .iter()
            .any(
                |entry| entry.generated_property.as_deref() == Some("lineHeight=\"23px\"")
                    && entry.property.as_deref() == Some("styledTextSegments")
            )
    );
}

#[test]
fn token_size_override_without_line_metrics_is_refused() {
    let options = CodegenOptions {
        text_style_tokens: [("s".into(), "heading".into())].into(),
        ..CodegenOptions::default()
    };
    let snapshot =
        styled_snapshot(json!({"characters":"Heading", "textStyleId":"s", "fontSize":52}));
    assert!(generate_component(&snapshot, "t", &options).is_err());
}

#[test]
fn variable_font_size_cannot_publish_a_fixed_percentage_advance() {
    let snapshot: VariableSnapshot = serde_json::from_value(json!({
        "styles":[{"id":"s","name":"heading","styleType":"TEXT","value":{
            "fontSize":52,"lineHeight":{"unit":"PERCENT","value":130},
            "boundVariables":{"fontSize":{"type":"VARIABLE_ALIAS","id":"size"}}
        }}]
    }))
    .unwrap();
    assert!(generate_devup_json(&snapshot, ThemeScope::File).is_err());
}

#[test]
fn percentage_style_uses_its_own_integer_advance() {
    for (size, advance) in [(38, "49px"), (18, "23px"), (52, "68px"), (36, "47px")] {
        let snapshot: VariableSnapshot = serde_json::from_value(json!({
            "styles": [{"id":"s", "name":"heading", "styleType":"TEXT", "value": {
                "fontSize":size, "lineHeight":{"unit":"PERCENT","value":129.99999523162842}
            }}]
        }))
        .unwrap();
        let output = generate_devup_json(&snapshot, ThemeScope::File).unwrap();
        let theme: Value = serde_json::from_str(&output.json).unwrap();
        assert_eq!(
            theme["theme"]["typography"]["heading"]["lineHeight"], advance,
            "size {size}"
        );
    }
}

#[test]
fn inline_text_uses_integer_advance() {
    let snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"test", "version":"1", "roots":["t"], "diagnostics":[],
        "nodes":{"t":{"id":"t","type":"TEXT","fields":{
            "characters":"Heading", "fontSize":52,
            "lineHeight":{"unit":"PERCENT","value":129.99999523162842}
        },"extra":{},"fieldErrors":{}}}
    }))
    .unwrap();
    let output = generate_component(&snapshot, "t", &CodegenOptions::default()).unwrap();
    assert!(output.tsx.contains("lineHeight=\"68px\""), "{}", output.tsx);
}

#[test]
fn inside_stroke_without_padding_does_not_expand_hugged_content() {
    let snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"test", "version":"1", "roots":["f"], "diagnostics":[],
        "nodes":{"f":{"id":"f","type":"FRAME","fields":{
            "childrenIds":["c"], "layoutMode":"HORIZONTAL",
            "layoutSizingHorizontal":"HUG", "layoutSizingVertical":"HUG",
            "paddingTop":0,"paddingBottom":0,"paddingLeft":0,"paddingRight":0,
            "strokeAlign":"INSIDE","strokeWeight":1,
            "strokes":[{"type":"SOLID","color":{"r":0,"g":0,"b":0}}]
        },"extra":{},"fieldErrors":{}},
        "c":{"id":"c","type":"FRAME","fields":{
            "parentId":"f","width":100,"height":44,
            "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED"
        },"extra":{},"fieldErrors":{}}}
    }))
    .unwrap();
    let output = generate_component(&snapshot, "f", &CodegenOptions::default()).unwrap();
    assert!(
        output.tsx.contains("outline=\"solid 1px #000\""),
        "{}",
        output.tsx
    );
    assert!(
        output.tsx.contains("outlineOffset=\"-1px\""),
        "{}",
        output.tsx
    );
    assert!(!output.tsx.contains("border="), "{}", output.tsx);
}
