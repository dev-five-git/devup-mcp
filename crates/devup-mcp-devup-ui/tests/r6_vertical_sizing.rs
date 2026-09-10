use devup_mcp_devup_ui::{
    codegen::{CodegenOptions, CodegenOutput, RootLayout, generate_component},
    provenance::validate_fidelity,
};
use devup_mcp_figma::Snapshot;
use serde_json::json;
fn snapshot() -> Snapshot {
    serde_json::from_str(include_str!("fixtures/r6-completion.json")).unwrap()
}
fn options() -> CodegenOptions {
    let tokens: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/r6-completion-tokens.json")).unwrap();
    CodegenOptions {
        inline_instances: true,
        variable_tokens: serde_json::from_value(tokens["variableTokens"].clone()).unwrap(),
        text_style_tokens: serde_json::from_value(tokens["textStyleTokens"].clone()).unwrap(),
        ..Default::default()
    }
}
fn generate(s: &Snapshot) -> CodegenOutput {
    generate_component(s, "3997:46333", &options()).unwrap()
}
fn tag<'a>(o: &'a CodegenOutput, id: &str) -> &'a str {
    let e = o
        .source_map
        .entries
        .iter()
        .find(|e| e.node_id.as_deref() == Some(id) && e.property.is_none())
        .unwrap();
    let r = e.generated_range.as_ref().unwrap();
    o.tsx[r.start..r.end].split('>').next().unwrap()
}
#[test]
fn r6_completion_fixed_root_fill_body_and_bottom_action() {
    let s = snapshot();
    let o = generate(&s);
    if let Ok(directory) = std::env::var("DEVUP_R6_EVIDENCE_DIR") {
        let directory = std::path::Path::new(&directory);
        std::fs::write(directory.join("completion.tsx"), &o.tsx).unwrap();
        let entries: Vec<_> = o
            .source_map
            .entries
            .iter()
            .filter(|e| {
                matches!(
                    e.node_id.as_deref(),
                    Some("3997:46333" | "3997:46334" | "3997:46347")
                ) && matches!(
                    e.property.as_deref(),
                    Some("height" | "width" | "layoutSizingVertical" | "layoutGrow")
                )
            })
            .collect();
        std::fs::write(directory.join("completion-sizing-evidence.json"), serde_json::to_string_pretty(&json!({
            "sourceMap":entries,"fidelity":o.fidelity_report,
            "sizingDiagnostics":o.diagnostics.iter().filter(|d| matches!(d.code.as_str(), "DEVUP_CODEGEN_LAYOUT_UNCOVERED" | "DEVUP_CODEGEN_LAYOUT_ACCOUNTED_FOR" | "DEVUP_CODEGEN_PLACEMENT_CONTRACT")).collect::<Vec<_>>()
        })).unwrap()).unwrap();
    }
    assert!(tag(&o, "3997:46333").contains("h=\"740px\""), "{}", o.tsx);
    assert!(tag(&o, "3997:46334").contains("flex=\"1\""), "{}", o.tsx);
    assert!(!tag(&o, "3997:46334").contains("h="));
    assert!(!o.tsx.contains("pos=\"absolute\""));
    let ids: Vec<_> = o
        .source_map
        .entries
        .iter()
        .filter(|e| e.property.is_none())
        .filter_map(|e| e.node_id.as_deref())
        .collect();
    assert!(
        ids.iter().position(|id| *id == "3997:46334")
            < ids.iter().position(|id| *id == "3997:46351")
    );
    for (id, field) in [
        ("3997:46333", "layoutSizingVertical"),
        ("3997:46334", "layoutSizingVertical"),
        ("3997:46334", "layoutGrow"),
    ] {
        assert!(
            o.source_map
                .entries
                .iter()
                .any(|e| e.node_id.as_deref() == Some(id) && e.property.as_deref() == Some(field)),
            "missing {id}#{field}"
        );
        assert!(
            !o.fidelity_report
                .uncovered_layout
                .contains(&format!("{id}#{field}"))
        );
    }
}
#[test]
fn r6_embedded_vertical_relation_requires_host_height() {
    let s = snapshot();
    let o = generate_component(
        &s,
        "3997:46333",
        &CodegenOptions {
            root_layout: RootLayout::Embedded,
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    let d = o
        .diagnostics
        .iter()
        .find(|d| d.code == "DEVUP_CODEGEN_PLACEMENT_CONTRACT")
        .unwrap();
    assert!(
        !d.details.as_ref().unwrap()["hostRequirements"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        o.diagnostics
            .iter()
            .any(|d| d.property.as_deref() == Some("layoutSizingVertical")
                && d.fidelity_impact() == devup_mcp_figma::FidelityImpact::Lossy)
    );
}
#[test]
fn r6_lost_vertical_semantics_cannot_pass_fidelity() {
    let s = snapshot();
    let mut o = generate(&s);
    o.source_map.entries.retain(|e| {
        !matches!(
            e.property.as_deref(),
            Some("layoutSizingVertical" | "layoutGrow")
        )
    });
    let f = validate_fidelity(&s, "3997:46333", &o).unwrap();
    for key in [
        "3997:46333#layoutSizingVertical",
        "3997:46334#layoutSizingVertical",
        "3997:46334#layoutGrow",
    ] {
        assert!(
            f.uncovered_layout.contains(&key.into()),
            "missing {key}: {f:?}"
        );
    }
}
#[test]
fn r6_divider_width_is_verified_implicit_stretch() {
    let o = generate(&snapshot());
    assert!(
        !o.fidelity_report
            .uncovered_layout
            .contains(&"3997:46347#width".into()),
        "{:?}",
        o.fidelity_report
    );
    assert!(
        o.source_map
            .entries
            .iter()
            .any(|e| e.node_id.as_deref() == Some("3997:46347")
                && e.property.as_deref() == Some("width")
                && e.resolution.contains("implicit-flex-stretch"))
    );
}
#[test]
fn r6_centered_parent_does_not_explain_divider_width() {
    let mut s = snapshot();
    s.nodes
        .get_mut("3997:46334")
        .unwrap()
        .fields
        .insert("counterAxisAlignItems".into(), json!("CENTER"));
    let o = generate(&s);
    let d = o
        .diagnostics
        .iter()
        .find(|d| {
            d.node_id.as_deref() == Some("3997:46347") && d.property.as_deref() == Some("width")
        })
        .unwrap();
    assert_eq!(d.fidelity_impact(), devup_mcp_figma::FidelityImpact::Lossy);
    assert_eq!(
        d.details.as_ref().unwrap()["implicitCssVerification"]["state"],
        "not-accounted-for"
    );
}

#[test]
fn r6_hug_and_fill_roots_are_not_pinned_to_captured_height() {
    for sizing in ["HUG", "FILL"] {
        let mut s = snapshot();
        s.nodes
            .get_mut("3997:46333")
            .unwrap()
            .fields
            .insert("layoutSizingVertical".into(), json!(sizing));
        let o = generate(&s);
        assert!(!tag(&o, "3997:46333").contains("740px"));
        assert!(!tag(&o, "3997:46334").contains("665px"));
    }
}
#[test]
fn r6_unknown_parent_css_is_not_claimed_as_verified() {
    let mut s = snapshot();
    s.nodes
        .get_mut("3997:46334")
        .unwrap()
        .fields
        .insert("layoutMode".into(), json!("NONE"));
    s.nodes
        .get_mut("3997:46334")
        .unwrap()
        .fields
        .insert("inferredAutoLayout".into(), json!(null));
    let o = generate(&s);
    let d = o
        .diagnostics
        .iter()
        .find(|d| {
            d.node_id.as_deref() == Some("3997:46347") && d.property.as_deref() == Some("width")
        })
        .unwrap();
    assert_eq!(d.fidelity_impact(), devup_mcp_figma::FidelityImpact::Lossy);
    assert_eq!(
        d.details.as_ref().unwrap()["implicitCssVerification"]["state"],
        "not-verifiable"
    );
}

#[test]
fn r6_nested_vertical_fill_uses_the_ancestor_height_basis() {
    let mut s = snapshot();
    let child = s.nodes.get_mut("3997:46348").unwrap();
    child
        .fields
        .insert("layoutSizingVertical".into(), json!("FILL"));
    child.fields.insert("layoutGrow".into(), json!(1));
    let o = generate(&s);
    assert!(
        !o.fidelity_report
            .uncovered_layout
            .contains(&"3997:46348#layoutSizingVertical".into()),
        "{:?}",
        o.fidelity_report.uncovered_layout
    );
}

#[test]
fn r6_replay_records_existing_frames_without_hiding_new_losses() {
    use devup_mcp_figma::CollectedPayload;
    for (raw, roots) in [
        (
            include_str!("../../../fixtures/r2/wquw-118-payload.json"),
            vec!["3997:46242", "3997:46277", "3997:46129"],
        ),
        (
            include_str!("../../../fixtures/r2/wquw-120-payload.json"),
            vec!["3997:46582"],
        ),
    ] {
        let payload: CollectedPayload = serde_json::from_str(raw).unwrap();
        for root in roots {
            for inline in [true, false] {
                let o = generate_component(
                    &payload.snapshot,
                    root,
                    &CodegenOptions {
                        inline_instances: inline,
                        ..Default::default()
                    }
                    .with_payload_tokens(&payload),
                )
                .unwrap();
                let issues:Vec<_>=o.diagnostics.iter().filter(|d|d.code=="DEVUP_CODEGEN_LAYOUT_UNCOVERED").map(|d|json!({"id":d.node_id,"property":d.property,"class":d.details.as_ref().unwrap()["classification"]})).collect();
                println!(
                    "R6_AUDIT {}",
                    json!({"root":root,"inline":inline,"layout":o.fidelity_report.layout,"issues":issues})
                );
                if root == "3997:46582" {
                    let t = tag(&o, root);
                    for p in ["h=\"740px\"", "w=\"360px\"", "pos=\"relative\""] {
                        assert!(t.contains(p));
                    }
                }
            }
        }
    }
}

#[test]
fn r6_stretch_proof_is_rechecked_after_generated_parent_changes() {
    let s = snapshot();
    let mut o = generate(&s);
    let r = o
        .source_map
        .entries
        .iter()
        .find(|e| e.node_id.as_deref() == Some("3997:46334") && e.property.is_none())
        .unwrap()
        .generated_range
        .as_ref()
        .unwrap()
        .clone();
    let source = o.tsx[r.start..r.end].replace("VStack", "Center");
    assert_eq!(source.len(), r.end - r.start);
    o.tsx.replace_range(r.start..r.end, &source);
    let f = validate_fidelity(&s, "3997:46333", &o).unwrap();
    assert!(f.uncovered_layout.contains(&"3997:46347#width".into()));
}

#[test]
fn r6_overflowing_fill_body_can_shrink_without_displacing_bottom_action() {
    let mut s = snapshot();
    let content = s.nodes.get_mut("3997:46344").unwrap();
    content
        .fields
        .insert("layoutSizingVertical".into(), json!("FIXED"));
    content.fields.insert("height".into(), json!(800));
    let o = generate(&s);
    assert!(tag(&o, "3997:46334").contains("flex=\"1\""));
    assert!(tag(&o, "3997:46334").contains("minH=\"0\""), "{}", o.tsx);
    assert!(!tag(&o, "3997:46334").contains("665px"));
    assert!(tag(&o, "3997:46344").contains("h=\"800px\""));
}

#[test]
fn r6_fractional_fixed_height_keeps_its_verified_sizing_mapping() {
    let mut s = snapshot();
    s.nodes
        .get_mut("3997:46333")
        .unwrap()
        .fields
        .insert("height".into(), json!(740.123456));
    let o = generate(&s);
    assert!(tag(&o, "3997:46333").contains("h=\"740.12px\""));
    assert!(
        !o.fidelity_report
            .uncovered_layout
            .contains(&"3997:46333#layoutSizingVertical".into())
    );
}

#[test]
fn r6_horizontal_body_does_not_sum_side_by_side_child_heights() {
    let mut s = snapshot();
    let body = s.nodes.get_mut("3997:46334").unwrap();
    body.fields.insert("layoutMode".into(), json!("HORIZONTAL"));
    body.fields.get_mut("inferredAutoLayout").unwrap()["layoutMode"] = json!("HORIZONTAL");
    body.fields
        .insert("childrenIds".into(), json!(["3997:46335", "3997:46344"]));
    for id in ["3997:46335", "3997:46344"] {
        s.nodes
            .get_mut(id)
            .unwrap()
            .fields
            .insert("height".into(), json!(400));
    }
    let o = generate(&s);
    assert!(!tag(&o, "3997:46334").contains("minH=\"0\""));
}
