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
    let offset = tsx.find("left=\"-277px\"").expect("the group's offset");
    let start = tsx[..offset].rfind('<').expect("group opening");
    let end = offset + tsx[offset..].find('>').expect("group opening end");
    let group_line = &tsx[start..=end];
    assert!(
        group_line.contains("pos=\"absolute\"") && !group_line.contains("relative"),
        "the group is not told `relative` over the `absolute` it already has: {group_line}"
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

/// Figma paints children in order; CSS paints a positioned element after
/// every in-flow sibling whatever the order. A pinned picture drawn first is
/// under everything in Figma and over everything in CSS - the landing page's
/// hero sat on its headline and the join-us badges on their buttons - so it
/// is sent behind with `zIndex="-1"`, inside a stacking context the parent
/// opens with `zIndex="0"` so it still clears the parent's own background.
///
/// Only a child at the very bottom. `-1` goes behind *every* in-flow
/// sibling, so a pinned header drawn second, after its banner, is left
/// alone: sent behind, the notice page's header vanished under the banner it
/// sits on.
#[test]
fn a_pinned_child_drawn_first_goes_behind_the_content_but_one_drawn_second_does_not() {
    let hero_first = generate(
        "1:screen",
        json!([
            {
                "id": "1:screen", "type": "FRAME",
                "fields": {
                    "name": "screen", "childrenIds": ["1:section"],
                    "layoutMode": "VERTICAL", "layoutPositioning": "AUTO",
                    "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "HUG",
                    "width": 1440.0, "height": 540.0,
                    "parentId": "0:page", "parentType": "SECTION"
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:section", "type": "FRAME",
                "fields": {
                    "name": "section", "parentId": "1:screen",
                    "childrenIds": ["1:picture", "1:headline"],
                    "layoutMode": "VERTICAL", "layoutPositioning": "AUTO",
                    "layoutSizingHorizontal": "FILL", "layoutSizingVertical": "HUG",
                    "width": 1440.0, "height": 540.0
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:picture", "type": "RECTANGLE",
                "fields": {
                    "name": "picture", "parentId": "1:section", "childrenIds": [],
                    "layoutPositioning": "ABSOLUTE",
                    "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                    "width": 600.0, "height": 600.0, "x": 700.0, "y": -100.0,
                    "constraints": {"horizontal": "MIN", "vertical": "MIN"},
                    "fills": [{"type": "SOLID", "visible": true, "color": {"r": 0.5, "g": 0.5, "b": 1}}]
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:headline", "type": "TEXT",
                "fields": {
                    "name": "headline", "parentId": "1:section", "childrenIds": [],
                    "characters": "Zero Config",
                    "layoutPositioning": "AUTO",
                    "layoutSizingHorizontal": "HUG", "layoutSizingVertical": "HUG",
                    "width": 500.0, "height": 120.0
                },
                "extra": {}, "fieldErrors": {}
            }
        ]),
    );
    assert!(
        hero_first.contains("zIndex=\"-1\""),
        "the picture goes behind: {hero_first}"
    );
    assert!(
        hero_first.contains("zIndex=\"0\""),
        "the section holds it: {hero_first}"
    );

    let header_second = generate(
        "1:page",
        json!([
            {
                "id": "1:page", "type": "FRAME",
                "fields": {
                    "name": "page", "childrenIds": ["1:banner", "1:header", "1:body"],
                    "layoutMode": "VERTICAL", "layoutPositioning": "AUTO",
                    "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "HUG",
                    "width": 360.0, "height": 1215.0,
                    "parentId": "0:page", "parentType": "SECTION"
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:banner", "type": "FRAME",
                "fields": {
                    "name": "banner", "parentId": "1:page", "childrenIds": [],
                    "layoutPositioning": "AUTO",
                    "layoutSizingHorizontal": "FILL", "layoutSizingVertical": "FIXED",
                    "width": 360.0, "height": 320.0,
                    "fills": [{"type": "SOLID", "visible": true, "color": {"r": 0, "g": 0.2, "b": 0.7}}]
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:header", "type": "FRAME",
                "fields": {
                    "name": "header", "parentId": "1:page", "childrenIds": [],
                    "layoutPositioning": "ABSOLUTE",
                    "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                    "width": 360.0, "height": 60.0, "x": 0.0, "y": 0.0,
                    "constraints": {"horizontal": "MIN", "vertical": "MIN"}
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:body", "type": "TEXT",
                "fields": {
                    "name": "body", "parentId": "1:page", "childrenIds": [],
                    "characters": "notice",
                    "layoutPositioning": "AUTO",
                    "layoutSizingHorizontal": "HUG", "layoutSizingVertical": "HUG",
                    "width": 200.0, "height": 40.0
                },
                "extra": {}, "fieldErrors": {}
            }
        ]),
    );
    assert!(
        !header_second.contains("zIndex="),
        "a header pinned over its banner stays where CSS puts it: {header_second}"
    );
}

/// An export carries the node's own opacity - Figma writes it into an SVG
/// as `<g opacity>` and into a PNG's alpha; the landing page's hero at 0.8
/// exports with its opaque pixels at alpha 204 - so it is not written on the
/// element as well. Written twice, a decoration at 0.2 came out at 0.04. A
/// node that is not an asset still carries its own.
#[test]
fn an_asset_is_not_given_the_opacity_its_export_already_carries() {
    let tsx = generate(
        "1:card",
        json!([
            {
                "id": "1:card", "type": "FRAME",
                "fields": {
                    "name": "card", "childrenIds": ["1:picture", "1:veil"],
                    "layoutMode": "VERTICAL", "layoutPositioning": "AUTO",
                    "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "HUG",
                    "width": 400.0, "height": 300.0,
                    "parentId": "0:page", "parentType": "SECTION"
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:picture", "type": "RECTANGLE",
                "fields": {
                    "name": "picture", "parentId": "1:card", "childrenIds": [],
                    "isAsset": true, "opacity": 0.8,
                    "layoutPositioning": "AUTO",
                    "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                    "width": 200.0, "height": 200.0,
                    "fills": [{"type": "IMAGE", "scaleMode": "FILL", "imageHash": "hash-1", "visible": true}]
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:veil", "type": "FRAME",
                "fields": {
                    "name": "veil", "parentId": "1:card", "childrenIds": [],
                    "opacity": 0.5,
                    "layoutPositioning": "AUTO",
                    "layoutSizingHorizontal": "FILL", "layoutSizingVertical": "FIXED",
                    "width": 400.0, "height": 40.0,
                    "fills": [{"type": "SOLID", "visible": true, "color": {"r": 0, "g": 0, "b": 0}}]
                },
                "extra": {}, "fieldErrors": {}
            }
        ]),
    );
    let picture = tsx
        .lines()
        .find(|line| line.contains("/images/picture"))
        .expect("the picture is an image");
    assert!(
        !picture.contains("opacity="),
        "the export already carries 0.8: {picture}"
    );
    assert!(
        tsx.contains("opacity=\"0.5\""),
        "a frame that is not an asset keeps its own opacity: {tsx}"
    );
}
