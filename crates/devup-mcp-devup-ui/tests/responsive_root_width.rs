//! A screen's widths are its frame widths, not a layout: the merged root
//! takes its container's width instead of `w={['360px', ..., '1440px']}`.

use devup_mcp_devup_ui::codegen::{CodegenOptions, responsive::merge_breakpoints};
use devup_mcp_figma::Snapshot;
use serde_json::json;

fn snapshot() -> Snapshot {
    let frame = |id: &str, name: &str, width: u32, child: &str| {
        json!({"id":id,"type":"FRAME","fields":{"name":name,"width":width,"height":600,
            "layoutMode":"VERTICAL","inferredAutoLayout":{"layoutMode":"VERTICAL"},
            "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED",
            "childrenIds":[child]}})
    };
    let card = |id: &str, parent: &str, width: u32| {
        json!({"id":id,"type":"FRAME","fields":{"name":"card","parentId":parent,
            "width":width,"height":120,"layoutSizingHorizontal":"FIXED",
            "layoutSizingVertical":"FIXED","visible":true}})
    };
    serde_json::from_value(json!({
        "fileKey":"root-width","version":null,"roots":["m","d"],"diagnostics":[],
        "nodes":{
            "m": frame("m", "mobile", 360, "mc"),
            "mc": card("mc", "m", 320),
            "d": frame("d", "desktop", 1440, "dc"),
            "dc": card("dc", "d", 1200),
        }
    }))
    .unwrap()
}

#[test]
fn the_merged_root_does_not_carry_its_frame_widths() {
    let merged = merge_breakpoints(&snapshot(), &CodegenOptions::default())
        .unwrap()
        .unwrap();
    let root = merged.tsx.split('>').next().unwrap();
    assert!(!root.contains("w="), "{}", merged.tsx);
    assert!(!merged.tsx.contains("\"1440px\""), "{}", merged.tsx);
    assert!(merged.tsx.contains("\"1200px\""), "{}", merged.tsx);
    assert!(
        !merged
            .gaps
            .iter()
            .any(|gap| gap["reason"] == "attribute-only-at-width" && gap["property"] == "w"),
        "{:?}",
        merged.gaps
    );
}
