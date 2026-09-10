use devup_mcp_devup_ui::{
    codegen::{CodegenOptions, generate_component},
    provenance::validate_fidelity,
};
use devup_mcp_figma::{FidelityImpact, Snapshot};
use serde_json::json;

fn scene(mode: &str) -> Snapshot {
    let mut s: Snapshot =
        serde_json::from_str(include_str!("fixtures/r6-completion.json")).unwrap();
    s.nodes.clear();
    s.roots = vec!["text".into()];
    s.nodes.insert("text".into(), serde_json::from_value(json!({"id":"text","type":"TEXT","fields":{
        "characters":"홍", "width":16,"height":18,"fontSize":18,"fontFamily":"Pretendard", "textTruncation":"DISABLED",
        "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED","textAutoResize":mode
    }})).unwrap());
    s
}

#[test]
fn r10_content_sizing_accounts_for_intent_and_keeps_pixel_uncertainty() {
    for (mode, fields) in [
        ("WIDTH_AND_HEIGHT", vec!["width", "height"]),
        ("HEIGHT", vec!["height"]),
    ] {
        let s = scene(mode);
        let o = generate_component(&s, "text", &CodegenOptions::default()).unwrap();
        for field in fields {
            assert!(
                o.source_map
                    .entries
                    .iter()
                    .any(|e| e.property.as_deref() == Some(field)
                        && e.resolution == "accounted-for-content-sizing"),
                "{mode} {field}: {}",
                o.tsx
            );
            assert!(
                !o.fidelity_report
                    .uncovered_layout
                    .contains(&format!("text#{field}"))
            );
            let d = o
                .diagnostics
                .iter()
                .find(|d| d.property.as_deref() == Some(field))
                .unwrap();
            assert_eq!(d.fidelity_impact(), FidelityImpact::None);
            assert_eq!(
                d.details.as_ref().unwrap()["verification"]["state"],
                "unverified"
            );
            assert_eq!(
                d.details.as_ref().unwrap()["verification"]["reasonCode"],
                "font-metrics-not-measured"
            );
        }
    }
}

#[test]
fn r10_content_sizing_does_not_require_unrelated_typography_props() {
    let mut s = scene("WIDTH_AND_HEIGHT");
    let n = s.nodes.get_mut("text").unwrap();
    n.fields.insert("characters".into(), json!("hello"));
    n.fields.remove("fontSize");
    n.fields.remove("fontFamily");
    let o = generate_component(&s, "text", &CodegenOptions::default()).unwrap();
    assert!(
        o.source_map
            .entries
            .iter()
            .any(|e| e.property.as_deref() == Some("width")
                && e.resolution == "accounted-for-content-sizing"),
        "{}",
        o.tsx
    );
}

#[test]
fn r10_content_sizing_claim_is_rechecked_against_source_and_generated_element() {
    let s = scene("WIDTH_AND_HEIGHT");
    let o = generate_component(&s, "text", &CodegenOptions::default()).unwrap();
    for (field, value) in [
        ("textAutoResize", json!("NONE")),
        ("textAutoResize", json!("TRUNCATE")),
    ] {
        let mut changed = s.clone();
        changed
            .nodes
            .get_mut("text")
            .unwrap()
            .fields
            .insert(field.into(), value);
        let report = validate_fidelity(&changed, "text", &o).unwrap();
        assert!(report.uncovered_layout.contains(&"text#width".into()));
        assert!(report.uncovered_layout.contains(&"text#height".into()));
    }
    let mut changed = o.clone();
    changed.tsx = changed
        .tsx
        .replace("<Text", "<Fake")
        .replace("</Text", "</Fake");
    assert!(
        validate_fidelity(&s, "text", &changed)
            .unwrap()
            .uncovered_layout
            .contains(&"text#width".into())
    );
    let mut unread = s.clone();
    unread
        .nodes
        .get_mut("text")
        .unwrap()
        .field_errors
        .insert("textAutoResize".into(), "read failed".into());
    assert!(
        validate_fidelity(&unread, "text", &o)
            .unwrap()
            .uncovered_layout
            .contains(&"text#width".into())
    );
}
