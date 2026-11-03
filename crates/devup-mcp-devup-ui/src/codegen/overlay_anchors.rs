//! Captured dropdown relationships, without adding wrappers or inferring layout.
use std::collections::BTreeSet;

use devup_mcp_figma::{Diagnostic, DiagnosticSeverity, FidelityImpact, RawNode, Snapshot};
use serde_json::{Value, json};

use super::{
    component::{Prop, PropValue},
    layout::string_prop,
};

#[derive(Clone, Copy)]
struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Bounds {
    fn read(node: &RawNode) -> Option<Self> {
        let value = node.typed_view().value("absoluteBoundingBox")?;
        let b = Self {
            x: value["x"].as_f64()?,
            y: value["y"].as_f64()?,
            width: value["width"].as_f64()?,
            height: value["height"].as_f64()?,
        };
        [b.x, b.y, b.width, b.height]
            .iter()
            .all(|v| v.is_finite())
            .then_some(b)
            .filter(|b| b.width > 0.0 && b.height > 0.0)
    }
    fn value(self) -> Value {
        json!({"x":self.x,"y":self.y,"width":self.width,"height":self.height})
    }
}

struct Relation {
    root: String,
    control: String,
    overlay: String,
    parent: String,
    name: String,
    control_box: Bounds,
    overlay_box: Bounds,
    parent_box: Bounds,
    gap: f64,
}

fn ancestors<'a>(snapshot: &'a Snapshot, node: &'a RawNode) -> Vec<&'a RawNode> {
    let mut nodes = vec![node];
    let mut seen = BTreeSet::from([node.id.as_str()]);
    while let Some(parent) = nodes
        .last()
        .and_then(|n| n.typed_view().string("parentId"))
        .and_then(|id| snapshot.nodes.get(id))
    {
        if !seen.insert(parent.id.as_str()) {
            break;
        }
        nodes.push(parent);
    }
    nodes
}

fn visible(snapshot: &Snapshot, node: &RawNode) -> bool {
    ancestors(snapshot, node).iter().all(|node| {
        let view = node.typed_view();
        let override_value = view
            .value("componentPropertyReferences")
            .and_then(|v| v.get("visible"))
            .and_then(Value::as_str)
            .and_then(|key| {
                ancestors(snapshot, node)
                    .into_iter()
                    .skip(1)
                    .find_map(|parent| {
                        parent
                            .typed_view()
                            .value("componentProperties")?
                            .get(key)?
                            .get("value")?
                            .as_bool()
                    })
            });
        override_value.unwrap_or(view.bool("visible") != Some(false))
    })
}

fn named(node: &RawNode, name: &str) -> bool {
    [
        node.typed_view().name(),
        node.typed_view().string("mainComponentName"),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.eq_ignore_ascii_case(name))
}

fn dropdown_overlay(snapshot: &Snapshot, node: &RawNode) -> bool {
    if node.typed_view().string("layoutPositioning") != Some("ABSOLUTE") || !visible(snapshot, node)
    {
        return false;
    }
    let children = node
        .typed_view()
        .child_ids()
        .filter_map(|id| snapshot.nodes.get(id))
        .filter(|child| visible(snapshot, child))
        .collect::<Vec<_>>();
    !children.is_empty()
        && children.iter().all(|child| {
            child.node_type == "INSTANCE"
                && named(child, "DropdownOption")
                && child.typed_view().string("mainComponentId").is_some()
        })
}

fn caret(snapshot: &Snapshot, node: &RawNode, seen: &mut BTreeSet<String>) -> bool {
    if !seen.insert(node.id.clone()) || !visible(snapshot, node) {
        return false;
    }
    let view = node.typed_view();
    let captured_down = named(node, "CaretDown")
        || view.string("mainComponentName").is_some_and(|name| {
            name.split('=')
                .next_back()
                .is_some_and(|v| v.eq_ignore_ascii_case("CaretDown"))
        })
        || view
            .value("variantProperties")
            .and_then(Value::as_object)
            .is_some_and(|values| {
                values.values().any(|v| {
                    v.as_str()
                        .is_some_and(|v| v.eq_ignore_ascii_case("CaretDown"))
                })
            });
    (node.node_type == "INSTANCE" && view.string("mainComponentId").is_some() && captured_down)
        || view
            .child_ids()
            .filter_map(|id| snapshot.nodes.get(id))
            .any(|child| caret(snapshot, child, seen))
}

