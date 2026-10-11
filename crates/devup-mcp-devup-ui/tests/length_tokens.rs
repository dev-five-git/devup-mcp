//! A length bound to a variable is written as the variable's token where the
//! token reproduces it at the width the screen was drawn for.
//!
//! The devup Test file pads its Mobile (360) and Desktop (1440) frames with
//! `containerX`, a variable whose collection's modes are Mobile 20 and Tablet
//! 40, and rounds and shadows its card with variables of the same collection.
//! The theme writes those as responsive tokens, `["20px", null, "40px"]`, so
//! `$containerX` is 20px under 768px and 40px from 768px: the drawn value of
//! each frame at the width it was drawn at. Written as the drawn pixels, each
//! frame held its own width's value at every width.

use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::{Snapshot, SnapshotChunk, UpstreamResult, merge_chunks};
use serde_json::{Value, json};

fn variables() -> UpstreamResult {
    UpstreamResult {
        raw: json!({
            "collections": [
                {"id": "semantic", "name": "Semantic", "defaultModeId": "m",
                 "modes": [{"modeId": "m", "name": "Mobile"}, {"modeId": "t", "name": "Tablet"}]},
                {"id": "color", "name": "Color", "defaultModeId": "light",
                 "modes": [{"modeId": "light", "name": "Light"}, {"modeId": "dark", "name": "Dark"}]}
            ],
            "variables": [
                {"id": "v:container", "name": "containerX", "resolvedType": "FLOAT",
                 "variableCollectionId": "semantic", "valuesByMode": {"m": 20, "t": 40}},
                {"id": "v:radius", "name": "borderRadiusMd", "resolvedType": "FLOAT",
                 "variableCollectionId": "semantic", "valuesByMode": {"m": 16, "t": 30}},
                {"id": "v:shadowY", "name": "shadowMdY", "resolvedType": "FLOAT",
                 "variableCollectionId": "semantic", "valuesByMode": {"m": 12, "t": 20}},
                {"id": "v:text", "name": "textTest", "resolvedType": "FLOAT",
                 "variableCollectionId": "semantic", "valuesByMode": {"m": 16, "t": 24}},
                {"id": "v:gap", "name": "gap", "resolvedType": "FLOAT",
                 "variableCollectionId": "color", "valuesByMode": {"light": 8, "dark": 12}},
                {"id": "v:bg", "name": "gray100", "resolvedType": "COLOR",
                 "variableCollectionId": "color",
                 "valuesByMode": {"light": {"r": 1, "g": 1, "b": 1, "a": 1}, "dark": {"r": 0, "g": 0, "b": 0, "a": 1}}}
            ],
            "styles": [],
            "usedVariableIds": ["v:container", "v:radius", "v:shadowY", "v:text", "v:gap", "v:bg"],
            "usedRemoteVariables": [],
            "localComplete": true,
            "usedRemoteComplete": true
        }),
    }
}

fn bound(id: &str) -> Value {
    json!({"type": "VARIABLE_ALIAS", "id": id})
}

