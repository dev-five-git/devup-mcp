// A structural wrapper at one width need not duplicate its children at another.
// Keep the real box and give the widths that had direct siblings a contents box:
// those children still participate in their original parent's formatting context.
// This is derived topology, so its browser verification remains explicit.
const CONTENTS_WRAPPER_PREFIX: &str = "__devup_responsive_contents__";

fn contents_wrapper_source(id: &str) -> Option<[String; 2]> {
    serde_json::from_str(id.strip_prefix(CONTENTS_WRAPPER_PREFIX)?).ok()
}

fn same_responsive_content(snapshot: &Snapshot, left: &Tree, right: &Tree) -> bool {
    if !(0..4).any(|pass| matches_at(snapshot, left, right, pass))
        || left.content.as_deref().map(|s| s.replace("<br />", ""))
            != right.content.as_deref().map(|s| s.replace("<br />", ""))
        || left.is_component != right.is_component
        || left.children.len() != right.children.len()
        || left.visible_when != right.visible_when
        || left.drawn_when != right.drawn_when
        || [
            "as",
            "role",
            "href",
            "type",
            "name",
            "id",
            "aria-label",
            "aria-labelledby",
            "aria-describedby",
            "htmlFor",
            "value",
            "defaultValue",
            "checked",
            "disabled",
            "tabIndex",
        ]
        .iter()
        .any(|key| left.props.get(*key) != right.props.get(*key))
    {
        return false;
    }
    if left.is_component {
        // A shared component name is not evidence that its text/business props
        // agree. Placement can vary, but never guess an instance's content.
        let content_props = |tree: &Tree| {
            tree.props
                .iter()
                .filter(|(name, _)| !is_placement_prop(name))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        if content_props(left) != content_props(right) {
            return false;
        }
    }
    left.children
        .iter()
        .zip(&right.children)
        .all(|(left, right)| same_responsive_content(snapshot, left, right))
}

fn structural_wrapper_reason(tree: &Tree, snapshot: &Snapshot) -> Option<&'static str> {
    if tree.is_component
        || tree.content.is_some()
        || !LAYOUT_ELEMENTS.contains(&tree.component.as_str())
        || tree.visible_when.is_some()
        || !tree.drawn_when.is_empty()
    {
        return Some("component, text, conditional or semantic wrapper");
    }
    // Whitelisting box-layout properties avoids dropping background, padding,
    // borders, clipping, semantics, selectors or inherited typography/colour.
    const BOX_LAYOUT: &[&str] = &[
        "display",
        "flexDir",
        "flexWrap",
        "justifyContent",
        "alignItems",
        "alignContent",
        "alignSelf",
        "gap",
        "rowGap",
        "columnGap",
        "flex",
        "flexGrow",
        "flexShrink",
        "flexBasis",
        "w",
        "h",
        "minW",
        "maxW",
        "minH",
        "maxH",
        "boxSize",
        "gridTemplateColumns",
        "gridTemplateRows",
        "gridAutoFlow",
        "gridColumn",
        "gridRow",
    ];
    if tree
        .props
        .keys()
        .any(|name| !BOX_LAYOUT.contains(&name.as_str()))
    {
        return Some("wrapper has paint, padding, clipping, semantics or inherited styles");
    }
    if tree
        .props
        .get("display")
        .is_some_and(|value| value == "none")
    {
        return Some("wrapper is hidden at a captured width");
    }
    let Some(node) = snapshot.nodes.get(&tree.node_id) else {
        return Some("wrapper source is not captured");
    };
    let view = node.typed_view();
    // The per-width projection can leave an unsupported or incompletely read
    // source property unrepresented. Its absence from props is not permission
    // to erase the corresponding source box.
    const BOX_EFFECTS: &[&str] = &[
        "fills",
        "strokes",
        "effects",
        "clipsContent",
        "opacity",
        "rotation",
        "paddingLeft",
        "paddingRight",
        "paddingTop",
        "paddingBottom",
        "cornerRadius",
    ];
    if BOX_EFFECTS
        .iter()
        .any(|field| node.field_errors.contains_key(*field))
        || ["fills", "strokes", "effects"].iter().any(|field| {
            view.value(field)
                .and_then(Value::as_array)
                .is_some_and(|values| !values.is_empty())
        })
        || view.bool("clipsContent") == Some(true)
        || view.number("opacity").is_some_and(|value| value != 1.0)
        || [
            "rotation",
            "paddingLeft",
            "paddingRight",
            "paddingTop",
            "paddingBottom",
            "cornerRadius",
        ]
        .iter()
        .any(|field| view.number(field).is_some_and(|value| value != 0.0))
        || view.string("layoutPositioning") == Some("ABSOLUTE")
    {
        return Some("captured wrapper has paint, padding, clipping or box effects");
    }
    if !matches!(
        node.typed_view().string("layoutMode"),
        Some("HORIZONTAL" | "VERTICAL")
    ) || node.field_errors.contains_key("layoutMode")
        || node.field_errors.contains_key("childrenIds")
    {
        return Some("wrapper layout or child source is incomplete");
    }
    None
}

