//! The merged module, read at each width's slot, against that width's own
//! TSX: what `mergedMappingVerified` claims.

use devup_mcp_devup_ui::codegen::{
    CodegenOptions,
    responsive::{MergedScreen, merge_breakpoints},
};
use devup_mcp_figma::Snapshot;
use serde_json::{Value, json};

/// A mobile and a desktop width of one card. `per_width` adjusts each
/// width's nodes before the snapshot is built.
fn merged(per_width: impl Fn(usize, &mut serde_json::Map<String, Value>)) -> MergedScreen {
    let mut nodes = serde_json::Map::new();
    let mut roots = Vec::new();
    for (index, (name, width)) in [("mobile", 390), ("desktop", 1440)].into_iter().enumerate() {
        let root = format!("frame:{index}");
        let card = format!("card:{index}");
        let label = format!("label:{index}");
        roots.push(root.clone());
        let mut width_nodes = serde_json::Map::new();
        // A capture carries `inferredAutoLayout` beside `layoutMode` for an
        // auto-layout frame, and both conversions read it.
        width_nodes.insert(
            root.clone(),
            json!({"id":root,"type":"FRAME","fields":{
            "name":name,"width":width,"height":400,"layoutMode":"VERTICAL",
            "inferredAutoLayout":{"layoutMode":"VERTICAL"},
            "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED",
            "paddingLeft":20,"paddingRight":20,"childrenIds":[card]}}),
        );
        width_nodes.insert(
            card.clone(),
            json!({"id":card,"type":"FRAME","fields":{
            "name":"card","parentId":root,"width":200,"height":100,"layoutMode":"HORIZONTAL",
            "inferredAutoLayout":{"layoutMode":"HORIZONTAL"},
            "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED",
            "fills":[{"type":"SOLID","visible":true,"color":{"r":1,"g":1,"b":1}}],
            "childrenIds":[label]}}),
        );
        width_nodes.insert(
            label.clone(),
            json!({"id":label,"type":"TEXT","fields":{
            "name":"label","parentId":card,"characters":"one two","fontSize":16,
            "lineHeight":{"unit":"PIXELS","value":24},"width":80,"height":24,
            "textAutoResize":"WIDTH_AND_HEIGHT"}}),
        );
        per_width(index, &mut width_nodes);
        nodes.extend(width_nodes);
    }
    let snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"contract","version":null,"roots":roots,"nodes":nodes,"diagnostics":[]
    }))
    .unwrap();
    merge_breakpoints(&snapshot, &CodegenOptions::default())
        .unwrap()
        .unwrap()
}

fn reasons(merged: &MergedScreen) -> Vec<&str> {
    merged
        .gaps
        .iter()
        .filter_map(|gap| gap["reason"].as_str())
        .collect()
}

#[test]
fn widths_that_agree_are_verified_attribute_by_attribute() {
    let merged = merged(|_, _| {});
    assert!(merged.verified, "{:?}\n{}", merged.gaps, merged.tsx);
    assert!(!merged.contract.is_empty());
    assert!(
        merged
            .contract
            .iter()
            .all(|entry| entry["verified"] == true),
        "{:?}",
        merged.contract
    );
}

#[test]
fn values_that_differ_by_width_are_verified_through_their_arrays() {
    let merged = merged(|index, nodes| {
        nodes[&format!("card:{index}")]["fields"]["height"] = json!([100, 160][index]);
    });
    assert!(merged.tsx.contains("h={["), "{}", merged.tsx);
    assert!(merged.verified, "{:?}\n{}", merged.gaps, merged.tsx);
}

#[test]
fn a_child_one_width_does_not_draw_is_hidden_there_and_still_verified() {
    let merged = merged(|index, nodes| {
        if index == 0 {
            return;
        }
        nodes["card:1"]["fields"]["childrenIds"] = json!(["label:1", "badge:1"]);
        nodes.insert(
            "badge:1".into(),
            json!({"id":"badge:1","type":"FRAME","fields":{
            "name":"badge","parentId":"card:1","width":24,"height":24,
            "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED"}}),
        );
    });
    assert!(merged.tsx.contains("display={["), "{}", merged.tsx);
    assert!(merged.verified, "{:?}\n{}", merged.gaps, merged.tsx);
}

