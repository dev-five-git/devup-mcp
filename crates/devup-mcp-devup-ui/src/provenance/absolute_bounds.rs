//! Export-boundary calculations are distinct from layout/constraint equivalence.
use devup_mcp_figma::{RawNode, Snapshot};
use serde_json::{Value, json};

use super::sizing::prop;

fn pixels(value: Option<&str>) -> Option<f64> {
    value?
        .strip_suffix("px")?
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

fn rounding(expected: Option<f64>, generated: Option<&str>) -> Value {
    let actual = pixels(generated);
    let error = expected.zip(actual).map(|(a, b)| (a - b).abs());
    // A centipixel serializer has a half-centipixel maximum error. Also require
    // the actual serializer result, so tolerance cannot excuse arbitrary CSS.
    let verified = expected
        .is_some_and(|v| v.is_finite() && generated == Some(crate::codegen::px(v).as_str()))
        && error.is_some_and(|e| e <= 0.005 + 1e-12);
    json!({"state":if verified {"verified"} else {"unverified"},
        "inputPixels":expected,"generatedValue":generated,"absoluteErrorPixels":error,
        "tolerancePixels":0.005,"floatingPointSlackPixels":1e-12,
        "calculation":"round(inputPixels * 100) / 100; compare emitted pixels independently of boundary selection and constraint interpretation."})
}

pub(super) fn annotate(
    snapshot: &Snapshot,
    node: &RawNode,
    owner: Option<&RawNode>,
    tag: &str,
    parent_tag: &str,
    components: &mut Value,
) {
    if crate::codegen::asset_kind(snapshot, node).is_none() || prop(tag, "pos") != Some("absolute")
    {
        return;
    }
    let view = node.typed_view();
    let export = crate::codegen::export_box(snapshot, node);
    // No render boundary was selected by generation. Retain the existing
    // explicit/intrinsic dimension and local-constraint audit in that case.
    if export.is_none() {
        return;
    }
    let boundary_ok = export.is_some_and(|b| {
        [b.x, b.y, b.w, b.h].into_iter().all(f64::is_finite) && b.w > 0.0 && b.h > 0.0
    }) && [
        "absoluteRenderBounds",
        "absoluteBoundingBox",
        "parentId",
        "layoutPositioning",
    ]
    .iter()
    .all(|f| !node.field_errors.contains_key(*f))
        && owner.is_some_and(|p| {
            view.string("parentId") == Some(p.id.as_str())
                && !p.field_errors.contains_key("absoluteBoundingBox")
        });
    let source_fields = json!([
        "absoluteRenderBounds",
        "absoluteBoundingBox",
        "layoutPositioning",
        "parentId",
        "parent.absoluteBoundingBox"
    ]);
    let original = json!({"absoluteRenderBounds":view.value("absoluteRenderBounds"),
        "absoluteBoundingBox":view.value("absoluteBoundingBox"),
        "parentId":view.string("parentId"),
        "parentBounds":owner.and_then(|p|p.typed_view().value("absoluteBoundingBox"))});
    for (axis, css, sizing) in [
        ("width", "w", "layoutSizingHorizontal"),
        ("height", "h", "layoutSizingVertical"),
    ] {
        let selected = export.map(|b| if axis == "width" { b.w } else { b.h });
        let generated = prop(tag, css).or_else(|| prop(tag, "boxSize"));
        let round = rounding(selected, generated);
        let component = &mut components[axis];
        component["boundary"] = json!({"state":if boundary_ok {"verified"} else {"unverified"},
            "selectedField":if export.is_some() {"absoluteRenderBounds"} else {"source-dimension-fallback"},
            "selectedValue":selected,"sourceFields":source_fields,"originalValue":original,
            "layoutDimension":view.number(axis),
            "boundaryDeltaPixels":selected.zip(view.number(axis)).map(|(a,b)|a-b),
            "calculation":format!("Absolute asset export policy: generated {css} = px(absoluteRenderBounds.{axis}); export origin = render.xy - parent.absoluteBoundingBox.xy. Bounding-box differences are boundary selection, not rounding."),
            "evidenceLimit":"Verifies the collected export-boundary projection, not equality with the layout bounding box or browser/SVG composition."});
        component["sourceFields"] = source_fields.clone();
        component["calculation"] = component["boundary"]["calculation"].clone();
        let verified = boundary_ok
            && round["state"] == "verified"
            && view.string(sizing) == Some("FIXED")
            && !node.field_errors.contains_key(sizing)
            && !node.field_errors.contains_key(axis)
            && ["minW", "maxW", "minH", "maxH", "boxSizing"]
                .iter()
                .all(|p| prop(tag, p).is_none());
        // This selected-boundary proof supersedes the generic dimension proof,
        // including its blocker. Never leave a percentage blocker on a verified asset.
        component["blockedBy"] = json!(if verified {
            None
        } else if [
            axis,
            sizing,
            "absoluteRenderBounds",
            "absoluteBoundingBox",
            "parentId",
            "layoutPositioning"
        ]
        .iter()
        .any(|f| node.field_errors.contains_key(*f))
            || owner.is_some_and(|p| p.field_errors.contains_key("absoluteBoundingBox"))
        {
            Some("read-error")
        } else if view.string(sizing) != Some("FIXED") {
            Some("non-fixed-sizing")
        } else if ["minW", "maxW", "minH", "maxH", "boxSizing"]
            .iter()
            .any(|p| prop(tag, p).is_some())
        {
            Some("conflicting-dimension-props")
        } else if !boundary_ok {
            Some("export-boundary-unproven")
        } else {
            Some("rounded-dimension-mismatch")
        });
        component["rounding"] = round;
        component["state"] = json!(if verified { "verified" } else { "approximated" });
        component["fidelityImpact"] = json!(if verified { "none" } else { "approximated" });
        component["resolvedPixels"] = json!(if verified { pixels(generated) } else { None });
        component["reason"] = json!(if verified {
            "FIXED exported asset dimension matches the selected render boundary within the explicit serializer tolerance; layout-box equality is not claimed."
        } else {
            "Export boundary, error-free FIXED sizing, conflicting props or rounded emitted dimension is not proven."
        });
        component["resolutionCondition"] = json!(
            "Collect error-free render bounds and parent bounds, confirm FIXED asset sizing and verify emitted dimensions with the separate 0.005px rounding check."
        );
    }
    // Match push_absolute's actual fallback: the first child is consulted only
    // when the node has no constraint object. This is policy, not source intent.
    let declared = view.value("constraints").and_then(Value::as_object);
    let first_child = view
        .child_ids()
        .next()
        .and_then(|id| snapshot.nodes.get(id));
    let inherited = first_child
        .and_then(|c| c.typed_view().value("constraints"))
        .and_then(Value::as_object);
    for (axis, coordinate, size, near, far) in [
        ("horizontal", "x", "width", "left", "right"),
        ("vertical", "y", "height", "top", "bottom"),
    ] {
        let constraint = declared
            .or(inherited)
            .and_then(|v| v.get(axis))
            .and_then(Value::as_str);
        let effective = constraint.unwrap_or("MIN");
        let origin = if constraint.is_none() {
            "default"
        } else if declared.is_some() {
            "node"
        } else {
            "first-child"
        };
        let source_id = match origin {
            "node" => Some(node.id.as_str()),
            "first-child" => first_child.map(|c| c.id.as_str()),
            _ => None,
        };
        let css = if effective == "MAX" { far } else { near };
        let offset = export.map(|b| if coordinate == "x" { b.x } else { b.y });
        let extent = export.map(|b| if size == "width" { b.w } else { b.h });
        let parent_size = owner.and_then(|p| p.typed_view().number(size));
        let expected = if effective == "MAX" {
            parent_size
                .zip(offset)
                .zip(extent)
                .map(|((p, x), w)| p - x - w)
        } else {
            offset
        };
        let generated = prop(tag, css);
        let round = rounding(expected, generated);
        let position_boundary_ok = boundary_ok
            && (effective != "MAX"
                || owner.is_some_and(|p| {
                    !p.field_errors.contains_key(size)
                        && parent_size.is_some_and(|v| v.is_finite() && v > 0.0)
                }));
        let mut position_fields = source_fields.clone();
        if effective == "MAX" {
            position_fields
                .as_array_mut()
                .unwrap()
                .push(json!(format!("parent.{size}")));
        }
        let component = &mut components[axis];
        component["constraint"] = json!(effective);
        component["constraintDeclared"] = json!(
            declared
                .and_then(|v| v.get(axis))
                .and_then(Value::as_str)
                .is_some()
        );
        component["generatedProperty"] = json!(css);
        component["generatedValue"] = json!(generated);
        component["constraintInterpretation"] = json!({"origin":origin,"sourceNodeId":source_id,
            "declaredValue":declared.and_then(|v|v.get(axis)),"effectiveValue":effective,
            "state":if origin == "node" && !node.field_errors.contains_key("constraints") {"declared"} else {"assumed"},
            "sourceFields":["constraints", "childrenIds", "children[0].constraints"],
            "calculation":"Use node constraint object; otherwise first child's constraint object; missing axis defaults to MIN. SCALE/STRETCH currently emit the near-edge offset, without a responsive scaling proof.",
            "resolutionCondition":"An inherited/default constraint is generation policy only; establish that it represents this GROUP's placement intent before verifying the axis."});
        component["boundary"] = json!({"state":if position_boundary_ok {"verified"} else {"unverified"},
            "selectedField":"absoluteRenderBounds","sourceFields":position_fields,"originalValue":original,
            "localOffset":offset,"extent":extent,"parentSize":parent_size,
            "calculation":if effective == "MAX" {"localOffset = render.coordinate - parent.bounds.coordinate; farEdge = parent.size - localOffset - render.size"} else {"nearEdge = render.coordinate - parent.bounds.coordinate; CENTER emits 50% plus translation and needs a separate center proof"},
            "expectedPixels":expected});
        component["rounding"] = round;
        // Preserve existing bounded proofs only when boundary selection agrees
        // with local geometry. New render-relative/inherited positions remain
        // approximated even when their arithmetic is exactly accounted for.
        let verified = component["fidelityImpact"] == "none"
            && position_boundary_ok
            && origin == "node"
            && !node.field_errors.contains_key("constraints")
            && offset == view.number(coordinate)
            && extent == view.number(size)
            && matches!(
                prop(parent_tag, "pos"),
                Some("relative" | "absolute" | "fixed")
            );
        if !verified {
            component["state"] = json!("approximated");
            component["fidelityImpact"] = json!("approximated");
            component["reason"] = json!(
                "Export-boundary arithmetic is reported separately. Inherited/default constraints, SCALE/STRETCH, changed local geometry or an unverified containing block do not prove source placement intent."
            );
            component["resolutionCondition"] = json!(
                "Prove the effective constraint belongs to this node, and that render-relative offsets preserve its positioning in the emitted containing block. A successful rounding check alone is insufficient."
            );
        }
    }
}