fn matching_direct_children(
    snapshot: &Snapshot,
    wrapper: &Tree,
    parent: &Tree,
) -> Option<Vec<usize>> {
    let indices = wrapper
        .children
        .iter()
        .map(|child| {
            let matches = parent
                .children
                .iter()
                .enumerate()
                .filter(|(_, other)| same_responsive_content(snapshot, child, other))
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            (matches.len() == 1).then(|| matches[0])
        })
        .collect::<Option<Vec<_>>>()?;
    let unique = indices.iter().copied().collect::<BTreeSet<_>>();
    (unique.len() == indices.len()).then_some(indices)
}

fn compatible_outer_order(
    snapshot: &Snapshot,
    source: &Tree,
    wrapper: &Tree,
    target: &Tree,
    indices: &[usize],
) -> bool {
    let mut previous = None;
    for child in &source.children {
        let index = if child.node_id == wrapper.node_id {
            Some(indices[0])
        } else {
            let matches = target
                .children
                .iter()
                .enumerate()
                .filter(|(index, other)| {
                    !indices.contains(index) && same_responsive_content(snapshot, child, other)
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if matches.len() > 1 {
                return false;
            }
            matches.first().copied()
        };
        if let Some(index) = index {
            if previous.is_some_and(|previous| previous >= index) {
                return false;
            }
            previous = Some(index);
        }
    }
    true
}

fn topology_review(context: &mut MergeContext<'_>, wrapper: &Tree, parent: &Tree, reason: &str) {
    if context.diagnostics.iter().any(|d| {
        d.code == "DEVUP_CODEGEN_RESPONSIVE_TOPOLOGY_UNREPRESENTED"
            && d.node_id.as_deref() == Some(wrapper.node_id.as_str())
            && d.details
                .as_ref()
                .is_some_and(|details| details["otherParentNodeId"] == parent.node_id)
    }) {
        return;
    }
    context.notes.push(Unrepresented {
        node_id: wrapper.node_id.clone(),
        detail: format!("Identical content has different wrapper topology near {} and {}. Responsive branches remain because {reason}; resolve this structure before claiming one responsive content tree.", wrapper.node_id, parent.node_id),
    });
    context.diagnostics.push(devup_mcp_figma::Diagnostic {
        code: "DEVUP_CODEGEN_RESPONSIVE_TOPOLOGY_UNREPRESENTED".into(),
        node_id: Some(wrapper.node_id.clone()),
        message: "Common responsive content crosses a wrapper boundary that could not safely be reconciled.".into(),
        severity: Some(devup_mcp_figma::DiagnosticSeverity::Warning),
        fidelity_impact: Some(devup_mcp_figma::FidelityImpact::Approximated),
        details: Some(json!({"otherParentNodeId":parent.node_id,"reason":reason,
            "output":"responsiveTsx","duplicatesContent":true,
            "nextAction":"Review the shared wrapper and carry every width's layout, inherited style and semantics into one responsive tree; do not flatten a painted, padded or meaningful wrapper."})),
        ..Default::default()
    });
}

fn reconcile_child_wrappers(
    by_slot: &BySlot<Tree>,
    context: &mut MergeContext<'_>,
) -> Option<BySlot<Tree>> {
    // Bounds keep unusual generated documents from turning matching into an
    // unbounded subtree search. Deep wrappers are considered at their own merge.
    if by_slot
        .iter()
        .flatten()
        .any(|parent| parent.children.len() > 128)
    {
        return None;
    }
    let templates = by_slot
        .iter()
        .enumerate()
        .filter_map(|(slot, parent)| {
            Some(
                parent
                    .as_ref()?
                    .children
                    .iter()
                    .filter(|child| {
                        !child.is_component
                            && child.content.is_none()
                            && (2..=16).contains(&child.children.len())
                            && LAYOUT_ELEMENTS.contains(&child.component.as_str())
                            && contents_wrapper_source(&child.node_id).is_none()
                    })
                    .cloned()
                    .map(move |child| (slot, child)),
            )
        })
        .flatten()
        .collect::<Vec<_>>();
    let mut output = by_slot.clone();
    let mut changed = false;
    for (source_slot, wrapper) in templates {
        for (slot, captured_parent) in output.iter_mut().enumerate() {
            if slot == source_slot {
                continue;
            }
            let Some(parent) = captured_parent.as_ref() else {
                continue;
            };
            // An existing exact wrapper can be aligned normally. Never wrap a
            // second copy around it just because a descendant happens to match.
            if parent
                .children
                .iter()
                .any(|other| same_responsive_content(context.snapshot, &wrapper, other))
            {
                continue;
            }
            let Some(indices) = matching_direct_children(context.snapshot, &wrapper, parent) else {
                let shared = wrapper.children.iter().any(|child| {
                    parent
                        .children
                        .iter()
                        .any(|other| same_responsive_content(context.snapshot, child, other))
                });
                if shared {
                    topology_review(
                        context,
                        &wrapper,
                        parent,
                        "child correspondence is partial or ambiguous",
                    );
                }
                continue;
            };
            let reason = structural_wrapper_reason(&wrapper, context.snapshot)
                .or_else(|| {
                    (!compatible_outer_order(context.snapshot, by_slot[source_slot].as_ref().expect("captured source"), &wrapper, parent, &indices))
                        .then_some("outer sibling order differs; CSS order on a contents wrapper would not order its children")
                })
                .or_else(|| {
                    parent.props.keys().any(|name| name == "as" || name == "role"
                        || name == "selectors" || name.starts_with('_'))
                        .then_some("the parent has semantics or selectors whose child structure must be preserved")
                })
                .or_else(|| {
                    (!indices.windows(2).all(|pair| pair[1] == pair[0] + 1))
                        .then_some("common children are reordered or are not contiguous siblings")
                })
                .or_else(|| {
                    parent
                        .children
                        .iter()
                        .filter(|child| {
                            indices
                                .iter()
                                .any(|index| parent.children[*index].node_id == child.node_id)
                        })
                        .any(|child| {
                            child.props.contains_key("order")
                                || child
                                    .props
                                    .get("pos")
                                    .is_some_and(|value| value == "absolute")
                        })
                        .then_some("common children already have order or absolute positioning")
                });
            if let Some(reason) = reason {
                topology_review(context, &wrapper, parent, reason);
                continue;
            }
            let start = indices[0];
            let end = start + indices.len();
            let mut contents = wrapper.clone();
            contents.node_id = format!(
                "{CONTENTS_WRAPPER_PREFIX}{}",
                json!([parent.node_id, wrapper.node_id])
            );
            contents.source_node_ids.clear();
            contents.props = BTreeMap::from([("display".into(), "contents".into())]);
            contents.children = parent.children[start..end].to_vec();
            contents.slots.clear();
            contents.scalar_sources.clear();
            contents.leading_comment = None;
            context.diagnostics.push(devup_mcp_figma::Diagnostic {
                code: "DEVUP_CODEGEN_RESPONSIVE_WRAPPER_RECONCILED".into(),
                node_id: Some(wrapper.node_id.clone()),
                message: "Common responsive children share one tree; a derived display:contents wrapper preserves their direct-parent layout at this width.".into(),
                severity: Some(devup_mcp_figma::DiagnosticSeverity::Info),
                details: Some(json!({"slot":slot,"width":context.slot_widths[slot],"sourceParentNodeId":parent.node_id,
                    "sourceChildren":contents.children.iter().map(|child| &child.node_id).collect::<Vec<_>>(),
                    "sourceWrapperNodeId":wrapper.node_id,"duplicatesContent":false,"runtimeVerified":false,
                    "nextAction":"Check layout, responsive ordering, focus and reading order in the browser at every captured width."})),
                ..Default::default()
            });
            captured_parent
                .as_mut()
                .expect("captured parent")
                .children
                .splice(start..end, [contents]);
            changed = true;
        }
    }
    changed.then_some(output)
}

fn derived_wrapper_provenance(tree: &Tree) -> Vec<Value> {
    tree.slots.iter().enumerate().filter_map(|(slot, source)| {
        let (id, _) = source.as_ref()?;
        let [parent, wrapper] = contents_wrapper_source(id)?;
        Some(json!({"slot":slot,"sourceNodeId":parent,"sourceField":"childrenIds",
            "capturedWrapperNodeId":wrapper,"calculation":"A derived display:contents wrapper groups the same contiguous children without adding a layout box at this width",
            "runtimeVerified":false}))
    }).collect()
}
