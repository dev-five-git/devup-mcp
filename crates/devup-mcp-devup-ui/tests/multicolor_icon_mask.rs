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

fn solid(r: f64, g: f64, b: f64, opacity: f64) -> Value {
    json!({
        "type": "SOLID", "visible": true, "opacity": opacity,
        "color": {"r": r, "g": g, "b": b}
    })
}

fn gradient() -> Value {
    json!({
        "type": "GRADIENT_LINEAR", "visible": true,
        "gradientTransform": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        "gradientStops": [
            {"position": 0.0, "color": {"r": 0.957, "g": 0.953, "b": 1.0, "a": 1.0}},
            {"position": 1.0, "color": {"r": 0.906, "g": 0.902, "b": 1.0, "a": 1.0}}
        ]
    })
}

/// A 64x64 icon frame whose vectors carry the given fills, one per vector.
fn icon(fills: Vec<Value>) -> Value {
    let ids = (0..fills.len())
        .map(|index| format!("1:v{index}"))
        .collect::<Vec<_>>();
    let mut nodes = vec![json!({
        "id": "1:find", "type": "FRAME",
        "fields": {
            "name": "find", "childrenIds": ids,
            "width": 64.0, "height": 64.0
        },
        "extra": {}, "fieldErrors": {}
    })];
    for (id, fill) in ids.iter().zip(fills) {
        nodes.push(json!({
            "id": id, "type": "VECTOR",
            "fields": {
                "name": "Vector", "parentId": "1:find", "childrenIds": [],
                "width": 48.0, "height": 48.0, "x": 8.0, "y": 8.0,
                "fills": [fill]
            },
            "extra": {}, "fieldErrors": {}
        }));
    }
    Value::Array(nodes)
}

const PURPLE: (f64, f64, f64) = (0.333, 0.165, 1.0);

#[test]
fn a_gradient_face_with_a_solid_outline_stays_an_image() {
    // The prime "기대 효과" icons: a gradient face under solid #552AFF
    // strokes. A mask keeps only the shape and fills it with one colour, so
    // the face and outline merged into one purple blob and the glyph inside
    // vanished. The gradient used to count as "no colour" and was ignored.
    let (r, g, b) = PURPLE;
    let tsx = generate(
        "1:find",
        icon(vec![gradient(), solid(r, g, b, 0.75), solid(r, g, b, 0.75)]),
    );

    assert!(tsx.contains("src=\"/icons/find.svg\""), "{tsx}");
    assert!(!tsx.contains("maskImage"), "{tsx}");
    assert!(!tsx.contains("bg="), "{tsx}");
}

#[test]
fn a_single_colour_icon_is_still_a_mask() {
    let (r, g, b) = PURPLE;
    let tsx = generate(
        "1:find",
        icon(vec![solid(r, g, b, 0.75), solid(r, g, b, 0.75)]),
    );

    assert!(tsx.contains("maskImage=\"url(/icons/find.svg)\""), "{tsx}");
    assert!(tsx.contains("bg=\"#552AFFBF\""), "{tsx}");
    assert!(!tsx.contains("src="), "{tsx}");
}

#[test]
fn two_different_solids_stay_an_image() {
    let (r, g, b) = PURPLE;
    let tsx = generate(
        "1:find",
        icon(vec![solid(r, g, b, 1.0), solid(1.0, 0.0, 0.0, 1.0)]),
    );

    assert!(tsx.contains("src=\"/icons/find.svg\""), "{tsx}");
    assert!(!tsx.contains("maskImage"), "{tsx}");
}

#[test]
fn a_gradient_stroke_spoils_the_colour_too() {
    // Strokes are paints the mask would flatten just the same.
    let (r, g, b) = PURPLE;
    let mut nodes = icon(vec![solid(r, g, b, 1.0), solid(r, g, b, 1.0)]);
    nodes[1]["fields"]["strokes"] = json!([gradient()]);
    let tsx = generate("1:find", nodes);

    assert!(tsx.contains("src=\"/icons/find.svg\""), "{tsx}");
    assert!(!tsx.contains("maskImage"), "{tsx}");
}

#[test]
fn an_unpainted_vector_leaves_the_colour_alone() {
    // No paint at all is neutral: it adds no colour for a mask to lose.
    let (r, g, b) = PURPLE;
    let mut nodes = icon(vec![solid(r, g, b, 1.0), solid(r, g, b, 1.0)]);
    nodes[2]["fields"]["fills"] = json!([]);
    let tsx = generate("1:find", nodes);

    assert!(tsx.contains("maskImage=\"url(/icons/find.svg)\""), "{tsx}");
    assert!(tsx.contains("bg=\"#552AFF\""), "{tsx}");
}

#[test]
fn layer_order_does_not_decide_the_mask() {
    // The same icon with its gradient face on top: the answer must not move.
    let (r, g, b) = PURPLE;
    let tsx = generate(
        "1:find",
        icon(vec![solid(r, g, b, 0.75), solid(r, g, b, 0.75), gradient()]),
    );

    assert!(tsx.contains("src=\"/icons/find.svg\""), "{tsx}");
    assert!(!tsx.contains("maskImage"), "{tsx}");
}

#[test]
fn an_image_paint_inside_an_icon_stays_an_image() {
    let (r, g, b) = PURPLE;
    let tsx = generate(
        "1:find",
        icon(vec![
            solid(r, g, b, 1.0),
            json!({"type": "IMAGE", "visible": true, "scaleMode": "FILL", "imageHash": "h"}),
        ]),
    );

    assert!(!tsx.contains("maskImage"), "{tsx}");
}

#[test]
fn purple_outline_with_light_lavender_fill_exports_as_image() {
    let (r, g, b) = PURPLE;
    let mut nodes = icon(vec![solid(0.941, 0.925, 1.0, 1.0)]);
    nodes[1]["fields"]["strokes"] = json!([solid(r, g, b, 1.0)]);
    nodes[1]["fields"]["strokeWeight"] = json!(2.0);
    let tsx = generate("1:find", nodes);
    assert!(tsx.contains("<Image"), "{tsx}");
    assert!(tsx.contains("src=\"/icons/find.svg\""), "{tsx}");
    assert!(!tsx.contains("maskImage"), "{tsx}");
}
