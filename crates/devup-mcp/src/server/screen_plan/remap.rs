/// The distinct owning Figma pages of some frames, as `{id, name}` rows.
fn distinct_pages(frames: &[Value]) -> Vec<Value> {
    let mut seen = BTreeSet::new();
    frames
        .iter()
        .filter(|frame| !frame["pageName"].is_null() || !frame["pageId"].is_null())
        .filter(|frame| seen.insert((text(&frame["pageId"]).to_owned(), text(&frame["pageName"]).to_owned())))
        .map(|frame| json!({"id": frame["pageId"], "name": frame["pageName"]}))
        .collect()
}

/// Moves one planned frame onto `url` in the confirmed plan, so an assumption
/// that says "use this frame instead" changes the mapping instead of only
/// being written down. Excluded and design-fix frames are never remapped.
fn remap_frame(plan: &mut Value, node: &str, url: &str, remapped: &mut Vec<Value>) -> Result<(), DevupError> {
    let frame = array(&plan["screens"])
        .iter()
        .flat_map(|screen| array(&screen["frames"]).iter())
        .find(|frame| frame["nodeId"] == node)
        .cloned()
        .ok_or_else(|| invalid(format!("{node} is not a frame of this plan; navigationTargets in assume can only move planned frames. Redraft to add it.")))?;
    let fixed = array(&plan["excluded"]).iter().any(|row| row["nodeId"] == node)
        || array(&plan["designFixes"]).iter().any(|fix| array(&fix["frames"]).iter().any(|row| row["nodeId"] == node));
    if fixed {
        return Err(invalid(format!("{node} was excluded or sent back for a design fix; an assumption cannot build it.")));
    }
    let width = frame["width"].as_f64().map(|width| width.round() as i64);
    let app_dir = text(&plan["appDir"]).to_owned();
    let mut from = Vec::new();
    let routes = plan["routes"].as_array_mut().ok_or_else(|| invalid("The plan has no routes."))?;
    for route in routes.iter_mut() {
        let ids = array(&route["nodeIds"]).to_vec();
        if let Some(index) = ids.iter().position(|id| id == node) {
            if route["url"] == url {
                return Ok(());
            }
            from.push(route["url"].clone());
            if let Some(list) = route["nodeIds"].as_array_mut() { list.remove(index); }
            if let Some(list) = route["drawnWidths"].as_array_mut() && index < list.len() { list.remove(index); }
            if let Some(list) = route["pages"].as_array_mut() { list.clear(); }
        }
    }
    routes.retain(|route| !array(&route["nodeIds"]).is_empty());
    let pages = distinct_pages(std::slice::from_ref(&frame));
    if let Some(route) = routes.iter_mut().find(|route| route["url"] == url && route["implementation"] == "page") {
        if let Some(list) = route["nodeIds"].as_array_mut() { list.push(json!(node)); }
        if let Some(list) = route["drawnWidths"].as_array_mut() { list.push(json!(width)); }
        route["pages"] = json!(pages);
    } else {
        routes.push(json!({
            "url": url, "screen": node, "name": frame["name"], "nodeIds": [node],
            "drawnWidths": [width], "pages": pages, "implementation": "page", "hostScreen": null,
            "file": planned_page(url, &app_dir), "fileExists": false, "collisionPolicy": null,
            "data": null, "routeEvidence": "assumption remap (navigationTargets in assume)",
        }));
    }
    remapped.push(json!({"nodeId": node, "from": from, "to": url}));
    Ok(())
}

/// The Figma page the plan is built from: the page of the linked node when it
/// is one of the planned frames, otherwise the page most frames sit on.
fn confirmed_page(plan: &Value) -> Value {
    let frames = array(&plan["screens"])
        .iter()
        .flat_map(|screen| array(&screen["frames"]))
        .filter(|frame| !frame["pageName"].is_null() || !frame["pageId"].is_null())
        .collect::<Vec<_>>();
    let linked = frames.iter().find(|frame| frame["nodeId"] == plan["source"]["nodeId"]);
    let mut counts = BTreeMap::<(String, String), usize>::new();
    for frame in &frames {
        *counts
            .entry((text(&frame["pageId"]).to_owned(), text(&frame["pageName"]).to_owned()))
            .or_default() += 1;
    }
    let majority = counts.iter().max_by_key(|(_, count)| **count).map(|(key, _)| key.clone());
    match (linked, majority) {
        (Some(frame), _) => json!({"id": frame["pageId"], "name": frame["pageName"], "basis": "linked node"}),
        (None, Some((id, name))) => json!({"id": id, "name": name, "basis": "most frames"}),
        _ => Value::Null,
    }
}

/// A planned frame on a different Figma page from the confirmed one.
fn page_warnings(plan: &Value) -> Vec<String> {
    let page = &plan["confirmedPage"];
    if page.is_null() {
        return Vec::new();
    }
    let built = array(&plan["routes"])
        .iter()
        .flat_map(|route| array(&route["nodeIds"]))
        .map(|node| text(node).to_owned())
        .collect::<BTreeSet<_>>();
    array(&plan["screens"])
        .iter()
        .flat_map(|screen| array(&screen["frames"]))
        .filter(|frame| built.contains(text(&frame["nodeId"])))
        .filter(|frame| !frame["pageId"].is_null() && frame["pageId"] != page["id"])
        .map(|frame| {
            format!(
                "frame {} sits on Figma page {:?}, not the confirmed page {:?}",
                text(&frame["nodeId"]),
                text(&frame["pageName"]),
                text(&page["name"])
            )
        })
        .collect()
}