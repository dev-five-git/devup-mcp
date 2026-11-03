/// A horizontal flex item with a definite captured cross size has a
/// transferred automatic minimum through its captured aspect ratio. The image
/// projection's generic minW=0 must not erase that sizing contract.
pub(crate) fn horizontal_transferred_image_minimum(
    snapshot: &Snapshot,
    node: &RawNode,
) -> Option<f64> {
    let view = node.typed_view();
    if view.string("layoutSizingHorizontal") != Some("FILL")
        || view.string("layoutSizingVertical") != Some("FIXED")
        || view.string("layoutPositioning") == Some("ABSOLUTE")
        || view.number("minWidth").is_some()
        || view.number("maxWidth").is_some()
    {
        return None;
    }
    let parent = snapshot.nodes.get(view.string("parentId")?)?;
    let parent_view = parent.typed_view();
    if parent_view.string("layoutMode") != Some("HORIZONTAL")
        || parent_view.string("layoutWrap") == Some("WRAP")
    {
        return None;
    }
    let width = view.number("width").filter(|n| n.is_finite() && *n > 0.0)?;
    let height = view
        .number("height")
        .filter(|n| n.is_finite() && *n > 0.0)?;
    let target = view.value("targetAspectRatio");
    let target_ratio = target
        .and_then(|v| Some(v["x"].as_f64()? / v["y"].as_f64()?))
        .filter(|ratio| ratio.is_finite() && *ratio > 0.0);
    if target.is_some_and(|value| !value.is_null()) {
        if target_ratio.is_none_or(|ratio| (height * ratio - width).abs() > 0.5) {
            return None;
        }
    } else if view.bool("preserveRatio") != Some(true) {
        return None;
    }
    // The renderer must actually project the source as a replaced image.
    if !matches!(
        style::asset_kind(snapshot, node),
        Some(style::AssetKind::Png)
    ) {
        return None;
    }
    Some(width)
}

fn preserve_transferred_minimum(snapshot: &Snapshot, node: &RawNode, mapping: &mut Value) {
    let Some(width) = horizontal_transferred_image_minimum(snapshot, node) else {
        return;
    };
    if mapping["css"]["wrapper"]["aspectRatio"].as_str().is_none()
        || !mapping["css"]["image"]["transform"].is_null()
    {
        return;
    }
    // Explicit auto is necessary when a responsive mobile branch emits zero.
    mapping["css"]["wrapper"]["minW"] = json!("auto");
    mapping["minimumSizingEvidence"] = json!({
        "property":"minW", "value":"auto", "sourceNodeId":node.id,
        "parentId":node.typed_view().string("parentId"),
        "capturedTransferredMinimum":width,
        "targetAspectRatio":node.typed_view().value("targetAspectRatio"),
        "preserveRatio":node.typed_view().bool("preserveRatio"),
        "reason":"horizontal main-axis FILL, definite FIXED cross-axis height and captured aspect ratio retain CSS transferred automatic minimum; no fixed width is introduced",
        "scope":"replaced-image-horizontal-flow-only"
    });
}
