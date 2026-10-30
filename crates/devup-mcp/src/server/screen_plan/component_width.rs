pub(super) fn component_width_mismatches(
    snapshot: &devup_mcp_figma::Snapshot,
    roots: &[String],
) -> Vec<Value> {
    let mut families = BTreeMap::<(String, String), BTreeMap<String, Vec<Value>>>::new();
    for screen in roots {
        let Some(root) = snapshot.nodes.get(screen) else { continue };
        let view = root.typed_view();
        let Some(width) = view.number("width").or_else(|| view.value("absoluteBoundingBox")?["width"].as_f64()) else { continue };
        let mut stack = view.child_ids().map(str::to_owned).collect::<Vec<_>>();
        let mut seen = BTreeSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id.clone()) { continue; }
            let Some(node) = snapshot.nodes.get(&id) else { continue };
            let view = node.typed_view();
            if view.bool("visible") == Some(false) { continue; }
            stack.extend(view.child_ids().map(str::to_owned));
            if node.node_type != "INSTANCE" { continue; }
            let (Some(name), Some(master)) = (view.name(), view.string("mainComponentId")) else { continue };
            let family = view.string("mainComponentSetId").or_else(|| view.string("mainComponentName"));
            let Some(family) = family else { continue };
            families.entry((name.to_lowercase(), family.into())).or_default()
                .entry(width.to_string()).or_default().push(json!({"screen":screen,"nodeId":id,"master":master,"width":width}));
        }
    }
    let mut result = Vec::new();
    for ((role, family), widths) in families {
        let majority = |members: &[Value]| -> Option<(String, usize, usize)> {
            let screens = members.iter().filter_map(|member| member["screen"].as_str()).collect::<BTreeSet<_>>();
            let mut counts = BTreeMap::<String, BTreeSet<String>>::new();
            for member in members {
                if let (Some(master), Some(screen)) = (member["master"].as_str(), member["screen"].as_str()) {
                    counts.entry(master.into()).or_default().insert(screen.into());
                }
            }
            counts.into_iter().find_map(|(master, count)| (count.len() > screens.len()/2).then_some((master,count.len(),screens.len())))
        };
        for (width, members) in &widths {
            let Some((expected, count, total)) = majority(members) else { continue };
            for suspect in members.iter().filter(|member| member["master"] != expected) {
                let other = widths.iter().filter(|(other, _)| *other != width).find_map(|(other, members)| {
                    let (master,count,total) = majority(members)?;
                    (suspect["master"] == master).then(|| json!({"width":other,"master":master,"count":count,"total":total}))
                });
                let Some(other) = other else { continue };
                let correct = members.iter().filter(|member| member["master"] == expected).min_by_key(|member| text(&member["nodeId"]));
                let Some(correct) = correct else { continue };
                result.push(json!({"code":"DEVUP_DESIGN_COMPONENT_WIDTH_MISMATCH","role":role,"family":family,
                    "screen":suspect["screen"],"width":suspect["width"],"nodeId":suspect["nodeId"],
                    "usedMaster":suspect["master"],"expectedMaster":expected,"correctNodeId":correct["nodeId"],
                    "expectedCount":count,"usedCount":members.iter().filter(|member|member["master"] == suspect["master"]).filter_map(|member|member["screen"].as_str()).collect::<BTreeSet<_>>().len(),"totalCount":total,"otherWidth":other,"render":"unavailable"}));
            }
        }
    }
    result
}

pub(super) fn apply_component_width_mismatches(plan: &mut Value, mismatches: &[Value]) {
    let mut assumptions = array(&plan["assumptions"]).to_vec();
    for mismatch in mismatches {
        let decision = format!("use {} ({})",text(&mismatch["expectedMaster"]),text(&mismatch["correctNodeId"]));
        let assumption = json!({"about":mismatch["nodeId"],"decision":decision,
            "reason":format!("{}/{} screens at {} use {}; used master is the majority at another width; render={}. Implement one responsive component, not a second variant.",mismatch["expectedCount"],mismatch["totalCount"],mismatch["width"],text(&mismatch["expectedMaster"]),text(&mismatch["render"])),
            "evidence":mismatch,"recordedAt":now()});
        if let Some(existing) = assumptions.iter_mut().find(|row| row["about"] == mismatch["nodeId"] && row["evidence"]["code"] == mismatch["code"]) {
            *existing = assumption;
        } else { assumptions.push(assumption); }
    }
    plan["assumptions"] = json!(assumptions);
    plan["componentWidthMismatches"] = json!(mismatches);
    for screen in plan["screens"].as_array_mut().into_iter().flatten() {
        let corrections = mismatches.iter().filter(|row| array(&screen["frames"]).iter().any(|frame|frame["nodeId"] == row["screen"])).cloned().collect::<Vec<_>>();
        screen["componentSubstitutions"] = json!(corrections);
    }
}

pub(super) fn attach_component_width_plan(result: &mut Value, root: &str) -> Result<(), DevupError> {
    let Some(plan) = read_plan(root)? else { return Ok(()) };
    let mismatches = array(&plan["componentWidthMismatches"]);
    if mismatches.is_empty() { return Ok(()) }
    result["componentWidthMismatches"] = json!(mismatches);
    result["componentWidthAssumptions"] = json!(array(&plan["assumptions"]).iter().filter(|row| row["evidence"]["code"] == "DEVUP_DESIGN_COMPONENT_WIDTH_MISMATCH").collect::<Vec<_>>());
    Ok(())
}
