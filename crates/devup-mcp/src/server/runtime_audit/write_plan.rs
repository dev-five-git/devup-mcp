
fn write_plan(
    input: PlanInput,
    policy: &OutputPolicy,
    output_path: Option<&str>,
    template_path: Option<&str>,
    dimension_references: Vec<Value>,
) -> Result<Value, DevupError> {
    validate_plan(&input)?;
    if template_path.is_some() && output_path.is_none() {
        return Err(invalid(
            "templatePath requires outputPath for the bound plan JSON.",
        ));
    }
    let id = plan_id(&input)?;
    let references = input
        .screens
        .iter()
        .filter_map(|screen| {
            let figma = screen.figma.as_ref()?;
            let (comparable, layout_only): (Vec<_>, Vec<_>) = input
                .viewports
                .iter()
                .map(|viewport| viewport.width)
                .partition(|width| figma.drawn_widths.contains(width));
            Some(json!({"screen": screen.id, "nodeId": figma.node_id,
                "drawnWidths": figma.drawn_widths, "visualOriginalAt": comparable,
                "layoutChecksOnlyAt": layout_only}))
        })
        .collect::<Vec<_>>();
    let document = json!({
        "schemaVersion": SCHEMA_VERSION,
        "kind": PLAN_KIND,
        "planId": id,
        "createdBy": super::delivery::server_identity(),
        "plan": input,
        "expected": {
            "runs": input.viewports.len(),
            "screensPerRun": input.screens.len(),
            "checkItems": planned_items(&input),
            "dimensionRows": input.viewports.len() * input.screens.iter().map(|s| s.checks.iter().filter(|c| c.kind == "dimensions").count()).sum::<usize>(),
        },
        "limits": {"dimensionRows": MAX_DIMENSION_ROWS, "dimensionRowsOmitted": 0},
        "figmaReferences": references,
        "dimensionReferences": dimension_references,
        "executor": {
            "package": "harness/runtime-audit",
            "install": "npm ci && npx playwright install chromium",
            "run": "node run.mjs --plan <plan.json> --out <report directory>",
            "isolation": "One fresh browser context per viewport, with no profile, cookies or storage from the user's browser. Requests to origins outside allowedOrigins are blocked and recorded.",
        },
    });
    let mut written = None;
    let mut template_files = Vec::new();
    let mut transaction = OutputTransaction::new();
    if let Some(directory) = template_path {
        transaction.refuse_existing();
        for (name, bytes) in [
            ("resource-checks.mjs", include_bytes!("../../../../../harness/runtime-audit/resource-checks.mjs").as_slice()),
            (
                "run.mjs",
                include_bytes!("../../../../../harness/runtime-audit/run.mjs").as_slice(),
            ),
            (
                "checks.mjs",
                include_bytes!("../../../../../harness/runtime-audit/checks.mjs").as_slice(),
            ),
            (
                "strip-figma-node-ids.mjs",
                include_bytes!("../../../../../harness/runtime-audit/strip-figma-node-ids.mjs")
                    .as_slice(),
            ),
            (
                "package.json",
                include_bytes!("../../../../../harness/runtime-audit/package.json").as_slice(),
            ),
            (
                "package-lock.json",
                include_bytes!("../../../../../harness/runtime-audit/package-lock.json").as_slice(),
            ),
        ] {
            let requested = Path::new(directory).join(name);
            let target = policy.resolve(&requested.to_string_lossy())?;
            template_files.push(json!({"path": target.display_path().display().to_string(), "sha256": sha256(bytes)}));
            transaction.stage(name, target, bytes)?;
        }
    }
    if let Some(path) = output_path {
        let target = policy.resolve(path)?;
        let display = target.display_path().display().to_string();
        let bytes = serde_json::to_vec_pretty(&document)
            .map_err(|error| invalid(format!("Cannot serialize the plan: {error}")))?;
        if bytes.len() > MAX_PLAN_BYTES {
            return Err(invalid(
                "The bound plan exceeds the ingest size limit; split the plan.",
            ));
        }
        transaction.stage("plan", target, &bytes)?;
        written = Some(display);
    }
    transaction.commit()?;
    let mut result = document;
    result["writtenTo"] = json!(written);
    result["template"] = json!({"files": template_files, "executed": false,
        "workingDirectory": template_path,
        "commands": [ ["npm", "ci"], ["npx", "playwright", "install", "chromium"],
            ["node", "run.mjs", "--plan", written.as_deref().unwrap_or("<saved plan.json>"), "--out", "report"] ],
        "note": include_str!("../text/runtime_audit-write_plan-2d2cc624.md")});
    result["nextAction"] = json!({
        "where": "the consumer project",
        "how": include_str!("../text/runtime_audit-write_plan-c3ca4e3c.md"),
    });
    Ok(result)
}
fn read_json(
    policy: &OutputPolicy,
    path: &str,
    limit: usize,
) -> Result<(Vec<u8>, Value, std::path::PathBuf), DevupError> {
    let target = policy.resolve(path)?;
    let bytes = target.read_bounded(limit)?;
    let value = serde_json::from_slice(&bytes)
        .map_err(|error| invalid(format!("{path} is not valid JSON: {error}")))?;
    Ok((bytes, value, target.display_path().to_path_buf()))
}
fn rejected(reason: &str, details: Value) -> Value {
    json!({
        "verdict": "rejected",
        "reason": reason,
        "details": details,
        "note": "Nothing in this report was counted. A report that cannot be tied to the plan it claims to run is not evidence of that plan.",
    })
}
#[cfg(test)]
fn ingest(policy: &OutputPolicy, plan_path: &str, report_path: &str) -> Result<Value, DevupError> {
    ingest_references(policy, plan_path, report_path, None)
}
pub(crate) async fn ingest_with_artifacts(
    policy: &OutputPolicy,
    plan_path: &str,
    report_path: &str,
    artifacts: &ArtifactStore,
) -> Result<Value, DevupError> {
    let read_policy = policy.clone();
    let read_path = plan_path.to_owned();
    let (_, document, _) =
        tokio::task::spawn_blocking(move || read_json(&read_policy, &read_path, MAX_PLAN_BYTES))
            .await
            .map_err(|_| invalid("The plan could not be read to the end."))??;
    let references = match serde_json::from_value::<PlanInput>(document["plan"].clone()) {
        Ok(plan) if validate_plan(&plan).is_ok() => dimension_references(&plan, artifacts).await,
        Ok(_) => Vec::new(),
        Err(_) => Vec::new(),
    };
    let policy = policy.clone();
    let plan_path = plan_path.to_owned();
    let report_path = report_path.to_owned();
    tokio::task::spawn_blocking(move || {
        ingest_references(&policy, &plan_path, &report_path, Some(&references))
    })
    .await
    .map_err(|_| invalid("The report could not be read to the end."))?
}
fn ingest_references(
    policy: &OutputPolicy,
    plan_path: &str,
    report_path: &str,
    acquired: Option<&[Value]>,
) -> Result<Value, DevupError> {
    let (plan_bytes, plan_document, _) = read_json(policy, plan_path, MAX_PLAN_BYTES)?;
    if plan_document["kind"] != PLAN_KIND || plan_document["schemaVersion"] != SCHEMA_VERSION {
        return Ok(rejected(
            "plan-kind",
            json!({"expected": PLAN_KIND, "schemaVersion": SCHEMA_VERSION}),
        ));
    }
    let Ok(plan) = serde_json::from_value::<PlanInput>(plan_document["plan"].clone()) else {
        return Ok(rejected("plan-schema", json!({"path": plan_path})));
    };
    if let Err(error) = validate_plan(&plan) {
        return Ok(rejected("plan-invalid", json!({"message": error.message})));
    }
    let id = plan_id(&plan)?;
    if plan_document["planId"] != id.as_str() {
        return Ok(rejected(
            "plan-edited",
            json!({"declared": plan_document["planId"], "recomputed": id}),
        ));
    }
    let (_, report, report_file) = read_json(policy, report_path, MAX_REPORT_BYTES)?;
    if report["kind"] != REPORT_KIND || report["schemaVersion"] != SCHEMA_VERSION {
        return Ok(rejected(
            "report-kind",
            json!({"expected": REPORT_KIND, "schemaVersion": SCHEMA_VERSION}),
        ));
    }
    if report["planId"] != id.as_str() {
        return Ok(rejected(
            "report-for-another-plan",
            json!({"planId": id, "reportPlanId": report["planId"]}),
        ));
    }
    let plan_file_hash = sha256(&plan_bytes);
    let plan_file_matches = report["planSha256"]
        .as_str()
        .map(|declared| declared == plan_file_hash);
    if plan_file_matches == Some(false) {
        return Ok(rejected(
            "plan-file-differs",
            json!({"planSha256": plan_file_hash, "reportPlanSha256": report["planSha256"]}),
        ));
    }
    let report_dir = report_file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let mut counts = BTreeMap::<&str, usize>::new();
    let mut failures = Vec::new();
    let mut runs = Vec::new();
    let mut screenshot_budget = MAX_SCREENSHOTS;
    let mut integrity_problems = Vec::new();
    let mut environment_invalid = false;
    let mut visual_comparisons = Vec::new();
    let mut dimension_rows = Vec::new();
    let mut screen_rows = Vec::new();
    let mut findings = Vec::new();
    let mut missing_nodes = Vec::new();
    let mut link_table = Vec::<Value>::new();
    let mut links = BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    for viewport in &plan.viewports {
        let matching_runs = report["runs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|run| {
                run["viewport"]["width"] == viewport.width
                    && run["viewport"]["height"] == viewport.height
                    && run["viewport"]["deviceScaleFactor"].as_f64()
                        == Some(viewport.device_scale_factor)
            })
            .count();
        if matching_runs > 1 {
            return Ok(rejected(
                "ambiguous-report-items",
                json!({"viewport": viewport}),
            ));
        }
        let run = report["runs"].as_array().into_iter().flatten().find(|run| {
            run["viewport"]["width"] == viewport.width
                && run["viewport"]["height"] == viewport.height
                && run["viewport"]["deviceScaleFactor"]
                    .as_f64()
                    .is_some_and(|scale| (scale - viewport.device_scale_factor).abs() < 1e-9)
        });
        let environment_errors = run
            .map(|run| {
                run["environmentErrors"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        if run.is_none() || !environment_errors.is_empty() {
            environment_invalid = true;
        }
        let mut screenshots_verified = 0;
        let mut screenshot_issues = Vec::new();
        let mut drawn_width_shots = Vec::new();
        let mut captures = Vec::new();
        for screen in &plan.screens {
            if run.is_some_and(|run| {
                run["screens"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|s| s["id"] == screen.id)
                    .count()
                    > 1
            }) {
                return Ok(rejected(
                    "ambiguous-report-items",
                    json!({"screen": screen.id}),
                ));
            }
            let reported = run.and_then(|run| {
                run["screens"]
                    .as_array()?
                    .iter()
                    .find(|candidate| candidate["id"] == screen.id.as_str())
            });
            if let (Some(route), Some(found)) = (
                screen.route_path.as_ref(),
                reported.and_then(|s| s["links"].as_array()),
            ) {
                links.entry(route.clone()).or_default().extend(
                    found
                        .iter()
                        .filter_map(Value::as_str)
                        .filter(|href| href.starts_with('/') && href.len() <= MAX_TEXT)
                        .take(200)
                        .map(str::to_owned),
                );
            }
            if let Some(reported) = reported {
                for check in reported["checks"].as_array().into_iter().flatten().filter(|check| check["kind"] == "links-resolve") {
                    link_table.extend(check["measured"]["linkTable"].as_array().into_iter().flatten().cloned());
                }
            }
            let mut outcomes = Vec::new();
            let mut judged = Vec::new();
            for (index, check) in screen.checks.iter().enumerate() {
                if !applies(check, viewport.width) {
                    continue;
                }
                if reported.is_some_and(|s| {
                    s["checks"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|item| item["index"] == index)
                        .count()
                        > 1
                }) {
                    return Ok(rejected(
                        "ambiguous-report-items",
                        json!({"screen": screen.id, "check": index}),
                    ));
                }
                let item = reported.and_then(|screen| {
                    screen["checks"]
                        .as_array()?
                        .iter()
                        .find(|item| item["index"] == index)
                });
                let dimension = (check.kind == "dimensions").then(|| {
                    dimension_row(
                        screen,
                        index,
                        check,
                        viewport,
                        item,
                        &plan_document,
                        acquired,
                        plan_file_matches == Some(true),
                        run.is_some()
                            && environment_errors.is_empty()
                            && reported.is_some_and(|s| s["status"] == "completed"),
                    )
                });
                let status = match dimension
                    .as_ref()
                    .map(|row| &row["status"])
                    .or_else(|| item.map(|item| &item["status"]))
                    .and_then(Value::as_str)
                {
                    Some("pass") => "pass",
                    Some("fail") => "fail",
                    Some("inconclusive") => "inconclusive",
                    Some("skipped") => "skipped",
                    Some("not-run") => "not-run",
                    Some(_) => "inconclusive",
                    None => "not-run",
                };
                *counts.entry(status).or_default() += 1;
                if status == "fail" && failures.len() < MAX_LISTED {
                    failures.push(json!({
                        "viewport": {"width": viewport.width, "height": viewport.height},
                        "screen": screen.id,
                        "check": index,
                        "kind": check.kind,
                        "label": check.label,
                        "reason": dimension.as_ref().map(|row| &row["reason"]).or_else(|| item.map(|item| &item["reason"])),
                        "measured": dimension.as_ref().map(|row| &row["domCssPx"]).or_else(|| item.map(|item| &item["measured"])),
                    }));
                }
                outcomes.push((
                    status,
                    check.kind.as_str(),
                    dimension
                        .as_ref()
                        .map(|row| &row["reason"])
                        .or_else(|| item.map(|item| &item["reason"]))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                ));
                let reason = dimension
                    .as_ref()
                    .map(|row| &row["reason"])
                    .or_else(|| item.map(|item| &item["reason"]))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                judged.push((status, check));
                if status == "fail" {
                    findings.push(finding(
                        screen,
                        viewport,
                        Some((index, check)),
                        failure_class(&check.kind, reason),
                        reason,
                        dimension
                            .as_ref()
                            .map(|row| &row["domCssPx"])
                            .or_else(|| item.map(|item| &item["measured"]))
                            .unwrap_or(&Value::Null),
                        reported.and_then(|s| s["screenshot"]["path"].as_str()),
                    ));
                }
                if status == "inconclusive"
                    && reason.contains("matched no element")
                    && let Some(node_id) = &check.node_id
                {
                    missing_nodes.push(json!({"screen": screen.id, "width": viewport.width,
                        "nodeId": node_id, "check": index, "kind": check.kind}));
                }
                if check.kind == "capture"
                    && status == "pass"
                    && let (Some(reference), Some(shot)) = (
                        &check.reference,
                        item.map(|item| &item["screenshot"])
                            .filter(|shot| shot.is_object()),
                    )
                {
                    match verify_capture(policy, &report_dir, shot, None) {
                        Ok(verified) => captures.push((screen, index, check, reference, verified)),
                        Err(Integrity::Tampered(issue)) => {
                            integrity_problems.push(json!({"screen": screen.id, "issue": issue}));
                        }
                        Err(Integrity::Unverified(issue)) => {
                            screenshot_issues.push(json!({"screen": screen.id, "issue": issue}));
                        }
                    }
                }
                if let Some(row) = dimension {
                    dimension_rows.push(row);
                }
            }
            let mut row = screen_row(
                screen,
                viewport,
                run.is_some(),
                reported,
                &environment_errors,
                &outcomes,
            );
            require_completeness(&mut row, screen, viewport, &judged);
            row["columns"] = columns(screen, viewport, &judged, &row);
            // A screen that never rendered fails as a runtime error, not as
            // the checks that could not measure it.
            if row["status"] == "fail" && !outcomes.iter().any(|(status, _, _)| *status == "fail") {
                let reason = array_text(&row["reasons"]);
                findings.push(finding(
                    screen,
                    viewport,
                    None,
                    "runtime-error",
                    &reason,
                    &Value::Null,
                    None,
                ));
            }
            screen_rows.push(row);
            let Some(shot) = reported
                .map(|screen| &screen["screenshot"])
                .filter(|shot| shot.is_object())
            else {
                screenshot_issues.push(
                    json!({"screen": screen.id, "issue": "not-verified: screenshot missing"}),
                );
                continue;
            };
            if screenshot_budget == 0 {
                screenshot_issues.push(
                    json!({"screen": screen.id, "issue": "not-verified: screenshot limit reached"}),
                );
                continue;
            }
            screenshot_budget -= 1;
            match verify_screenshot(policy, &report_dir, shot, viewport) {
                Ok(verified) => {
                    screenshots_verified += 1;
                    if let Some(figma) = screen
                        .figma
                        .as_ref()
                        .filter(|figma| figma.drawn_widths.contains(&viewport.width))
                    {
                        drawn_width_shots.push((screen, figma.node_id.as_str(), verified));
                    }
                }
                Err(Integrity::Tampered(issue)) => {
                    integrity_problems.push(json!({"screen": screen.id, "issue": issue}));
                }
                Err(Integrity::Unverified(issue)) => {
                    screenshot_issues.push(json!({"screen": screen.id, "issue": issue}));
                }
            }
        }
        if !screenshot_issues.is_empty() {
            environment_invalid = true;
        }
        let run_valid =
            run.is_some() && environment_errors.is_empty() && screenshot_issues.is_empty();
        for (screen, node_id, shot) in &drawn_width_shots {
            visual_comparisons.push(visual_comparison(
                screen, node_id, viewport, shot, &report, run_valid,
            ));
        }
        for (screen, index, check, reference, shot) in &captures {
            visual_comparisons.push(capture_comparison(
                screen, *index, check, reference, viewport, shot,
            ));
        }
        runs.push(json!({
            "viewport": viewport,
            "reported": run.is_some(),
            "environmentErrors": environment_errors,
            "environmentValid": run.is_some() && environment_errors.is_empty(),
            "screenshots": {"verified": screenshots_verified, "issues": screenshot_issues},
        }));
    }
    if !integrity_problems.is_empty() {
        return Ok(rejected(
            "artifact-changed",
            json!({"artifacts": integrity_problems}),
        ));
    }
    let planned = planned_items(&plan);
    let count = |status: &str| counts.get(status).copied().unwrap_or(0);
    let verdict = if count("fail") > 0 && !environment_invalid {
        "fail"
    } else if count("pass") == planned && !environment_invalid && report["status"] == "completed" {
        "pass"
    } else {
        "inconclusive"
    };
    let declared = &report["summary"];
    let declared_matches = declared.is_object().then(|| {
        ["pass", "fail", "inconclusive", "skipped"]
            .iter()
            .all(|status| declared[*status].as_u64().unwrap_or(0) == count(status) as u64)
    });
    let figma = plan
        .screens
        .iter()
        .filter_map(|screen| {
            let figma = screen.figma.as_ref()?;
            let (visual, layout_only): (Vec<_>, Vec<_>) = plan
                .viewports
                .iter()
                .map(|viewport| viewport.width)
                .partition(|width| figma.drawn_widths.contains(width));
            Some(json!({"screen": screen.id, "nodeId": figma.node_id,
                "visualOriginalAt": visual, "layoutChecksOnlyAt": layout_only,
                "note": include_str!("../text/runtime_audit-ingest_references-72b6591d.md")}))
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "verdict": verdict,
        "planId": id,
        "integrity": {"planIdRecomputed": true, "planFileHashMatches": plan_file_matches},
        "coverage": {"plannedCheckItems": planned, "notRun": count("not-run"),
            "reportStatus": report["status"]},
        "summary": {"pass": count("pass"), "fail": count("fail"),
            "inconclusive": count("inconclusive"), "skipped": count("skipped"),
            "notRun": count("not-run")},
        "declaredSummaryMatches": declared_matches,
        "failures": failures,
        "runs": runs,
        "figmaReferences": figma,
        "visualComparisons": visual_comparisons,
        "findings": findings,
        "missingNodes": missing_nodes,
        "links": links,
        "linkTable": link_table,
        "screenTable": screen_rows,
        "widthTable": plan.viewports.iter().map(|viewport| json!({
            "width": viewport.width, "height": viewport.height,
            "deviceScaleFactor": viewport.device_scale_factor,
            "dimensions": dimension_rows.iter().filter(|row| row["viewport"] == json!(viewport)).collect::<Vec<_>>()
        })).collect::<Vec<_>>(),
        "toleranceExceedances": dimension_rows.iter().filter(|row| row["status"] == "fail").collect::<Vec<_>>(),
        "dimensionCoverage": {"plannedRows": dimension_rows.len(), "returnedRows": dimension_rows.len(),
            "omittedRows": 0, "limit": MAX_DIMENSION_ROWS},
        "executor": report["executor"],
        "environment": report["environment"],
        "evidence": {
            "independentlyVerified": [
                "the plan file's content id and SHA-256",
                "the report's binding to that plan",
                "every count above, recomputed from the reported items",
                "each screenshot's SHA-256, byte length, PNG decoding and width",
                "dimension arithmetic and per-check tolerance filtering from reported CSS geometry; available retained Figma reference dimensions are rechecked",
            ],
            "executorDeclared": [
                "element geometry, locator counts, visibility and non-dimension check outcomes",
                "the same-origin links found on each page",
                "page, console and network errors",
                "timing, browser and environment versions",
            ],
            "notJudged": "Whether the screens look right to a person. Layout checks measure the relations the plan names and nothing else.",
        },
    }))
}
/// One screen at one width, folded from its checks, its own record and the
/// environment errors the run could pin on it. This is the row a person reads
/// to learn whether `/about` works at 390; `failures` and `runs` say the same
/// things but spread across three lists.
///
/// A page error is attributed only when its message names this screen's route
/// (`navigation-failed` does). An uncaught script error carries no route, so
/// it leaves every screen at that width inconclusive rather than failing the
/// one that may not have thrown it.
fn screen_row(
    screen: &ScreenInput,
    viewport: &ViewportInput,
    run_reported: bool,
    reported: Option<&Value>,
    environment_errors: &[Value],
    outcomes: &[(&str, &str, String)],
) -> Value {
    let message = |error: &Value| error["message"].as_str().unwrap_or_default().to_owned();
    let own_navigation = environment_errors
        .iter()
        .filter(|error| {
            error["kind"] == "navigation-failed"
                && screen.route_path.as_deref().is_some_and(|route| {
                    error["message"]
                        .as_str()
                        .is_some_and(|text| text.starts_with(&format!("{route}:")))
                })
        })
        .map(|error| format!("page error: {}", message(error)))
        .collect::<Vec<_>>();
    let unattributed = environment_errors
        .iter()
        .filter(|error| error["kind"] == "page-error")
        .map(|error| format!("page error during this width's run: {}", message(error)))
        .collect::<Vec<_>>();
    let failed_checks = outcomes
        .iter()
        .filter(|(status, _, _)| *status == "fail")
        .map(|(_, kind, reason)| match *kind {
            "no-horizontal-overflow" => format!("horizontal overflow: {reason}"),
            "view-transition" => format!("view transition: {reason}"),
            kind => format!("{kind}: {reason}"),
        })
        .collect::<Vec<_>>();
    let record_status = reported.and_then(|screen| screen["status"].as_str());
    let record_reason = reported
        .and_then(|screen| screen["reason"].as_str())
        .unwrap_or_default();
    let (status, reasons) = if !run_reported || reported.is_none() {
        (
            "not-run",
            vec!["no report item for this screen at this width".to_owned()],
        )
    } else if !own_navigation.is_empty() {
        ("fail", own_navigation)
    } else if record_status == Some("failed") {
        ("fail", vec![format!("screen not reached: {record_reason}")])
    } else if !failed_checks.is_empty() {
        ("fail", failed_checks)
    } else if record_status == Some("skipped") {
        ("not-run", vec![format!("skipped: {record_reason}")])
    } else if !unattributed.is_empty() {
        ("inconclusive", unattributed)
    } else if outcomes.is_empty() {
        (
            "inconclusive",
            vec!["reached, but the plan names no checks for this screen".to_owned()],
        )
    } else if outcomes.iter().all(|(status, _, _)| *status == "pass") {
        ("pass", Vec::new())
    } else {
        (
            "inconclusive",
            outcomes
                .iter()
                .filter(|(status, _, _)| *status != "pass")
                .map(|(status, kind, reason)| format!("{kind} {status}: {reason}"))
                .collect(),
        )
    };
    json!({
        "screen": screen.id,
        "routePath": screen.route_path,
        "width": viewport.width,
        "status": status,
        "reasons": reasons,
        "screenshot": reported.and_then(|screen| screen["screenshot"]["path"].as_str()),
    })
}
#[allow(clippy::too_many_arguments)]
fn dimension_row(
    screen: &ScreenInput,
    index: usize,
    check: &CheckInput,
    viewport: &ViewportInput,
    item: Option<&Value>,
    document: &Value,
    acquired: Option<&[Value]>,
    hash_bound: bool,
    environment_valid: bool,
) -> Value {
    let matches = |reference: &&Value| {
        reference["screen"] == screen.id
            && reference["check"] == index
            && reference["viewportWidth"] == viewport.width
    };
    let reference = document["dimensionReferences"]
        .as_array()
        .into_iter()
        .flatten()
        .find(matches);
    let verified = acquired
        .and_then(|refs| refs.iter().find(matches))
        .filter(|fresh| {
            fresh["status"] == "acquired" && reference.is_some_and(|saved| *fresh == saved)
        });
    let measured = item.map(|item| &item["measured"]);
    // Report files are untrusted: expose only bounded identity and numeric
    // geometry, rather than echoing arbitrary objects attached to a check.
    let mapping = check
        .figma_dimensions
        .iter()
        .find(|r| r.viewport_width == viewport.width);
    let reference_summary = mapping.map(|mapping| {
        json!({
            "artifactId": mapping.artifact_id, "nodeId": mapping.node_id,
            "status": if verified.is_some() { "acquired" } else { "unverified" },
            "width": reference.and_then(|r| r["width"].as_f64()),
            "height": reference.and_then(|r| r["height"].as_f64()),
            "units": "figma-logical-px",
            "artifactContentHash": verified.and_then(|r| r["artifactContentHash"].as_str()),
            "staleness": "retained snapshot; not compared with live Figma",
        })
    });
    let dom = measured.map(|m| {
        json!({"x": m["subject"]["x"].as_f64(), "y": m["subject"]["y"].as_f64(),
        "width": m["subject"]["width"].as_f64(), "height": m["subject"]["height"].as_f64()})
    });
    let mut row = json!({"screen": screen.id, "check": index, "label": check.label,
        "viewport": viewport, "subject": check.subject, "mapping": "caller-declared",
        "reference": reference_summary, "referenceVerifiedAgainstRetainedAcquisition": verified.is_some(),
        "domCssPx": dom, "domEvidence": "executor-declared",
        "tolerancePx": check.tolerance_px, "deltaPx": null, "status": "inconclusive",
        "reason": "Design reference or reported CSS geometry is unverified."});
    let declared_status = item.and_then(|item| item["status"].as_str());
    if declared_status.is_none() {
        row["status"] = json!("not-run");
        row["reason"] = json!("No measurement was reported for this planned check.");
    } else if declared_status == Some("skipped") {
        row["status"] = json!("skipped");
        row["reason"] = json!("The executor skipped this measurement.");
    } else if reference.is_none() {
        row["reason"] = json!(
            "No acquired Figma dimensions were explicitly mapped at this width; no interpolation was applied."
        );
    } else if verified.is_none() {
        row["reason"] = json!(
            "The saved reference could not be matched to the retained acquisition; acquire and plan again."
        );
    } else if !hash_bound || !environment_valid {
        row["reason"] =
            json!("The plan file hash, screen completion or runtime environment is unverified.");
    } else if matches!(declared_status, Some("pass" | "fail")) {
        let m = measured.unwrap();
        if m["units"] == "css-px"
            && m["subjectCount"] == 1
            && m["subjectVisible"] == true
            && let (Some(width), Some(height)) = (
                m["subject"]["width"].as_f64(),
                m["subject"]["height"].as_f64(),
            )
            && width.is_finite()
            && height.is_finite()
            && width >= 0.0
            && height >= 0.0
        {
            let expected = verified.unwrap();
            let dw = width - expected["width"].as_f64().unwrap();
            let dh = height - expected["height"].as_f64().unwrap();
            let tolerance = check.tolerance_px.unwrap();
            row["deltaPx"] = json!({"width": dw, "height": dh});
            row["status"] = json!(if dw.abs() <= tolerance && dh.abs() <= tolerance {
                "pass"
            } else {
                "fail"
            });
            row["reason"] = json!(
                "Compared acquired logical dimensions with declared DOM CSS pixels without DPR or screenshot scaling."
            );
            row["executorOutcomeMatches"] = json!(row["status"] == item.unwrap()["status"]);
        }
    }
    row
}
enum Integrity {
    Tampered(String),
    Unverified(String),
}
struct VerifiedShot {
    path: String,
    sha256: String,
    width: u32,
    height: u32,
}
fn verify_screenshot(
    policy: &OutputPolicy,
    report_dir: &Path,
    shot: &Value,
    viewport: &ViewportInput,
) -> Result<VerifiedShot, Integrity> {
    verify_capture(policy, report_dir, shot, Some(viewport))
}

/// An element capture has the element's size, not the viewport's; only its
/// bytes and PNG decoding are checked.
fn verify_capture(
    policy: &OutputPolicy,
    report_dir: &Path,
    shot: &Value,
    viewport: Option<&ViewportInput>,
) -> Result<VerifiedShot, Integrity> {
    let path = shot["path"]
        .as_str()
        .ok_or_else(|| Integrity::Unverified("no path".into()))?;
    let relative = Path::new(path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(Integrity::Unverified(format!(
            "{path} must be relative to the report directory, without .. or a root"
        )));
    }
    let joined = report_dir.join(relative);
    let target = policy
        .resolve(&joined.to_string_lossy())
        .map_err(|error| Integrity::Unverified(format!("{path}: {}", error.message)))?;
    let bytes = target
        .read_bounded(MAX_SCREENSHOT_BYTES)
        .map_err(|error| Integrity::Unverified(format!("{path}: {}", error.message)))?;
    let digest = sha256(&bytes);
    if shot["sha256"].as_str() != Some(digest.as_str())
        || shot["bytes"].as_u64() != Some(bytes.len() as u64)
    {
        return Err(Integrity::Tampered(format!(
            "{path}: its SHA-256 or length differs from the report"
        )));
    }
    let [width, height] = devup_mcp_visual::png_dimensions(&bytes).map_err(|_| {
        Integrity::Unverified(format!(
            "{path}: not a decodable PNG within 8192px per side"
        ))
    })?;
    if let Some(viewport) = viewport {
        let expected = (f64::from(viewport.width) * viewport.device_scale_factor).round() as u32;
        let minimum = (f64::from(viewport.height) * viewport.device_scale_factor).round() as u32;
        if width != expected || height < minimum {
            return Err(Integrity::Unverified(format!(
                "{path}: {width}x{height} is not {expected} wide and at least {minimum} tall; the capture does not show this viewport"
            )));
        }
    }
    Ok(VerifiedShot {
        path: target.display_path().display().to_string(),
        sha256: digest,
        width,
        height,
    })
}

/// The one place a finding's identity is made, so a later run of the same
/// check at the same width finds its earlier attempts.
fn comparison_id(screen: &ScreenInput, kind: &str, subject: &str, width: u32) -> String {
    format!("{}|{kind}|{subject}|{width}", screen.id)
}

fn diff_path(actual: &str, id: &str) -> String {
    let directory = Path::new(actual).parent().unwrap_or(Path::new(""));
    let name = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    directory
        .join("diff")
        .join(format!("{name}.png"))
        .display()
        .to_string()
}

fn array_text(value: &Value) -> String {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("; ")
}

/// What kind of problem a failed check is, so a fix starts in the right
/// place: a page that does not render, content wider than its box, a box in
/// the wrong place or of the wrong size, a state or a transition missing.
pub(crate) fn failure_class(kind: &str, reason: &str) -> &'static str {
    match kind {
        "page-health" => "runtime-error",
        "image-loaded" | "stale-resources" => "asset-mismatch",
        "no-low-contrast-text" | "native-appearance" => "section-visual",
        "rendered-completeness" => "page-visual",
        "no-horizontal-overflow" | "no-clipped-text" => "overflow",
        "bounds" if reason.starts_with("position") => "layout-position",
        "bounds" | "dimensions" | "visible" => "layout-size",
        "contained" | "aligned" | "spans" => "layout-position",
        "interaction" => "interaction",
        "view-transition" => "view-transition",
        _ => "layout-position",
    }
}

