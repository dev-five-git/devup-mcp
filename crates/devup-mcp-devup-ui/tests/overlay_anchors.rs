use devup_mcp_devup_ui::codegen::{
    CodegenOptions, generate_component, overlay_anchor_metadata,
    responsive::merge_confirmed_breakpoints,
};
use devup_mcp_figma::Snapshot;
use serde_json::json;

fn options() -> CodegenOptions {
    CodegenOptions {
        inline_instances: true,
        ..Default::default()
    }
}

fn source() -> Snapshot {
    serde_json::from_value(json!({"fileKey":"dropdown-anchor","roots":["root"],"diagnostics":[],"nodes":{
        "root":{"id":"root","type":"FRAME","fields":{"name":"Screen","width":360,"height":500,"layoutMode":"VERTICAL","childrenIds":["control","overlay"],"absoluteBoundingBox":{"x":20,"y":30,"width":360,"height":500}}},
        "control":{"id":"control","type":"INSTANCE","fields":{"name":"input","mainComponentId":"input-master","parentId":"root","layoutMode":"HORIZONTAL","width":320,"height":51,"layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED","childrenIds":["caret"],"absoluteBoundingBox":{"x":40,"y":100,"width":320,"height":51}}},
        "caret":{"id":"caret","type":"INSTANCE","fields":{"name":"Caret","mainComponentId":"caret-master","mainComponentName":"type=CaretDown","parentId":"control","width":12,"height":12}},
        "overlay":{"id":"overlay","type":"FRAME","fields":{"name":"Frame 547","parentId":"root","layoutMode":"VERTICAL","layoutPositioning":"ABSOLUTE","width":320,"height":94,"childrenIds":["option"],"absoluteBoundingBox":{"x":40,"y":159,"width":320,"height":94}}},
        "option":{"id":"option","type":"INSTANCE","fields":{"name":"DropdownOption","mainComponentId":"option-master","parentId":"overlay","width":318,"height":47}}
    }})).unwrap()
}

#[test]
fn unique_captured_dropdown_relation_emits_fallback_anchors_and_source_metadata() {
    let snapshot = source();
    let output = generate_component(&snapshot, "root", &options()).unwrap();
    for expected in [
        "anchorName=",
        "anchorScope=\"all\"",
        "positionAnchor=",
        "top=\"calc(anchor(bottom, 121px) + 8px)\"",
        "left=\"anchor(left, 20px)\"",
        "w=\"anchor-size(width, 320px)\"",
    ] {
        assert!(
            output.tsx.contains(expected),
            "missing {expected}: {}",
            output.tsx
        );
    }
    assert!(!output.tsx.contains("// @devup-"));
    if let Ok(path) = std::env::var("DEVUP_ANCHOR_PROBE_OUTPUT") {
        std::fs::write(path, &output.tsx).unwrap();
    }
    let mut without_relation = snapshot.clone();
    without_relation
        .nodes
        .get_mut("option")
        .unwrap()
        .fields
        .insert("name".into(), json!("Option"));
    let baseline = generate_component(&without_relation, "root", &options()).unwrap();
    let tags = |tsx: &str| {
        devup_mcp_devup_ui::provenance::attributes::opening_elements(tsx)
            .into_iter()
            .map(|element| {
                tsx[element.start + 1..element.end]
                    .split(|c: char| !c.is_alphanumeric())
                    .next()
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        tags(&output.tsx),
        tags(&baseline.tsx),
        "Anchoring must not add or reparent DOM elements"
    );
    let records = overlay_anchor_metadata(&snapshot, &snapshot.roots, &output.tsx);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["controlNodeId"], "control");
    assert_eq!(records[0]["gap"].as_f64(), Some(8.0));
    assert_eq!(records[0]["capturedFallback"]["top"].as_f64(), Some(129.0));
    assert_eq!(records[0]["projected"], true);
    assert!(
        records[0]["cssIdentifier"]
            .as_str()
            .unwrap()
            .starts_with("\\2d \\2d ")
    );
    let theme_variable_spelling = output.tsx.replace("\\2d \\2d ", "--");
    assert!(
        overlay_anchor_metadata(&snapshot, &snapshot.roots, &theme_variable_spelling).is_empty()
    );
    assert_eq!(records[0]["runtimeVerified"], false);
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_PROPERTY_UNMAPPED"),
        "{:?}",
        output.diagnostics
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_OVERLAY_ANCHOR_RUNTIME_UNVERIFIED")
    );
    let fake = format!("// @devup-overlay-anchor {}", records[0]);
    assert!(overlay_anchor_metadata(&snapshot, &snapshot.roots, &fake).is_empty());
}

