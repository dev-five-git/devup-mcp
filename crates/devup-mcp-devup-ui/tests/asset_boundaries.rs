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
fn separate_image_fills_do_not_claim_the_same_file() {
    // Two fills on one node are two different images. A single hard-coded
    // reference gave both the same URL, so the layered background repeated one
    // picture and whichever was exported last overwrote the other on disk.
    let tsx = generate(
        "1:card",
        json!([{
            "id": "1:card", "type": "FRAME",
            "fields": {
                "name": "Card", "childrenIds": [],
                "width": 125.0, "height": 100.0,
                "fills": [
                    {"type": "IMAGE", "visible": true, "scaleMode": "FILL", "imageHash": "aaa"},
                    {"type": "IMAGE", "visible": true, "scaleMode": "FILL", "imageHash": "bbb"}
                ]
            },
            "extra": {}, "fieldErrors": {}
        }]),
    );

    assert!(tsx.contains("/images/Card.png"), "{tsx}");
    assert!(tsx.contains("/images/Card-1.png"), "{tsx}");
}

#[test]
fn image_filled_asset_container_preserves_text_children() {
    let tsx = generate(
        "1:cover",
        json!([
            {
                "id": "1:cover", "type": "FRAME",
                "fields": {
                    "name": "Book cover", "childrenIds": ["1:title"], "isAsset": true,
                    "fills": [{"type": "IMAGE", "visible": true, "scaleMode": "FILL"}]
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:title", "type": "TEXT",
                "fields": {
                    "name": "Title", "parentId": "1:cover", "childrenIds": [],
                    "characters": "Preserved title"
                },
                "extra": {}, "fieldErrors": {}
            }
        ]),
    );

    // The fill names the node it came from, so two different images cannot
    // claim the same file. The name has a space, hence the quoting.
    assert!(tsx.contains("bg=\"url('/images/Book cover.png') center/cover no-repeat\""));
    assert!(tsx.contains("Preserved title"));
    assert!(!tsx.contains("<Image"));
}

#[test]
fn transparent_single_child_container_is_the_asset_boundary() {
    let tsx = generate(
        "2:outer",
        json!([
            {
                "id": "2:outer", "type": "FRAME",
                "fields": {"name": "Outer icon", "childrenIds": ["2:inner"]},
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "2:inner", "type": "FRAME",
                "fields": {
                    "name": "Inner icon", "parentId": "2:outer", "childrenIds": ["2:a", "2:b"],
                    "isAsset": true
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "2:a", "type": "VECTOR",
                "fields": {"name": "First", "parentId": "2:inner", "childrenIds": []},
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "2:b", "type": "VECTOR",
                "fields": {"name": "Second", "parentId": "2:inner", "childrenIds": []},
                "extra": {}, "fieldErrors": {}
            }
        ]),
    );

    assert!(tsx.contains("src=\"/icons/Outer icon.svg\""));
    assert!(!tsx.contains("src=\"/icons/Inner icon.svg\""));
}

#[test]
fn container_with_only_visible_vector_children_is_an_svg() {
    let tsx = generate(
        "3:group",
        json!([
            {
                "id": "3:group", "type": "FRAME",
                "fields": {"name": "Vector group", "childrenIds": ["3:a", "3:b"]},
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "3:a", "type": "VECTOR",
                "fields": {"name": "First", "parentId": "3:group", "visible": true},
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "3:b", "type": "VECTOR",
                "fields": {"name": "Second", "parentId": "3:group", "visible": true},
                "extra": {}, "fieldErrors": {}
            }
        ]),
    );

    assert!(tsx.contains("src=\"/icons/Vector group.svg\""));
}

#[test]
fn container_with_vector_and_text_children_is_not_an_asset() {
    let tsx = generate(
        "4:mixed",
        json!([
            {
                "id": "4:mixed", "type": "FRAME",
                "fields": {"name": "Mixed group", "childrenIds": ["4:vector", "4:text"]},
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "4:vector", "type": "VECTOR",
                "fields": {"name": "Mixed icon", "parentId": "4:mixed", "visible": true},
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "4:text", "type": "TEXT",
                "fields": {
                    "name": "Label", "parentId": "4:mixed", "visible": true,
                    "childrenIds": [], "characters": "Visible label"
                },
                "extra": {}, "fieldErrors": {}
            }
        ]),
    );

    assert!(!tsx.contains("src=\"/icons/Mixed group.svg\""));
    assert!(tsx.contains("src=\"/icons/Mixed icon.svg\""));
    assert!(tsx.contains("Visible label"));
}

#[test]
fn text_node_is_never_an_asset() {
    let tsx = generate(
        "5:text",
        json!([{
            "id": "5:text", "type": "TEXT",
            "fields": {
                "name": "Text asset", "childrenIds": [], "characters": "Still text",
                "isAsset": true
            },
            "extra": {}, "fieldErrors": {}
        }]),
    );

    assert!(tsx.contains("<Text"));
    assert!(tsx.contains("Still text"));
    assert!(!tsx.contains("<Image"));
}

#[test]
fn decorated_single_child_containers_do_not_collapse() {
    let padded = generate(
        "6:padded",
        json!([
            {
                "id": "6:padded", "type": "FRAME",
                "fields": {
                    "name": "Padded container", "childrenIds": ["6:padded-vector"],
                    "paddingLeft": 8
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "6:padded-vector", "type": "VECTOR",
                "fields": {"name": "Padded child", "parentId": "6:padded"},
                "extra": {}, "fieldErrors": {}
            }
        ]),
    );
    let filled = generate(
        "6:filled",
        json!([
            {
                "id": "6:filled", "type": "FRAME",
                "fields": {
                    "name": "Filled container", "childrenIds": ["6:filled-vector"],
                    "fills": [{
                        "type": "SOLID", "visible": true,
                        "color": {"r": 1, "g": 1, "b": 1}
                    }]
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "6:filled-vector", "type": "VECTOR",
                "fields": {"name": "Filled child", "parentId": "6:filled"},
                "extra": {}, "fieldErrors": {}
            }
        ]),
    );

    assert!(!padded.contains("src=\"/icons/Padded container.svg\""));
    assert!(padded.contains("src=\"/icons/Padded child.svg\""));
    assert!(!filled.contains("maskImage="));
    assert!(filled.contains("src=\"/icons/Filled child.svg\""));
}

#[test]
fn smart_animate_node_and_parent_reactions_prevent_asset_classification() {
    let reactions = json!([{
        "actions": [{"type": "NODE", "transition": {"type": "SMART_ANIMATE"}}]
    }]);
    let direct = generate(
        "7:direct",
        json!([{
            "id": "7:direct", "type": "VECTOR",
            "fields": {"name": "Direct target", "reactions": reactions},
            "extra": {}, "fieldErrors": {}
        }]),
    );
    let inherited = generate(
        "7:child",
        json!([
            {
                "id": "7:parent", "type": "FRAME",
                "fields": {"name": "Parent", "childrenIds": ["7:child"], "reactions": reactions},
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "7:child", "type": "VECTOR",
                "fields": {"name": "Inherited target", "parentId": "7:parent"},
                "extra": {}, "fieldErrors": {}
            }
        ]),
    );

    assert!(!direct.contains("<Image"));
    assert!(!inherited.contains("<Image"));
}
