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

/// A group pinned into a card, holding shapes at their own coordinates. Three
/// things went wrong at once on the devup-ui landing page's join-us panel, and
/// the arcs and badges it draws were simply not there:
///
/// - the group is `ABSOLUTE`, so its children were read as being in flow and
///   stacked from its corner, clipped away;
/// - once placed, the group was told `pos="relative"` for holding positioned
///   children, over the `absolute` it already had, and took 1,102px of page;
/// - a group's children carry `x` and `y` in the group's parent's space, and
///   placed as read every circle sat 277px left and 187px high of Figma.
///
/// A shape in a group also keeps its own size: `h="100%"` and no width, which
/// is the plugin's rule for a positioned shape, is no circle at all.
#[test]
fn shapes_in_a_pinned_group_are_placed_in_the_group_at_their_own_size() {
    let tsx = generate(
        "1:card",
        json!([
            {
                "id": "1:card", "type": "FRAME",
                "fields": {
                    "name": "card", "childrenIds": ["1:group", "1:title"],
                    "layoutMode": "HORIZONTAL", "layoutPositioning": "AUTO",
                    "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                    "width": 1440.0, "height": 356.0, "clipsContent": true,
                    "absoluteBoundingBox": {"x": 1000.0, "y": 2000.0, "width": 1440.0, "height": 356.0},
                    "parentId": "0:page", "parentType": "SECTION"
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:group", "type": "GROUP",
                "fields": {
                    "name": "Group 2", "parentId": "1:card",
                    "childrenIds": ["1:outer", "1:badge"],
                    "layoutPositioning": "ABSOLUTE",
                    "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                    "width": 1102.0, "height": 1102.0, "x": -277.0, "y": -187.0,
                    "absoluteBoundingBox": {"x": 723.0, "y": 1813.0, "width": 1102.0, "height": 1102.0},
                    "constraints": {"horizontal": "MIN", "vertical": "MIN"}
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:outer", "type": "ELLIPSE",
                "fields": {
                    "name": "Ellipse 6", "parentId": "1:group", "childrenIds": [],
                    "layoutPositioning": "AUTO",
                    "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                    "width": 1102.0, "height": 1102.0, "x": -277.0, "y": -187.0,
                    "absoluteBoundingBox": {"x": 723.0, "y": 1813.0, "width": 1102.0, "height": 1102.0},
                    "constraints": {"horizontal": "MIN", "vertical": "MIN"},
                    "arcData": {"startingAngle": 0, "endingAngle": 6.0, "innerRadius": 0},
                    "strokes": [{"type": "SOLID", "visible": true, "color": {"r": 1, "g": 1, "b": 1}, "opacity": 0.4}],
                    "strokeWeight": 4.0, "strokeAlign": "INSIDE"
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:badge", "type": "ELLIPSE",
                "fields": {
                    "name": "Ellipse 9", "parentId": "1:group", "childrenIds": [],
                    "layoutPositioning": "AUTO",
                    "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                    "width": 130.0, "height": 130.0, "x": 376.0, "y": 12.0,
                    "absoluteBoundingBox": {"x": 1376.0, "y": 2012.0, "width": 130.0, "height": 130.0},
                    "constraints": {"horizontal": "MIN", "vertical": "MIN"},
                    "arcData": {"startingAngle": 0, "endingAngle": 6.0, "innerRadius": 0},
                    "fills": [{"type": "SOLID", "visible": true, "color": {"r": 0.15, "g": 0.42, "b": 0.8}}]
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:title", "type": "TEXT",
                "fields": {
                    "name": "title", "parentId": "1:card", "childrenIds": [],
                    "characters": "Join our community",
                    "layoutPositioning": "AUTO",
                    "layoutSizingHorizontal": "HUG", "layoutSizingVertical": "HUG",
                    "width": 300.0, "height": 40.0,
                    "absoluteBoundingBox": {"x": 1600.0, "y": 2100.0, "width": 300.0, "height": 40.0}
                },
                "extra": {}, "fieldErrors": {}
            }
        ]),
    );

    // The group stays pinned - not put back in flow for holding positioned
    // children - and the card is what holds it.
    assert!(
        tsx.contains("left=\"-277px\"") && tsx.contains("top=\"-187px\""),
        "the group keeps its place in the card: {tsx}"
    );
    assert!(
        tsx.contains("<Box boxSize=\"1102px\" left=\"-277px\" pos=\"absolute\" top=\"-187px\">"),
        "the group is not told `relative` over the `absolute` it already has: {tsx}"
    );
    // The badge is at 653,199 inside the group - its absolute box against the
    // group's - not at the 376,12 it reads in the card's space, and it is
    // 130px, not `h="100%"` with no width.
    assert!(
        tsx.contains("left=\"653px\""),
        "the badge is placed in the group's space: {tsx}"
    );
    assert!(tsx.contains("top=\"199px\""), "{tsx}");
    assert!(
        tsx.contains("boxSize=\"130px\""),
        "a shape in a group is its own size: {tsx}"
    );
    // The outermost circle coincides with the group, so it sits at 0,0 and
    // fills it.
    assert!(tsx.contains("left=\"0px\""), "{tsx}");
    assert!(
        !tsx.contains("left=\"376px\""),
        "the card-space coordinate must not leak through: {tsx}"
    );
}
