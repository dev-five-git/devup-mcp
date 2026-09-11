use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;

// Hand-computed boundary: (12-10, 23-20), size 80x40.
fn check(positioning: &str, parent_layout: &str, boundary: bool) {
    let s: Snapshot = serde_json::from_value(json!({
        "fileKey":"r15", "roots":["p"], "diagnostics":[], "nodes":{
            "p":{"id":"p","type":"FRAME","fields":{
                "width":200,"height":100,"layoutPositioning":"AUTO","layoutMode":parent_layout,"childrenIds":["c"],
                "fills":[{"type":"SOLID","visible":true,"color":{"r":1,"g":1,"b":1}}],"absoluteBoundingBox":{"x":0,"y":0,"width":200,"height":100}}},
            "c":{"id":"c","type":"VECTOR","fields":{
                "name":"Mask","parentId":"p","isAsset":true,"width":100,"height":50,
                "layoutPositioning":positioning,"constraints":{"horizontal":"MIN","vertical":"MIN"},
                "absoluteBoundingBox":{"x":10,"y":20,"width":100,"height":50},
                "absoluteRenderBounds":{"x":12,"y":23,"width":80,"height":40},
                "fills":[{"type":"SOLID","visible":true,"color":{"r":1,"g":0,"b":0}}]}}
        }
    })).unwrap();
    let o = generate_component(&s, "p", &CodegenOptions::default()).unwrap();
    for (prop, value) in [
        ("maskPos", if boundary { "2px 3px" } else { "center" }),
        ("maskSize", if boundary { "80px 40px" } else { "contain" }),
        ("maskRepeat", "no-repeat"),
    ] {
        let attr = format!("{prop}=\"{value}\"");
        assert!(o.tsx.contains(&attr), "{}", o.tsx);
        let d = o
            .diagnostics
            .iter()
            .find(|d| {
                d.node_id.as_deref() == Some("c")
                    && d.property.as_deref() == Some(prop)
                    && d.code == "DEVUP_CODEGEN_PROPERTY_EVIDENCE"
            })
            .unwrap();
        let e = d.details.as_ref().unwrap();
        assert_eq!(e["generatedProperty"], attr);
        assert_eq!(e["propertyMappingVerified"], true);
        let overridden = boundary && prop != "maskRepeat";
        let calculation = e["calculation"].as_str().unwrap();
        assert!(
            calculation.starts_with(if overridden {
                "In-flow export boundary:"
            } else {
                "SVG mask projection policy"
            }),
            "{prop}: {calculation}"
        );
        assert_eq!(
            e["derivationPath"],
            if overridden {
                "in-flow-export-boundary"
            } else {
                "svg-mask-policy"
            }
        );
        assert_eq!(
            e["evidenceLimit"],
            "No browser or SVG/CSS composition measurement."
        );
    }
}

#[test]
fn r15_absolute_mask_reports_policy_despite_export_offset() {
    check("ABSOLUTE", "HORIZONTAL", false);
}

#[test]
fn r15_boundary_override_reports_boundary_only_for_overwritten_props() {
    check("AUTO", "HORIZONTAL", true);
}

#[test]
fn r15_free_layout_mask_reports_policy_despite_export_offset() {
    check("AUTO", "NONE", false);
}

#[test]
fn r15_observed_absolute_mask_keeps_values_and_reports_policy() {
    let s: Snapshot =
        serde_json::from_str(include_str!("../../../fixtures/r14/asset-snapshot.json")).unwrap();
    let o = generate_component(
        &s,
        "3997:46667",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    for attr in [
        "maskPos=\"center\"",
        "maskSize=\"contain\"",
        "maskRepeat=\"no-repeat\"",
    ] {
        assert!(o.tsx.contains(attr));
        let e = o
            .diagnostics
            .iter()
            .filter(|d| d.node_id.as_deref() == Some("3997:46668"))
            .filter_map(|d| d.details.as_ref())
            .find(|e| e["generatedProperty"] == attr)
            .unwrap();
        assert!(
            e["calculation"]
                .as_str()
                .unwrap()
                .starts_with("SVG mask projection policy"),
            "{e}"
        );
        assert_eq!(e["derivationPath"], "svg-mask-policy");
        assert_eq!(e["propertyMappingVerified"], true);
    }
}
