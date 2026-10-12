//! `layoutGrow` takes effect only inside an auto-layout parent. An instance
//! placed straight on the canvas keeps the 1 its component had in a row; the
//! value is inert there, and exporting the instance alone must not report it
//! as a layout field the code failed to carry.

use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::{Snapshot, SnapshotChunk, merge_chunks};
use serde_json::{Value, json};

fn snapshot(nodes: Vec<Value>, root: &str) -> Snapshot {
    let chunk: SnapshotChunk = serde_json::from_value(json!({
        "fileKey": "file", "version": "1", "rootIds": [root], "diagnostics": [], "nodes": nodes
    }))
    .expect("synthetic snapshot");
    merge_chunks(vec![chunk]).expect("snapshot")
}

fn label(id: &str, parent: &str, extra: Value) -> Value {
    let mut fields = json!({
        "name": id, "parentId": parent, "childrenIds": [], "characters": id,
        "textAutoResize": "WIDTH_AND_HEIGHT", "textTruncation": "DISABLED",
        "width": 40, "height": 20, "fontSize": 14,
        "styledTextSegments": [{"characters": id, "fontSize": 14,
            "fontName": {"family": "Inter", "style": "Regular"}, "fontWeight": 400,
            "lineHeight": {"unit": "PIXELS", "value": 20},
            "fills": [{"type": "SOLID", "color": {"r": 0, "g": 0, "b": 0, "a": 1}}]}]
    });
    if let (Some(fields), Some(extra)) = (fields.as_object_mut(), extra.as_object()) {
        fields.extend(extra.clone());
    }
    json!({"id": id, "type": "TEXT", "extra": {}, "fieldErrors": {}, "fields": fields})
}

fn uncovered(snapshot: &Snapshot, root: &str) -> Vec<String> {
    let options = CodegenOptions {
        include_diagnostics: true,
        ..CodegenOptions::default()
    };
    generate_component(snapshot, root, &options)
        .expect("codegen")
        .fidelity_report
        .uncovered_layout
}

#[test]
fn growth_left_on_an_instance_placed_on_the_canvas_is_not_a_layout_obligation() {
    let card = json!({"id": "1:1", "type": "INSTANCE", "extra": {}, "fieldErrors": {}, "fields": {
        "name": "StatusCard", "parentId": "0:1", "parentType": "PAGE", "childrenIds": ["1:2"],
        "layoutMode": "VERTICAL", "layoutGrow": 1, "layoutPositioning": "AUTO",
        "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "HUG",
        "width": 315, "height": 115,
        "inferredAutoLayout": {"layoutMode": "VERTICAL", "itemSpacing": 0}
    }});
    let snapshot = snapshot(vec![card, label("1:2", "1:1", json!({}))], "1:1");

    let uncovered = uncovered(&snapshot, "1:1");

    assert!(
        !uncovered.iter().any(|pair| pair.ends_with("#layoutGrow")),
        "{uncovered:?}"
    );
}

#[test]
fn growth_inside_a_row_is_still_an_obligation_the_code_carries() {
    let row = json!({"id": "2:1", "type": "FRAME", "extra": {}, "fieldErrors": {}, "fields": {
        "name": "Row", "parentType": "PAGE", "childrenIds": ["2:2", "2:3"],
        "layoutMode": "HORIZONTAL", "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "HUG",
        "width": 300, "height": 20,
        "inferredAutoLayout": {"layoutMode": "HORIZONTAL", "itemSpacing": 0}
    }});
    let grows = label(
        "2:2",
        "2:1",
        json!({"layoutGrow": 1, "layoutSizingHorizontal": "FILL", "textAutoResize": "HEIGHT", "width": 260}),
    );
    let snapshot = snapshot(vec![row, grows, label("2:3", "2:1", json!({}))], "2:1");
    let options = CodegenOptions {
        include_diagnostics: true,
        ..CodegenOptions::default()
    };
    let output = generate_component(&snapshot, "2:1", &options).expect("codegen");

    assert!(output.tsx.contains("flex=\"1\""), "{}", output.tsx);
    assert!(
        !output
            .fidelity_report
            .uncovered_layout
            .iter()
            .any(|pair| pair == "2:2#layoutGrow"),
        "{:?}",
        output.fidelity_report.uncovered_layout
    );
    assert!(output.fidelity_report.layout.total > 0);
}

#[test]
fn a_row_with_one_child_laid_out_is_not_held_to_a_gap_it_has_no_use_for() {
    // StatusCard's shape: a card stacking a 267px row spaced 12px that holds a
    // filling number and a hidden badge. With one child laid out the code
    // writes no gap, and the allocation proof must not fail on the 12px.
    let card = json!({"id": "4:1", "type": "FRAME", "extra": {}, "fieldErrors": {}, "fields": {
        "name": "Card", "parentType": "PAGE", "childrenIds": ["4:2"],
        "layoutMode": "VERTICAL", "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "HUG",
        "width": 315, "height": 80, "itemSpacing": 0,
        "paddingLeft": 24, "paddingRight": 24, "paddingTop": 19, "paddingBottom": 19,
        "inferredAutoLayout": {"layoutMode": "VERTICAL", "itemSpacing": 0,
            "paddingLeft": 24, "paddingRight": 24, "paddingTop": 19, "paddingBottom": 19}
    }});
    let row = json!({"id": "4:2", "type": "FRAME", "extra": {}, "fieldErrors": {}, "fields": {
        "name": "Row", "parentId": "4:1", "childrenIds": ["4:3", "4:4"],
        "layoutMode": "HORIZONTAL", "itemSpacing": 12, "counterAxisAlignItems": "MAX",
        "layoutSizingHorizontal": "FILL", "layoutSizingVertical": "HUG",
        "width": 267, "height": 42, "paddingLeft": 0, "paddingRight": 0,
        "inferredAutoLayout": {"layoutMode": "HORIZONTAL", "itemSpacing": 12, "counterAxisAlignItems": "MAX"}
    }});
    let number = label(
        "4:3",
        "4:2",
        json!({"layoutGrow": 1, "layoutSizingHorizontal": "FILL", "layoutSizingVertical": "HUG",
               "textAutoResize": "HEIGHT", "width": 267, "height": 42}),
    );
    let badge = label("4:4", "4:2", json!({"visible": false}));
    let snapshot = snapshot(vec![card, row, number, badge], "4:1");
    let options = CodegenOptions {
        include_diagnostics: true,
        ..CodegenOptions::default()
    };
    let output = generate_component(&snapshot, "4:1", &options).expect("codegen");

    assert!(!output.tsx.contains("gap=\"12px\""), "{}", output.tsx);
    let reasons = output
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.code == "DEVUP_CODEGEN_LAYOUT_UNCOVERED"
                && diagnostic.node_id.as_deref() == Some("4:3")
        })
        .filter_map(|diagnostic| {
            diagnostic.details.as_ref()?["implicitCssVerification"]["reasonCode"]
                .as_str()
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    assert!(
        !reasons.iter().any(|reason| reason == "gap-mismatch"),
        "{reasons:?}\n{}",
        output.tsx
    );
}