/// A screen `width` wide, padded `padding`, holding a card rounded `radius`
/// with a shadow `shadow_y` down and a text `size` tall.
fn screen(width: f64, padding: f64, radius: f64, shadow_y: f64, size: f64) -> Snapshot {
    let node = |id: &str, kind: &str, fields: Value| json!({"id": id, "type": kind, "extra": {}, "fieldErrors": {}, "fields": fields});
    let card_bindings = json!({
        "paddingLeft": bound("v:container"), "paddingRight": bound("v:container"),
        "paddingTop": bound("v:container"), "paddingBottom": bound("v:container"),
        "topLeftRadius": bound("v:radius"), "topRightRadius": bound("v:radius"),
        "bottomRightRadius": bound("v:radius"), "bottomLeftRadius": bound("v:radius")
    });
    let shadow = json!({
        "type": "DROP_SHADOW", "visible": true, "offset": {"x": 0, "y": shadow_y},
        "radius": 20, "spread": 0, "color": {"r": 0, "g": 0, "b": 0, "a": 0.15},
        "blendMode": "NORMAL", "boundVariables": {"offsetY": bound("v:shadowY")}
    });
    let fill = json!({
        "type": "SOLID", "visible": true, "color": {"r": 1, "g": 1, "b": 1, "a": 1},
        "boundVariables": {"color": bound("v:bg")}
    });
    let run = json!({
        "characters": "Title", "fontSize": size,
        "fontName": {"family": "Inter", "style": "Regular"}, "fontWeight": 400,
        "lineHeight": {"unit": "PERCENT", "value": 150},
        "fills": [{"type": "SOLID", "color": {"r": 0, "g": 0, "b": 0, "a": 1}}],
        "boundVariables": {"fontSize": bound("v:text")}
    });
    let nodes = vec![
        node(
            "s",
            "FRAME",
            json!({
                "name": "Screen", "childrenIds": ["c"], "parentType": "SECTION",
                "layoutMode": "VERTICAL", "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "HUG",
                "width": width, "height": 200,
                "absoluteBoundingBox": {"x": 0, "y": 0, "width": width, "height": 200},
                "paddingLeft": padding, "paddingRight": padding, "paddingTop": 0, "paddingBottom": 0,
                "boundVariables": {"paddingLeft": bound("v:container"), "paddingRight": bound("v:container")}
            }),
        ),
        node(
            "c",
            "FRAME",
            json!({
                "name": "Card", "parentId": "s", "childrenIds": ["t", "u"],
                "layoutMode": "VERTICAL", "layoutSizingHorizontal": "FILL", "layoutSizingVertical": "HUG",
                "width": width - 2.0 * padding, "height": 100,
                "absoluteBoundingBox": {"x": padding, "y": 0, "width": width - 2.0 * padding, "height": 100},
                "paddingLeft": padding, "paddingRight": padding, "paddingTop": padding, "paddingBottom": padding,
                "cornerRadius": radius, "topLeftRadius": radius, "topRightRadius": radius,
                "bottomRightRadius": radius, "bottomLeftRadius": radius,
                "fills": [fill], "effects": [shadow], "boundVariables": card_bindings
            }),
        ),
        node(
            "t",
            "TEXT",
            json!({
                "name": "Title", "parentId": "c", "childrenIds": [], "characters": "Title",
                "textAutoResize": "WIDTH_AND_HEIGHT", "textTruncation": "DISABLED",
                "fontSize": size, "boundVariables": {"fontSize": [bound("v:text")]},
                "styledTextSegments": [run]
            }),
        ),
        node(
            "u",
            "RECTANGLE",
            json!({
                "name": "Bar", "parentId": "c", "childrenIds": [],
                "layoutSizingHorizontal": "FILL", "layoutSizingVertical": "FIXED",
                "width": width - 4.0 * padding, "height": 4,
                "fills": [{"type": "SOLID", "visible": true, "color": {"r": 0, "g": 0, "b": 0, "a": 1}}]
            }),
        ),
    ];
    let chunk: SnapshotChunk = serde_json::from_value(json!({
        "fileKey": "file", "version": "1", "rootIds": ["s"], "diagnostics": [], "nodes": nodes
    }))
    .expect("synthetic screen");
    merge_chunks(vec![chunk]).expect("snapshot")
}

