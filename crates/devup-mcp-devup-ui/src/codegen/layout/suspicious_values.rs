pub(super) fn suspicious_values(
    snapshot: &Snapshot,
    node: &RawNode,
    props: &[Prop],
) -> Vec<devup_mcp_figma::Diagnostic> {
    let view = node.typed_view();
    if super::style::asset_kind(snapshot, node).is_some() {
        return Vec::new();
    }
    let parent = snapshot.nodes.values().find(|candidate| {
        candidate.typed_view().child_ids().any(|id| id == node.id)
    });
    if parent.is_some_and(|parent| {
        let parent = parent.typed_view();
        parent.string("counterAxisAlignItems") == Some("CENTER")
            || parent.string("primaryAxisAlignItems") == Some("CENTER")
    }) || view.string("layoutAlign") == Some("CENTER") {
        return Vec::new();
    }
    let mut diagnostics = Vec::new();
    for (property, PropValue::String(value)) in props {
        let field = match property.as_str() {
            "w" => "width",
            "h" => "height",
            "boxSize" => "width",
            "aspectRatio" => "targetAspectRatio",
            _ => continue,
        };
        if field == "width" && view.node_type() == "TEXT"
            && (view.string("layoutSizingHorizontal") == Some("HUG")
                || view.string("textAutoResize") == Some("WIDTH_AND_HEIGHT")) {
            continue;
        }
        let advice = if property == "aspectRatio" {
            let ratio = value.split_once('/').and_then(|(x,y)| Some(x.trim().parse::<f64>().ok()? / y.trim().parse::<f64>().ok()?))
                .or_else(|| value.parse::<f64>().ok());
            let Some(ratio) = ratio.filter(|ratio| ratio.is_finite() && *ratio > 0.0) else { continue; };
            let simple = [1.0, 2.0, 0.5, 3.0, 1.0/3.0, 1.5, 2.0/3.0, 4.0/3.0, 0.75, 16.0/9.0, 9.0/16.0, 21.0/9.0, 9.0/21.0];
            if simple.iter().any(|simple| (ratio / simple - 1.0).abs() <= 0.01) { continue; }
            format!("Aspect ratio {value} is not within 1% of a common simple ratio; confirm the intended ratio with the designer in Figma's aspect-ratio control.")
        } else {
            let Some(number) = value.strip_suffix("px").and_then(|value| value.parse::<f64>().ok()) else { continue; };
            if !number.is_finite() || number <= 0.0 || (number.fract()).abs() > 0.001
                || (number.rem_euclid(2.0) - 1.0).abs() > 0.001
                || parent.is_some_and(|parent| explained_by_layout(snapshot, parent, field, number)) {
                continue;
            }
            format!("{value} is odd; nearest even {}/{}px: confirm with the designer or round the {field} in Figma, then re-export.", format_number(number-1.0), format_number(number+1.0))
        };
        diagnostics.push(devup_mcp_figma::Diagnostic {
            code: "DEVUP_CODEGEN_SUSPICIOUS_VALUE".into(),
            node_id: Some(node.id.clone()),
            property: Some(field.into()),
            severity: Some(devup_mcp_figma::DiagnosticSeverity::Warning),
            fidelity_impact: Some(devup_mcp_figma::FidelityImpact::None),
            message: format!("Suspicious design value for {field}: {value}; the captured value is preserved."),
            details: Some(serde_json::json!({"originalValue":view.value(field),"appliedValue":value,"generatedProperty":property,"nextAction":advice})),
            ..Default::default()
        });
    }
    diagnostics
}

fn explained_by_layout(snapshot: &Snapshot, parent: &RawNode, field: &str, number: f64) -> bool {
    let view = parent.typed_view();
    let Some(size) = view.number(field) else { return false; };
    let (start, end, main_axis) = match field {
        "width" => ("paddingLeft", "paddingRight", "HORIZONTAL"),
        "height" => ("paddingTop", "paddingBottom", "VERTICAL"),
        _ => return false,
    };
    if !matches!(view.string("layoutMode"), Some("HORIZONTAL" | "VERTICAL")) { return false; }
    let available = size - view.number(start).unwrap_or(0.0) - view.number(end).unwrap_or(0.0);
    let children = view.child_ids().filter_map(|id| snapshot.nodes.get(id))
        .filter(|child| child.typed_view().bool("visible") != Some(false)
            && child.typed_view().string("layoutPositioning") != Some("ABSOLUTE"))
        .collect::<Vec<_>>();
    if view.string("layoutMode") != Some(main_axis) {
        return (available-number).abs() <= 0.01;
    }
    let Some(sizes) = children.iter().map(|child| child.typed_view().number(field)).collect::<Option<Vec<_>>>() else { return false; };
    let gaps = sizes.iter().skip(1).map(|_| view.number("itemSpacing").unwrap_or(0.0)).sum::<f64>();
    (available - sizes.iter().sum::<f64>() - gaps).abs() <= 0.01
        && sizes.iter().any(|size| (size-number).abs() <= 0.01)
}
