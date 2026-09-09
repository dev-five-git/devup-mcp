//! What the generated code does when a value is bound to a variable or a
//! style whose name never arrived.
//!
//! The binding is in the snapshot either way - the node says which variable
//! paints it - so the design's own answer is "this is a token". If the
//! resource catalog did not carry that variable, the token name is unknown
//! and the only thing left to write is the resolved value. Writing it is
//! right: the module still has to compile and render. Writing it *silently*
//! is not, because the response then grades the conversion `exact` while the
//! code has a hardcoded `#7d7f83` where the design has `$caption`, and
//! nothing in the answer says so.

use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::{SnapshotChunk, merge_chunks};
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
                    "name": "Card", "childrenIds": ["1:2"],
                    "layoutMode": "VERTICAL",
                    "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                    "width": 320, "height": 80,
                    // Bound to a variable the catalog never carried.
                    "fills": [{
                        "type": "SOLID",
                        "color": {"r": 0.49, "g": 0.498, "b": 0.514, "a": 1},
                        "boundVariables": {"color": {"type": "VARIABLE_ALIAS", "id": "VariableID:1:1009"}}
                    }]
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:2", "type": "TEXT",
                "fields": {
                    "name": "Caption", "childrenIds": [], "characters": "caption",
                    "textTruncation": "DISABLED",
                    "styledTextSegments": [{
                        "characters": "caption",
                        // Bound to a text style the catalog never carried.
                        "textStyleId": "S:ddba35e8000000000000000000000000,",
                        "fontName": {"family": "Pretendard", "style": "Regular"},
                        "fontSize": 14,
                        "fontWeight": 400,
                        "lineHeight": {"unit": "PERCENT", "value": 160},
                        "fills": [{
                            "type": "SOLID",
                            "color": {"r": 0.49, "g": 0.498, "b": 0.514, "a": 1},
                            "boundVariables": {"color": {"type": "VARIABLE_ALIAS", "id": "VariableID:1:1009"}}
                        }]
                    }]
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
fn a_binding_whose_name_never_arrived_is_reported_rather_than_quietly_resolved() {
    let output = generate_component(
        &snapshot(),
        "1:1",
        &CodegenOptions {
            include_diagnostics: true,
            ..CodegenOptions::default()
        },
    )
    .expect("codegen");

    // The module still has to be usable, so the resolved value is written.
    assert!(
        output.tsx.contains("#7d7f83") || output.tsx.contains("#7D7F83"),
        "a value with no reachable token name still has to render:\n{}",
        output.tsx
    );

    // But the answer has to admit it. Each binding that could not be named is
    // reported once, with the node and the resource it came from, so a caller
    // reading only the response knows a token was lost.
    let unresolved = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "DEVUP_CODEGEN_TOKEN_NAME_UNRESOLVED")
        .collect::<Vec<_>>();
    assert!(
        !unresolved.is_empty(),
        "a hardcoded value standing in for a token must be reported, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|diagnostic| &diagnostic.code)
            .collect::<Vec<_>>()
    );
    for issue in &unresolved {
        assert!(issue.node_id.is_some());
        assert!(issue.property.is_some());
        let details = issue.details.as_ref().expect("result evidence");
        assert!(details.get("originalValue").is_some());
        assert!(details["appliedValue"].is_object());
    }
    let reported = serde_json::to_string(&unresolved).expect("diagnostics serialize");
    assert!(
        reported.contains("VariableID:1:1009"),
        "the variable that could not be named must be identified: {reported}"
    );
    assert!(
        reported.contains("S:ddba35e8000000000000000000000000,"),
        "the text style that could not be named must be identified: {reported}"
    );

    // And it must not be graded as an exact reproduction of the design.
    assert!(
        !output.fidelity_report.strict_compatible(),
        "a lost token is a fidelity shortfall"
    );
}

#[test]
fn a_binding_the_catalog_carried_is_written_as_its_token() {
    let output = generate_component(
        &snapshot(),
        "1:1",
        &CodegenOptions {
            include_diagnostics: true,
            variable_tokens: [("VariableID:1:1009".to_owned(), "caption".to_owned())]
                .into_iter()
                .collect(),
            text_style_tokens: [(
                "S:ddba35e8000000000000000000000000,".to_owned(),
                "captionSm".to_owned(),
            )]
            .into_iter()
            .collect(),
            ..CodegenOptions::default()
        },
    )
    .expect("codegen");

    assert!(
        output.tsx.contains("$caption"),
        "a named binding belongs in the code as its token:\n{}",
        output.tsx
    );
    assert!(
        output.tsx.contains("typography=\"captionSm\""),
        "a named text style belongs in the code as its typography:\n{}",
        output.tsx
    );
    assert!(
        !output.tsx.contains("#7d7f83") && !output.tsx.contains("#7D7F83"),
        "no resolved value should survive beside the token that names it:\n{}",
        output.tsx
    );
    assert!(
        !output.tsx.contains("fontSize=\"14px\""),
        "a typography token replaces the font properties it stands for:\n{}",
        output.tsx
    );
    assert!(
        output
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "DEVUP_CODEGEN_TOKEN_NAME_UNRESOLVED"),
        "nothing was lost, so nothing should be reported"
    );
}