/// A row `width` wide of two words `gap` apart, the gap bound to `$gap`.
fn row(width: f64, gap: f64) -> Snapshot {
    let word = |id: &str, x: f64| {
        let run = json!({
            "characters": id, "fontSize": 14,
            "fontName": {"family": "Inter", "style": "Regular"}, "fontWeight": 400,
            "lineHeight": {"unit": "PIXELS", "value": 20},
            "fills": [{"type": "SOLID", "color": {"r": 0, "g": 0, "b": 0, "a": 1}}]
        });
        json!({"id": id, "type": "TEXT", "extra": {}, "fieldErrors": {}, "fields": {
            "name": id, "parentId": "s", "childrenIds": [], "characters": id,
            "textAutoResize": "WIDTH_AND_HEIGHT", "textTruncation": "DISABLED",
            "width": 10, "height": 20, "fontSize": 14,
            "absoluteBoundingBox": {"x": x, "y": 0, "width": 10, "height": 20},
            "styledTextSegments": [run]
        }})
    };
    let frame = json!({"id": "s", "type": "FRAME", "extra": {}, "fieldErrors": {}, "fields": {
        "name": "Row", "childrenIds": ["a", "b"], "parentType": "SECTION",
        "layoutMode": "HORIZONTAL", "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "HUG",
        "primaryAxisAlignItems": "MIN", "counterAxisAlignItems": "MIN",
        "width": width, "height": 20, "itemSpacing": gap,
        "absoluteBoundingBox": {"x": 0, "y": 0, "width": width, "height": 20},
        "inferredAutoLayout": {"layoutMode": "HORIZONTAL", "itemSpacing": gap,
            "primaryAxisAlignItems": "MIN", "counterAxisAlignItems": "MIN"},
        "boundVariables": {"itemSpacing": bound("v:gap")}
    }});
    let chunk: SnapshotChunk = serde_json::from_value(json!({
        "fileKey": "file", "version": "1", "rootIds": ["s"], "diagnostics": [],
        "nodes": [frame, word("a", 0.0), word("b", 10.0 + gap)]
    }))
    .expect("synthetic row");
    merge_chunks(vec![chunk]).expect("snapshot")
}

fn tsx(snapshot: &Snapshot) -> (String, Vec<String>) {
    let options = CodegenOptions {
        include_diagnostics: true,
        ..CodegenOptions::default()
    }
    .with_resource_results(Some(&variables()), None);
    let output = generate_component(snapshot, "s", &options).expect("codegen");
    let unmapped = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "DEVUP_CODEGEN_PROPERTY_UNMAPPED")
        .map(|diagnostic| format!("{:?}", diagnostic.details))
        .collect();
    (output.tsx, unmapped)
}

#[test]
fn a_bound_length_is_its_token_where_the_token_comes_to_the_drawn_value() {
    for (width, padding, radius, shadow_y, size) in [
        (360.0, 20.0, 16.0, 12.0, 16.0),
        (1440.0, 40.0, 30.0, 20.0, 24.0),
    ] {
        let (tsx, unmapped) = tsx(&screen(width, padding, radius, shadow_y, size));
        assert!(tsx.contains("px=\"$containerX\""), "{width}:\n{tsx}");
        assert!(tsx.contains("p=\"$containerX\""), "{width}:\n{tsx}");
        assert!(
            tsx.contains("borderRadius=\"$borderRadiusMd\""),
            "{width}:\n{tsx}"
        );
        assert!(
            tsx.contains("boxShadow=\"0 $shadowMdY 20px 0 "),
            "{width}:\n{tsx}"
        );
        assert!(tsx.contains("fontSize=\"$textTest\""), "{width}:\n{tsx}");
        assert!(unmapped.is_empty(), "{width}: {unmapped:?}");
    }
    // A theme-keyed token is checked in the theme a page shows first: Light's
    // 8 is `$gap`, Dark's 12 is not what a page shows without `data-theme`.
    let (light, unmapped) = tsx(&row(360.0, 8.0));
    assert!(light.contains("gap=\"$gap\""), "{light}");
    assert!(unmapped.is_empty(), "{unmapped:?}");
    let (dark, _) = tsx(&row(360.0, 12.0));
    assert!(dark.contains("gap=\"12px\""), "{dark}");
}

#[test]
fn a_bound_length_the_token_does_not_reproduce_stays_drawn() {
    // Mobile values on a screen 800 wide: the token is 40px there.
    let (tsx, _) = tsx(&screen(800.0, 20.0, 16.0, 12.0, 16.0));
    assert!(tsx.contains("px=\"20px\""), "{tsx}");
    assert!(tsx.contains("borderRadius=\"16px\""), "{tsx}");
    assert!(tsx.contains("boxShadow=\"0 12px 20px 0 "), "{tsx}");
    assert!(tsx.contains("fontSize=\"16px\""), "{tsx}");
    assert!(!tsx.contains("$containerX"), "{tsx}");
}
