//! Regression: a Figma SOLID paint's `opacity` must survive into the emitted
//! colour on **every** path, including the masked-asset path.
//!
//! Figma expresses a translucent solid two different ways: alpha inside
//! `color.a`, and a separate `opacity` on the paint. The effective alpha is the
//! product. `color_from_paint` does that multiplication; formatting
//! `paint["color"]` directly does not, and silently drops `opacity`.
//!
//! The nodes below are the real `3997:47765` / `3997:47766` pair captured
//! read-only from `85CgSws3o5XsLv7aAwWJyS` (the speech-bubble tail on
//! `A : STORY-SUBSEL`). Its VECTOR fill is rgb(0.2388, 0.0647, 0.0647) at
//! `opacity: 0.85`, i.e. `#3D1010` at 85% => `#3D1010D9`. The same paint on the
//! bubble body (`3997:47760`, not an asset) already rendered as `#3D1010D9`,
//! so the two paths disagreed on identical input.

use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_node};
use devup_mcp_figma::{RawNode, SnapshotChunk, merge_chunks};
use serde_json::{Value, json};

/// The masked-asset wrapper: `isAsset`, no fills of its own, one VECTOR child
/// that carries the colour.
fn mask_asset_node() -> Value {
    json!({
        "id": "3997:47765",
        "type": "FRAME",
        "fields": {
            "childrenIds": ["3997:47766"],
            "constraints": { "horizontal": "MIN", "vertical": "MIN" },
            "height": 10,
            "isAsset": true,
            "layoutMode": "NONE",
            "layoutPositioning": "AUTO",
            "layoutSizingHorizontal": "FIXED",
            "layoutSizingVertical": "FIXED",
            "maxHeight": null,
            "maxWidth": null,
            "name": "Frame 1321315298",
            "visible": true,
            "width": 40,
            "x": 200,
            "y": 136
        }
    })
}

/// The VECTOR that owns the paint. `paint_opacity` is the only thing varied.
fn vector_child(paint_opacity: Value) -> Value {
    json!({
        "id": "3997:47766",
        "type": "VECTOR",
        "fields": {
            "parentId": "3997:47765",
            "constraints": { "horizontal": "MIN", "vertical": "MIN" },
            "fills": [{
                "blendMode": "NORMAL",
                "boundVariables": {},
                "color": {
                    "b": 0.064_670_071_005_821_23,
                    "g": 0.064_670_071_005_821_23,
                    "r": 0.238_782_152_533_531_2
                },
                "opacity": paint_opacity,
                "type": "SOLID",
                "visible": true
            }],
            "height": 10,
            "layoutPositioning": "AUTO",
            "layoutSizingHorizontal": "FIXED",
            "layoutSizingVertical": "FIXED",
            "maxHeight": null,
            "maxWidth": null,
            "name": "Vector 13",
            "strokeAlign": "CENTER",
            "visible": true,
            "width": 10,
            "x": 0,
            "y": 0
        }
    })
}

/// A plain (non-asset) frame carrying the *same* paint directly. This is the
/// path that was already correct, and is what the asset path must agree with.
fn plain_frame_with_same_paint(paint_opacity: Value) -> Value {
    json!({
        "id": "plain:1",
        "type": "FRAME",
        "fields": {
            "constraints": { "horizontal": "MIN", "vertical": "MIN" },
            "fills": [{
                "blendMode": "NORMAL",
                "boundVariables": {},
                "color": {
                    "b": 0.064_670_071_005_821_23,
                    "g": 0.064_670_071_005_821_23,
                    "r": 0.238_782_152_533_531_2
                },
                "opacity": paint_opacity,
                "type": "SOLID",
                "visible": true
            }],
            "height": 10,
            "layoutMode": "NONE",
            "layoutPositioning": "AUTO",
            "layoutSizingHorizontal": "FIXED",
            "layoutSizingVertical": "FIXED",
            "maxHeight": null,
            "maxWidth": null,
            "name": "Plain",
            "visible": true,
            "width": 40,
            "x": 0,
            "y": 0
        }
    })
}

fn tsx(root_id: &str, nodes: Vec<Value>) -> String {
    let nodes = nodes
        .into_iter()
        .map(|node| serde_json::from_value::<RawNode>(node).expect("node deserializes"))
        .collect::<Vec<_>>();
    let snapshot = merge_chunks(vec![SnapshotChunk {
        file_key: "85CgSws3o5XsLv7aAwWJyS".to_owned(),
        version: None,
        root_ids: vec![root_id.to_owned()],
        nodes,
        diagnostics: Vec::new(),
    }])
    .expect("snapshot merges");
    generate_node(&snapshot, root_id, &CodegenOptions::default())
        .expect("codegen succeeds")
        .tsx
}

fn mask_tsx(paint_opacity: Value) -> String {
    tsx(
        "3997:47765",
        vec![mask_asset_node(), vector_child(paint_opacity)],
    )
}

/// Extracts the single `bg="..."` value so a failure reports the colour, not a
/// whole JSX blob.
fn bg_value(tsx: &str) -> String {
    let start = tsx.find("bg=\"").expect("emitted a bg prop") + 4;
    let rest = &tsx[start..];
    let end = rest.find('"').expect("bg prop terminates");
    rest[..end].to_owned()
}

#[test]
fn masked_asset_bg_keeps_the_paint_opacity() {
    let bg = bg_value(&mask_tsx(json!(0.850_000_023_841_785_9)));
    assert_eq!(
        bg, "#3D1010D9",
        "0.85 paint opacity must survive as the alpha byte (0.85 * 255 = 217 = 0xD9); \
         dropping it renders the speech-bubble tail fully opaque"
    );
}

#[test]
fn masked_asset_bg_omits_the_alpha_byte_when_the_paint_is_opaque() {
    let bg = bg_value(&mask_tsx(json!(1.0)));
    assert_eq!(
        bg, "#3D1010",
        "an opaque paint must not grow a redundant FF alpha byte"
    );
}

#[test]
fn the_asset_path_and_the_plain_path_agree_on_the_same_paint() {
    for opacity in [json!(1.0), json!(0.85), json!(0.5), json!(0.1)] {
        let masked = bg_value(&mask_tsx(opacity.clone()));
        let plain = bg_value(&tsx(
            "plain:1",
            vec![plain_frame_with_same_paint(opacity.clone())],
        ));
        assert_eq!(
            masked, plain,
            "identical paint (opacity {opacity}) must produce the identical colour \
             whether it is read through the masked-asset path or a plain fill"
        );
    }
}