fn dropdown_control(snapshot: &Snapshot, node: &RawNode) -> bool {
    node.node_type == "INSTANCE"
        && node.typed_view().string("mainComponentId").is_some()
        && ["input", "select", "dropdown", "combobox"]
            .iter()
            .any(|name| named(node, name))
        && visible(snapshot, node)
        && caret(snapshot, node, &mut BTreeSet::new())
}

fn transformed(snapshot: &Snapshot, node: &RawNode) -> bool {
    ancestors(snapshot, node).iter().any(|node| {
        let view = node.typed_view();
        view.number("rotation").is_some_and(|v| v.abs() > 0.001)
            || view
                .value("relativeTransform")
                .and_then(Value::as_array)
                .is_some_and(|rows| {
                    let coefficient = |row: usize, col: usize| rows.get(row)?.get(col)?.as_f64();
                    [(0, 0, 1.0), (0, 1, 0.0), (1, 0, 0.0), (1, 1, 1.0)]
                        .iter()
                        .any(|(row, col, expected)| {
                            coefficient(*row, *col).is_none_or(|v| (v - expected).abs() > 0.001)
                        })
                })
    })
}

fn selected_root(snapshot: &Snapshot, node: &RawNode) -> Option<String> {
    ancestors(snapshot, node)
        .iter()
        .find(|node| snapshot.roots.contains(&node.id))
        .map(|node| node.id.clone())
}

fn issue(overlay: &RawNode, reason: &str, candidates: Vec<String>) -> Diagnostic {
    Diagnostic { code:"DEVUP_CODEGEN_OVERLAY_ANCHOR_UNRESOLVED".into(), node_id:Some(overlay.id.clone()),
        severity:Some(DiagnosticSeverity::Warning),fidelity_impact:Some(FidelityImpact::Lossy),
        message:"A captured dropdown overlay has no unique, untransformed control relationship; its original absolute placement is retained.".into(),
        details:Some(json!({"classification":reason,"candidateControlNodeIds":candidates,"runtimeVerified":false,
            "nextAction":"Capture an unambiguous dropdown control and options overlay in the same frame; do not infer an anchor from unrelated fields."})),..Default::default() }
}

