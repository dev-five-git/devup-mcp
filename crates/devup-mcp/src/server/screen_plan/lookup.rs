fn audit_plan_location(input: &ScreenPlanInput) -> Result<(PathBuf, Value), DevupError> {
    let requested = dunce::canonicalize(require_root(input)?)
        .map_err(|error| invalid(format!("Cannot resolve projectRoot: {error}")))?;
    let mut candidates = Vec::new();
    let mut superseded = None;
    for root in requested.ancestors() {
        let Some(plan) = read_plan(&root.to_string_lossy())? else { continue; };
        let matches_id = input.plan_id.as_deref().is_none_or(|id| plan["planId"] == id);
        let matches_app = root == requested || array(&plan["routes"]).iter().any(|route| {
            route["app"].as_str().is_some_and(|app| {
                dunce::canonicalize(root.join(app)).ok().as_ref() == Some(&requested)
            })
        });
        if matches_id && (input.plan_id.is_some() || matches_app) {
            return Ok((root.to_path_buf(), plan));
        }
        if superseded.is_none()
            && let Some(id) = input.plan_id.as_deref()
            && array(&plan["previousPlanIds"]).iter().any(|previous| previous == id)
        {
            superseded = Some(plan["planId"].clone());
        }
        candidates.push(json!({"projectRoot":root,"planId":plan["planId"],"status":plan["status"]}));
    }
    let Some(current) = candidates.first() else {
        return Err(DevupError::with_details(
            ErrorCode::DevupInvalidInput,
            "No screen plan exists in this project; run devup_screen_plan action draft first.",
            false,
            json!({"existingPlans":candidates}),
        ));
    };
    let Some(requested_id) = input.plan_id.as_deref() else {
        return Err(DevupError::with_details(
            ErrorCode::DevupInvalidInput,
            "No ancestor screen plan belongs to this projectRoot or app; use one of the existing plan roots.",
            false,
            json!({"existingPlans":candidates}),
        ));
    };
    // The nearest plan is the one the caller most likely meant: say what it
    // is and what to call next, instead of only that the id did not match.
    let (current_id, status) = (text(&current["planId"]), text(&current["status"]));
    let next_action = if status == "confirmed" {
        json!({"tool":"devup_screen_plan","arguments":{"action":"status","projectRoot":current["projectRoot"],"planId":current_id},
            "how":"The plan is already confirmed; read its status and continue implementation. Do not confirm again."})
    } else {
        json!({"tool":"devup_screen_plan","arguments":{"action":"confirm","projectRoot":current["projectRoot"],"planId":current_id,"acceptRecommended":true},
            "how":"Show the user the current draft's proposal table first; its questions may differ from the ones answered for the old id."})
    };
    let message = match &superseded {
        Some(by) => format!(
            "planId {requested_id} was superseded by a later draft; the plan at {} is now {} ({status}).",
            current["projectRoot"].as_str().unwrap_or_default(), text(by)
        ),
        None => format!(
            "planId {requested_id} is not the plan of this project or any ancestor; the plan at {} is {current_id} ({status}).",
            current["projectRoot"].as_str().unwrap_or_default()
        ),
    };
    Err(DevupError::with_details(
        ErrorCode::DevupInvalidInput,
        message,
        false,
        json!({"existingPlans":candidates,"requestedPlanId":requested_id,"supersededBy":superseded,"nextAction":next_action}),
    ))
}
