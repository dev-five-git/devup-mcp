const MAX_RESPONSIVE_GROUP_FRAMES: usize = 6;

fn responsive_route_widths(route: &Value) -> Result<BTreeMap<String, u64>, String> {
    let ids = array(&route["nodeIds"]);
    let widths = array(&route["drawnWidths"]);
    if ids.len() != widths.len() || ids.is_empty() {
        return Err(
            "The route has missing or unpaired frame IDs and widths; refresh its frame inventory."
                .into(),
        );
    }
    let mut result = BTreeMap::new();
    for (id, width) in ids.iter().zip(widths) {
        let Some(id) = id.as_str().filter(|id| !id.is_empty()) else {
            return Err("A route frame has no node ID.".into());
        };
        let Some(width) = width
            .as_f64()
            .filter(|width| width.is_finite() && *width >= 1.0 && *width <= f64::from(u32::MAX))
        else {
            return Err(format!("Frame {id} has no valid drawn width."));
        };
        if result.insert(id.to_owned(), width.round() as u64).is_some() {
            return Err(format!("Frame {id} appears more than once on the route."));
        }
    }
    Ok(result)
}

/// No inference from order: duplicate widths require explicit, evidenced groups.
pub(super) fn responsive_groups(route: &Value) -> Result<Vec<Value>, String> {
    let widths = responsive_route_widths(route)?;
    let explicit = array(&route["responsiveGroups"]);
    if explicit.is_empty() {
        if widths.values().copied().collect::<BTreeSet<_>>().len() != widths.len() {
            return Err("Several frames occupy the same drawn width. Read their actual state/content evidence and record baseline/state responsiveGroups with action assume; names or frame order alone do not select a winner.".into());
        }
        if widths.len() > MAX_RESPONSIVE_GROUP_FRAMES {
            return Err("This route exceeds the six-frame export limit; record executable responsiveGroups before exporting.".into());
        }
        if widths
            .values()
            .map(|width| devup_mcp_devup_ui::codegen::responsive::slot_of_width(*width as u32))
            .collect::<BTreeSet<_>>()
            .len()
            != widths.len()
        {
            return Err("Several drawn widths occupy the same responsive slot. Record separate evidenced baseline/state groups; one responsive export cannot represent two frames in one slot.".into());
        }
        return Ok(vec![
            json!({"id":"baseline", "kind":"baseline", "frameIds":route["nodeIds"], "drawnWidths":route["drawnWidths"], "reason":"One recorded frame at each distinct width."}),
        ]);
    }
    let mut seen = BTreeSet::new();
    let mut group_ids = BTreeSet::new();
    let mut groups = Vec::new();
    let mut baselines = 0;
    for group in explicit {
        let id = text(&group["id"]);
        if id.trim().is_empty() || id.len() > 100 || !group_ids.insert(id.to_owned()) {
            return Err(
                "Every responsive group needs a distinct non-empty id (at most 100 bytes).".into(),
            );
        }
        match text(&group["kind"]) {
            "baseline" => baselines += 1,
            "state" => {}
            _ => return Err(format!("Group {id} kind must be baseline or state.")),
        }
        if text(&group["reason"]).trim().is_empty() {
            return Err(format!("Group {id} needs source evidence in reason."));
        }
        let ids = array(&group["frameIds"]);
        if ids.is_empty() || ids.len() > MAX_RESPONSIVE_GROUP_FRAMES {
            return Err(format!("Group {id} must contain one to six frames."));
        }
        let mut drawn = BTreeSet::new();
        let mut slots = BTreeSet::new();
        let mut ordered = Vec::new();
        for node in ids {
            let node = text(node);
            let Some(width) = widths.get(node) else {
                return Err(format!(
                    "Group {id} contains frame {node} outside this route."
                ));
            };
            if !drawn.insert(*width) {
                return Err(format!(
                    "Group {id} repeats drawn width {width}; states cannot share a responsive slot."
                ));
            }
            let slot = devup_mcp_devup_ui::codegen::responsive::slot_of_width(*width as u32);
            if !slots.insert(slot) {
                return Err(format!(
                    "Group {id} repeats responsive slot {slot}; distinct widths in the same slot cannot be merged."
                ));
            }
            if !seen.insert(node.to_owned()) {
                return Err(format!(
                    "Frame {node} belongs to more than one responsive group."
                ));
            }
            ordered.push((*width, node.to_owned()));
        }
        ordered.sort();
        let mut group = group.clone();
        group["frameIds"] = json!(ordered.iter().map(|(_, node)| node).collect::<Vec<_>>());
        group["drawnWidths"] = json!(ordered.iter().map(|(width, _)| width).collect::<Vec<_>>());
        groups.push(group);
    }
    if baselines != 1 {
        return Err("A route needs exactly one baseline responsive group.".into());
    }
    let omitted = widths
        .keys()
        .filter(|id| !seen.contains(*id))
        .collect::<Vec<_>>();
    if !omitted.is_empty() {
        return Err(format!(
            "Responsive groups omit planned frames: {}.",
            omitted.into_iter().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    groups.sort_by_key(|group| group["kind"] != "baseline");
    Ok(groups)
}

fn apply_responsive_groups(
    plan: &mut Value,
    inputs: &[ResponsiveGroupInput],
) -> Result<Vec<Value>, DevupError> {
    let mut by_url = BTreeMap::<String, Vec<Value>>::new();
    for group in inputs {
        let url = normalize_route(&group.url)
            .filter(|url| valid_route(url))
            .ok_or_else(|| invalid(format!("Invalid responsive group URL {:?}.", group.url)))?;
        by_url.entry(url).or_default().push(json!(group));
    }
    let mut recorded = Vec::new();
    for (url, groups) in by_url {
        let route = plan["routes"]
            .as_array_mut()
            .and_then(|routes| {
                routes
                    .iter_mut()
                    .find(|route| route["url"] == url && route["implementation"] == "page")
            })
            .ok_or_else(|| invalid(format!("Responsive group URL {url} is not a planned page.")))?;
        let mut candidate = route.clone();
        candidate["responsiveGroups"] = json!(groups);
        let verified = responsive_groups(&candidate).map_err(invalid)?;
        route["responsiveGroups"] = json!(verified);
        recorded.push(json!({"about":url, "decision":"recorded explicit baseline/state responsive groups", "reason":"Pairing evidence is retained per group.","recordedAt":now(),"evidence":verified.iter().map(|group| json!({"id":group["id"],"frameIds":group["frameIds"],"evidence":group["reason"]})).collect::<Vec<_>>()}));
    }
    Ok(recorded)
}
