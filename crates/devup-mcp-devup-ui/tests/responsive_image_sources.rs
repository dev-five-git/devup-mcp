use devup_mcp_devup_ui::codegen::{CodegenOptions, responsive::merge_breakpoints};
use devup_mcp_figma::Snapshot;
use serde_json::json;

fn screen(widths: [u32; 3], names: [&str; 3]) -> String {
    let mut nodes = serde_json::Map::new();
    let mut roots = Vec::new();
    for (index, (width, name)) in widths.into_iter().zip(names).enumerate() {
        let root = format!("frame:{index}");
        let image = format!("asset:{index}");
        let text = format!("text:{index}");
        roots.push(root.clone());
        nodes.insert(
            root.clone(),
            json!({"id":root,"type":"FRAME","fields":{
                "name":(["mobile","tablet","desktop"][index]),"width":width,"height":200,
                "layoutMode":"HORIZONTAL","layoutSizingHorizontal":"FIXED",
                "layoutSizingVertical":"FIXED","childrenIds":[image,text]
            }}),
        );
        nodes.insert(
            image.clone(),
            json!({"id":image,"type":"RECTANGLE","fields":{
                "name":name,"parentId":root,"width":24,"height":24,"isAsset":true,
                "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED",
                "fills":[{"type":"IMAGE","visible":true,"scaleMode":"FILL","imageHash":name}]
            }}),
        );
        nodes.insert(
            text.clone(),
            json!({"id":text,"type":"TEXT","fields":{
                "name":"label","parentId":root,"characters":"label","fontSize":14,
                "lineHeight":{"unit":"PIXELS","value":20},"width":40,"height":20
            }}),
        );
    }
    let snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"images","version":null,"roots":roots,"nodes":nodes,"diagnostics":[]
    }))
    .unwrap();
    merge_breakpoints(&snapshot, &CodegenOptions::default())
        .unwrap()
        .unwrap()
        .tsx
}

#[test]
fn different_image_sources_stay_scalar_through_responsive_merging() {
    for widths in [[320, 700, 1600], [450, 900, 1400]] {
        let tsx = screen(widths, ["first", "second", "third"]);
        assert!(
            !tsx.contains("src={["),
            "HTML src cannot receive a CSS array: {tsx}"
        );
        for name in ["first", "second", "third"] {
            assert!(
                tsx.contains(&format!("src=\"/images/{name}.png\"")),
                "{tsx}"
            );
        }
        assert_eq!(tsx.matches("<Image").count(), 3, "{tsx}");
        assert_eq!(tsx.matches("display={[").count(), 3, "{tsx}");
    }
}

#[test]
fn identical_sources_still_share_one_image() {
    let tsx = screen([330, 710, 1500], ["shared", "shared", "shared"]);
    assert_eq!(tsx.matches("<Image").count(), 1, "{tsx}");
    assert!(tsx.contains("src=\"/images/shared.png\""), "{tsx}");
    assert!(!tsx.contains("display={["), "{tsx}");
}

#[test]
fn a_source_that_returns_keeps_its_existing_visibility_slots() {
    let tsx = screen([330, 710, 1500], ["shared", "other", "shared"]);
    assert_eq!(tsx.matches("<Image").count(), 2, "{tsx}");
    let compact = tsx.split_whitespace().collect::<String>();
    assert!(
        compact.contains("display={[\"inline\",\"none\",null,null,\"inline\"]}"),
        "{tsx}"
    );
    assert!(!tsx.contains("src={["), "{tsx}");
}
