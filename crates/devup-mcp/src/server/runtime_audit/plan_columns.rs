/// One screen at one width by what was checked: health (renders, no
/// overflow, no clipped text), geometry (size and position), interaction
/// (states and view transitions), and the three visual levels, which stay
/// "to compare" until devup_visual_compare results come back through
/// devup_screen_plan status.
fn columns(
    screen: &ScreenInput,
    viewport: &ViewportInput,
    judged: &[(&str, &CheckInput)],
    row: &Value,
) -> Value {
    let fold = |kinds: &[&str]| {
        let statuses = judged
            .iter()
            .filter(|(_, check)| kinds.contains(&check.kind.as_str()))
            .map(|(status, _)| *status)
            .collect::<Vec<_>>();
        if statuses.is_empty() {
            "-"
        } else if statuses.contains(&"fail") {
            "fail"
        } else if statuses.iter().all(|status| *status == "pass") {
            "pass"
        } else if statuses.contains(&"inconclusive") {
            "inconclusive"
        } else {
            "not-run"
        }
    };
    let unreached = row["reasons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|reason| reason.starts_with("page error") || reason.starts_with("screen not reached"));
    let health = if unreached {
        "fail"
    } else {
        fold(&[
            "links-resolve",
            "page-health",
            "no-horizontal-overflow",
            "no-clipped-content",
            "styles-applied",
            "no-clipped-text",
            "no-low-contrast-text",
            "image-loaded",
            "native-appearance",
            "stale-resources",
        ])
    };
    let captures = |scope: &str| {
        let count = judged
            .iter()
            .filter(|(status, check)| {
                *status == "pass"
                    && check
                        .reference
                        .as_ref()
                        .is_some_and(|reference| reference.scope == scope)
            })
            .count();
        let missing = judged
            .iter()
            .filter(|(status, check)| {
                *status != "pass"
                    && check
                        .reference
                        .as_ref()
                        .is_some_and(|reference| reference.scope == scope)
            })
            .count();
        match (count, missing) {
            (0, 0) => json!("-"),
            (_, 0) => json!(format!("{count} to compare")),
            _ => json!(format!("{count} to compare, {missing} not captured")),
        }
    };
    let drawn = screen
        .figma
        .as_ref()
        .is_some_and(|figma| figma.drawn_widths.contains(&viewport.width));
    let completeness = match fold(&["rendered-completeness"]) {
        "-" if drawn => "not-run",
        "-" => "not drawn",
        other => other,
    };
    json!({
        "health": health,
        "completeness": completeness,
        "geometry": fold(&["text-align", "sequence-order", "section-layout", "bounds", "dimensions", "contained", "aligned", "spans", "visible"]),
        "interaction": fold(&["interaction", "view-transition"]),
        "page": if drawn { json!(if row["screenshot"].is_string() { "1 to compare" } else { "not captured" }) } else { json!("not drawn") },
        "section": captures("section"),
        "asset": captures("asset"),
    })
}

/// A check limited to some widths does not exist at the others.
fn applies(check: &CheckInput, width: u32) -> bool {
    check.widths.is_empty() || check.widths.contains(&width)
}

fn planned_items(plan: &PlanInput) -> usize {
    plan.viewports
        .iter()
        .map(|viewport| {
            plan.screens
                .iter()
                .flat_map(|screen| &screen.checks)
                .filter(|check| applies(check, viewport.width))
                .count()
        })
        .sum()
}

/// A view-transition check clicks away from its screen, so every one moves
/// behind the screen's other checks, keeping their order otherwise. The
/// executor also returns to the screen's URL after it; together a plan
/// cannot measure the wrong page whatever order it was written in.
pub(crate) fn order_checks(input: &mut PlanInput) {
    for screen in &mut input.screens {
        screen
            .checks
            .sort_by_key(|check| check.kind == "view-transition");
    }
}
