use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;

fn source(id: &str) -> Snapshot {
    let captured: Snapshot =
        serde_json::from_str(include_str!("fixtures/vision-solid-ellipses.json")).unwrap();
    let mut ellipse = captured.nodes[id].clone();
    ellipse.fields.insert("parentId".into(), json!("parent"));
    let mut snapshot: Snapshot = serde_json::from_value(json!({"fileKey":"fixed-ellipse","roots":["parent"],"diagnostics":[],"nodes":{
        "parent":{"id":"parent","type":"FRAME","fields":{"name":"EllipseProbe","width":400,"height":300,
            "layoutMode":"VERTICAL","layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED",
            "childrenIds":[id,"label"],"absoluteBoundingBox":{"x":0,"y":0,"width":400,"height":300}}},
        "label":{"id":"label","type":"TEXT","fields":{"parentId":"parent","characters":"Live sibling",
            "fontSize":16,"textAutoResize":"WIDTH_AND_HEIGHT","layoutSizingHorizontal":"HUG","layoutSizingVertical":"HUG"}}
    }})).unwrap();
    snapshot.nodes.insert(id.into(), ellipse);
    snapshot
}

fn render(snapshot: &Snapshot) -> String {
    generate_component(
        snapshot,
        "parent",
        &CodegenOptions {
            inline_instances: true,
            component_name: Some("EllipseProbe".into()),
            ..Default::default()
        },
    )
    .unwrap()
    .tsx
}

#[test]
fn captured_vision_solid_ellipses_preserve_both_absolute_fixed_axes() {
    for (id, size) in [
        ("I44:2832;44:3209", 48),
        ("I44:2833;44:3209", 48),
        ("I44:2834;44:3209", 48),
        ("I44:2835;44:3209", 48),
        ("I44:3235;44:3700", 32),
        ("I44:3236;44:3700", 32),
        ("I44:3237;44:3700", 32),
        ("I44:3238;44:3700", 32),
    ] {
        let output = render(&source(id));
        assert!(
            output.contains(&format!("boxSize=\"{size}px\"")),
            "{id}: {output}"
        );
        assert!(output.contains("borderRadius=\"50%\""), "{output}");
        assert!(!output.contains("h=\"100%\""), "{output}");
        if id == "I44:2832;44:3209"
            && let Ok(path) = std::env::var("DEVUP_ELLIPSE_PROBE_OUTPUT")
        {
            std::fs::write(path, output).unwrap();
        }
    }
}

#[test]
fn fluid_unknown_and_unreadable_geometry_do_not_gain_fixed_shape_sizes() {
    for (field, value) in [
        ("width", json!(null)),
        ("height", json!({"$unsupported":"unknown"})),
        ("layoutSizingHorizontal", json!("FILL")),
        ("layoutSizingVertical", json!("HUG")),
    ] {
        let mut snapshot = source("I44:2832;44:3209");
        snapshot
            .nodes
            .get_mut("I44:2832;44:3209")
            .unwrap()
            .fields
            .insert(field.into(), value);
        let output = render(&snapshot);
        assert!(!output.contains("boxSize=\"48px\""), "{field}: {output}");
    }
    let mut snapshot = source("I44:2832;44:3209");
    snapshot
        .nodes
        .get_mut("I44:2832;44:3209")
        .unwrap()
        .field_errors
        .insert("width".into(), "read failed".into());
    assert!(!render(&snapshot).contains("boxSize=\"48px\""));
}

#[test]
fn relative_ellipses_keep_their_path_and_absolute_rectangles_preserve_measured_axes() {
    for positioning in [json!("AUTO"), json!({"$unsupported":"unknown"})] {
        let mut snapshot = source("I44:2832;44:3209");
        snapshot
            .nodes
            .get_mut("I44:2832;44:3209")
            .unwrap()
            .fields
            .insert("layoutPositioning".into(), positioning);
        let output = render(&snapshot);
        assert!(!output.contains("pos=\"absolute\""), "{output}");
    }
    let mut snapshot = source("I44:2832;44:3209");
    snapshot
        .nodes
        .get_mut("I44:2832;44:3209")
        .unwrap()
        .node_type = "RECTANGLE".into();
    let output = render(&snapshot);
    assert!(output.contains("boxSize=\"48px\""), "{output}");
}
