//! A screen's own width is the canvas it was drawn on, not a constraint.
//!
//! The frame being exported sits on a page or section, so its width is simply
//! the size the designer worked at. Emitting it pins the result to a device
//! width that does not exist. The parent that establishes this is outside the
//! collected subtree, so the node carries its parent's type and that is what
//! the decision reads.

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

fn screen(parent_type: Option<&str>) -> String {
    let mut root = json!({
        "name": "Screen",
        "childrenIds": ["1:header"],
        "layoutMode": "VERTICAL",
        "layoutSizingHorizontal": "FIXED",
        "layoutSizingVertical": "HUG",
        "width": 360.0,
        "height": 1238.0,
        "parentId": "0:page"
    });
    if let Some(parent_type) = parent_type {
        root["parentType"] = json!(parent_type);
    }

    generate(
        "1:screen",
        json!([
            {"id": "1:screen", "type": "FRAME", "fields": root, "extra": {}, "fieldErrors": {}},
            {
                "id": "1:header", "type": "FRAME",
                "fields": {
                    "name": "Header", "parentId": "1:screen", "childrenIds": [],
                    "layoutMode": "HORIZONTAL",
                    "layoutSizingHorizontal": "FIXED",
                    "layoutSizingVertical": "FIXED",
                    "width": 360.0, "height": 66.0
                },
                "extra": {}, "fieldErrors": {}
            }
        ]),
    )
}

/// The root's opening tag. Props are formatted across several lines, so
/// matching a single line would find `<VStack` alone and prove nothing.
fn root_tag(tsx: &str) -> String {
    let start = tsx
        .find("return (")
        .and_then(|from| tsx[from..].find('<').map(|at| from + at))
        .unwrap_or_else(|| panic!("a root element in:\n{tsx}"));
    let end = tsx[start..].find('>').expect("a closed tag") + start;
    tsx[start..=end].to_owned()
}

#[test]
fn a_screen_on_a_section_does_not_restate_its_canvas_width() {
    let tag = root_tag(&screen(Some("SECTION")));

    assert!(
        !tag.contains("w=\"360px\""),
        "the canvas width must not become a constraint: {tag}"
    );
}

#[test]
fn a_child_that_happens_to_be_full_width_still_states_it() {
    let tsx = screen(Some("SECTION"));

    // The header is 360 wide too, but it is a child rather than the canvas, so
    // its width is a real measurement and has to survive.
    assert!(
        tsx[root_tag(&tsx).len()..].contains("w=\"360px\""),
        "a child's own width is not canvas geometry: {tsx}"
    );
}

#[test]
fn without_a_recorded_parent_type_the_width_is_still_emitted() {
    // Nothing says this frame is a screen, so the width is all there is to go
    // on. This is what the upstream fixtures exercise, and it must not change.
    let tag = root_tag(&screen(None));

    assert!(
        tag.contains("w=\"360px\""),
        "an unattributed frame keeps its width: {tag}"
    );
}
