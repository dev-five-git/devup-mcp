use devup_mcp_devup_ui::codegen::{CodegenOptions, responsive::merge_breakpoints};
use devup_mcp_figma::Snapshot;
use serde_json::{Value, json};

fn merge(
    changes: impl Fn(usize, &mut Value),
) -> devup_mcp_devup_ui::codegen::responsive::MergedScreen {
    let mut nodes = serde_json::Map::new();
    for (index, width) in [1440, 360].into_iter().enumerate() {
        let frame = json!({"id":format!("root:{index}"),"type":"FRAME","fields":{
            "name":"screen","width":width,"height":200,"layoutMode":"VERTICAL",
            "childrenIds":[format!("section:{index}")]
        }});
        let mut section = json!({"id":format!("section:{index}"),"type":"FRAME","fields":{
            "name":"Footer","parentId":format!("root:{index}"),"width":width,"height":120,
            "layoutMode":"HORIZONTAL","layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED",
            "paddingTop":0,"paddingRight":0,"paddingBottom":0,"paddingLeft":0,
            "childrenIds":[format!("logo:{index}"),format!("links:{index}")]
        }});
        changes(index, &mut section);
        let layout = section["fields"].clone();
        section["fields"]["inferredAutoLayout"] = layout;
        let logo = json!({"id":format!("logo:{index}"),"type":"FRAME","fields":{
            "name":"Logo","parentId":format!("section:{index}"),"width":if index == 0 {240} else {173},
            "height":28,"layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED"
        }});
        let links = json!({"id":format!("links:{index}"),"type":"TEXT","fields":{
            "name":"Links","parentId":format!("section:{index}"),"characters":"Company",
            "fontSize":16,"width":100,"height":24,"textAutoResize":"WIDTH_AND_HEIGHT"
        }});
        for node in [frame, section, logo, links] {
            nodes.insert(node["id"].as_str().unwrap().to_owned(), node.clone());
        }
        for name in ["search", "menu", "locale"] {
            let id = format!("{name}:{index}");
            nodes.insert(
                id.clone(),
                json!({"id":id,"type":"TEXT","fields":{
                    "name":name,"parentId":format!("section:{index}"),"characters":name,
                    "fontSize":16,"width":36,"height":24,"textAutoResize":"WIDTH_AND_HEIGHT"
                }}),
            );
        }
    }
    let snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"structural","version":null,"roots":["root:0","root:1"],"nodes":nodes,"diagnostics":[]
    })).unwrap();
    merge_breakpoints(&snapshot, &CodegenOptions::default())
        .unwrap()
        .unwrap()
}

#[test]
fn component_identity_pairs_renamed_containers_before_shape_matching() {
    let merged = merge(|index, section| {
        section["fields"]["mainComponentId"] = json!("footer-master");
        section["fields"]["name"] = json!(if index == 0 {
            "wide footer"
        } else {
            "compact footer"
        });
        section["fields"]["layoutMode"] = json!(if index == 0 { "HORIZONTAL" } else { "VERTICAL" });
    });
    let sections = merged
        .elements
        .iter()
        .filter(|element| element.ids.iter().any(|(_, id)| id.starts_with("section:")))
        .collect::<Vec<_>>();
    assert_eq!(sections.len(), 1, "{}", merged.tsx);
    assert_eq!(sections[0].ids.len(), 2, "{}", merged.tsx);
    assert!(sections[0].hidden_at.is_empty());
    assert!(merged.diagnostics.is_empty(), "{:?}", merged.diagnostics);
}

#[test]
fn footer_direction_and_padding_merge_despite_different_container_shapes() {
    let merged = merge(|index, section| {
        section["fields"]["layoutMode"] = json!(if index == 0 { "HORIZONTAL" } else { "VERTICAL" });
        section["fields"]["paddingTop"] = json!(if index == 0 { 60 } else { 36 });
        section["fields"]["paddingBottom"] = json!(if index == 0 { 60 } else { 36 });
    });
    let compact = merged.tsx.split_whitespace().collect::<String>();
    assert!(
        compact.contains("flexDir={[\"column\",null,null,null,\"row\"]}"),
        "{}",
        merged.tsx
    );
    assert!(
        compact.contains("py={[\"36px\",null,null,null,\"60px\"]}"),
        "{}",
        merged.tsx
    );
    assert!(!compact.contains("\"none\""), "{}", merged.tsx);
    assert_eq!(
        merged
            .elements
            .iter()
            .filter(|element| element.ids.iter().any(|(_, id)| id.starts_with("logo:")))
            .count(),
        1
    );
}

#[test]
fn prop_names_do_not_split_the_same_child_between_widths() {
    let merged = merge(|index, section| {
        if index == 1 {
            section["fields"]["paddingLeft"] = json!(12);
            section["fields"]["paddingRight"] = json!(16);
        }
    });
    let compact = merged.tsx.split_whitespace().collect::<String>();
    assert!(!compact.contains("\"none\""), "{}", merged.tsx);
    assert!(compact.contains("pl={[\"12px\""), "{}", merged.tsx);
    assert!(compact.contains("pr={[\"16px\""), "{}", merged.tsx);
}

#[test]
fn header_shared_logo_survives_different_sibling_sets() {
    let merged = merge(|index, section| {
        section["fields"]["name"] = json!("Header");
        section["fields"]["childrenIds"] = if index == 1 {
            json!(["logo:1", "search:1", "menu:1"])
        } else {
            json!(["logo:0", "links:0", "locale:0"])
        };
    });
    let logos = merged
        .elements
        .iter()
        .filter(|element| element.ids.iter().any(|(_, id)| id.starts_with("logo:")))
        .collect::<Vec<_>>();
    assert_eq!(logos.len(), 1, "{}", merged.tsx);
    assert_eq!(logos[0].ids.len(), 2, "{}", merged.tsx);
    assert!(logos[0].hidden_at.is_empty(), "{}", merged.tsx);
    for (id, hidden_width) in [
        ("links:0", 360),
        ("locale:0", 360),
        ("search:1", 1440),
        ("menu:1", 1440),
    ] {
        assert!(
            merged
                .elements
                .iter()
                .any(|element| element.ids.iter().any(|(_, node)| node == id)
                    && element.hidden_at == [hidden_width]),
            "{id}: {}",
            merged.tsx
        );
    }
    assert!(
        merged
            .tsx
            .split_whitespace()
            .collect::<String>()
            .contains("w={[\"173px\",null,null,null,\"240px\"]}"),
        "{}",
        merged.tsx
    );
}

#[test]
fn ambiguous_counterparts_report_node_ids_instead_of_silently_hiding() {
    let merged = merge(|index, section| {
        if index == 0 {
            section["fields"]["childrenIds"] = json!(["logo:0", "logo:0", "links:0"]);
        }
    });
    let issue = merged
        .diagnostics
        .iter()
        .find(|issue| issue.code == "DEVUP_CODEGEN_RESPONSIVE_UNMATCHED")
        .expect("ambiguous pairing must be reported");
    let details = issue.details.as_ref().unwrap();
    assert!(
        details["nodesByWidth"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["nodeId"] == "logo:0")
    );
    assert!(
        details["nodesByWidth"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["nodeId"] == "logo:1")
    );
    assert!(details.get("reason").is_some());
    assert!(details.get("nextAction").is_some());
}