fn discover(snapshot: &Snapshot) -> (Vec<Relation>, Vec<Diagnostic>) {
    let controls = snapshot
        .nodes
        .values()
        .filter(|node| dropdown_control(snapshot, node))
        .collect::<Vec<_>>();
    let mut relations = Vec::new();
    let mut diagnostics = Vec::new();
    for overlay in snapshot
        .nodes
        .values()
        .filter(|node| dropdown_overlay(snapshot, node))
    {
        let Some((overlay_box, parent, parent_box, root)) = Bounds::read(overlay).and_then(|b| {
            let p = snapshot
                .nodes
                .get(overlay.typed_view().string("parentId")?)?;
            Some((b, p, Bounds::read(p)?, selected_root(snapshot, overlay)?))
        }) else {
            diagnostics.push(issue(overlay, "missing-captured-geometry", vec![]));
            continue;
        };
        if ["horizontal", "vertical"].iter().any(|axis| {
            overlay
                .typed_view()
                .value("constraints")
                .and_then(|v| v.get(axis))
                .and_then(Value::as_str)
                .is_some_and(|v| v != "MIN")
        }) {
            diagnostics.push(issue(overlay, "non-minimum-overlay-constraints", vec![]));
            continue;
        }
        let matches = controls
            .iter()
            .filter_map(|control| {
                if selected_root(snapshot, control).as_deref() != Some(root.as_str())
                    || !ancestors(snapshot, control)
                        .iter()
                        .any(|n| n.id == parent.id)
                {
                    return None;
                }
                let b = Bounds::read(control)?;
                let gap = overlay_box.y - b.y - b.height;
                // The tolerance only pairs recorded edges; emitted values always
                // come from the complete captured boxes, never rounded guesses.
                ((b.x - overlay_box.x).abs() <= 0.01
                    && (b.width - overlay_box.width).abs() <= 0.01
                    && gap >= 0.0
                    && gap <= b.height)
                    .then_some((*control, b, gap))
            })
            .collect::<Vec<_>>();
        let Some((control, control_box, gap)) = matches.first().copied() else {
            diagnostics.push(issue(overlay, "no-captured-dropdown-control", vec![]));
            continue;
        };
        if matches.len() != 1 {
            diagnostics.push(issue(
                overlay,
                "ambiguous-captured-dropdown-control",
                matches.iter().map(|m| m.0.id.clone()).collect(),
            ));
            continue;
        }
        if transformed(snapshot, overlay)
            || transformed(snapshot, control)
            || ancestors(snapshot, control)
                .iter()
                .skip(1)
                .any(|n| dropdown_control(snapshot, n))
        {
            diagnostics.push(issue(
                overlay,
                "transformed-or-nested-dropdown-control",
                vec![control.id.clone()],
            ));
            continue;
        }
        let identity = control
            .id
            .bytes()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        relations.push(Relation {
            root,
            control: control.id.clone(),
            overlay: overlay.id.clone(),
            parent: parent.id.clone(),
            name: format!("--devup-dropdown-{identity}"),
            control_box,
            overlay_box,
            parent_box,
            gap,
        });
    }
    let duplicated = relations
        .iter()
        .filter(|r| {
            relations
                .iter()
                .filter(|other| other.control == r.control)
                .count()
                > 1
        })
        .map(|r| r.control.clone())
        .collect::<BTreeSet<_>>();
    relations.retain(|r| {
        if duplicated.contains(&r.control) {
            diagnostics.push(issue(
                &snapshot.nodes[&r.overlay],
                "multiple-overlays-for-control",
                vec![r.control.clone()],
            ));
            false
        } else {
            true
        }
    });
    (relations, diagnostics)
}

impl Relation {
    // devup-ui treats a raw leading `--` as a theme variable reference.
    // CSS escapes retain the same dashed identifier without that conversion.
    fn css_identifier(&self) -> String {
        format!("\\2d \\2d {}", self.name.trim_start_matches("--"))
    }
    fn props(&self, node: &RawNode) -> Vec<Prop> {
        let mut props = Vec::new();
        if node.id == self.control {
            string_prop(&mut props, "anchorName", self.css_identifier());
        }
        if node.id == self.parent {
            string_prop(&mut props, "anchorScope", "all");
        }
        if node.id == self.overlay {
            let top = self.overlay_box.y - self.parent_box.y;
            string_prop(&mut props, "positionAnchor", self.css_identifier());
            string_prop(
                &mut props,
                "top",
                format!(
                    "calc(anchor(bottom, {}) + {})",
                    super::layout::px(top - self.gap),
                    super::layout::px(self.gap)
                ),
            );
            string_prop(
                &mut props,
                "left",
                format!(
                    "anchor(left, {})",
                    super::layout::px(self.overlay_box.x - self.parent_box.x)
                ),
            );
            string_prop(
                &mut props,
                "w",
                format!(
                    "anchor-size(width, {})",
                    super::layout::px(self.overlay_box.width)
                ),
            );
        }
        props
    }
    fn metadata(&self) -> Value {
        json!({"schemaVersion":1,"kind":"captured-dropdown-control","rootNodeId":self.root,"controlNodeId":self.control,
            "overlayNodeId":self.overlay,"scopeNodeId":self.parent,"anchorName":self.name,"cssIdentifier":self.css_identifier(),"gap":self.gap,
            "controlBounds":self.control_box.value(),"overlayBounds":self.overlay_box.value(),"scopeBounds":self.parent_box.value(),
            "capturedFallback":{"top":self.overlay_box.y-self.parent_box.y,"left":self.overlay_box.x-self.parent_box.x,"width":self.overlay_box.width},
            "runtimeVerified":false,"requiresCSSAnchorPositioning":true,"unsupportedBrowsersVerified":false,
            "relationEvidence":"Visible captured dropdown input with mapped CaretDown and mapped DropdownOption children; same captured frame, matching left/width, unique nearby lower edge.",
            "verification":"Source relationship only; verify generated CSS, the native control mapping, and runtime error/open combinations in the consumer browser."})
    }
}

