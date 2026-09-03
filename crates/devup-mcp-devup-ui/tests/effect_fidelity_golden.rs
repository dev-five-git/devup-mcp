//! `DEVUP_CODEGEN_EFFECT_FALLBACK` must describe what actually happened.
//!
//! The diagnostic used to fire whenever a node merely *had* an `effects`
//! array, without asking whether those effects converted. Because a drop
//! shadow is ubiquitous, that made `projection: lossy` -- and therefore
//! `status: partial` -- unavoidable for essentially every real design, which
//! in turn made `strict: true` unusable.
//!
//! The first test is the real `3997:47759` node from `A : STORY-SUBSEL`
//! (`85CgSws3o5XsLv7aAwWJyS`): a `BACKGROUND_BLUR` plus a `DROP_SHADOW`, both
//! of which `push_effects` converts exactly, to
//! `backdropFilter="blur(8px)"` and `boxShadow="0 4px 12px 0 #0000001A"`.
//!
//! The remaining tests pin the effects that genuinely cannot be expressed, so
//! tightening the guard cannot silently under-report real infidelity.

use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_node};
use devup_mcp_figma::{RawNode, SnapshotChunk, merge_chunks};
use serde_json::{Value, json};

fn frame_with_effects(effects: Value) -> Value {
    json!({
        "id": "node:1",
        "type": "FRAME",
        "fields": {
            "constraints": { "horizontal": "MIN", "vertical": "MIN" },
            "effects": effects,
            "height": 146,
            "layoutMode": "VERTICAL",
            "layoutPositioning": "AUTO",
            "layoutSizingHorizontal": "FIXED",
            "layoutSizingVertical": "FIXED",
            "maxHeight": null,
            "maxWidth": null,
            "name": "Frame 1321315031",
            "visible": true,
            "width": 240,
            "x": 0,
            "y": 0
        }
    })
}

fn text_with_effects(effects: Value) -> Value {
    json!({
        "id": "node:1",
        "type": "TEXT",
        "fields": {
            "characters": "shadowed",
            "constraints": { "horizontal": "MIN", "vertical": "MIN" },
            "effects": effects,
            "fontName": { "family": "Pretendard", "style": "Regular" },
            "fontSize": 15,
            "height": 24,
            "layoutPositioning": "AUTO",
            "layoutSizingHorizontal": "HUG",
            "layoutSizingVertical": "HUG",
            "maxHeight": null,
            "maxWidth": null,
            "name": "shadowed",
            "textAutoResize": "WIDTH_AND_HEIGHT",
            "visible": true,
            "width": 80,
            "x": 0,
            "y": 0
        }
    })
}

fn drop_shadow(extra: Value) -> Value {
    let mut shadow = json!({
        "blendMode": "NORMAL",
        "boundVariables": {},
        "color": { "a": 0.100_000_001_490_116_12, "b": 0, "g": 0, "r": 0 },
        "offset": { "x": 0, "y": 4 },
        "radius": 12,
        "showShadowBehindNode": false,
        "spread": 0,
        "type": "DROP_SHADOW",
        "visible": true
    });
    let object = shadow.as_object_mut().expect("shadow is an object");
    for (key, value) in extra.as_object().expect("extra is an object") {
        object.insert(key.clone(), value.clone());
    }
    shadow
}

const BACKGROUND_BLUR: fn() -> Value = || {
    json!({
        "blurType": "NORMAL",
        "boundVariables": {},
        "radius": 8,
        "type": "BACKGROUND_BLUR",
        "visible": true
    })
};

struct Rendered {
    tsx: String,
    reported_lossy: bool,
}

fn render(node: Value) -> Rendered {
    let node = serde_json::from_value::<RawNode>(node).expect("node deserializes");
    let snapshot = merge_chunks(vec![SnapshotChunk {
        file_key: "85CgSws3o5XsLv7aAwWJyS".to_owned(),
        version: None,
        root_ids: vec!["node:1".to_owned()],
        nodes: vec![node],
        diagnostics: Vec::new(),
    }])
    .expect("snapshot merges");
    let output =
        generate_node(&snapshot, "node:1", &CodegenOptions::default()).expect("codegen succeeds");
    let reported_lossy = output
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "DEVUP_CODEGEN_EFFECT_FALLBACK");
    Rendered {
        tsx: output.tsx,
        reported_lossy,
    }
}

