use devup_mcp_devup_ui::codegen::{CodegenOptions, responsive::merge_breakpoints};
use devup_mcp_figma::Snapshot;
use serde_json::{Value, json};

fn compact(source: &str) -> String {
    source.split_whitespace().collect()
}

fn frame(id: &str, name: &str, width: u32, mode: &str, main: &str, cross: &str) -> Value {
    json!({"id":id,"type":"FRAME","fields":{
        "name":name,"width":width,"height":100,"visible":true,"childrenIds":[],
        "layoutMode":mode,"primaryAxisAlignItems":main,"counterAxisAlignItems":cross,
        "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED",
        "inferredAutoLayout":{"layoutMode":mode,"primaryAxisAlignItems":main,
            "counterAxisAlignItems":cross,"primaryAxisSizingMode":"FIXED","counterAxisSizingMode":"FIXED"}
    }})
}

fn merged(desktop: Value, mobile: Value) -> String {
    let snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"primitive-defaults","version":null,"roots":["d","m"],
        "nodes":{"d":desktop,"m":mobile},"diagnostics":[]
    }))
    .unwrap();
    let result = merge_breakpoints(
        &snapshot,
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    assert!(result.gaps.is_empty(), "{:?}", result.gaps);
    compact(&result.tsx)
}

#[test]
fn desktop_center_does_not_center_or_cross_align_mobile_flex() {
    let source = merged(
        frame("d", "desktop", 1440, "HORIZONTAL", "CENTER", "CENTER"),
        frame("m", "mobile", 360, "HORIZONTAL", "MIN", "MIN"),
    );
    assert!(source.contains("<Center"), "{source}");
    assert!(
        source.contains("justifyContent={[\"initial\",null,null,null,\"center\"]}"),
        "{source}"
    );
    assert!(
        source.contains("alignItems={[\"initial\",null,null,null,\"center\"]}"),
        "{source}"
    );
}

#[test]
fn mobile_center_resets_desktop_vstack_direction_without_losing_alignment() {
    let source = merged(
        frame("d", "desktop", 1440, "VERTICAL", "MIN", "MIN"),
        frame("m", "mobile", 360, "HORIZONTAL", "CENTER", "CENTER"),
    );
    assert!(source.contains("<VStack"), "{source}");
    assert!(
        source.contains("flexDir={[\"initial\",null,null,null,\"column\"]}"),
        "{source}"
    );
    assert!(
        source.contains("alignItems={[\"center\",null,null,null,\"initial\"]}"),
        "{source}"
    );
    assert!(
        source.contains("justifyContent={[\"center\",null,null,null,\"initial\"]}"),
        "{source}"
    );
}

#[test]
fn explicit_mobile_alignment_is_preserved_instead_of_reset() {
    let source = merged(
        frame("d", "desktop", 1440, "HORIZONTAL", "CENTER", "CENTER"),
        frame("m", "mobile", 360, "HORIZONTAL", "MIN", "MAX"),
    );
    assert!(
        source.contains("alignItems={[\"flex-end\",null,null,null,\"center\"]}"),
        "{source}"
    );
}

#[test]
fn matching_flex_primitives_gain_no_unnecessary_alignment_resets() {
    let source = merged(
        frame("d", "desktop", 1440, "HORIZONTAL", "MIN", "MIN"),
        frame("m", "mobile", 360, "HORIZONTAL", "MIN", "MIN"),
    );
    assert!(!source.contains("justifyContent="), "{source}");
    assert!(!source.contains("alignItems="), "{source}");
}

