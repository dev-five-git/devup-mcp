fn transferred_minimum_is_emitted(
    snapshot: &Snapshot,
    node: &devup_mcp_figma::RawNode,
    tag: &str,
) -> Option<f64> {
    let width = crate::codegen::horizontal_transferred_image_minimum(snapshot, node)?;
    if !tag.trim_start().starts_with("<Image") || prop(tag, "minW") != Some("auto") {
        return None;
    }
    let height = node.typed_view().number("height")?;
    let emitted_height = prop(tag, "h").and_then(css_px)?;
    let (x, y) = prop(tag, "aspectRatio")?.split_once('/')?;
    let ratio = x.trim().parse::<f64>().ok()? / y.trim().parse::<f64>().ok()?;
    ((height - emitted_height).abs() <= 0.5
        && ratio.is_finite()
        && (height * ratio - width).abs() <= 0.5)
        .then_some(width)
}

/// Positive-free-space flex allocation with only source-proven replaced-image
/// minimums. Unknown text/content constraints still reject in the caller.
fn allocate_with_transferred_minimums(
    siblings: &mut [Value],
    free: f64,
    total_grow: f64,
) -> Result<(), &'static str> {
    let mut remaining = free;
    let mut grow = total_grow;
    loop {
        let freezing = siblings
            .iter()
            .enumerate()
            .filter_map(|(index, sibling)| {
                let own_grow = sibling["grow"].as_f64()?;
                let minimum = sibling["transferredMinimum"].as_f64()?;
                (sibling["allocatedWidth"].is_null()
                    && remaining * own_grow / grow.max(1.0) < minimum)
                    .then_some((index, minimum, own_grow))
            })
            .collect::<Vec<_>>();
        if freezing.is_empty() {
            break;
        }
        for (index, minimum, own_grow) in freezing {
            siblings[index]["allocatedWidth"] = json!(minimum);
            remaining -= minimum;
            grow -= own_grow;
        }
        if remaining < -0.5 {
            return Err("transferred-minimum-exceeds-parent");
        }
    }
    for sibling in siblings {
        if let Some(g) = sibling["grow"].as_f64() {
            let allocated = sibling["allocatedWidth"]
                .as_f64()
                .unwrap_or(remaining * g / grow.max(1.0));
            sibling["allocatedWidth"] = json!(allocated);
            if (allocated - sibling["capturedWidth"].as_f64().unwrap()).abs() > 0.5 {
                return Err("allocated-width-mismatch");
            }
        }
    }
    Ok(())
}
