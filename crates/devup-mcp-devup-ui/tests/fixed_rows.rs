use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;

#[test]
fn fixed_card_rows_wrap_without_losing_drawn_widths() {
    for (count, width) in [(6,180), (6,160), (3,350)] {
        let mut scene: Snapshot = serde_json::from_str(include_str!("fixtures/staggered-sequence.json")).unwrap();
        scene.nodes.retain(|id,_| id == "root");
        let ids = (0..count).map(|i| format!("card{i}")).collect::<Vec<_>>();
        let root = scene.nodes.get_mut("root").unwrap();
        root.fields.insert("childrenIds".into(), json!(ids));
        root.fields.insert("width".into(), json!(count*width+20*(count-1)));
        root.fields.insert("itemSpacing".into(), json!(20));
        root.fields.insert("inferredAutoLayout".into(), json!({"layoutMode":"HORIZONTAL", "itemSpacing":20}));
        for id in ids {
            scene.nodes.insert(id.clone(), serde_json::from_value(json!({"id":id,"type":"FRAME","fields":{"parentId":"root","width":width,"height":100,"layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED","maxWidth":null,"maxHeight":null}})).unwrap());
        }
        let out = generate_component(&scene,"root",&CodegenOptions::default()).unwrap();
        assert!(out.tsx.contains("flexWrap=\"wrap\""), "{}", out.tsx);
        assert!(out.tsx.contains(&format!("w=\"{width}px\"")), "{}", out.tsx);
        assert!(out.tsx.contains("maxW=\"100%\""), "{}", out.tsx);
        assert!(out.tsx.contains("flexShrink=\"0\""), "{}", out.tsx);
    }
}
