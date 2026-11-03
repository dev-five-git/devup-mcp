use devup_mcp_devup_ui::codegen::{CodegenOptions, responsive::merge_confirmed_breakpoints};
use devup_mcp_figma::Snapshot;
use serde_json::json;

fn render(snapshot: &Snapshot) -> String {
    let screen = merge_confirmed_breakpoints(
        snapshot,
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    assert!(
        !screen
            .gaps
            .iter()
            .any(|g| ["p", "px", "py", "pl", "pr", "pt", "pb"]
                .iter()
                .any(|name| g["property"] == *name)),
        "{:?}",
        screen.gaps
    );
    screen.module("PaddingHeaderProbe")
}

fn synthetic(mobile: [i32; 4], desktop: [i32; 4]) -> Snapshot {
    let frame = |id: &str, width: i32, p: [i32; 4]| {
        json!({"id":id,"type":"FRAME","fields":{
            "name":id,"width":width,"height":64,"layoutMode":"HORIZONTAL","childrenIds":[],
            "paddingLeft":p[0],"paddingRight":p[1],"paddingTop":p[2],"paddingBottom":p[3],
            "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED"
        }})
    };
    serde_json::from_value(
        json!({"fileKey":"padding","roots":["desktop","mobile"],"nodes":{
        "desktop":frame("desktop",1440,desktop),"mobile":frame("mobile",360,mobile)
    },"diagnostics":[]}),
    )
    .unwrap()
}

fn compact(s: &str) -> String {
    s.split_whitespace().collect()
}

#[test]
fn mixed_horizontal_and_left_padding_become_independent_sides() {
    let output = compact(&render(&synthetic([12, 0, 0, 0], [16, 16, 0, 0])));
    assert!(
        output.contains("pl={[\"12px\",null,null,null,\"16px\"]}"),
        "{output}"
    );
    assert!(
        output.contains("pr={[\"initial\",null,null,null,\"16px\"]}"),
        "{output}"
    );
    assert!(!output.contains("px="), "{output}");
}

#[test]
fn uniform_and_axis_padding_preserve_all_four_physical_values() {
    let output = compact(&render(&synthetic([8, 8, 8, 8], [16, 16, 4, 4])));
    for (side, desktop) in [("px", 16), ("py", 4)] {
        assert!(
            output.contains(&format!(
                "{side}={{[\"8px\",null,null,null,\"{desktop}px\"]}}"
            )),
            "{output}"
        );
    }
    assert!(!output.contains("pl="));
    assert!(!output.contains("pt="));
    assert!(!output.contains("p="));
}

#[test]
fn matching_axis_aliases_are_left_unchanged() {
    let output = compact(&render(&synthetic([12, 12, 0, 0], [16, 16, 0, 0])));
    assert!(
        output.contains("px={[\"12px\",null,null,null,\"16px\"]}"),
        "{output}"
    );
    assert!(!output.contains("pl="));
    assert!(!output.contains("pr="));
}

#[test]
fn actual_header_source_keeps_mobile_twelve_and_desktop_sixteen() {
    let snapshot: Snapshot =
        serde_json::from_str(include_str!("fixtures/header-padding-aliases.json")).unwrap();
    assert_eq!(snapshot.nodes["51:6610"].fields["paddingLeft"], 12);
    assert_eq!(snapshot.nodes["45:2080"].fields["paddingLeft"], 16);
    assert_eq!(snapshot.nodes["45:2080"].fields["paddingRight"], 16);
    let output = render(&snapshot);
    if let Ok(path) = std::env::var("DEVUP_PADDING_PROBE_OUTPUT") {
        std::fs::write(path, &output).unwrap();
    }
    let compressed = compact(&output);
    assert!(
        compressed.contains("pl={[\"12px\",null,null,null,\"16px\"]}"),
        "{output}"
    );
    assert!(
        compressed.contains("pr={[\"initial\",null,null,null,\"16px\"]}"),
        "{output}"
    );
}
