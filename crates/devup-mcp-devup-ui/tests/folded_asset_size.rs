use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::{SnapshotChunk, merge_chunks};
use serde_json::json;

#[test]
fn fixed_non_square_frame_folded_into_mask_keeps_its_size() {
    let chunk: SnapshotChunk = serde_json::from_value(json!({
        "fileKey": "file-key",
        "version": "1",
        "rootIds": ["1:root"],
        "nodes": [
            {
                "id": "1:root", "type": "FRAME",
                "fields": {
                    "name": "Screen", "childrenIds": ["1:logo"],
                    "width": 100, "height": 100,
                    "fills": [{
                        "type": "SOLID", "visible": true,
                        "color": {"r": 1, "g": 1, "b": 1}
                    }]
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:logo", "type": "FRAME",
                "fields": {
                    "name": "BI Logo", "parentId": "1:root", "childrenIds": ["1:vector"],
                    "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                    "layoutPositioning": "ABSOLUTE", "width": 24, "height": 9,
                    "x": 64, "y": 79,
                    "targetAspectRatio": {"x": 79.9, "y": 29.9}
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:vector", "type": "VECTOR",
                "fields": {
                    "name": "BI Logo Vector", "parentId": "1:logo", "childrenIds": [],
                    "fills": [{
                        "type": "SOLID", "visible": true,
                        "color": {"r": 0, "g": 0, "b": 0}
                    }]
                },
                "extra": {}, "fieldErrors": {}
            }
        ],
        "diagnostics": []
    }))
    .expect("synthetic snapshot");
    let snapshot = merge_chunks(vec![chunk]).expect("snapshot");

    let tsx = generate_component(&snapshot, "1:root", &CodegenOptions::default())
        .expect("codegen")
        .tsx;

    assert!(tsx.contains("maskImage=\"url('/icons/BI Logo.svg')\""));
    assert!(
        tsx.contains("h=\"9px\"") && tsx.contains("w=\"24px\""),
        "folded mask lost its fixed dimensions:\n{tsx}"
    );
}
