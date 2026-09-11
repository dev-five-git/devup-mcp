use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;

#[test]
fn r8_absolute_fixed_height_survives_derived_padding() {
    let mut snapshot: Snapshot =
        serde_json::from_str(include_str!("../../../fixtures/r8/modal-snapshot.json")).unwrap();
    for sizing in ["FIXED", "FILL", "HUG"] {
        snapshot
            .nodes
            .get_mut("3997:46621")
            .unwrap()
            .fields
            .insert("layoutSizingVertical".into(), json!(sizing));
        let o = generate_component(
            &snapshot,
            "3997:46582",
            &CodegenOptions {
                inline_instances: true,
                ..Default::default()
            },
        )
        .unwrap();
        let diagnostic = o
            .diagnostics
            .iter()
            .find(|d| {
                d.code
                    == if sizing == "FIXED" {
                        "DEVUP_CODEGEN_ABSOLUTE_VERIFIED"
                    } else {
                        "DEVUP_CODEGEN_ABSOLUTE_FALLBACK"
                    }
                    && d.node_id.as_deref() == Some("3997:46621")
            })
            .unwrap();
        let details = diagnostic.details.as_ref().unwrap();
        let tag = details["appliedValue"]["generatedSource"][0]
            .as_str()
            .unwrap()
            .split('>')
            .next()
            .unwrap();
        assert!(
            tag.contains("left=\"50%\"")
                && tag.contains("top=\"0px\"")
                && tag.contains("translateX(-50%)")
        );
        assert_eq!(
            details["appliedValue"]["heightPreservation"]["state"],
            if sizing == "FIXED" {
                "preserved"
            } else {
                "not-fixed"
            }
        );
        assert_eq!(
            tag.contains("h=\"740px\""),
            sizing == "FIXED",
            "{sizing}: {tag}"
        );
        if sizing == "FIXED" {
            assert!(o.source_map.entries.iter().any(|e| {
                e.node_id.as_deref() == Some("3997:46621")
                    && e.property.as_deref() == Some("height")
                    && e.generated_property.as_deref() == Some("h=\"740px\"")
            }));
        }
    }
}

#[test]
fn r8_scroll_intent_overrides_clipping() {
    for (direction, expected) in [
        ("VERTICAL_SCROLLING", "overflowY=\"auto\""),
        ("HORIZONTAL_SCROLLING", "overflowX=\"auto\""),
        ("HORIZONTAL_AND_VERTICAL_SCROLLING", "overflow=\"auto\""),
        ("NONE", "overflow=\"hidden\""),
    ] {
        let s: Snapshot = serde_json::from_value(json!({"fileKey":"scroll", "diagnostics":[], "roots":["1:1"], "nodes":{"1:1":{"id":"1:1","type":"FRAME","fields":{"name":"Reader", "width":360,"height":720,"layoutSizingVertical":"FIXED","clipsContent":true,"overflowDirection":direction}}}})).unwrap();
        let o = generate_component(&s, "1:1", &CodegenOptions::default()).unwrap();
        assert!(o.tsx.contains(expected), "{direction}: {}", o.tsx);
        if direction != "NONE" {
            assert!(!o.tsx.contains("overflow=\"hidden\""));
        }
        assert!(
            o.source_map
                .entries
                .iter()
                .any(|e| e.property.as_deref() == Some("overflowDirection"))
        );
    }
}

#[test]
fn r8_semantic_map_has_properties_without_encoding_offsets() {
    let snapshot: Snapshot =
        serde_json::from_str(include_str!("../../../fixtures/r8/modal-snapshot.json")).unwrap();
    let o = generate_component(
        &snapshot,
        "3997:46582",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    let map = serde_json::to_value(&o.source_map).unwrap();
    for e in map["entries"].as_array().unwrap() {
        assert!(
            e.get("generatedRange").is_none(),
            "offset removed from public map: {e}"
        );
        assert!(
            e["generatedProperty"].is_string(),
            "semantic mapping required: {e}"
        );
    }
    assert!(o.tsx.contains("추가 체험"));
}

#[test]
fn r8_all_four_sizing_resolutions_have_semantic_mapping() {
    for (fixture, root, id, field, resolution, generated) in [
        (
            include_str!("../../../fixtures/r2/wquw-119-payload.json"),
            "3997:46333",
            "3997:46333",
            "layoutSizingVertical",
            "verified-layout-sizing",
            "h=\"740px\"",
        ),
        (
            include_str!("../../../fixtures/r2/wquw-119-payload.json"),
            "3997:46333",
            "3997:46334",
            "layoutGrow",
            "verified-layout-sizing",
            "flex=\"1\"",
        ),
        (
            include_str!("../../../fixtures/r2/wquw-119-payload.json"),
            "3997:46333",
            "3997:46347",
            "width",
            "accounted-for-implicit-flex-stretch",
            "implicit:align-self:stretch",
        ),
        (
            include_str!("../../../fixtures/r2/wquw-118-payload.json"),
            "3997:46277",
            "3997:46293",
            "width",
            "accounted-for-implicit-flex-grow",
            "flex=\"1\"",
        ),
        (
            include_str!("../../../fixtures/r2/wquw-119-payload.json"),
            "3997:46315",
            "3997:46317",
            "height",
            "restored-hug-after-mask-child-folding",
            "aspectRatio=\"320 / 48\"",
        ),
    ] {
        let payload: devup_mcp_figma::CollectedPayload = serde_json::from_str(fixture).unwrap();
        let o = generate_component(
            &payload.snapshot,
            root,
            &CodegenOptions {
                inline_instances: true,
                ..Default::default()
            }
            .with_payload_tokens(&payload),
        )
        .unwrap();
        let map = serde_json::to_value(&o.source_map).unwrap();
        let e = map["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["nodeId"] == id && e["property"] == field && e["resolution"] == resolution)
            .unwrap_or_else(|| panic!("missing {id} {field} {resolution}"));
        assert_eq!(e["generatedProperty"], generated, "{id} {field}");
        assert!(e.get("generatedRange").is_none());
    }
}
