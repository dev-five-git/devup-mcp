use devup_mcp_devup_ui::codegen::{CodegenOptions, responsive::merge_confirmed_breakpoints};
use devup_mcp_figma::Snapshot;
use serde_json::json;
fn fixture() -> Snapshot {
    serde_json::from_str(include_str!("fixtures/oa-overview-child-order.json")).unwrap()
}
fn render(s: &Snapshot) -> devup_mcp_devup_ui::codegen::responsive::MergedScreen {
    merge_confirmed_breakpoints(s, &CodegenOptions::default())
        .unwrap()
        .unwrap()
}
fn compact(s: &str) -> String {
    s.split_whitespace().collect()
}
#[test]
fn actual_oa_overview_keeps_text_first_desktop_and_media_first_mobile() {
    let source = fixture();
    assert_eq!(
        source.nodes["73:9289"].fields["childrenIds"],
        json!(["73:9290", "73:9310"])
    );
    assert_eq!(
        source.nodes["73:9400"].fields["childrenIds"],
        json!(["73:9401", "73:9404"])
    );
    let output = render(&source);
    let text = compact(&output.tsx);
    assert!(
        text.contains("order={[\"0\",null,null,null,\"1\"]}"),
        "{text}"
    );
    assert!(
        text.contains("order={[\"1\",null,null,null,\"0\"]}"),
        "{text}"
    );
    if let Ok(file) = std::env::var("DEVUP_CHILD_ORDER_PROBE") {
        std::fs::write(file, output.module("ChildOrderProbe")).unwrap();
    }
    let derived = output
        .contract
        .iter()
        .filter(|c| c["derivedSource"].is_array())
        .collect::<Vec<_>>();
    assert_eq!(derived.len(), 2);
    assert!(derived.iter().all(|c| {
        c["derivedSource"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["sourceField"] == "childrenIds")
    }));
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_RESPONSIVE_READING_ORDER_REVIEW")
    );
}
#[test]
fn identical_order_does_not_add_css_order_or_review() {
    let mut s = fixture();
    s.nodes
        .get_mut("73:9400")
        .unwrap()
        .fields
        .insert("childrenIds".into(), json!(["73:9404", "73:9401"]));
    let o = render(&s);
    assert!(!o.tsx.contains("order="));
    assert!(
        !o.diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_RESPONSIVE_READING_ORDER_REVIEW")
    );
}
#[test]
fn missing_or_unknown_source_correspondence_is_not_guessed() {
    for case in ["missing-child", "unknown-layout", "missing-children"] {
        let mut s = fixture();
        let parent = s.nodes.get_mut("73:9400").unwrap();
        match case {
            "missing-child" => {
                parent
                    .fields
                    .insert("childrenIds".into(), json!(["73:9401"]));
            }
            "unknown-layout" => {
                parent.fields.remove("layoutMode");
            }
            _ => {
                parent.fields.remove("childrenIds");
            }
        }
        assert!(!render(&s).tsx.contains("order="), "{case}");
    }
}

#[test]
fn ambiguous_identity_or_absolute_child_does_not_invent_order() {
    for case in ["ambiguous", "absolute"] {
        let mut s = fixture();
        if case == "absolute" {
            s.nodes
                .get_mut("73:9401")
                .unwrap()
                .fields
                .insert("layoutPositioning".into(), json!("ABSOLUTE"));
        } else {
            for id in ["73:9290", "73:9310", "73:9401", "73:9404"] {
                s.nodes
                    .get_mut(id)
                    .unwrap()
                    .fields
                    .insert("name".into(), json!("same"));
            }
        }
        assert!(!render(&s).tsx.contains("order="), "{case}");
    }
}

#[test]
fn viewport_visibility_gates_are_retained_without_order_guessing() {
    let mut source = fixture();
    source
        .nodes
        .get_mut("73:9401")
        .unwrap()
        .fields
        .insert("visible".into(), json!(false));
    let output = render(&source);
    // A captured hidden child can still have a known sibling index; its
    // existing visibility gate must survive the independent order projection.
    assert!(output.tsx.contains("display="), "{}", output.tsx);
    assert!(output.tsx.contains("none"), "{}", output.tsx);
}
