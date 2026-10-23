fn audit_plan_location(input: &ScreenPlanInput) -> Result<(PathBuf, Value), DevupError> {
    let requested = dunce::canonicalize(require_root(input)?)
        .map_err(|error| invalid(format!("Cannot resolve projectRoot: {error}")))?;
    let mut candidates = Vec::new();
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
        candidates.push(json!({"projectRoot":root,"planId":plan["planId"],"status":plan["status"]}));
    }
    Err(DevupError::with_details(
        ErrorCode::DevupInvalidInput,
        if candidates.is_empty() { "No screen plan exists in this project; run devup_screen_plan action draft first." }
        else { "No ancestor screen plan matches the requested planId or app; use one of the existing plan roots." },
        false, json!({"existingPlans":candidates}),
    ))
}
