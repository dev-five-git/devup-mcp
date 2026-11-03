use devup_mcp_devup_ui::codegen::{CodegenOptions, responsive::merge_breakpoints};
use devup_mcp_figma::Snapshot;
use serde_json::{Value, json};

fn merged(snapshot: Snapshot) -> String {
    render(snapshot, true)
}

fn render(snapshot: Snapshot, verified: bool) -> String {
    let output = merge_breakpoints(
        &snapshot,
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(output.gaps.is_empty(), verified, "{:?}", output.gaps);
    output.tsx.split_whitespace().collect()
}

fn simple(mobile: Value, desktop: Value) -> Snapshot {
    let node = |id: &str, width, paints: Value| {
        let mut fields = json!({"name":if id=="m" {"mobile"} else {"desktop"},
            "width":width,"height":50,"layoutMode":"HORIZONTAL",
            "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED"});
        fields
            .as_object_mut()
            .unwrap()
            .extend(paints.as_object().unwrap().clone());
        json!({"id":id,"type":"FRAME","fields":fields})
    };
    serde_json::from_value(json!({"fileKey":"paint","version":null,"roots":["d","m"],
        "nodes":{"d":node("d",1440,desktop),"m":node("m",360,mobile)},"diagnostics":[]}))
    .unwrap()
}

fn paints() -> Value {
    json!({"fills":[{"type":"SOLID","color":{"r":1,"g":0,"b":0}}],
        "strokes":[{"type":"SOLID","color":{"r":0,"g":0,"b":0}}],
        "strokeWeight":1,"strokeAlign":"INSIDE","strokesIncludedInLayout":true,
        "paddingTop":2,"paddingBottom":2,"paddingLeft":2,"paddingRight":2})
}

#[test]
fn actual_oa_desktop_wrapper_does_not_inherit_mobile_paint() {
    // Exact captured PageMenu subtrees; only root names/parent links are adapted
    // to make this bounded collection independently discoverable as breakpoints.
    let snapshot: Snapshot =
        serde_json::from_str(include_str!("fixtures/oa-pagemenu-paints.json")).unwrap();
    let source = merged(snapshot);
    assert!(
        source.contains("bg={[\"$349\",null,null,null,\"initial\"]}"),
        "{source}"
    );
    assert!(
        source.contains("borderBottom={[\"solid1px#E5E7EB\",null,null,null,\"initial\"]}"),
        "{source}"
    );
}

#[test]
fn absent_paints_reset_in_both_directions_and_explicit_empty_arrays_match_omission() {
    for empty in [json!({}), json!({"fills":[],"strokes":[]})] {
        let forward = merged(simple(paints(), empty.clone()));
        assert!(
            forward.contains("bg={[\"#F00\",null,null,null,\"initial\"]}"),
            "{forward}"
        );
        assert!(
            forward.contains("border={[\"solid1px#000\",null,null,null,\"initial\"]}"),
            "{forward}"
        );
        let reverse = merged(simple(empty, paints()));
        assert!(
            reverse.contains("bg={[\"initial\",null,null,null,\"#F00\"]}"),
            "{reverse}"
        );
    }
}

#[test]
fn unreadable_or_malformed_paints_are_not_asserted_empty() {
    for field in [json!({"$unsupported":"mixed"}), Value::Null] {
        let source = render(
            simple(paints(), json!({"fills":field,"strokes":field})),
            false,
        );
        assert!(!source.contains("bg={["), "{source}");
        assert!(!source.contains("border={["), "{source}");
    }
    let mut snapshot = simple(paints(), json!({}));
    for field in ["fills", "strokes"] {
        snapshot
            .nodes
            .get_mut("d")
            .unwrap()
            .field_errors
            .insert(field.into(), "read failed".into());
    }
    let source = render(snapshot, false);
    assert!(!source.contains("bg={["), "{source}");
    assert!(!source.contains("border={["), "{source}");
}

#[test]
fn known_empty_effects_reset_shadows_but_keep_unrelated_text_color_policy() {
    let mut painted = paints();
    painted["effects"] = json!([{"type":"DROP_SHADOW","color":{"r":0,"g":0,"b":0,"a":1},"offset":{"x":0,"y":2},"radius":4,"spread":0}]);
    let source = merged(simple(painted, json!({})));
    assert!(source.contains("boxShadow={["), "{source}");
    assert!(source.contains("\"initial\"]}"), "{source}");
}

fn mixed_edges(right: u32, bottom: u32) -> Value {
    let mut value = paints();
    value["strokeWeight"] = json!({"$unsupported":"symbol"});
    value["strokeTopWeight"] = json!(0);
    value["strokeLeftWeight"] = json!(0);
    value["strokeRightWeight"] = json!(right);
    value["strokeBottomWeight"] = json!(bottom);
    value
}

#[test]
fn captured_product_card_resets_only_the_known_zero_stroke_edge() {
    // Captured page roots keep their real1440/360 breakpoint widths; each is
    // bounded to the corresponding unchanged card subtree for this regression.
    let snapshot: Snapshot =
        serde_json::from_str(include_str!("fixtures/product-card-mixed-strokes.json")).unwrap();
    let source = merged(snapshot);
    assert!(
        source.contains("borderBottom={[\"solid1px#E5E7EB\",null,null,null,\"initial\"]}"),
        "{source}"
    );
    assert!(
        source.contains("borderRight={[\"initial\",null,null,null,\"solid1px#E5E7EB\"]}"),
        "{source}"
    );
}

#[test]
fn mixed_stroke_edges_reset_in_both_responsive_directions() {
    for (mobile, desktop, first, second) in [
        (
            mixed_edges(0, 1),
            mixed_edges(1, 0),
            "borderBottom",
            "borderRight",
        ),
        (
            mixed_edges(1, 0),
            mixed_edges(0, 1),
            "borderRight",
            "borderBottom",
        ),
    ] {
        let source = merged(simple(mobile, desktop));
        assert!(
            source.contains(&format!(
                "{first}={{[\"solid1px#000\",null,null,null,\"initial\"]}}"
            )),
            "{source}"
        );
        assert!(
            source.contains(&format!(
                "{second}={{[\"initial\",null,null,null,\"solid1px#000\"]}}"
            )),
            "{source}"
        );
    }
}

#[test]
fn unreadable_zero_edge_does_not_become_a_verified_reset() {
    for unknown in [Value::Null, json!({"$unsupported":"symbol"})] {
        let mut desktop = mixed_edges(1, 0);
        desktop["strokeBottomWeight"] = unknown;
        let source = render(simple(mixed_edges(0, 1), desktop), false);
        assert!(!source.contains("borderBottom={["), "{source}");
    }
    let mut snapshot = simple(mixed_edges(0, 1), mixed_edges(1, 0));
    snapshot
        .nodes
        .get_mut("d")
        .unwrap()
        .field_errors
        .insert("strokeBottomWeight".into(), "read failed".into());
    let source = render(snapshot, false);
    assert!(!source.contains("borderBottom={["), "{source}");
    let mut snapshot = simple(mixed_edges(0, 1), mixed_edges(1, 0));
    snapshot
        .nodes
        .get_mut("d")
        .unwrap()
        .fields
        .remove("strokeBottomWeight");
    let source = render(snapshot, false);
    assert!(!source.contains("borderBottom={["), "{source}");
}
