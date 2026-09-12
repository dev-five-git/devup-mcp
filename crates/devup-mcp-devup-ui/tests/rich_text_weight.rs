use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;

/// Korean text keeps `word-break: keep-all`, and this test exists to stop it
/// being removed again as a fidelity optimisation. Figma breaks Korean
/// mid-word, so deleting this measurably narrows the pixel gap - by 0.21
/// percent on one screen - while chopping every Korean word in the generated
/// screen and breaking 38 plugin byte-parity goldens. Figma's behaviour is a
/// limitation to compensate for, not a specification to reproduce; the plugin
/// makes the same call in its own text renderer.
#[test]
fn korean_characters_keep_words_whole() {
    let snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"test", "version":"1", "roots":["t"], "diagnostics":[],
        "nodes":{"t":{"id":"t","type":"TEXT","fields":{
            "characters":"정신건강간호사입니다", "width":100,
            "layoutSizingHorizontal":"FIXED", "textAutoResize":"HEIGHT",
            "styledTextSegments":[{"characters":"정신건강간호사입니다"}]
        },"extra":{},"fieldErrors":{}}}
    }))
    .unwrap();
    let output = generate_component(&snapshot, "t", &CodegenOptions::default()).unwrap();
    assert!(
        output.tsx.contains("wordBreak=\"keep-all\""),
        "{}",
        output.tsx
    );
}

/// The weight fix must not start emitting the constraint on text that has no
/// Korean in it.
#[test]
fn latin_only_text_gets_no_word_break_constraint() {
    let snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"test", "version":"1", "roots":["t"], "diagnostics":[],
        "nodes":{"t":{"id":"t","type":"TEXT","fields":{
            "characters":"Mental health nurse", "width":100,
            "layoutSizingHorizontal":"FIXED", "textAutoResize":"HEIGHT",
            "styledTextSegments":[{"characters":"Mental health nurse"}]
        },"extra":{},"fieldErrors":{}}}
    }))
    .unwrap();
    let output = generate_component(&snapshot, "t", &CodegenOptions::default()).unwrap();
    assert!(!output.tsx.contains("wordBreak"), "{}", output.tsx);
}

#[test]
fn shared_typography_token_preserves_resolved_weight_and_segment_provenance() {
    let snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"test", "version":"1", "roots":["t"], "diagnostics":[],
        "nodes":{"t":{"id":"t","type":"TEXT","fields":{
            "characters":"Bold regular text continues",
            "styledTextSegments":[
                {"characters":"Bold", "textStyleId":"body", "fontWeight":700},
                {"characters":" regular text continues", "textStyleId":"body", "fontWeight":400}
            ]
        },"extra":{},"fieldErrors":{}}}
    }))
    .unwrap();
    let options = CodegenOptions {
        text_style_tokens: [("body".into(), "body".into())].into(),
        ..CodegenOptions::default()
    };
    let output = generate_component(&snapshot, "t", &options).unwrap();
    assert!(output.tsx.contains("fontWeight=\"700\""), "{}", output.tsx);
    assert!(output.tsx.contains("fontWeight=\"400\""), "{}", output.tsx);
    assert!(
        output
            .source_map
            .entries
            .iter()
            .any(
                |entry| entry.generated_property.as_deref() == Some("fontWeight=\"400\"")
                    && entry.property.as_deref() == Some("styledTextSegments")
            ),
        "{:#?}",
        output.source_map
    );
    assert!(
        output
            .source_map
            .entries
            .iter()
            .any(
                |entry| entry.generated_property.as_deref() == Some("fontWeight=\"700\"")
                    && entry.property.as_deref() == Some("styledTextSegments")
            ),
        "{:#?}",
        output.source_map
    );
}
