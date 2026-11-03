/// A single straight outside edge is a translated, zero-blur box shadow.
/// Unlike a CSS border it does not reduce a fixed content box or grow a HUG
/// box. Rounded corners, dashes and multi-edge joins need a different paint
/// construction, so retain their existing emission rather than approximate it.
pub(crate) fn single_outside_stroke(view: &TypedNode<'_>) -> Option<(usize, f64)> {
    if view.string("strokeAlign") != Some("OUTSIDE")
        || !matches!(view.string("layoutMode"), Some("HORIZONTAL" | "VERTICAL"))
        || view.number("strokeWeight").is_some()
        || [
            "cornerRadius",
            "topLeftRadius",
            "topRightRadius",
            "bottomRightRadius",
            "bottomLeftRadius",
        ]
        .iter()
        .any(|field| view.number(field).is_some_and(|radius| radius != 0.0))
        || view
            .value("dashPattern")
            .and_then(Value::as_array)
            .is_some_and(|dash| !dash.is_empty())
        || !view
            .value("strokes")
            .and_then(Value::as_array)
            .is_some_and(|paints| {
                // Match push_strokes' selected paint, not a later paint that
                // it will never reach after rejecting an unpaintable first one.
                paints
                    .iter()
                    .find(|paint| {
                        paint.get("visible").and_then(Value::as_bool) != Some(false)
                            && paint.get("type").and_then(Value::as_str) == Some("SOLID")
                    })
                    .is_some_and(|paint| color_from_paint(paint).is_some())
            })
    {
        return None;
    }
    let mut edge = None;
    for (index, field) in [
        "strokeTopWeight",
        "strokeRightWeight",
        "strokeBottomWeight",
        "strokeLeftWeight",
    ]
    .iter()
    .enumerate()
    {
        let weight = view.number(field)?;
        if !weight.is_finite() || weight < 0.0 {
            return None;
        }
        if weight > 0.0 {
            if edge.is_some() {
                return None;
            }
            edge = Some((index, weight));
        }
    }
    edge
}

/// Replay the same stroke writer used by the parent element; outlines and
/// shadows are not native borders and must never shift absolute coordinates.
pub(super) fn emitted_leading_border(node: &RawNode) -> Option<(f64, f64)> {
    let view = node.typed_view();
    if [
        "strokes",
        "strokeAlign",
        "strokeWeight",
        "strokeTopWeight",
        "strokeRightWeight",
        "strokeBottomWeight",
        "strokeLeftWeight",
        "strokesIncludedInLayout",
    ]
    .iter()
    .any(|field| view.field_error(field).is_some())
        || view
            .value("strokeAlign")
            .is_some_and(|value| value.as_str().is_none())
    {
        return None;
    }
    let mut props = Vec::new();
    push_strokes(
        &view,
        single_outside_stroke(&view),
        &mut props,
        &mut BTreeSet::new(),
        &BTreeMap::new(),
    );
    let (mut left, mut top) = (0.0, 0.0);
    let mut native = false;
    for (name, PropValue::String(value)) in props {
        if !matches!(name.as_str(), "border" | "borderLeft" | "borderTop") {
            continue;
        }
        let weight = value
            .split_whitespace()
            .nth(1)?
            .strip_suffix("px")?
            .parse::<f64>()
            .ok()?;
        if !weight.is_finite() || weight < 0.0 {
            return None;
        }
        if name == "border" || name == "borderLeft" {
            left = weight;
        }
        if name == "border" || name == "borderTop" {
            top = weight;
        }
        native = true;
    }
    (native && (left > 0.0 || top > 0.0)).then_some((left, top))
}

fn push_strokes(
    view: &TypedNode<'_>,
    outside_edge: Option<(usize, f64)>,
    props: &mut Vec<Prop>,
    used_tokens: &mut BTreeSet<String>,
    variable_tokens: &std::collections::BTreeMap<String, String>,
) {
    let Some(paint) = view
        .value("strokes")
        .and_then(Value::as_array)
        .and_then(|strokes| {
            strokes.iter().find(|paint| {
                paint.get("visible").and_then(Value::as_bool) != Some(false)
                    && paint.get("type").and_then(Value::as_str) == Some("SOLID")
            })
        })
    else {
        return;
    };
    let color = if let Some(token) = bound_paint_token(paint, variable_tokens) {
        used_tokens.insert(token.clone());
        format!("${token}")
    } else if let Some(color) = color_from_paint(paint) {
        color
    } else {
        return;
    };
    let style = if view
        .value("dashPattern")
        .and_then(Value::as_array)
        .is_some_and(|value| !value.is_empty())
    {
        "dashed"
    } else {
        "solid"
    };
    let align = view.string("strokeAlign").unwrap_or("INSIDE");
    let explicit_weight = view.number("strokeWeight");
    if let Some((edge, weight)) = outside_edge {
        let (x, y) = [(0.0, -weight), (weight, 0.0), (0.0, weight), (-weight, 0.0)][edge];
        string_prop(
            props,
            "boxShadow",
            format!("{} {} 0 0 {color}", zero_or_px(x), zero_or_px(y)),
        );
        return;
    }
    if explicit_weight.is_none() {
        let sides = [
            ("strokeTopWeight", "borderTop"),
            ("strokeRightWeight", "borderRight"),
            ("strokeBottomWeight", "borderBottom"),
            ("strokeLeftWeight", "borderLeft"),
        ];
        if sides.iter().all(|(field, _)| view.number(field).is_some()) {
            for (field, prop) in sides {
                let weight = view.number(field).unwrap_or(0.0);
                if weight != 0.0 {
                    string_prop(props, prop, format!("{style} {} {color}", px(weight)));
                }
            }
        }
        return;
    }
    let weight = explicit_weight.unwrap_or(0.0);
    if view.node_type() == "LINE" {
        string_prop(props, "outline", format!("{style} {} {color}", px(weight)));
        let base = if view.string("layoutSizingHorizontal") == Some("FIXED") {
            view.number("width")
                .map(px)
                .unwrap_or_else(|| "100%".to_owned())
        } else {
            "100%".to_owned()
        };
        string_prop(
            props,
            "maxW",
            format!("calc({base} - {})", px(weight * 2.0)),
        );
        if view
            .number("rotation")
            .is_none_or(|rotation| rotation.abs() <= 0.01)
        {
            string_prop(
                props,
                "transform",
                format!("translate({}, {})", px(weight), px(-weight)),
            );
        }
        return;
    }
    if inside_stroke_uses_outline(view) {
        string_prop(props, "outline", format!("{style} {} {color}", px(weight)));
        string_prop(props, "outlineOffset", px(-weight));
    } else if align == "INSIDE" {
        string_prop(props, "border", format!("{style} {} {color}", px(weight)));
    } else {
        string_prop(props, "outline", format!("{style} {} {color}", px(weight)));
        if align == "CENTER" {
            string_prop(props, "outlineOffset", px(-weight / 2.0));
        }
    }
}
