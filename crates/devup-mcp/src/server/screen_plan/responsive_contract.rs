pub(super) fn responsive_export_action(plan: &Value, route: &Value, root: &str) -> Value {
    let mut frames = array(&route["nodeIds"]).iter().enumerate().collect::<Vec<_>>();
    frames.sort_by(|(left, _), (right, _)| {
        array(&route["drawnWidths"]).get(*left).and_then(Value::as_f64).unwrap_or_default()
            .total_cmp(&array(&route["drawnWidths"]).get(*right).and_then(Value::as_f64).unwrap_or_default())
    });
    let mut arguments = json!({"projectRoot": root, "frameIds": frames.iter().map(|(_, id)| *id).collect::<Vec<_>>(), "outputs": ["responsiveTsx"]});
    if let Some(url) = plan["source"]["url"].as_str() {
        arguments["url"] = json!(url);
    }
    json!({"tool": "devup_figma_export", "arguments": arguments})
}

fn multi_width(route: &Value) -> bool {
    array(&route["drawnWidths"]).iter().map(Value::to_string).collect::<BTreeSet<_>>().len() > 1
        && array(&route["nodeIds"]).len() > 1 && route["implementation"] != "state"
}

pub(super) fn responsive_required(root: &str, frames: &[String], outputs: &[String]) -> Result<Option<Value>, DevupError> {
    let responsive = outputs.iter().any(|output| output == "responsiveTsx");
    if !outputs.iter().any(|output| matches!(output.as_str(), "tsx" | "componentTsx" | "pageScaffold" | "responsiveTsx")) {
        return Ok(None);
    }
    for ancestor in Path::new(root).ancestors() {
        let Some(plan) = read_plan(&ancestor.to_string_lossy())? else { continue; };
        if plan["status"] != "confirmed" { continue; }
        let route = array(&plan["routes"]).iter().find(|route| multi_width(route) && frames.iter().any(|id| {
            plan["source"]["nodeId"] == id.as_str() || array(&route["nodeIds"]).contains(&json!(id)) || array(&plan["frameInventory"]["frames"]).iter().any(|frame| {
                frame["nodeId"] == id.as_str() && array(&frame["ancestors"]).iter().any(|parent| array(&route["nodeIds"]).contains(parent))
            })
        }));
        if let Some(route) = route {
            if responsive && !outputs.iter().any(|output| matches!(output.as_str(), "tsx" | "componentTsx"))
                && array(&route["nodeIds"]).iter().all(|id| frames.iter().any(|frame| id == frame)) {
                return Ok(None);
            }
            return Ok(Some(json!({"status": "responsive_required", "written": false,
                "responsiveRequired": {"code": "DEVUP_RESPONSIVE_REQUIRED", "url": route["url"], "frameIds": route["nodeIds"],
                    "reason": "This confirmed URL has multiple drawn widths. responsiveTsx is the deliverable; never start from single-width TSX or change generated layout values."},
                "nextAction": responsive_export_action(&plan, route, root)})));
        }
        break;
    }
    Ok(None)
}

fn responsive_status(plan: &Value, route: &Value, evidence: &ProjectEvidence, records: &[Value]) -> Value {
    let ids = array(&route["nodeIds"]);
    let matching = records.iter().filter(|record| array(&record["frameIds"]).iter().any(|id| ids.contains(id))
        || ids.contains(&record["rootNodeId"])).collect::<Vec<_>>();
    let responsive = matching.iter().find(|record| record["output"] == "responsiveTsx"
        && ids.iter().all(|id| array(&record["frameIds"]).contains(id)));
    let sources = evidence.route_sources.get(text(&route["url"])).cloned().unwrap_or_default();
    json!({"required": multi_width(route), "recorded": !matching.is_empty(), "responsive": responsive.is_some(),
        "issue": if multi_width(route) && responsive.is_none() && !matching.is_empty() { Some("single-width export used for a multi-width URL") } else { None },
        "nextAction": if multi_width(route) { responsive_export_action(plan, route, &evidence.root.to_string_lossy()) } else { Value::Null },
        "generatedDrift": responsive.map(|record| super::export_record::generated_drift(record, &evidence.root, &sources))})
}
