//! A frame without auto-layout places its children itself.
//!
//! Where the gap around them can be measured it becomes padding, which puts
//! them where they belong. Where nothing can be measured — the child fills the
//! frame, or carries no position of its own — the containing block is still
//! what keeps the child resolvable.

use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::{SnapshotChunk, merge_chunks};
use serde_json::{Value, json};

fn generate(root_id: &str, nodes: Value) -> String {
    let chunk: SnapshotChunk = serde_json::from_value(json!({
        "fileKey": "file-key",
        "version": "1",
        "rootIds": [root_id],
        "nodes": nodes,
        "diagnostics": []
    }))
    .expect("synthetic snapshot");
    let snapshot = merge_chunks(vec![chunk]).expect("snapshot");

    generate_component(&snapshot, root_id, &CodegenOptions::default())
        .expect("codegen")
        .tsx
}

#[test]
fn a_measurable_inset_becomes_padding_and_needs_no_anchor() {
    let tsx = generate(
        "1:panel",
        json!([
            {
                "id": "1:panel", "type": "FRAME",
                "fields": {
                    "name": "Panel", "childrenIds": ["1:book"],
                    "layoutMode": "NONE", "layoutPositioning": "AUTO",
                    "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                    "width": 360.0, "height": 240.0,
                    "paddingTop": 10.0, "paddingRight": 10.0,
                    "paddingBottom": 10.0, "paddingLeft": 10.0,
                    "parentId": "0:page", "parentType": "SECTION"
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:book", "type": "FRAME",
                "fields": {
                    "name": "Book", "parentId": "1:panel", "childrenIds": [],
                    "layoutPositioning": "AUTO",
                    "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                    "width": 129.0, "height": 200.0, "x": 116.0, "y": 20.0
                },
                "extra": {}, "fieldErrors": {}
            }
        ]),
    );

    // The stale padding fields say 10 on every side; the child's real position
    // says otherwise, and 116 + 129 + 115 returns the frame's own 360.
    assert!(tsx.contains("pl=\"116px\""), "{tsx}");
    assert!(tsx.contains("pr=\"115px\""), "{tsx}");
    assert!(tsx.contains("py=\"20px\""), "{tsx}");
    assert!(
        !tsx.contains("p=\"10px\""),
        "stale padding must not survive: {tsx}"
    );
    assert!(
        !tsx.contains("pos=\"relative\""),
        "padding already places the child: {tsx}"
    );
}

#[test]
fn a_child_that_fills_its_frame_keeps_the_anchor() {
    let tsx = generate(
        "1:icon",
        json!([
            {
                "id": "1:icon", "type": "FRAME",
                "fields": {
                    "name": "Social", "childrenIds": ["1:layer"],
                    "layoutMode": "NONE", "layoutPositioning": "AUTO",
                    "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                    "width": 32.0, "height": 32.0,
                    "fills": [{"type": "SOLID", "visible": true, "color": {"r": 1.0, "g": 1.0, "b": 1.0}}],
                    "parentId": "0:row", "parentType": "FRAME"
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:layer", "type": "GROUP",
                "fields": {
                    "name": "Layer 2", "parentId": "1:icon", "childrenIds": [],
                    "layoutPositioning": "AUTO",
                    "width": 32.0, "height": 32.0
                },
                "extra": {}, "fieldErrors": {}
            }
        ]),
    );

    // No position to measure, so nothing became padding and the anchor stays.
    assert!(
        tsx.contains("pos=\"relative\""),
        "an unmeasurable placement still needs its containing block: {tsx}"
    );
}
