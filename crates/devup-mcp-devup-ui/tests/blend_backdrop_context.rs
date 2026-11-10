//! Feedback 005: a blended negative-z decoration must composite against the
//! backdrop Figma blends it with. Mirrors the Prime SEO-GEO contact banner
//! (instance 32:1929): a gradient owner holds a transparent content wrapper
//! whose first child is a `multiply` decoration sent behind the content.
//! With `zIndex="0"` on the wrapper, the decoration composited against
//! transparency and drew a white blob where Figma has deep purple.

use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::{Value, json};

fn banner(mutate: impl FnOnce(&mut Value)) -> Snapshot {
    let gradient = json!([{
        "type": "GRADIENT_LINEAR",
        "gradientTransform": [[0, 1, 0], [-1, 0, 1]],
        "gradientStops": [
            {"position": 0, "color": {"r": 0.267, "g": 0.114, "b": 0.878, "a": 1}},
            {"position": 1, "color": {"r": 0.447, "g": 0.306, "b": 1, "a": 1}}
        ]
    }]);
    let mut value = json!({
        "fileKey": "blend-backdrop", "version": null, "diagnostics": [], "roots": ["banner"],
        "nodes": {
            "banner": {"id": "banner", "type": "FRAME", "fields": {
                "name": "ContactBanner", "width": 1280, "height": 320,
                "layoutMode": "VERTICAL", "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "HUG",
                "paddingTop": 64, "paddingRight": 0, "paddingBottom": 48, "paddingLeft": 0, "clipsContent": true,
                "fills": gradient, "childrenIds": ["content"]
            }},
            "content": {"id": "content", "type": "FRAME", "fields": {
                "name": "Content", "parentId": "banner", "width": 1280, "height": 208,
                "layoutMode": "VERTICAL", "layoutSizingHorizontal": "FILL", "layoutSizingVertical": "HUG",
                "itemSpacing": 36, "inferredAutoLayout": {"layoutMode": "VERTICAL", "itemSpacing": 36},
                "fills": [], "childrenIds": ["blob", "title"]
            }},
            "blob": {"id": "blob", "type": "RECTANGLE", "fields": {
                "name": "Glass blob", "parentId": "content", "x": -80, "y": -83, "width": 230, "height": 247,
                "layoutPositioning": "ABSOLUTE", "blendMode": "MULTIPLY",
                "constraints": {"horizontal": "MIN", "vertical": "MIN"},
                "fills": [{"type": "SOLID", "color": {"r": 0.8, "g": 0.8, "b": 0.85}}]
            }},
            "title": {"id": "title", "type": "TEXT", "fields": {
                "name": "Title", "parentId": "content", "characters": "Talk to us",
                "width": 1280, "height": 52, "fontSize": 40, "textAutoResize": "HEIGHT",
                "layoutSizingHorizontal": "FILL",
                "fills": [{"type": "SOLID", "color": {"r": 1, "g": 1, "b": 1}}]
            }}
        }
    });
    mutate(&mut value);
    serde_json::from_value(value).unwrap()
}

/// The opening tag of each element, in document order.
fn tags(tsx: &str) -> Vec<String> {
    tsx.split('<')
        .skip(1)
        .filter(|tag| !tag.starts_with('/'))
        .map(|tag| tag.split('>').next().unwrap_or_default().to_owned())
        .collect()
}

fn tag_with<'a>(tags: &'a [String], marker: &str) -> &'a str {
    tags.iter()
        .find(|tag| tag.contains(marker))
        .unwrap_or_else(|| panic!("no element with {marker} in {tags:#?}"))
}

fn generate(snapshot: &Snapshot) -> String {
    generate_component(snapshot, "banner", &CodegenOptions::default())
        .unwrap()
        .tsx
}

#[test]
fn multiply_decoration_stacks_on_the_gradient_owner_not_the_transparent_wrapper() {
    let tsx = generate(&banner(|_| {}));
    let tags = tags(&tsx);
    let owner = tag_with(&tags, "linear-gradient");
    let wrapper = tag_with(&tags, "gap=\"36px\"");
    let blob = tag_with(&tags, "mixBlendMode=\"multiply\"");
    assert!(owner.contains("pos=\"relative\""), "{tsx}");
    assert!(owner.contains("zIndex=\"0\""), "{tsx}");
    // The wrapper still anchors the decoration's coordinates, but opens no
    // stacking context between it and the gradient.
    assert!(wrapper.contains("pos=\"relative\""), "{tsx}");
    assert!(!wrapper.contains("zIndex"), "{tsx}");
    assert!(blob.contains("zIndex=\"-1\""), "{tsx}");
    assert!(blob.contains("pos=\"absolute\""), "{tsx}");
}

#[test]
fn a_wrapper_that_paints_its_own_backdrop_keeps_the_context() {
    let tsx = generate(&banner(|value| {
        value["nodes"]["content"]["fields"]["fills"] =
            json!([{"type": "SOLID", "color": {"r": 1, "g": 1, "b": 1}}]);
    }));
    let tags = tags(&tsx);
    let owner = tag_with(&tags, "linear-gradient");
    let wrapper = tag_with(&tags, "gap=\"36px\"");
    assert!(wrapper.contains("zIndex=\"0\""), "{tsx}");
    assert!(!owner.contains("zIndex"), "{tsx}");
}

#[test]
fn an_unblended_decoration_keeps_the_existing_containment() {
    let tsx = generate(&banner(|value| {
        value["nodes"]["blob"]["fields"]["blendMode"] = json!("NORMAL");
    }));
    let tags = tags(&tsx);
    let owner = tag_with(&tags, "linear-gradient");
    let wrapper = tag_with(&tags, "gap=\"36px\"");
    assert!(wrapper.contains("zIndex=\"0\""), "{tsx}");
    assert!(!owner.contains("zIndex"), "{tsx}");
}

#[test]
fn content_drawn_before_the_wrapper_keeps_the_context_on_the_wrapper() {
    // Lifting would paint the decoration under this earlier sibling, which
    // Figma draws beneath it.
    let tsx = generate(&banner(|value| {
        value["nodes"]["banner"]["fields"]["childrenIds"] = json!(["eyebrow", "content"]);
        value["nodes"]["eyebrow"] = json!({"id": "eyebrow", "type": "TEXT", "fields": {
            "name": "Eyebrow", "parentId": "banner", "characters": "Prime",
            "width": 1280, "height": 20, "fontSize": 14, "textAutoResize": "HEIGHT",
            "layoutSizingHorizontal": "FILL"
        }});
    }));
    let tags = tags(&tsx);
    let owner = tag_with(&tags, "linear-gradient");
    let wrapper = tag_with(&tags, "gap=\"36px\"");
    assert!(wrapper.contains("zIndex=\"0\""), "{tsx}");
    assert!(!owner.contains("zIndex"), "{tsx}");
}