#[test]
fn a_break_one_width_has_is_verified_by_reading_the_text_at_each_slot() {
    let merged = merged(|index, nodes| {
        nodes[&format!("label:{index}")]["fields"]["characters"] =
            json!(["one\ntwo", "onetwo"][index]);
    });
    assert!(merged.tsx.contains("<Box as=\"br\""), "{}", merged.tsx);
    assert!(merged.verified, "{:?}\n{}", merged.gaps, merged.tsx);
}

/// A fill only the mobile screen has is not cleared at desktop — devup-ui
/// keeps the narrower value — so the desktop width is not reproduced.
#[test]
fn a_value_carried_into_a_width_that_does_not_have_it_is_not_verified() {
    let merged = merged(|index, nodes| {
        if index == 0 {
            nodes["frame:0"]["fields"]["fills"] =
                json!([{"type":"SOLID","visible":true,"color":{"r":1,"g":0,"b":0}}]);
        }
    });
    assert!(!merged.verified, "{}", merged.tsx);
    assert!(
        reasons(&merged).contains(&"value-differs-at-width"),
        "{:?}",
        merged.gaps
    );
    let bg = merged
        .contract
        .iter()
        .find(|entry| {
            entry["generatedProperty"]
                .as_str()
                .is_some_and(|property| property.starts_with("bg="))
        })
        .unwrap();
    assert_eq!(bg["verified"], false);
    let gap = merged
        .gaps
        .iter()
        .find(|gap| gap["property"] == "bg")
        .unwrap();
    assert_eq!(gap["slot"], 4);
    assert_eq!(gap["widthNodeId"], "frame:1");
}

/// Two children the widths order oppositely keep one order in the module, so
/// at the other width the elements are not in that width's order.
#[test]
fn children_the_widths_order_differently_are_not_verified() {
    let merged = merged(|index, nodes| {
        let root = format!("frame:{index}");
        let card = format!("card:{index}");
        let side = format!("side:{index}");
        nodes[&root]["fields"]["childrenIds"] = if index == 0 {
            json!([card, side])
        } else {
            json!([side, card])
        };
        nodes.insert(
            side.clone(),
            json!({"id":side,"type":"FRAME","fields":{
            "name":"side","parentId":root,"width":50,"height":50,
            "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED",
            "fills":[{"type":"SOLID","visible":true,"color":{"r":0,"g":0,"b":1}}]}}),
        );
    });
    assert!(!merged.verified, "{}", merged.tsx);
    assert!(
        reasons(&merged).contains(&"order-differs-at-width"),
        "{:?}",
        merged.gaps
    );
}

/// A layer only the middle width draws is shown there. The plugin's single
/// `"initial"` after the last value left the narrower `none` in effect, so
/// the layer was hidden at every width.
#[test]
fn a_layer_only_the_middle_width_draws_is_shown_there() {
    let mut nodes = serde_json::Map::new();
    let mut roots = Vec::new();
    for (index, (name, width)) in [("mobile", 390), ("tablet", 992), ("desktop", 1440)]
        .into_iter()
        .enumerate()
    {
        let root = format!("frame:{index}");
        roots.push(root.clone());
        let mut children = vec![format!("label:{index}")];
        if index == 1 {
            children.push("glow:1".to_owned());
            nodes.insert(
                "glow:1".into(),
                json!({"id":"glow:1","type":"FRAME","fields":{
                "name":"glow","parentId":root,"width":40,"height":40,
                "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED",
                "fills":[{"type":"SOLID","visible":true,"color":{"r":0,"g":0,"b":1}}]}}),
            );
        }
        nodes.insert(
            root.clone(),
            json!({"id":root,"type":"FRAME","fields":{
            "name":name,"width":width,"height":400,"layoutMode":"VERTICAL",
            "inferredAutoLayout":{"layoutMode":"VERTICAL"},
            "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED",
            "childrenIds":children}}),
        );
        nodes.insert(
            format!("label:{index}"),
            json!({"id":format!("label:{index}"),"type":"TEXT",
            "fields":{"name":"label","parentId":root,"characters":"one","fontSize":16,
            "lineHeight":{"unit":"PIXELS","value":24},"width":40,"height":24,
            "textAutoResize":"WIDTH_AND_HEIGHT"}}),
        );
    }
    let snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"contract","version":null,"roots":roots,"nodes":nodes,"diagnostics":[]
    }))
    .unwrap();
    let merged = merge_breakpoints(&snapshot, &CodegenOptions::default())
        .unwrap()
        .unwrap();
    let compact = merged.tsx.split_whitespace().collect::<String>();
    assert!(
        compact.contains(r#"display={["none",null,"block",null,"none"]}"#),
        "{}",
        merged.tsx
    );
    assert!(merged.verified, "{:?}\n{}", merged.gaps, merged.tsx);
}

