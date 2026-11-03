use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::{Value, json};

fn source(flag: Option<bool>, padding: u32) -> Snapshot {
    let mut snapshot: Snapshot = serde_json::from_value(json!({"fileKey":"stroke-layout","roots":["input"],"diagnostics":[],"nodes":{
        "input":{"id":"input","type":"FRAME","fields":{
            "name":"input","layoutMode":"HORIZONTAL","layoutPositioning":"AUTO","counterAxisAlignItems":"CENTER",
            "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"HUG","width":300,"height":50,
            "paddingTop":padding,"paddingRight":padding,"paddingBottom":padding,"paddingLeft":padding,
            "strokeWeight":1,"strokes":[{"type":"SOLID","visible":true,"color":{"r":0,"g":0,"b":0}}],"childrenIds":["icon","text"]
        }},
        "icon":{"id":"icon","type":"FRAME","fields":{"parentId":"input","width":24,"height":24,"layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED"}},
        "text":{"id":"text","type":"TEXT","fields":{"parentId":"input","characters":"Search","fontSize":16,"lineHeight":{"unit":"PIXELS","value":24},"textAutoResize":"WIDTH_AND_HEIGHT","layoutSizingHorizontal":"HUG","layoutSizingVertical":"HUG"}}
    }})).unwrap();
    if let Some(flag) = flag {
        snapshot
            .nodes
            .get_mut("input")
            .unwrap()
            .fields
            .insert("strokesIncludedInLayout".into(), json!(flag));
    }
    snapshot
}

fn render(snapshot: &Snapshot) -> String {
    generate_component(
        snapshot,
        "input",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap()
    .tsx
}

#[test]
fn captured_strokes_in_layout_keep_padding_and_css_border() {
    let output = render(&source(Some(true), 12));
    assert!(output.contains("p=\"12px\""), "{output}");
    assert!(output.contains("border=\"solid 1px #000\""), "{output}");
    assert!(!output.contains("outline="), "{output}");
    if let Ok(path) = std::env::var("DEVUP_STROKE_LAYOUT_PROBE_OUTPUT") {
        std::fs::write(path, output).unwrap();
    }
}

#[test]
fn explicit_false_and_missing_legacy_capture_keep_existing_inside_stroke_compensation() {
    for flag in [Some(false), None] {
        let output = render(&source(flag, 12));
        assert!(output.contains("p=\"11px\""), "{flag:?}: {output}");
    }
}

#[test]
fn explicit_true_with_zero_padding_must_not_turn_layout_border_into_outline() {
    let output = render(&source(Some(true), 0));
    assert!(output.contains("border=\"solid 1px #000\""), "{output}");
    assert!(!output.contains("outline="), "{output}");
    let legacy = render(&source(Some(false), 0));
    assert!(legacy.contains("outline=\"solid 1px #000\""), "{legacy}");
}

#[test]
fn nested_inferred_flag_is_used_only_when_direct_flag_is_absent() {
    for (direct, expected) in [(None, "12px"), (Some(false), "11px"), (Some(true), "12px")] {
        let mut snapshot = source(direct, 12);
        snapshot.nodes.get_mut("input").unwrap().fields.insert(
            "inferredAutoLayout".into(),
            json!({"strokesIncludedInLayout":true}),
        );
        let output = render(&snapshot);
        assert!(
            output.contains(&format!("p=\"{expected}\"")),
            "{direct:?}: {output}"
        );
    }
}

#[test]
fn outside_strokes_retain_the_existing_padding_rule() {
    let mut snapshot = source(Some(true), 12);
    snapshot
        .nodes
        .get_mut("input")
        .unwrap()
        .fields
        .insert("strokeAlign".into(), Value::String("OUTSIDE".into()));
    let output = render(&snapshot);
    assert!(output.contains("p=\"12px\""), "{output}");
}

#[test]
fn missing_flag_reports_legacy_rendering_as_unverified_without_calling_it_false() {
    for flag in [None, Some(false), Some(true)] {
        let output =
            generate_component(&source(flag, 12), "input", &CodegenOptions::default()).unwrap();
        let unknown = output
            .diagnostics
            .iter()
            .find(|d| d.code == "DEVUP_CODEGEN_STROKE_LAYOUT_UNVERIFIED");
        assert_eq!(
            unknown.is_some(),
            flag.is_none(),
            "{flag:?}: {:?}",
            output.diagnostics
        );
        if let Some(diagnostic) = unknown {
            assert_eq!(
                diagnostic.details.as_ref().unwrap()["verification"]["state"],
                "unverified"
            );
        }
    }
}

#[test]
fn explicit_free_layout_ignores_stale_true_flag_and_inferred_auto_mode() {
    let mut explicit = source(Some(true), 12);
    let fields = &mut explicit.nodes.get_mut("input").unwrap().fields;
    fields.insert("layoutMode".into(), json!("NONE"));
    fields.insert(
        "inferredAutoLayout".into(),
        json!({"layoutMode":"HORIZONTAL","strokesIncludedInLayout":true}),
    );
    let mut absent = explicit.clone();
    absent
        .nodes
        .get_mut("input")
        .unwrap()
        .fields
        .remove("strokesIncludedInLayout");
    absent.nodes.get_mut("input").unwrap().fields["inferredAutoLayout"]
        .as_object_mut()
        .unwrap()
        .remove("strokesIncludedInLayout");
    assert_eq!(render(&explicit), render(&absent));
    assert!(render(&explicit).contains("p=\"11px\""));
    let output = generate_component(&absent, "input", &CodegenOptions::default()).unwrap();
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_STROKE_LAYOUT_UNVERIFIED"),
        "free layout has no applicable flag contract"
    );
}

#[test]
fn actual_auto_modes_honor_the_captured_true_flag() {
    for mode in ["HORIZONTAL", "VERTICAL", "GRID"] {
        let mut snapshot = source(Some(true), 12);
        snapshot
            .nodes
            .get_mut("input")
            .unwrap()
            .fields
            .insert("layoutMode".into(), json!(mode));
        assert!(render(&snapshot).contains("p=\"12px\""), "{mode}");
    }
}
