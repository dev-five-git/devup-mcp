use devup_mcp_devup_ui::codegen::{CodegenOptions, responsive::merge_confirmed_breakpoints};
use devup_mcp_figma::Snapshot;
use serde_json::json;
fn fixture() -> Snapshot {
    serde_json::from_str(include_str!("fixtures/oa-media-size-aliases.json")).unwrap()
}
fn render(snapshot: &Snapshot) -> String {
    let merged = merge_confirmed_breakpoints(snapshot, &CodegenOptions::default())
        .unwrap()
        .unwrap();
    // This minimal captured media fixture has independent synthetic-paint
    // provenance gaps. Size equivalence must pass without waiving those gaps.
    assert!(
        !merged.gaps.iter().any(|g| ["boxSize", "w", "h"]
            .iter()
            .any(|name| g["property"] == *name)),
        "{:?}",
        merged.gaps
    );
    let axes = merged
        .contract
        .iter()
        .filter(|c| {
            c["elementIndex"] == 1
                && c["generatedProperty"]
                    .as_str()
                    .is_some_and(|s| ["w=", "h=", "boxSize="].iter().any(|p| s.starts_with(p)))
        })
        .collect::<Vec<_>>();
    assert!(!axes.is_empty(), "Size contract evidence must be present");
    assert!(axes.iter().all(|c| c["verified"] == true), "{axes:?}");
    merged.module("SizeAliasProbe")
}
fn compact(s: &str) -> String {
    s.split_whitespace().collect()
}
#[test]
fn actual_oa_media_keeps_desktop_square_and_mobile_fixed_height() {
    let source = fixture();
    assert_eq!(source.nodes["73:9310"].fields["width"], 560);
    assert_eq!(source.nodes["73:9310"].fields["height"], 560);
    let text = compact(&render(&source));
    assert!(!text.contains("boxSize="), "{text}");
    assert!(
        text.contains("h={[\"240px\",null,null,null,\"560px\"]}"),
        "{text}"
    );
    assert!(
        text.contains("w={[\"initial\",null,null,null,\"560px\"]}"),
        "{text}"
    );
}
#[test]
fn reverse_square_to_rectangular_transition_keeps_both_axes() {
    let mut source = fixture();
    for (id, w, h) in [("73:9401", 240, 240), ("73:9310", 560, 420)] {
        let node = source.nodes.get_mut(id).unwrap();
        node.fields.insert("width".into(), json!(w));
        node.fields.insert("height".into(), json!(h));
        node.fields
            .insert("layoutSizingHorizontal".into(), json!("FIXED"));
        node.fields.insert(
            "absoluteBoundingBox".into(),
            json!({"x":0,"y":0,"width":w,"height":h}),
        );
        node.fields.insert(
            "absoluteRenderBounds".into(),
            json!({"x":0,"y":0,"width":w,"height":h}),
        );
    }
    let text = compact(&render(&source));
    assert!(
        text.contains("h={[\"240px\",null,null,null,\"420px\"]}"),
        "{text}"
    );
    assert!(
        text.contains("w={[\"240px\",null,null,null,\"560px\"]}"),
        "{text}"
    );
    assert!(!text.contains("boxSize="));
}
#[test]
fn identical_square_alias_family_is_not_expanded() {
    let mut source = fixture();
    let mobile = source.nodes.get_mut("73:9401").unwrap();
    mobile.fields.insert("width".into(), json!(240));
    mobile
        .fields
        .insert("layoutSizingHorizontal".into(), json!("FIXED"));
    let text = compact(&render(&source));
    assert!(
        text.contains("boxSize={[\"240px\",null,null,null,\"560px\"]}"),
        "{text}"
    );
}
#[test]
fn independent_rectangular_axes_remain_independent() {
    let mut source = fixture();
    source
        .nodes
        .get_mut("73:9310")
        .unwrap()
        .fields
        .insert("height".into(), json!(420));
    let text = compact(&render(&source));
    assert!(!text.contains("boxSize="));
    assert!(
        text.contains("h={[\"240px\",null,null,null,\"420px\"]}"),
        "{text}"
    );
}

#[test]
fn physical_size_equivalence_does_not_hide_other_contract_failures() {
    let mut source = fixture();
    for id in ["73:9310", "73:9401"] {
        let fields = &mut source.nodes.get_mut(id).unwrap().fields;
        fields.remove("fills");
        fields.remove("isAsset");
    }
    let merged = merge_confirmed_breakpoints(&source, &CodegenOptions::default())
        .unwrap()
        .unwrap();
    assert!(merged.verified, "{:?}", merged.gaps);
    assert!(!merged.contract.is_empty());
    assert!(merged.contract.iter().all(|item| item["verified"] == true));
}
