/// A native solid ellipse has no intrinsic CSS content. Percentage height and
/// an omitted width therefore destroy its captured fixed axes under an auto-
/// layout frame. Preserve only this explicit, read-safe absolute shape case;
/// exported assets and unproven/fluid geometry retain their existing policy.
fn fixed_native_ellipse_size(view: &TypedNode<'_>, component: &str, is_asset: bool) -> Option<(f64, f64)> {
    if is_asset || component != "Box" || view.node_type() != "ELLIPSE"
        || view.string("layoutPositioning") != Some("ABSOLUTE")
        || view.string("layoutSizingHorizontal") != Some("FIXED")
        || view.string("layoutSizingVertical") != Some("FIXED")
        || view.child_ids().next().is_some()
        || ["width", "height", "layoutPositioning", "layoutSizingHorizontal", "layoutSizingVertical", "arcData", "fills"]
            .iter().any(|field| view.field_error(field).is_some())
    { return None; }
    let arc = view.value("arcData")?;
    if arc.get("innerRadius")?.as_f64()? != 0.0
        || arc.get("startingAngle")?.as_f64()?.abs() > 0.00001
        || (arc.get("endingAngle")?.as_f64()? - std::f64::consts::TAU).abs() > 0.00001
    { return None; }
    let paints = view.value("fills")?.as_array()?;
    if !paints.iter().any(|paint| paint.get("type").and_then(Value::as_str) == Some("SOLID")
        && paint.get("visible").and_then(Value::as_bool) != Some(false)
        && paint.get("opacity").and_then(Value::as_f64).unwrap_or(1.0) > 0.0
        && ["r", "g", "b"].iter().all(|channel| paint.get("color").and_then(|color| color.get(channel))
            .and_then(Value::as_f64).is_some_and(f64::is_finite)))
    { return None; }
    let (width, height) = (view.number("width")?, view.number("height")?);
    (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0).then_some((width, height))
}

/// Rectangles have no intrinsic content either. Figma's shape API may omit
/// layoutSizing*, unlike auto-layout frames; error-free measured axes and
/// non-stretch constraints are enough to retain their native fixed geometry.
fn fixed_native_rectangle_size(view: &TypedNode<'_>, component: &str, is_asset: bool) -> Option<(f64, f64)> {
    if is_asset || component != "Box" || view.node_type() != "RECTANGLE"
        || view.string("layoutPositioning") != Some("ABSOLUTE")
        || view.child_ids().next().is_some()
        || ["width", "height", "layoutPositioning", "layoutSizingHorizontal", "layoutSizingVertical", "constraints"]
            .iter().any(|field| view.field_error(field).is_some())
        || ["layoutSizingHorizontal", "layoutSizingVertical"].iter()
            .any(|field| view.string(field).is_some_and(|sizing| sizing != "FIXED"))
        || ["horizontal", "vertical"].iter().any(|axis| view.value("constraints")
            .and_then(|constraints| constraints.get(axis)).and_then(Value::as_str)
            .is_some_and(|constraint| matches!(constraint, "STRETCH" | "SCALE")))
    { return None; }
    let (width, height) = (view.number("width")?, view.number("height")?);
    (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0).then_some((width, height))
}
