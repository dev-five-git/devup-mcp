use devup_mcp_devup_ui::{
    codegen::{CodegenOptions, generate_component},
    provenance::validate_fidelity,
};
use devup_mcp_figma::Snapshot;
use serde_json::json;
fn scene(alignment: &str) -> Snapshot {
    let mut s: Snapshot =
        serde_json::from_str(include_str!("fixtures/r6-completion.json")).unwrap();
    s.nodes.clear();
    s.roots = vec!["root".into()];
    for (id, kind, mut fields) in [
        (
            "root",
            "FRAME",
            json!({"layoutMode":"VERTICAL","counterAxisAlignItems":alignment,"layoutSizingHorizontal":"FIXED","layoutSizingVertical":"HUG","width":360,"height":100,"childrenIds":["fill"]}),
        ),
        (
            "fill",
            "FRAME",
            json!({"parentId":"root","layoutMode":"HORIZONTAL","primaryAxisAlignItems":"CENTER","counterAxisAlignItems":"CENTER","layoutSizingHorizontal":"FILL","layoutSizingVertical":"HUG","width":280,"height":60,"childrenIds":["text"]}),
        ),
        (
            "text",
            "TEXT",
            json!({"parentId":"fill","characters":"짧음","fontSize":15,"layoutSizingHorizontal":"HUG","layoutSizingVertical":"HUG","width":30,"height":20}),
        ),
    ] {
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
#[test]
fn r17_non_stretch_parent_emits_fluid_horizontal_fill() {
    for align in ["MAX", "CENTER", "BASELINE"] {
        let s = scene(align);
        let o = generate_component(&s, "root", &CodegenOptions::default()).unwrap();
        let node = o
            .source_map
            .entries
            .iter()
            .find(|e| e.node_id.as_deref() == Some("fill") && e.property.is_none())
            .unwrap();
        let r = node.generated_range.as_ref().unwrap();
        let tag = o.tsx[r.start..r.end].split('>').next().unwrap();
        assert!(
            tag.contains("w=\"100%\"") || tag.contains("alignSelf=\"stretch\""),
            "{tag}"
        );
        assert!(!tag.contains("280px"));
        assert!(
            !o.fidelity_report
                .uncovered_layout
                .contains(&"fill#layoutSizingHorizontal".into())
        );
    }
}
#[test]
fn r17_source_to_generated_contract_detects_absent_horizontal_fill_mapping() {
    let s = scene("MAX");
    let mut o = generate_component(&s, "root", &CodegenOptions::default()).unwrap();
    o.source_map
        .entries
        .retain(|e| e.property.as_deref() != Some("layoutSizingHorizontal"));
    let report = validate_fidelity(&s, "root", &o).unwrap();
    assert!(
        report
            .uncovered_layout
            .contains(&"fill#layoutSizingHorizontal".into()),
        "{report:?}"
    );
}

#[test]
fn r17_token_recommendations_include_mode_scope() {
    use devup_mcp_devup_ui::{theme::parse_project_theme, ui_validate::validate_devup_ui_tsx};
    let theme = parse_project_theme(r##"{"theme":{"colors":{"light":{"base":"#FFF","innerBg":"#FFF"},"dark":{"base":"#000","innerBg":"#FFF"}}}}"##).unwrap();
    let v = serde_json::to_value(validate_devup_ui_tsx(
        r##"<Box color="#FFF"/>"##,
        Some(&theme),
        false,
    ))
    .unwrap();
    let matches = &v["violations"][0]["tokenMatches"];
    assert_eq!(matches[0]["matchingModes"], json!(["light"]));
    assert_eq!(matches[0]["allModesMatch"], false);
    assert_eq!(matches[1]["allModesMatch"], true);
}
#[test]
fn r17_korean_diagnostic_has_character_position_and_snippet() {
    use devup_mcp_devup_ui::ui_validate::validate_devup_ui_tsx;
    let v = serde_json::to_value(validate_devup_ui_tsx(
        "// 한글😀\n<Box 잘못=\"값\"/>",
        None,
        false,
    ))
    .unwrap();
    assert_eq!(v["violations"][0]["line"], 2);
    assert_eq!(v["violations"][0]["column"], 6);
    assert!(
        v["violations"][0]["snippet"]
            .as_str()
            .unwrap_or("")
            .contains("잘못")
    );
}
#[test]
fn r17_finite_conditional_branches_are_extractable_but_dynamic_leaves_are_not() {
    use devup_mcp_devup_ui::ui_validate::validate_devup_ui_tsx;
    for code in [
        "css({backdropFilter: backdropVariant === 'strong' ? 'blur(4px)' : undefined})",
        "css({bg: a ? '#fff' : b ? '#000' : undefined})",
    ] {
        let report = validate_devup_ui_tsx(code, None, false);
        assert!(report.ok, "{report:?}");
    }
    for code in [
        "css({bg: a ? dynamic : '#fff'})",
        "css({bg: a ? getColor() : '#fff'})",
        "globalCss({body: {bg: active ? '#fff' : '#000'}})",
        "keyframes({from: {opacity: active ? 0 : 1}})",
    ] {
        assert!(!validate_devup_ui_tsx(code, None, false).ok);
    }
}

#[test]
fn r17_source_layout_contract_checks_each_obligation_independent_of_emitted_attributes() {
    use devup_mcp_devup_ui::provenance::attributes::{attribute_contract, uncovered_attributes};
    let mut s = scene("MAX");
    for (key, value) in [
        ("paddingTop", json!(20)),
        ("paddingRight", json!(20)),
        ("paddingBottom", json!(20)),
        ("paddingLeft", json!(20)),
    ] {
        s.nodes
            .get_mut("fill")
            .unwrap()
            .fields
            .insert(key.into(), value);
    }
    let o = generate_component(&s, "root", &CodegenOptions::default()).unwrap();
    // Both directions are required: a complete generated-attribute catalog
    // cannot compensate for an omitted source obligation.
    assert!(uncovered_attributes(&o.tsx, &attribute_contract(&o)).is_empty());
    for (id, field) in [
        ("root", "layoutMode"),
        ("root", "width"),
        ("fill", "layoutMode"),
        ("fill", "layoutSizingHorizontal"),
        ("fill", "paddingTop"),
        ("fill", "paddingRight"),
        ("fill", "paddingBottom"),
        ("fill", "paddingLeft"),
    ] {
        let mut missing = o.clone();
        missing.source_map.entries.retain(|e| {
            !(e.node_id.as_deref() == Some(id) && e.property.as_deref() == Some(field))
        });
        let f = validate_fidelity(&s, "root", &missing).unwrap();
        assert!(
            f.uncovered_layout.contains(&format!("{id}#{field}")),
            "{id}#{field}: {f:?}"
        );
    }
}
#[test]
fn r17_source_contract_rechecks_emitted_css_not_resolution_labels() {
    let s = scene("MAX");
    let mut o = generate_component(&s, "root", &CodegenOptions::default()).unwrap();
    o.tsx = o.tsx.replace("w=\"100%\"", "w=\"280px\"");
    assert!(
        validate_fidelity(&s, "root", &o)
            .unwrap()
            .uncovered_layout
            .contains(&"fill#layoutSizingHorizontal".into())
    );
}
#[test]
fn r17_unknown_parent_stays_lossy_for_horizontal_fill() {
    let mut s = scene("MAX");
    let root = s.nodes.get_mut("root").unwrap();
    root.fields.insert("layoutMode".into(), json!("NONE"));
    root.fields.insert("inferredAutoLayout".into(), json!(null));
    let o = generate_component(&s, "root", &CodegenOptions::default()).unwrap();
    assert!(
        o.fidelity_report
            .uncovered_layout
            .contains(&"fill#layoutSizingHorizontal".into())
    );
    assert!(
        o.diagnostics
            .iter()
            .any(|d| d.property.as_deref() == Some("layoutSizingHorizontal")
                && d.fidelity_impact() == devup_mcp_figma::FidelityImpact::Lossy)
    );
}
#[test]
fn r17_final_contract_detects_deleted_attribute() {
    use devup_mcp_devup_ui::provenance::attributes::{attribute_contract, uncovered_attributes};
    let s = scene("MAX");
    let o = generate_component(&s, "root", &CodegenOptions::default()).unwrap();
    let contract = attribute_contract(&o);
    let removed = o.tsx.replace("w=\"100%\"", "");
    assert!(!uncovered_attributes(&removed, &contract).is_empty());
}

#[test]
fn r17_default_stretch_is_verified_without_redundant_width() {
    let s = scene("MIN");
    let o = generate_component(&s, "root", &CodegenOptions::default()).unwrap();
    assert!(!o.tsx.contains("w=\"100%\""));
    assert!(
        !o.fidelity_report
            .uncovered_layout
            .contains(&"fill#layoutSizingHorizontal".into())
    );
    assert!(
        o.source_map
            .entries
            .iter()
            .any(|e| e.node_id.as_deref() == Some("fill")
                && e.property.as_deref() == Some("layoutSizingHorizontal")
                && e.resolution == "accounted-for-implicit-flex-stretch")
    );
}
#[test]
fn r17_real_3831_10720_fill_has_explicit_width_and_source_mapping() {
    let s: Snapshot = serde_json::from_str(include_str!("fixtures/r17-loading.json")).unwrap();
    let o = generate_component(
        &s,
        "3831:10708",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    let entry = o
        .source_map
        .entries
        .iter()
        .find(|e| {
            e.node_id.as_deref() == Some("3831:10720")
                && e.property.as_deref() == Some("layoutSizingHorizontal")
        })
        .expect("original FILL obligation must have mapping");
    assert_eq!(entry.generated_property.as_deref(), Some("w=\"100%\""));
    assert!(
        !o.fidelity_report
            .uncovered_layout
            .contains(&"3831:10720#layoutSizingHorizontal".into())
    );
}
#[test]
fn r17_horizontal_sizing_never_claims_vertical_property_as_its_mapping() {
    let mut s = scene("MIN");
    s.nodes
        .get_mut("fill")
        .unwrap()
        .fields
        .insert("layoutSizingVertical".into(), json!("FIXED"));
    let o = generate_component(&s, "root", &CodegenOptions::default()).unwrap();
    assert!(!o.source_map.entries.iter().any(|e| {
        e.property.as_deref() == Some("layoutSizingHorizontal")
            && e.resolution == "verified-layout-sizing"
            && e.generated_property
                .as_deref()
                .is_some_and(|p| p.starts_with("h="))
    }));
}

#[test]
fn r17_horizontal_fill_does_not_claim_vertical_flex_grow() {
    let mut s = scene("MIN");
    s.nodes
        .get_mut("root")
        .unwrap()
        .fields
        .insert("layoutSizingVertical".into(), json!("FIXED"));
    s.nodes
        .get_mut("fill")
        .unwrap()
        .fields
        .insert("layoutSizingVertical".into(), json!("FILL"));
    let o = generate_component(&s, "root", &CodegenOptions::default()).unwrap();
    assert!(o.tsx.contains("flex=\"1\""));
    let horizontal: Vec<_> = o
        .source_map
        .entries
        .iter()
        .filter(|e| {
            e.node_id.as_deref() == Some("fill")
                && e.property.as_deref() == Some("layoutSizingHorizontal")
        })
        .collect();
    assert!(!horizontal.is_empty());
    assert!(
        horizontal
            .iter()
            .all(|e| e.generated_property.as_deref() != Some("flex=\"1\""))
    );
    assert!(
        horizontal
            .iter()
            .any(|e| e.resolution == "accounted-for-implicit-flex-stretch")
    );
}