/// Siblings that share a name keep their places around the ones between
/// them: the opening and closing quote of a title stay either side of it.
#[test]
fn same_name_siblings_keep_their_places_around_the_others() {
    let merged = merged(|index, nodes| {
        let card = format!("card:{index}");
        let ids = [
            format!("open:{index}"),
            format!("label:{index}"),
            format!("close:{index}"),
        ];
        nodes[&card]["fields"]["childrenIds"] = json!(ids);
        let width = [17, 20][index];
        for id in [&ids[0], &ids[2]] {
            nodes.insert(
                id.clone(),
                json!({"id":id,"type":"FRAME","fields":{
                "name":"quote","parentId":card,"width":width,"height":14,
                "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED",
                "fills":[{"type":"SOLID","visible":true,"color":{"r":0,"g":0,"b":1}}]}}),
            );
        }
    });
    let quote = merged.tsx.find("h=\"14px\"").unwrap();
    let title = merged.tsx.find("one two").unwrap();
    let last = merged.tsx.rfind("h=\"14px\"").unwrap();
    assert!(quote < title && title < last, "{}", merged.tsx);
    assert!(merged.verified, "{:?}\n{}", merged.gaps, merged.tsx);
}

/// A frame that stacks at mobile and lines up at desktop is one element in
/// the module, so the stacking it did by being a `VStack` is written out.
#[test]
fn a_frame_laid_out_differently_by_width_keeps_each_direction() {
    let merged = merged(|index, nodes| {
        if index == 1 {
            nodes["frame:1"]["fields"]["layoutMode"] = json!("HORIZONTAL");
            nodes["frame:1"]["fields"]["inferredAutoLayout"] = json!({"layoutMode":"HORIZONTAL"});
        }
    });
    let compact = merged.tsx.split_whitespace().collect::<String>();
    assert!(
        compact.contains(r#"flexDir={["column",null,null,null,"initial"]}"#),
        "{}",
        merged.tsx
    );
    assert!(merged.verified, "{:?}\n{}", merged.gaps, merged.tsx);
}

/// The same component at the same variant at both widths, its label
/// overridden at desktop: each width's own `tsx` writes its own label, and
/// the module's reference draws whatever the component file draws.
#[test]
fn a_component_reference_is_not_verified_through_its_body() {
    let merged = merged(|index, nodes| {
        let card = format!("card:{index}");
        let button = format!("button:{index}");
        let text = format!("button-label:{index}");
        let characters = ["A", "B"][index];
        nodes[&card]["fields"]["childrenIds"] = json!([format!("label:{index}"), button]);
        nodes.insert(
            button.clone(),
            json!({"id":button,"type":"INSTANCE","fields":{
            "name":"Button","parentId":card,"width":80,"height":32,
            "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED",
            "componentProperties":{"Property 1":{"type":"VARIANT","value":"primary"}},
            "childrenIds":[text]}}),
        );
        nodes.insert(
            text.clone(),
            json!({"id":text,"type":"TEXT","fields":{
            "name":"label","parentId":button,"characters":characters,"fontSize":14,
            "lineHeight":{"unit":"PIXELS","value":20},"width":10,"height":20,
            "textAutoResize":"WIDTH_AND_HEIGHT"}}),
        );
        if index == 0 {
            nodes.insert(
                "main".into(),
                json!({"id":"main","type":"COMPONENT",
                "fields":{"name":"Button","width":80,"height":32}}),
            );
        }
    });
    assert!(merged.tsx.contains("<Button"), "{}", merged.tsx);
    assert!(!merged.verified, "{}", merged.tsx);
    assert!(
        reasons(&merged).contains(&"component-body-unverified"),
        "{:?}",
        merged.gaps
    );
}

/// A layer hidden at every width is drawn at none, so the module leaves it
/// out as each width's own `tsx` does, asset reference and all.
#[test]
fn a_layer_no_width_draws_is_left_out_and_the_rest_verified() {
    let merged = merged(|index, nodes| {
        let card = format!("card:{index}");
        let hidden = format!("hidden:{index}");
        nodes[&card]["fields"]["childrenIds"] = json!([format!("label:{index}"), hidden]);
        nodes.insert(
            hidden.clone(),
            json!({"id":hidden,"type":"RECTANGLE","fields":{
            "name":"photo","parentId":card,"visible":false,"width":24,"height":24,
            "isAsset":true,"layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED",
            "fills":[{"type":"IMAGE","visible":true,"scaleMode":"FILL","imageHash":"photo"}]}}),
        );
    });
    assert!(!merged.tsx.contains("photo"), "{}", merged.tsx);
    assert!(merged.verified, "{:?}\n{}", merged.gaps, merged.tsx);
}

/// Words that differ by more than their breaks keep the first width's, and
/// the other width's text is then not what the module writes there.
#[test]
fn text_that_reads_differently_at_a_width_is_not_verified() {
    let merged = merged(|index, nodes| {
        if index == 1 {
            nodes["label:1"]["fields"]["characters"] = json!("three four");
        }
    });
    assert!(!merged.verified, "{}", merged.tsx);
    assert!(
        reasons(&merged).contains(&"text-differs-at-width"),
        "{:?}",
        merged.gaps
    );
}

/// Every merged element names the node each width drew it from, so a
/// runtime audit finds it at every drawn width; one drawn at a single width
/// is expected hidden at the other.
#[test]
fn a_merged_page_carries_the_node_of_every_width() {
    let mut screen = merged(|index, nodes| {
        if index == 1 {
            nodes.insert(
                "promo:1".into(),
                json!({"id":"promo:1","type":"FRAME","fields":{
                "name":"promo","parentId":"frame:1","width":300,"height":60,
                "fills":[{"type":"SOLID","visible":true,"color":{"r":0,"g":0,"b":1}}]}}),
            );
            nodes["frame:1"]["fields"]["childrenIds"] = json!(["card:1", "promo:1"]);
        }
    });
    let written = devup_mcp_devup_ui::codegen::node_ids::annotate_merged(&mut screen);
    assert!(
        screen.tsx.contains(
            r#"data-figma-node-id="frame:1" data-figma-node-ids="1440:frame:1 390:frame:0""#
        ),
        "{}",
        screen.tsx
    );
    assert!(
        screen.tsx.contains(
            r#"data-figma-node-id="card:1" data-figma-node-ids="1440:card:1 390:card:0""#
        ),
        "{}",
        screen.tsx
    );
    assert!(
        screen.tsx.contains(r#"data-figma-node-ids="1440:promo:1""#),
        "{}",
        screen.tsx
    );
    let promo = written
        .iter()
        .find(|element| element["ids"][0]["nodeId"] == "promo:1")
        .unwrap();
    assert_eq!(promo["hiddenAt"], json!([390]));
    devup_mcp_devup_ui::validation::validate_tsx(&screen.module("Page")).unwrap();
}