/// Called at the end of layout projection, replacing only captured placement
/// values. No DOM wrapper, control child, paint or flow property is changed.
pub(super) fn apply_props(snapshot: &Snapshot, node: &RawNode, props: &mut Vec<Prop>) {
    if !dropdown_control(snapshot, node)
        && !dropdown_overlay(snapshot, node)
        && !node
            .typed_view()
            .child_ids()
            .filter_map(|id| snapshot.nodes.get(id))
            .any(|n| dropdown_overlay(snapshot, n))
    {
        return;
    }
    for relation in discover(snapshot).0 {
        for (name, value) in relation.props(node) {
            props.retain(|(existing, _)| existing != &name);
            props.push((name, value));
        }
    }
}

pub(super) fn derivation(
    snapshot: &Snapshot,
    node: &RawNode,
    name: &str,
    value: &str,
) -> Option<Value> {
    if ![
        "anchorName",
        "anchorScope",
        "positionAnchor",
        "top",
        "left",
        "w",
    ]
    .contains(&name)
        || (matches!(name, "top" | "left" | "w")
            && !value.contains("anchor(")
            && !value.contains("anchor-size("))
    {
        return None;
    }
    discover(snapshot).0.into_iter().find_map(|relation| {
        relation.props(node).iter().any(|(p,PropValue::String(v))|p==name && v==value).then(||json!({
            "stage":"captured-overlay-anchor","generatedProperty":super::component::render_static_attribute(name,value),
            "sourceFields":["absoluteBoundingBox","parentId","childrenIds","name","mainComponentId","mainComponentName","variantProperties","componentProperties","componentPropertyReferences","visible","rotation","relativeTransform"],
            "originalValue":relation.metadata(),"calculation":"Project only the unique captured dropdown control/overlay relationship with CSS anchors and captured numeric fallbacks. Dynamic browser placement is not measured by this source derivation.","runtimeVerified":false}))
    })
}

fn attribute_identifier(source: &str, expected: &str) -> bool {
    let Some((_, value)) = source.split_once('=') else {
        return false;
    };
    let value = value.trim();
    if let Some(literal) = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    {
        return literal == expected;
    }
    let Some(expression) = value
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
    else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(expression) else {
        return false;
    };
    match value {
        Value::String(value) => value == expected,
        Value::Array(values) => values.iter().any(|value| value.as_str() == Some(expected)),
        _ => false,
    }
}

/// Safe source relationships actually represented by native anchor props in
/// this delivered TSX. Callers may expose the same JSON beside the annotation.
pub fn overlay_anchor_metadata(snapshot: &Snapshot, roots: &[String], tsx: &str) -> Vec<Value> {
    let attributes = crate::provenance::attributes::generated_attributes(tsx);
    discover(snapshot)
        .0
        .into_iter()
        .filter(|r| roots.contains(&r.root))
        .filter(|r| {
            ["anchorName", "positionAnchor"].iter().all(|name| {
                attributes.iter().any(|a| {
                    a.name == *name && attribute_identifier(&a.source, &r.css_identifier())
                })
            })
        })
        .map(|r| {
            let mut value = r.metadata();
            value["projected"] = json!(true);
            value
        })
        .collect()
}

pub(super) fn diagnostics(snapshot: &Snapshot, root: &str, tsx: &str) -> Vec<Diagnostic> {
    let (relations, mut issues) = discover(snapshot);
    issues.retain(|d| {
        d.node_id
            .as_ref()
            .and_then(|id| snapshot.nodes.get(id))
            .and_then(|node| selected_root(snapshot, node))
            .as_deref()
            == Some(root)
    });
    let metadata = overlay_anchor_metadata(snapshot, &[root.to_owned()], tsx);
    for relation in relations.into_iter().filter(|r| r.root == root) {
        if !metadata
            .iter()
            .any(|record| record["overlayNodeId"] == relation.overlay)
        {
            issues.push(issue(
                &snapshot.nodes[&relation.overlay],
                "native-control-not-expanded",
                vec![relation.control],
            ));
        }
    }
    issues
}
