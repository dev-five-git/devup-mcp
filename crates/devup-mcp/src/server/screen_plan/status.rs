

/// A page file on disk is only written. It is implemented once the browser
/// rendered it complete (rendered-completeness passed) at every width its
/// frame was drawn at.
fn implemented_state(page_exists: bool, drawn: &Value, per_width: &[Value]) -> &'static str {
    if !page_exists {
        return "no";
    }
    let drawn = array(drawn)
        .iter()
        .filter_map(Value::as_u64)
        .collect::<Vec<_>>();
    let complete = !drawn.is_empty()
        && drawn.iter().all(|width| {
            per_width.iter().any(|row| {
                row["width"] == *width && row["result"] == "pass" && row["columns"]["completeness"] == "pass"
            })
        });
    if complete { "yes" } else { "written-unverified" }
}

/// The pure half of `status`, so the not-run rule is tested on its own.
pub(super) fn status_report(
    plan: &Value,
    evidence: &ProjectEvidence,
    audit: Option<&Value>,
) -> Value {
    let audit_rows = audit
        .map(|audit| array(&audit["screenTable"]))
        .unwrap_or_default();
    let mut widths = audit
        .map(|audit| {
            array(&audit["widthTable"])
                .iter()
                .filter_map(|row| row["width"].as_u64())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let audited = !widths.is_empty();
    let mut rows = Vec::new();
    let mut states = Vec::new();
    for route in array(&plan["routes"]) {
        if route["implementation"] == "state" {
            states.push(json!({"screen": route["screen"], "name": route["name"], "hostUrl": route["hostUrl"]}));
            continue;
        }
        let Some(url) = route["url"].as_str() else {
            continue;
        };
        let page = evidence
            .routes
            .iter()
            .find(|(pattern, _)| pattern == url || super::navigation::matches_route(pattern, url))
            .map(|(_, page)| page.clone());
        let mut row_widths = DEFAULT_WIDTHS.iter().map(|width| u64::from(*width))
            .chain(widths.iter().copied())
            .chain(array(&route["drawnWidths"]).iter().filter_map(Value::as_u64))
            .collect::<Vec<_>>();
        row_widths.sort_unstable_by(|a, b| b.cmp(a));
        row_widths.dedup();
        let per_width = row_widths
            .iter()
            .map(|width| {
                let matching = audit_rows
                    .iter()
                    .filter(|row| {
                        row["width"] == *width
                            && row["routePath"]
                                .as_str()
                                .and_then(normalize_route)
                                .is_some_and(|path| path == url)
                    })
                    .collect::<Vec<_>>();
                if matching.is_empty() {
                    return json!({"width": width, "result": "not-run",
                        "reasons": [if audited { "no runtime audit item for this URL at this width" } else { "no runtime audit report supplied" }]});
                }
                let mut result = matching
                    .iter()
                    .fold("pass", |acc, row| worst(acc, text(&row["status"])));
                let mut reasons = matching.iter().flat_map(|row| array(&row["reasons"]).to_vec()).collect::<Vec<_>>();
                let missing_reference = array(&plan["audit"]["plans"]).iter().filter(|entry| entry["url"] == url)
                    .flat_map(|entry| array(&entry["missingReferences"]))
                    .any(|reference| reference["width"].is_null() || reference["width"] == *width);
                let pending_visual = array(&plan["audit"]["visualPending"]).iter()
                    .any(|comparison| comparison["routePath"] == url && comparison["viewport"]["width"] == *width);
                if missing_reference || pending_visual {
                    if result == "pass" { result = "not-run"; }
                    reasons.push(json!("Figma page, section or asset visual comparison missing, blocked or not yet judged"));
                }
                json!({
                    "width": width,
                    "result": result,
                    "columns": matching[0]["columns"],
                    "reasons": reasons,
                    "screenshots": matching.iter().filter_map(|row| row["screenshot"].as_str()).collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>();
        rows.push(json!({
            "url": url,
            "screen": route["screen"],
            "figmaScreen": route["name"],
            "nodeIds": route["nodeIds"],
            "file": page.clone().map_or_else(|| route["file"].clone(), Value::from),
            "implemented": implemented_state(page.is_some(), &route["drawnWidths"], &per_width),
            "widths": per_width,
        }));
    }
    if !audited {
        widths = DEFAULT_WIDTHS
            .iter()
            .map(|width| u64::from(*width))
            .collect();
    }
    let implemented = rows
        .iter()
        .filter(|row| row["implemented"] == "yes")
        .count();
    let verified = rows
        .iter()
        .filter(|row| {
            array(&row["widths"])
                .iter()
                .all(|width| width["result"] == "pass")
        })
        .count();
    let mut problems = Vec::new();
    for row in &rows {
        let mut by_kind = BTreeMap::<&str, Vec<String>>::new();
        for width in array(&row["widths"]) {
            if width["result"] != "fail" {
                continue;
            }
            let reasons = array(&width["reasons"])
                .iter()
                .map(text)
                .collect::<Vec<_>>();
            let kind = if reasons
                .iter()
                .any(|reason| reason.starts_with("horizontal overflow"))
            {
                "overflows"
            } else if reasons.iter().any(|reason| {
                reason.starts_with("page error") || reason.starts_with("screen not reached")
            }) {
                "fails to load"
            } else {
                "fails checks"
            };
            by_kind
                .entry(kind)
                .or_default()
                .push(width["width"].to_string());
        }
        for (kind, at) in by_kind {
            problems.push(format!("{} {kind} at {}", text(&row["url"]), at.join(", ")));
        }
    }
    let written = rows
        .iter()
        .filter(|row| row["implemented"] == "written-unverified")
        .count();
    let mut summary = format!(
        "{} URLs: {implemented} implemented, {verified} verified at all widths",
        rows.len()
    );
    if written > 0 {
        summary.push_str(&format!(
            ", {written} written but not rendered complete at every drawn width"
        ));
    }
    for problem in &problems {
        summary.push_str(&format!(", {problem}"));
    }
    if !audited {
        summary.push_str("; no runtime audit report, so every width is not-run");
    }
    let assumptions = array(&plan["assumptions"]);
    if !assumptions.is_empty() {
        summary.push_str(&format!(
            "; {} assumption{} made during implementation",
            assumptions.len(),
            if assumptions.len() == 1 { "" } else { "s" }
        ));
    }
    let design_fixes = array(&plan["designFixes"]);
    if !design_fixes.is_empty() {
        summary.push_str(&format!(
            "; {} design fix{} requested, not built",
            design_fixes.len(),
            if design_fixes.len() == 1 { "" } else { "es" }
        ));
    }
    let excluded = array(&plan["excluded"])
        .iter()
        .filter(|row| row["decision"] == "excluded")
        .cloned()
        .collect::<Vec<_>>();
    if !excluded.is_empty() {
        summary.push_str(&format!("; {} frames excluded, not built", excluded.len()));
    }
    let mut table = String::from(
        "| URL | Figma screen | file | status | widths verified | screenshots |\n|---|---|---|---|---|---|\n",
    );
    for row in &rows {
        let row_widths = array(&row["widths"]);
        let passed = row_widths
            .iter()
            .filter(|width| width["result"] == "pass")
            .map(|width| width["width"].to_string())
            .collect::<Vec<_>>();
        let failing = row_widths
            .iter()
            .filter(|width| width["result"] == "fail")
            .map(|width| width["width"].to_string())
            .collect::<Vec<_>>();
        let state = if row["implemented"] == "no" {
            "not implemented".to_owned()
        } else if !failing.is_empty() {
            format!("fails at {}", failing.join(", "))
        } else if row["implemented"] == "written-unverified" {
            "written, not rendered complete".to_owned()
        } else if passed.len() == row_widths.len() {
            "verified".to_owned()
        } else {
            "implemented, not verified".to_owned()
        };
        let shots = row_widths
            .iter()
            .flat_map(|width| array(&width["screenshots"]).to_vec())
            .collect::<Vec<_>>();
        table.push_str(&format!(
            "| {} | {} | {} | {state} | {} of {} | {} |\n",
            cell(&row["url"]),
            cell(&row["figmaScreen"]),
            cell(&row["file"]),
            if passed.is_empty() {
                "none".to_owned()
            } else {
                passed.join(", ")
            },
            row_widths
                .iter()
                .map(|width| width["width"].to_string())
                .collect::<Vec<_>>()
                .join("/"),
            cell(&json!(shots)),
        ));
    }
    let completeness = site_completeness(plan, evidence);
    let incomplete = completeness
        .iter()
        .filter(|item| item["state"] == "missing")
        .count();
    if incomplete > 0 {
        summary.push_str(&format!("; site completeness: {incomplete} missing"));
    }
    table.push_str("\nSite completeness:\n");
    for item in &completeness {
        table.push_str(&format!(
            "- {}: {} ({})\n",
            text(&item["item"]),
            text(&item["state"]),
            cell(&item["detail"])
        ));
    }
    let trailing = trailing_lists(plan);
    table.push_str(&trailing);
    // Visual failures and fixed findings change a width's verdict after the
    // run: a failing comparison fails its width, and the columns say which
    // level failed.
    let open = super::audit_plan::findings_by_status(plan, "open");
    let unresolved = super::audit_plan::findings_by_status(plan, "unresolved");
    for row in &mut rows {
        let url = text(&row["url"]).to_owned();
        for width in row["widths"].as_array_mut().into_iter().flatten() {
            let at = width["width"].clone();
            for finding in open.iter().chain(&unresolved).filter(|finding| {
                finding["kind"] == "visual"
                    && finding["routePath"] == url.as_str()
                    && finding["width"] == at
            }) {
                width["result"] = json!("fail");
                let column = match text(&finding["class"]) {
                    "asset-mismatch" => "asset",
                    "section-visual" => "section",
                    _ => "page",
                };
                width["columns"][column] = json!("fail");
                if let Some(reasons) = width["reasons"].as_array_mut() {
                    reasons.push(json!(format!(
                        "{}: {}",
                        text(&finding["class"]),
                        text(&finding["reason"])
                    )));
                }
            }
        }
    }
    let link_table = array(&plan["audit"]["linkTable"]);
    let links_fail = link_table.iter().any(|row| row["status"] != "pass");
    let mut link_markdown = String::from("\n| from | label | href | status | planned? |\n|---|---|---|---|---|\n");
    for row in link_table {
        link_markdown.push_str(&format!("| {} | {} | {} | {} | {} |\n", cell(&row["from"]), cell(&row["label"]), cell(&row["href"]), cell(&row["status"]), cell(&row["planned"])));
    }
    let mut queue = super::audit_plan::work_queue(plan, &rows, &[]);
    if links_fail {
        for item in &mut queue { item["done"] = json!(false); }
    }
    let final_report = super::audit_plan::final_report(plan, &rows, &trailing) + &link_markdown;
    let converter_bugs = plan["audit"]["converterBugs"]
        .as_object()
        .map(|bugs| bugs.values().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    json!({
        "findings": open,
        "unresolved": unresolved,
        "converterBugs": converter_bugs,
        "workQueue": queue,
        "pendingDecisions": plan["pendingDecisions"],
        "finalReport": final_report,
        "linkTable": link_table,
        "linkTableMarkdown": link_markdown,
        "doneWhen": DONE_RULE,
        "status": "confirmed",
        "planId": plan["planId"],
        "summary": summary,
        "rows": rows,
        "states": states,
        "widths": widths,
        "audit": audit.map(|audit| json!({"verdict": audit["verdict"], "planId": audit["planId"], "reason": audit["reason"]})),
        "markdownTable": table,
        "assumptions": assumptions,
        "siteCompleteness": completeness,
        "defaults": plan["defaults"],
        "designFixes": design_fixes,
        "excluded": excluded,
        "frameInventory": plan["frameInventory"],
        "linkLabelCheck": plan["linkLabelCheck"],
        "inventoryNote": evidence.inventory_note,
        "rule": "A width without a runtime audit item is not-run, never pass. Implemented requires rendered completeness at every drawn width; done requires every URL at every required width to pass.",
    })
}

/// The lists every final report ends with: design fixes requested, frames
/// excluded, and assumptions made during implementation. Shared by the plan
/// file and `status`, so the two never disagree about what was not built.
fn trailing_lists(plan: &Value) -> String {
    let mut out = String::new();
    let defaults = array(&plan["defaults"]);
    if !defaults.is_empty() {
        out.push_str("\nFilled in by default (no question):\n");
        for default in defaults {
            out.push_str(&format!(
                "- {}: {}\n",
                text(&default["item"]),
                text(&default["decision"])
            ));
        }
    }
    let fixes = array(&plan["designFixes"]);
    if !fixes.is_empty() {
        out.push_str("\nDesign fixes requested (not built):\n");
        for fix in fixes {
            out.push_str(&format!("- {}\n", text(&fix["fix"])));
        }
    }
    let excluded = array(&plan["excluded"])
        .iter()
        .filter(|row| row["decision"] == "excluded" || row["decision"].is_null())
        .collect::<Vec<_>>();
    if !excluded.is_empty() {
        out.push_str("\nExcluded (never built):\n");
        for row in excluded {
            out.push_str(&format!(
                "- \"{}\" ({}): {}\n",
                text(&row["name"]),
                text(&row["nodeId"]),
                text(&row["reason"])
            ));
        }
    }
    let assumptions = array(&plan["assumptions"]);
    if !assumptions.is_empty() {
        out.push_str("\nAssumptions made during implementation:\n");
        for assumption in assumptions {
            out.push_str(&format!(
                "- {}: {}{}\n",
                text(&assumption["about"]),
                text(&assumption["decision"]),
                assumption["reason"]
                    .as_str()
                    .map(|reason| format!(" ({reason})"))
                    .unwrap_or_default()
            ));
        }
    }
    out
}

/// `app-icon` for `devup_ui_validate` with a projectRoot: an App Router app
/// with no icon or favicon. Read from disk here because the devup-ui checks
/// open no file.
pub(super) fn missing_app_icon(
    project_root: &str,
) -> Option<devup_mcp_devup_ui::ui_validate::Violation> {
    let root = Path::new(project_root);
    let app = ["src/app", "app"]
        .into_iter()
        .find(|app| join_relative(root, app).is_dir())?;
    let mut evidence = ProjectEvidence {
        app_dir: app.to_owned(),
        ..ProjectEvidence::default()
    };
    read_site_facts(root, &mut evidence);
    if !evidence.icon_files.is_empty() {
        return None;
    }
    let mut context = serde_json::Map::new();
    context.insert("appDir".into(), json!(app));
    Some(devup_mcp_devup_ui::ui_validate::Violation {
        rule: "app-icon",
        severity: devup_mcp_devup_ui::ui_validate::Severity::Warning,
        byte_range: [0, 0],
        context,
        message: format!("{app} has no icon.* or favicon, and public/ has no favicon either, so the browser tab shows a blank icon."),
        suggestion: Some("Add app/icon.svg (or favicon.ico) from a file the user provides or a Figma logo node exported as SVG; devup_screen_plan asks which. Never draw one.".into()),
    })
}

/// What the export gate decided.
pub(super) enum Gate {
    /// Not a page write, or no plan applies.
    Open,
    /// A confirmed plan covers the frame and names this route for pageScaffold.
    Route(String),
    /// Not written: the response to return instead.
    Blocked(Value),
}

/// The facts of one export call the gate decides on.
pub(super) struct ExportRequest<'a> {
    pub project_root: &'a str,
    pub url: Option<&'a str>,
    /// Frame ids, or the one node the url or artifact names when there are none.
    pub frames: Vec<String>,
    pub all_screens: bool,
    pub scaffold_write: bool,
    pub scaffold_route: Option<&'a str>,
    pub output_paths: Vec<&'a str>,
}

/// Whether a write lands in the App Router tree. Judged below the project
/// root, so a checkout that happens to sit in a folder named `app` is not one.
fn under_app(project_root: &str, path: &str) -> bool {
    let path = Path::new(path);
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .components()
        .any(|component| component.as_os_str() == "app")
}

fn plan_required(request: &ExportRequest<'_>, reason: &str, details: Value) -> Gate {
    let mut arguments = json!({"action": "draft", "projectRoot": request.project_root});
    if let Some(url) = request.url {
        arguments["url"] = json!(url);
    }
    Gate::Blocked(json!({
        "status": "plan_required",
        "written": false,
        "planRequired": {"reason": reason, "details": details},
        "nextAction": {"tool": "devup_screen_plan", "arguments": arguments,
            "how": "Draft the screen plan, ask every round of its questions back to back before implementing, confirm, then repeat this export. Nothing was collected or written."},
    }))
}

/// The soft gate in front of `devup_figma_export`. A call that would write a
/// page - a pageScaffold write, an outputPath under app/, or several screens
/// with outputPaths - needs a confirmed plan covering its frames. A single
/// component, or a preview that writes nothing, is never blocked.
pub(super) fn export_gate(request: &ExportRequest<'_>) -> Result<Gate, DevupError> {
    let several = request.all_screens || request.frames.len() > 1;
    let writes = request.scaffold_write || !request.output_paths.is_empty();
    let gated = request.scaffold_write
        || request
            .output_paths
            .iter()
            .any(|path| under_app(request.project_root, path))
        || (several && writes);
    if !gated && request.scaffold_route.is_none() {
        return Ok(Gate::Open);
    }
    let plan = match read_plan(request.project_root)? {
        Some(plan) if plan["status"] == "confirmed" => plan,
        Some(plan) if gated => {
            return Ok(plan_required(
                request,
                "The screen plan is a draft; its questions are unanswered.",
                json!({"planId": plan["planId"], "askInstruction": ASK_INSTRUCTION}),
            ));
        }
        None if gated => {
            return Ok(plan_required(
                request,
                "This call writes a page and the project has no confirmed screen plan.",
                json!({"planFile": PLAN_FILE}),
            ));
        }
        _ => return Ok(Gate::Open),
    };
    // Never built, whatever else the call says: not by this export, and not
    // by an assumption recorded during implementation.
    let refused = request
        .frames
        .iter()
        .filter_map(|frame| {
            let excluded = array(&plan["excluded"])
                .iter()
                .find(|row| row["nodeId"] == frame.as_str())
                .map(|row| format!("excluded: {}", text(&row["reason"])));
            let fix = array(&plan["designFixes"])
                .iter()
                .find(|fix| {
                    array(&fix["frames"])
                        .iter()
                        .any(|f| f["nodeId"] == frame.as_str())
                })
                .map(|fix| format!("design fix requested: {}", text(&fix["fix"])));
            excluded
                .or(fix)
                .map(|reason| json!({"frame": frame, "reason": reason}))
        })
        .collect::<Vec<_>>();
    if !refused.is_empty() && (gated || request.scaffold_route.is_some()) {
        return Ok(Gate::Blocked(json!({
            "status": "refused",
            "written": false,
            "refused": refused,
            "how": "Do not build these frames and do not record an assumption to build them. They are listed in devup_screen_plan status; continue with the rest of the plan.",
        })));
    }
    let routes = array(&plan["routes"]);
    let row_of = |frame: &str| {
        routes
            .iter()
            .find(|row| array(&row["nodeIds"]).iter().any(|id| id == frame))
    };
    if request.all_screens {
        let section = request
            .url
            .and_then(|url| FigmaTarget::parse(url).ok())
            .and_then(|target| target.node_id);
        let planned = plan["source"]["nodeId"].as_str();
        if section.is_some() && planned.is_some() && section.as_deref() != planned {
            return Ok(plan_required(
                request,
                "allScreens names a Section the confirmed plan was not drafted from.",
                json!({"requested": section, "planned": planned}),
            ));
        }
        return Ok(Gate::Open);
    } else if request.frames.is_empty() {
        if gated {
            return Ok(plan_required(
                request,
                "The frames this call writes are not named, so the plan cannot be checked. Pass frameIds or a frame url.",
                json!({}),
            ));
        }
        return Ok(Gate::Open);
    }
    let uncovered = request
        .frames
        .iter()
        .filter(|frame| {
            row_of(frame)
                .is_none_or(|row| !matches!(text(&row["implementation"]), "page" | "state"))
        })
        .cloned()
        .collect::<Vec<_>>();
    if !uncovered.is_empty() {
        if !gated {
            return Ok(Gate::Open);
        }
        // The plan is confirmed, so this is implementation: nobody is asked
        // now. The conservative option is to leave the frame unbuilt.
        return Ok(Gate::Blocked(json!({
            "status": "plan_required",
            "written": false,
            "planRequired": {"reason": "The confirmed screen plan does not place these frames on a page.",
                "details": {"uncoveredFrames": uncovered, "planId": plan["planId"]}},
            "nextAction": {"tool": "devup_screen_plan",
                "arguments": {"action": "assume", "projectRoot": request.project_root,
                    "assumptions": [{"about": uncovered.join(", "), "decision": "Not built: outside the confirmed screen plan."}]},
                "how": "Do not ask the user during implementation. Leave these frames unbuilt, record that with action assume, keep going, and list it in the final report."},
        })));
    }
    let Some(requested) = request.scaffold_route else {
        return Ok(Gate::Open);
    };
    let [frame] = request.frames.as_slice() else {
        return Ok(Gate::Open);
    };
    let Some(row) = row_of(frame) else {
        return Ok(Gate::Open);
    };
    let Some(planned) = row["url"].as_str() else {
        return Err(DevupError::with_details(
            ErrorCode::DevupInvalidInput,
            format!(
                "The confirmed plan builds {frame} as a state of another page, not as its own route; export it without pageScaffold."
            ),
            false,
            json!({"planId": plan["planId"], "hostUrl": row["hostUrl"]}),
        ));
    };
    if !requested.is_empty() && normalize_route(requested).as_deref() != Some(planned) {
        return Err(DevupError::with_details(
            ErrorCode::DevupInvalidInput,
            format!(
                "pageScaffold.route {requested} does not match the confirmed plan, which serves {frame} at {planned}."
            ),
            false,
            json!({"routeMismatch": {"requested": requested, "planned": planned, "frame": frame, "planId": plan["planId"]},
                "how": "Omit pageScaffold.route to take the plan's route, or redraft and confirm the plan if the URL should change."}),
        ));
    }
    Ok(Gate::Route(planned.to_owned()))
}

#[cfg(test)]
#[path = "../screen_plan_tests.rs"]
mod tests;