/// A concrete first step for each class. The measured evidence names the
/// element where the executor reported one.
pub(crate) fn fix_hint(class: &str, kind: &str, measured: &Value) -> String {
    match class {
        "runtime-error" => include_str!("../text/runtime_audit-fix_hint-3a12f919.md").to_owned(),
        "overflow" if kind == "no-clipped-text" => {
            include_str!("../text/runtime_audit-fix_hint-b3941912.md").to_owned()
        }
        "overflow" => format!(
            include_str!("../text/runtime_audit-fix_hint-975acb57.md"),
            measured["widest"]
                .as_str()
                .map(|widest| format!(" (widest: {widest})"))
                .unwrap_or_default()
        ),
        "layout-position" | "layout-size" if kind == "bounds" => {
            include_str!("../text/runtime_audit-fix_hint-1f874fed.md").to_owned()
        }
        "layout-position" => include_str!("../text/runtime_audit-fix_hint-516a692f.md").to_owned(),
        "layout-size" => include_str!("../text/runtime_audit-fix_hint-1ca5b7eb.md").to_owned(),
        "code-drift" => include_str!("../text/runtime_audit-fix_hint-60ba4f27.md").to_owned(),
        "converter-bug" => include_str!("../text/runtime_audit-fix_hint-0e677049.md").to_owned(),
        "asset-mismatch" => include_str!("../text/runtime_audit-fix_hint-650b18d4.md").to_owned(),
        "section-visual" => include_str!("../text/runtime_audit-fix_hint-a13b2885.md").to_owned(),
        "page-visual" => include_str!("../text/runtime_audit-fix_hint-5960cebc.md").to_owned(),
        "interaction" => include_str!("../text/runtime_audit-fix_hint-e19dc0a1.md").to_owned(),
        "view-transition" => include_str!("../text/runtime_audit-fix_hint-7a003bba.md").to_owned(),
        _ => "Re-run the check and read its reason.".to_owned(),
    }
}

fn finding(
    screen: &ScreenInput,
    viewport: &ViewportInput,
    check: Option<(usize, &CheckInput)>,
    class: &str,
    reason: &str,
    measured: &Value,
    screenshot: Option<&str>,
) -> Value {
    let kind = check.map_or("screen", |(_, check)| check.kind.as_str());
    let subject = check
        .and_then(|(index, check)| {
            check
                .node_id
                .clone()
                .or_else(|| check.label.clone())
                .or_else(|| Some(format!("#{index}")))
        })
        .unwrap_or_else(|| "page".to_owned());
    json!({
        "id": comparison_id(screen, kind, &subject, viewport.width),
        "class": class,
        "screen": screen.id,
        "routePath": screen.route_path,
        "width": viewport.width,
        "check": check.map(|(index, _)| index),
        "kind": kind,
        "label": check.and_then(|(_, check)| check.label.clone()),
        "nodeId": check.and_then(|(_, check)| check.node_id.clone()),
        "codeFile": screen.component_path,
        "reason": reason,
        "fixHint": fix_hint(class, kind, measured),
        "evidence": {"screenshot": screenshot, "measured": measured},
    })
}