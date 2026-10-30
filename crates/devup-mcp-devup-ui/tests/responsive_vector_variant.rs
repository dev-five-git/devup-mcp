//! A component set whose mobile variant is drawn with vectors only and whose
//! desktop variant carries text stays one component reference at every width.

use devup_mcp_devup_ui::codegen::{CodegenOptions, responsive::merge_breakpoints};
use devup_mcp_figma::Snapshot;
use serde_json::json;

fn snapshot() -> Snapshot {
    let vector = |id: &str, parent: &str| {
        json!({"id":id,"type":"VECTOR","fields":{"name":"logo","parentId":parent,
            "width":120,"height":24,"visible":true,
            "fills":[{"type":"SOLID","visible":true,"color":{"r":0,"g":0,"b":0}}]}})
    };
    let text = |id: &str, parent: &str| {
        json!({"id":id,"type":"TEXT","fields":{"name":"menu","parentId":parent,
            "characters":"Company","fontSize":16,"width":80,"height":24,
            "textAutoResize":"WIDTH_AND_HEIGHT","visible":true}})
    };
    let frame = |id: &str, name: &str, width: u32, child: &str| {
        json!({"id":id,"type":"FRAME","fields":{"name":name,"width":width,"height":600,
            "layoutMode":"VERTICAL","inferredAutoLayout":{"layoutMode":"VERTICAL"},
            "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED",
            "childrenIds":[child]}})
    };
    let instance = |id: &str, parent: &str, main: &str, width: u32, child: &str, value: &str| {
        json!({"id":id,"type":"INSTANCE","fields":{"name":"Header","parentId":parent,
            "mainComponentId":main,"mainComponentSetId":"set","width":width,"height":64,
            "componentProperties":{"type":{"type":"VARIANT","value":value}},
            "childrenIds":[child]}})
    };
    let nodes = json!({
        "set": {"id":"set","type":"COMPONENT_SET","fields":{"name":"Header","childrenIds":["cm","cd"]}},
        "cm": {"id":"cm","type":"COMPONENT","fields":{"name":"type=mobile","parentId":"set",
            "width":360,"height":64,"variantProperties":{"type":"mobile"},"childrenIds":["cmv"]}},
        "cmv": vector("cmv", "cm"),
        "cd": {"id":"cd","type":"COMPONENT","fields":{"name":"type=desktop","parentId":"set",
            "width":1440,"height":80,"layoutMode":"HORIZONTAL","variantProperties":{"type":"desktop"},
            "childrenIds":["cdt"]}},
        "cdt": text("cdt", "cd"),
        "m": frame("m", "mobile", 360, "mi"),
        "mi": instance("mi", "m", "cm", 360, "miv", "mobile"),
        "miv": vector("miv", "mi"),
        "d": frame("d", "desktop", 1440, "di"),
        "di": instance("di", "d", "cd", 1440, "dit", "desktop"),
        "dit": text("dit", "di"),
    });
    serde_json::from_value(json!({
        "fileKey":"vector-variant","version":null,"roots":["m","d"],"nodes":nodes,"diagnostics":[]
    }))
    .unwrap()
}

#[test]
fn a_vector_only_variant_of_a_text_component_is_still_referenced() {
    let merged = merge_breakpoints(&snapshot(), &CodegenOptions::default())
        .unwrap()
        .unwrap();
    assert!(
        merged.components.contains("Header"),
        "{:?}\n{}",
        merged.components,
        merged.tsx
    );
    assert!(!merged.tsx.contains("{/* <Header"), "{}", merged.tsx);
}

#[test]
fn a_screen_export_without_the_set_still_references_the_component() {
    let mut snapshot = snapshot();
    for id in ["set", "cm", "cmv", "cd", "cdt"] {
        snapshot.nodes.remove(id);
    }
    let merged = merge_breakpoints(&snapshot, &CodegenOptions::default())
        .unwrap()
        .unwrap();
    assert!(
        merged.components.contains("Header"),
        "{:?}\n{}",
        merged.components,
        merged.tsx
    );
    assert!(!merged.tsx.contains("{/* <Header"), "{}", merged.tsx);
}