#[test]
fn effects_that_convert_exactly_are_not_reported_lossy() {
    let rendered = render(frame_with_effects(json!([
        BACKGROUND_BLUR(),
        drop_shadow(json!({}))
    ])));

    // Both effects really did land in the output, so the claim below is about
    // a converted node rather than an empty one.
    assert!(
        rendered.tsx.contains(r#"backdropFilter="blur(8px)""#),
        "BACKGROUND_BLUR should convert; got:\n{}",
        rendered.tsx
    );
    assert!(
        rendered
            .tsx
            .contains(r#"boxShadow="0 4px 12px 0 #0000001A""#),
        "DROP_SHADOW should convert; got:\n{}",
        rendered.tsx
    );
    assert!(
        !rendered.reported_lossy,
        "both effects converted exactly, so EFFECT_FALLBACK must not fire -- \
         otherwise any design with a shadow can never reach status=complete"
    );
}

#[test]
fn a_lone_layer_blur_is_not_reported_lossy() {
    let rendered = render(frame_with_effects(json!([{
        "radius": 4, "type": "LAYER_BLUR", "visible": true
    }])));
    assert!(rendered.tsx.contains(r#"filter="blur(4px)""#));
    assert!(!rendered.reported_lossy);
}

#[test]
fn a_blur_without_a_radius_is_reported_lossy() {
    // `push_effects` reads the radius with `unwrap_or(0.0)`, so a missing one
    // is silently fabricated into `blur(0px)` -- the blur is gone, not converted.
    assert!(
        render(frame_with_effects(json!([{
            "type": "BACKGROUND_BLUR", "visible": true
        }])))
        .reported_lossy
    );
    assert!(
        render(frame_with_effects(json!([{
            "type": "LAYER_BLUR", "visible": true
        }])))
        .reported_lossy
    );
}

#[test]
fn noise_is_still_reported_lossy() {
    // Converted to a no-op `contrast(100%) brightness(100%)` placeholder.
    assert!(
        render(frame_with_effects(json!([{
            "type": "NOISE", "visible": true
        }])))
        .reported_lossy
    );
}

#[test]
fn texture_is_still_reported_lossy() {
    assert!(
        render(frame_with_effects(json!([{
            "type": "TEXTURE", "visible": true
        }])))
        .reported_lossy
    );
}

#[test]
fn glass_is_still_reported_lossy() {
    // Flattened to a plain backdrop blur, which is an approximation.
    assert!(
        render(frame_with_effects(json!([{
            "radius": 8, "type": "GLASS", "visible": true
        }])))
        .reported_lossy
    );
}

#[test]
fn an_unknown_effect_type_is_still_reported_lossy() {
    // Silently dropped by `push_effects`; that must stay visible.
    assert!(
        render(frame_with_effects(json!([{
            "radius": 8, "type": "SOME_FUTURE_EFFECT", "visible": true
        }])))
        .reported_lossy
    );
}

#[test]
fn an_invisible_unsupported_effect_is_not_reported_lossy() {
    // `push_effects` skips invisible effects, so nothing was lost.
    assert!(
        !render(frame_with_effects(json!([{
            "type": "NOISE", "visible": false
        }])))
        .reported_lossy
    );
}

#[test]
fn a_shadow_with_a_non_normal_blend_mode_is_reported_lossy() {
    // CSS box-shadow has no per-shadow blend mode.
    assert!(
        render(frame_with_effects(json!([drop_shadow(
            json!({ "blendMode": "MULTIPLY" })
        )])))
        .reported_lossy
    );
}

#[test]
fn a_text_shadow_that_needs_spread_is_reported_lossy() {
    // `text-shadow` has no spread component, so a non-zero spread is dropped.
    let rendered = render(text_with_effects(json!([drop_shadow(
        json!({ "spread": 4 })
    )])));
    assert!(rendered.tsx.contains("textShadow="));
    assert!(
        rendered.reported_lossy,
        "spread cannot survive in text-shadow and must be reported"
    );
}

#[test]
fn a_text_shadow_without_spread_is_not_reported_lossy() {
    let rendered = render(text_with_effects(json!([drop_shadow(json!({}))])));
    assert!(rendered.tsx.contains("textShadow="));
    assert!(!rendered.reported_lossy);
}
