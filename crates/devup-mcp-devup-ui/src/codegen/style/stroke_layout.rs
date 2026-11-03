/// The captured stroke-layout contract; direct node state takes precedence
/// over inferred layout. Missing data keeps the established legacy behavior.
pub(super) fn stroke_layout_is_auto(view: &TypedNode<'_>) -> bool {
    matches!(
        view.string("layoutMode").or_else(|| view
            .value("inferredAutoLayout")?
            .get("layoutMode")?
            .as_str()),
        Some("HORIZONTAL" | "VERTICAL" | "GRID")
    )
}

pub(super) fn strokes_included_in_layout(view: &TypedNode<'_>) -> bool {
    // An explicit NONE is authoritative: a stale flag cannot add border
    // layout space to manually positioned children, even if inference exists.
    if !stroke_layout_is_auto(view) {
        return false;
    }
    view.bool("strokesIncludedInLayout")
        .or_else(|| {
            view.value("inferredAutoLayout")?
                .get("strokesIncludedInLayout")?
                .as_bool()
        })
        .unwrap_or(false)
}

/// A stroke excluded from layout cannot consume space when padding is too
/// small to absorb a CSS border. Paint it inward with an outline instead.
pub(super) fn inside_stroke_uses_outline(view: &TypedNode<'_>) -> bool {
    !strokes_included_in_layout(view)
        && view.string("strokeAlign").unwrap_or("INSIDE") == "INSIDE"
        && matches!(view.string("layoutMode"), Some("HORIZONTAL" | "VERTICAL"))
        && view.number("strokeWeight").is_some_and(|weight| {
            weight > 0.0
                && ["paddingTop", "paddingRight", "paddingBottom", "paddingLeft"]
                    .iter()
                    .any(|field| view.number(field).is_some_and(|padding| padding < weight))
        })
}

/// Mixed per-edge inside strokes excluded from Figma layout use the same
/// border-box conversion as uniform strokes. This only applies when captured
/// padding can absorb every emitted SOLID border without clamping.
pub(super) fn mixed_excluded_stroke_padding(
    view: &TypedNode<'_>,
    padding: [f64; 4],
) -> Option<[f64; 4]> {
    let explicit_flag = view.bool("strokesIncludedInLayout").or_else(|| {
        view.value("inferredAutoLayout")?
            .get("strokesIncludedInLayout")?
            .as_bool()
    });
    if explicit_flag != Some(false)
        || !stroke_layout_is_auto(view)
        || view.number("strokeWeight").is_some()
        || view
            .value("strokeAlign")
            .is_some_and(|value| value.as_str() != Some("INSIDE"))
        || view
            .value("dashPattern")
            .is_some_and(|value| !value.as_array().is_some_and(Vec::is_empty))
    {
        return None;
    }
    let weight_fields = [
        "strokeTopWeight",
        "strokeRightWeight",
        "strokeBottomWeight",
        "strokeLeftWeight",
    ];
    let padding_fields = ["paddingTop", "paddingRight", "paddingBottom", "paddingLeft"];
    if [
        "strokesIncludedInLayout",
        "inferredAutoLayout",
        "strokeAlign",
        "strokeWeight",
        "strokes",
        "dashPattern",
    ]
    .iter()
    .chain(weight_fields.iter())
    .chain(padding_fields.iter())
    .any(|field| view.field_error(field).is_some())
    {
        return None;
    }
    let paint = view
        .value("strokes")?
        .as_array()?
        .iter()
        .find(|paint| paint["visible"] != false && paint["type"] == "SOLID")?;
    color_from_paint(paint)?; // Match the representable paint selected by push_strokes.
    let mut result = padding;
    let mut painted = false;
    for index in 0..4 {
        let weight = view.number(weight_fields[index])?;
        let captured = view
            .value("inferredAutoLayout")
            .and_then(|v| v.get(padding_fields[index]))
            .and_then(Value::as_f64)
            .or_else(|| view.number(padding_fields[index]))?;
        if !weight.is_finite()
            || weight < 0.0
            || !captured.is_finite()
            || (captured - padding[index]).abs() > 0.001
            || captured < weight
        {
            return None;
        }
        painted |= weight > 0.0;
        result[index] -= weight;
    }
    painted.then_some(result)
}

pub(super) fn mixed_excluded_stroke_source_padding(view: &TypedNode<'_>) -> Option<[f64; 4]> {
    let get = |field| {
        view.value("inferredAutoLayout")
            .and_then(|v| v.get(field))
            .and_then(Value::as_f64)
            .or_else(|| view.number(field))
    };
    let padding = [
        get("paddingTop")?,
        get("paddingRight")?,
        get("paddingBottom")?,
        get("paddingLeft")?,
    ];
    mixed_excluded_stroke_padding(view, padding)
}
