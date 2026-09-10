use devup_mcp_devup_ui::{
    codegen::{CodegenOptions, CodegenOutput, generate_component},
    provenance::validate_fidelity,
};
use devup_mcp_figma::Snapshot;
use serde_json::{Value, json};
fn scene(vertical_fill: bool) -> Snapshot {
    let mut s: Snapshot =
        serde_json::from_str(include_str!("fixtures/r6-completion.json")).unwrap();
    s.nodes.clear();
    s.roots = vec!["root".into()];
    let nodes = if vertical_fill {
        vec![
            (
                "root",
                "FRAME",
                json!({"layoutMode":"HORIZONTAL","counterAxisAlignItems":"CENTER","layoutSizingHorizontal":"FIXED","layoutSizingVertical":"HUG","width":320,"height":55,"itemSpacing":8,"childrenIds":["sibling","fill"]}),
            ),
            (
                "sibling",
                "FRAME",
                json!({"parentId":"root","layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED","width":257,"height":55}),
            ),
            (
                "fill",
                "FRAME",
                json!({"parentId":"root","layoutMode":"HORIZONTAL","primaryAxisAlignItems":"CENTER","counterAxisAlignItems":"CENTER","layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FILL","width":55,"height":55,"targetAspectRatio":{"x":1,"y":1}}),
            ),
        ]
    } else {
        vec![
            (
                "root",
                "FRAME",
                json!({"layoutMode":"HORIZONTAL","counterAxisAlignItems":"CENTER","primaryAxisAlignItems":"CENTER","layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED","width":320,"height":20,"itemSpacing":16,"childrenIds":["fill","sibling","other"]}),
            ),
            (
                "fill",
                "RECTANGLE",
                json!({"parentId":"root","layoutSizingHorizontal":"FILL","layoutSizingVertical":"FIXED","layoutGrow":1,"width":101,"height":1,"absoluteRenderBounds":{"x":0,"y":0,"width":101,"height":1}}),
            ),
            (
                "sibling",
                "RECTANGLE",
                json!({"parentId":"root","layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED","width":86,"height":20}),
            ),
            (
                "other",
                "RECTANGLE",
                json!({"parentId":"root","layoutSizingHorizontal":"FILL","layoutSizingVertical":"FIXED","layoutGrow":1,"width":101,"height":1}),
            ),
        ]
    };
    for (id, kind, mut fields) in nodes {
        fields["maxWidth"] = json!(null);
        fields["maxHeight"] = json!(null);
        if fields.get("layoutMode").is_some() {
            fields["inferredAutoLayout"] = fields.clone();
        }
        s.nodes.insert(
            id.into(),
            serde_json::from_value(json!({"id":id,"type":kind,"fields":fields})).unwrap(),
        );
    }
    s
}
fn generate(s: &Snapshot) -> CodegenOutput {
    generate_component(s, "root", &CodegenOptions::default()).unwrap()
}
fn tag<'a>(o: &'a CodegenOutput, id: &str) -> &'a str {
    let r = o
        .source_map
        .entries
        .iter()
        .find(|e| e.node_id.as_deref() == Some(id) && e.property.is_none())
        .unwrap()
        .generated_range
        .as_ref()
        .unwrap();
    o.tsx[r.start..r.end].split('>').next().unwrap()
}
#[test]
fn r7_main_axis_fill_accounts_for_remaining_width() {
    let o = generate(&scene(false));
    assert!(tag(&o, "fill").contains("flex=\"1\""));
    assert!(!tag(&o, "fill").contains("101px"));
    assert!(
        o.source_map
            .entries
            .iter()
            .any(|e| e.node_id.as_deref() == Some("fill")
                && e.property.as_deref() == Some("width")
                && e.resolution == "accounted-for-implicit-flex-grow"),
        "{}\n{:?}",
        o.tsx,
        o.diagnostics
    );
}
#[test]
fn r7_main_axis_changed_gap_stays_lossy_with_reason() {
    let mut s = scene(false);
    s.nodes
        .get_mut("root")
        .unwrap()
        .fields
        .insert("itemSpacing".into(), json!(20));
    s.nodes.get_mut("root").unwrap().fields["inferredAutoLayout"]["itemSpacing"] = json!(20);
    let o = generate(&s);
    assert!(
        o.fidelity_report
            .uncovered_layout
            .contains(&"fill#width".into())
    );
    let d = o
        .diagnostics
        .iter()
        .find(|d| d.node_id.as_deref() == Some("fill") && d.property.as_deref() == Some("width"))
        .unwrap();
    assert_eq!(
        d.details.as_ref().unwrap()["implicitCssVerification"]["reasonCode"],
        "allocated-width-mismatch"
    );
}
#[test]
fn r7_cross_axis_fill_overrides_center_without_percentage_height() {
    let s = scene(true);
    let o = generate(&s);
    assert!(tag(&o, "root").contains("alignItems=\"center\""));
    assert!(!tag(&o, "root").contains("h="));
    assert!(
        tag(&o, "fill").contains("alignSelf=\"stretch\""),
        "{}",
        o.tsx
    );
    assert!(!tag(&o, "fill").contains("h=\"100%\""));
    assert!(
        !o.fidelity_report
            .uncovered_layout
            .contains(&"fill#layoutSizingVertical".into()),
        "{:?}",
        o.diagnostics
    );
}
#[test]
fn r7_forged_main_axis_resolution_is_rechecked() {
    let s = scene(false);
    let mut o = generate(&s);
    for e in &mut o.source_map.entries {
        if e.node_id.as_deref() == Some("fill") && e.property.as_deref() == Some("width") {
            e.resolution = "accounted-for-implicit-flex-grow".into();
        }
    }
    let mut changed = s.clone();
    changed
        .nodes
        .get_mut("root")
        .unwrap()
        .fields
        .insert("width".into(), Value::from(400));
    assert!(
        validate_fidelity(&changed, "root", &o)
            .unwrap()
            .uncovered_layout
            .contains(&"fill#width".into())
    );
}
#[test]
fn r7_auto_text_explicitly_reports_unverified_font_metrics() {
    let mut s = scene(false);
    let n = s.nodes.get_mut("sibling").unwrap();
    n.node_type = "TEXT".into();
    n.fields.insert("characters".into(), json!("홍"));
    n.fields
        .insert("textAutoResize".into(), json!("WIDTH_AND_HEIGHT"));
    let o = generate(&s);
    let d = o
        .diagnostics
        .iter()
        .find(|d| d.node_id.as_deref() == Some("sibling") && d.property.as_deref() == Some("width"))
        .unwrap();
    let detail = d.details.as_ref().unwrap();
    assert_eq!(detail["classification"], "text-auto-size");
    assert_eq!(detail["verification"]["state"], "unverified");
    assert_eq!(
        detail["verification"]["reasonCode"],
        "font-metrics-not-measured"
    );
    // R10: the content-size instruction is preserved; font pixels remain unverified.
    assert_eq!(d.fidelity_impact(), devup_mcp_figma::FidelityImpact::None);
    assert_eq!(detail["resolution"], "accounted-for-content-sizing");
    assert_eq!(d.code, "DEVUP_CODEGEN_LAYOUT_ACCOUNTED_FOR");
}
#[test]
fn r7_main_axis_unproven_constraints_remain_lossy() {
    for (id, field, value) in [
        ("fill", "minWidth", json!(120)),
        ("fill", "maxWidth", json!(90)),
        ("root", "layoutWrap", json!("WRAP")),
        ("sibling", "layoutSizingHorizontal", json!("HUG")),
    ] {
        let mut s = scene(false);
        s.nodes
            .get_mut(id)
            .unwrap()
            .fields
            .insert(field.into(), value.clone());
        if id == "root" {
            s.nodes.get_mut(id).unwrap().fields["inferredAutoLayout"][field] = value;
        }
        let o = generate(&s);
        assert!(
            !o.source_map
                .entries
                .iter()
                .any(|e| e.node_id.as_deref() == Some("fill")
                    && e.property.as_deref() == Some("width")
                    && e.resolution == "accounted-for-implicit-flex-grow"),
            "{id} {field}"
        );
    }
}
#[test]
fn r7_cross_axis_constraints_are_not_hidden_by_stretch() {
    let mut s = scene(true);
    s.nodes
        .get_mut("fill")
        .unwrap()
        .fields
        .insert("maxHeight".into(), json!(20));
    let o = generate(&s);
    assert!(
        o.fidelity_report
            .uncovered_layout
            .contains(&"fill#layoutSizingVertical".into())
    );
}
#[test]
fn r7_fixed_sibling_with_flex_cannot_be_subtracted_as_fixed_width() {
    let s = scene(false);
    let mut o = generate(&s);
    let r = o
        .source_map
        .entries
        .iter()
        .find(|e| e.node_id.as_deref() == Some("sibling") && e.property.is_none())
        .unwrap()
        .generated_range
        .as_ref()
        .unwrap()
        .clone();
    let start = r.start + o.tsx[r.start..r.end].find("h=\"20px\"").unwrap();
    o.tsx.replace_range(start..start + 8, "flex=\"1\"");
    assert!(
        validate_fidelity(&s, "root", &o)
            .unwrap()
            .uncovered_layout
            .contains(&"fill#width".into()),
        "{}",
        o.tsx
    );
}
#[test]
fn r7_replaced_element_minimum_is_not_assumed_zero() {
    let s = scene(false);
    let mut o = generate(&s);
    let r = o
        .source_map
        .entries
        .iter()
        .find(|e| e.node_id.as_deref() == Some("fill") && e.property.is_none())
        .unwrap()
        .generated_range
        .as_ref()
        .unwrap();
    let start = r.start + o.tsx[r.start..r.end].find("Box").unwrap();
    o.tsx.replace_range(start..start + 3, "img");
    assert!(
        validate_fidelity(&s, "root", &o)
            .unwrap()
            .uncovered_layout
            .contains(&"fill#width".into())
    );
}
#[test]
fn r7_mixed_horizontal_strokes_are_not_ignored() {
    for id in ["root", "fill"] {
        let mut s = scene(false);
        let f = &mut s.nodes.get_mut(id).unwrap().fields;
        f.insert(
            "strokes".into(),
            json!([{"type":"SOLID","color":{"r":0,"g":0,"b":0},"opacity":1}]),
        );
        for (side, w) in [
            ("strokeTopWeight", 0),
            ("strokeRightWeight", 0),
            ("strokeBottomWeight", 0),
            ("strokeLeftWeight", 10),
        ] {
            f.insert(side.into(), json!(w));
        }
        let o = generate(&s);
        assert!(tag(&o, id).contains("borderLeft="));
        assert!(
            o.fidelity_report
                .uncovered_layout
                .contains(&"fill#width".into()),
            "{id}: {}",
            o.tsx
        );
    }
}