#[test]
fn captured_pagemenu_starts_at_source_left_edge_and_adds_no_scrolling() {
    // Exact bounded mobile PageMenu capture. Its missing explicit main alignment
    // is MIN in inferredAutoLayout; its first tab begins at the slot's left edge.
    let mut capture: Value =
        serde_json::from_str(include_str!("fixtures/pagemenu-primitive-defaults.json")).unwrap();
    let mobile: Snapshot = serde_json::from_value(capture.clone()).unwrap();
    let snapshot = &mobile;
    let slot = &snapshot.nodes["I82:13391;44:5412"].fields;
    let first = &snapshot.nodes["I82:13391;44:5412;82:13386"].fields;
    assert_eq!(slot["inferredAutoLayout"]["primaryAxisAlignItems"], "MIN");
    assert_eq!(slot["overflowDirection"], "NONE");
    assert_eq!(
        first["absoluteBoundingBox"]["x"],
        slot["absoluteBoundingBox"]["x"]
    );
    // Synthetic centered desktop counterpart isolates responsive primitive
    // selection while retaining the captured mobile subtree unchanged.
    let desktop: Value = serde_json::from_str(
        &capture["nodes"]
            .to_string()
            .replace("82:13391", "desktop-clone"),
    )
    .unwrap();
    capture["nodes"]
        .as_object_mut()
        .unwrap()
        .extend(desktop.as_object().unwrap().clone());
    capture["roots"] = json!(["desktop-clone", "82:13391"]);
    capture["nodes"]["desktop-clone"]["fields"]["width"] = json!(1440);
    capture["nodes"]["Idesktop-clone;44:5412"]["fields"]["primaryAxisAlignItems"] = json!("CENTER");
    capture["nodes"]["Idesktop-clone;44:5412"]["fields"]["inferredAutoLayout"]["primaryAxisAlignItems"] =
        json!("CENTER");
    let snapshot: Snapshot = serde_json::from_value(capture).unwrap();
    let result = merge_breakpoints(
        &snapshot,
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    let source = compact(&result.tsx);
    assert!(
        source.contains("justifyContent={[\"initial\",null,null,null,\"center\"]}"),
        "{source}"
    );
    assert!(!source.contains("overflowX="), "{source}");
    assert!(!source.contains("overflowY="), "{source}");
    assert!(
        !result
            .gaps
            .iter()
            .any(|gap| gap["property"] == "justifyContent"),
        "{:?}",
        result.gaps
    );
}

#[test]
fn responsive_css_identifier_escapes_remain_valid_js_and_exact_css_values() {
    use devup_mcp_devup_ui::{
        codegen::{overlay_anchor_metadata, responsive::merge_confirmed_breakpoints},
        provenance::attributes::generated_attributes,
        validation::validate_tsx,
    };
    let snapshot: Snapshot = serde_json::from_str(include_str!(
        "fixtures/chondrogen_dropdown_anchor_nodes.json"
    ))
    .unwrap();
    let screen = merge_confirmed_breakpoints(
        &snapshot,
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    validate_tsx(&screen.tsx)
        .expect("responsive CSS escapes must not become illegal JS octal escapes");
    let attributes = generated_attributes(&screen.tsx);
    let responsive = attributes
        .iter()
        .find(|a| a.name == "anchorName" && a.source.contains("={["))
        .expect("captured mobile/desktop identities require a responsive array");
    let array = responsive
        .source
        .strip_prefix("anchorName={")
        .unwrap()
        .strip_suffix('}')
        .unwrap();
    let values: Vec<Option<String>> = serde_json::from_str(array).unwrap();
    assert!(
        values
            .iter()
            .flatten()
            .all(|value| value.starts_with("\\2d \\2d "))
    );
    assert!(
        !screen
            .gaps
            .iter()
            .any(|gap| gap["property"] == "anchorName"),
        "{:?}",
        screen.gaps
    );
    let metadata = overlay_anchor_metadata(&snapshot, &snapshot.roots, &screen.tsx);
    assert_eq!(metadata.len(), 2);
    assert!(metadata.iter().all(|record| {
        values
            .iter()
            .flatten()
            .any(|value| Some(value.as_str()) == record["cssIdentifier"].as_str())
    }));
}