#[test]
fn ambiguous_controls_and_transformed_geometry_keep_original_absolute_placement() {
    for ambiguous in [true, false] {
        let mut snapshot = source();
        if ambiguous {
            let mut other = snapshot.nodes["control"].clone();
            other.id = "other-control".into();
            other.fields["absoluteBoundingBox"]["y"] = json!(99);
            snapshot.nodes.insert(other.id.clone(), other);
            snapshot.nodes.get_mut("root").unwrap().fields["childrenIds"] =
                json!(["control", "other-control", "overlay"]);
        } else {
            snapshot
                .nodes
                .get_mut("overlay")
                .unwrap()
                .fields
                .insert("rotation".into(), json!(15));
        }
        let output = generate_component(&snapshot, "root", &options()).unwrap();
        assert!(!output.tsx.contains("positionAnchor="), "{}", output.tsx);
        assert!(
            output
                .diagnostics
                .iter()
                .any(|d| d.code == "DEVUP_CODEGEN_OVERLAY_ANCHOR_UNRESOLVED")
        );
    }
}

#[test]
fn source_name_or_geometry_alone_cannot_invent_a_dropdown_control() {
    let mut snapshot = source();
    snapshot
        .nodes
        .get_mut("caret")
        .unwrap()
        .fields
        .insert("visible".into(), json!(false));
    let output = generate_component(&snapshot, "root", &options()).unwrap();
    assert!(!output.tsx.contains("positionAnchor="));
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_OVERLAY_ANCHOR_UNRESOLVED")
    );
}

#[test]
fn captured_contact_controls_keep_zero_gap_and_responsive_aliases() {
    // Read-only selected-node extraction, 2026-11-02: control, caret,
    // DropdownOption subtrees and their ancestors; unrelated siblings omitted.
    let snapshot: Snapshot = serde_json::from_str(include_str!(
        "fixtures/chondrogen_dropdown_anchor_nodes.json"
    ))
    .unwrap();
    for (root, control, overlay, top, width) in [
        ("44:5652", "I44:5664;44:5509", "45:2039", 414, 540),
        ("44:6155", "I44:6167;44:5509", "45:2050", 489, 320),
    ] {
        let output = generate_component(&snapshot, root, &options()).unwrap();
        let records = overlay_anchor_metadata(&snapshot, &[root.into()], &output.tsx);
        assert_eq!(records.len(), 1, "{} {:?}", output.tsx, output.diagnostics);
        assert_eq!(records[0]["controlNodeId"], control);
        assert_eq!(records[0]["overlayNodeId"], overlay);
        assert_eq!(records[0]["gap"].as_f64(), Some(0.0));
        assert_eq!(
            records[0]["capturedFallback"]["top"].as_f64(),
            Some(f64::from(top))
        );
        assert_eq!(
            records[0]["capturedFallback"]["width"].as_f64(),
            Some(f64::from(width))
        );
    }
    let merged = merge_confirmed_breakpoints(&snapshot, &options())
        .unwrap()
        .unwrap();
    let metadata = overlay_anchor_metadata(&snapshot, &snapshot.roots, &merged.tsx);
    assert_eq!(metadata.len(), 2, "{}", merged.tsx);
    assert!(
        metadata
            .iter()
            .all(|r| r["gap"].as_f64() == Some(0.0) && r["runtimeVerified"] == false)
    );
    assert!(merged.tsx.contains("anchorName="), "{}", merged.tsx);
    assert!(merged.tsx.contains("positionAnchor="), "{}", merged.tsx);
}

#[test]
fn shared_component_references_keep_numeric_offsets_and_explicit_recovery() {
    let snapshot = source();
    let output = generate_component(
        &snapshot,
        "root",
        &CodegenOptions {
            inline_instances: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!output.tsx.contains("positionAnchor="), "{}", output.tsx);
    assert!(output.diagnostics.iter().any(|d| {
        d.code == "DEVUP_CODEGEN_OVERLAY_ANCHOR_UNRESOLVED"
            && d.details
                .as_ref()
                .is_some_and(|v| v["classification"] == "native-control-not-expanded")
    }));
}
