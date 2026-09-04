//! An absolutely positioned node states its pinned size only when nothing
//! else accounts for it.
//!
//! Where the gap around the children became padding, that padding and the
//! content already add back up to the frame. Where the node was folded into a
//! single asset there are no children at all, so the size is the only thing
//! left to give it one.

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

fn card(child: Value) -> Value {
    json!([
        {
            "id": "1:card", "type": "FRAME",
            "fields": {
                "name": "Card", "childrenIds": ["1:badge"],
                "layoutMode": "VERTICAL",
                "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                "width": 125.0, "height": 100.0,
                "parentId": "0:page", "parentType": "SECTION"
            },
            "extra": {}, "fieldErrors": {}
        },
        child,
        {
            "id": "1:inner", "type": "FRAME",
            "fields": {
                "name": "Icons", "parentId": "1:badge", "childrenIds": [],
                "width": 14.285714149475098, "height": 14.285714149475098,
                "x": 2.857142686843872, "y": 2.857142686843872
            },
            "extra": {}, "fieldErrors": {}
        }
    ])
}

#[test]
fn a_padded_container_does_not_also_restate_its_size() {
    let tsx = generate(
        "1:card",
        card(json!({
            "id": "1:badge", "type": "FRAME",
            "fields": {
                "name": "Badge", "parentId": "1:card", "childrenIds": ["1:inner"],
                "layoutMode": "NONE", "layoutPositioning": "ABSOLUTE",
                "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                "width": 20.0, "height": 20.0, "x": 6.0, "y": 6.0,
                "cornerRadius": 1000.0,
                "constraints": {"horizontal": "MIN", "vertical": "MIN"}
            },
            "extra": {}, "fieldErrors": {}
        })),
    );

    // 2.86 + 14.29 + 2.86 comes back to the 20px Figma pinned.
    assert!(tsx.contains("p=\"2.86px\""), "{tsx}");
    assert!(
        !tsx.contains("boxSize=\"20px\""),
        "padding and content already give the size: {tsx}"
    );
}

#[test]
fn a_folded_asset_still_states_the_size_it_was_pinned_to() {
    // Its children are baked into the exported image, so no padding is derived
    // and nothing else would give this box a size.
    let tsx = generate(
        "1:card",
        card(json!({
            "id": "1:badge", "type": "FRAME",
            "fields": {
                "name": "Logo", "parentId": "1:card", "childrenIds": ["1:inner"],
                "layoutMode": "NONE", "layoutPositioning": "ABSOLUTE",
                "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                "width": 24.0, "height": 9.0, "x": 93.0, "y": 79.0,
                "isAsset": true,
                "constraints": {"horizontal": "MAX", "vertical": "MAX"}
            },
            "extra": {}, "fieldErrors": {}
        })),
    );

    assert!(
        tsx.contains("w=\"24px\""),
        "a folded asset needs its size: {tsx}"
    );
    assert!(tsx.contains("h=\"9px\""), "{tsx}");
    assert!(
        !tsx.contains("p=\"2.86px\""),
        "an asset's hidden children are not a padding: {tsx}"
    );
}
