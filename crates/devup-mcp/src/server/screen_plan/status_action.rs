fn worst(a: &str, b: &str) -> &'static str {
    let rank = |status: &str| match status {
        "fail" => 3,
        "inconclusive" => 2,
        "not-run" => 1,
        _ => 0,
    };
    match rank(a).max(rank(b)) {
        3 => "fail",
        2 => "inconclusive",
        1 => "not-run",
        _ => "pass",
    }
}

/// `status`: the confirmed plan read against the routes on disk and, when
/// given, a runtime audit. A width without an audit item is `not-run`.
pub(super) async fn status(
    input: &ScreenPlanInput,
    policy: &OutputPolicy,
    artifacts: &ArtifactStore,
) -> Result<Value, DevupError> {
    let (plan_root, plan) = audit_plan_location(input)?;
    let root = plan_root.to_str().ok_or_else(|| invalid("The plan root is not UTF-8."))?;
    if plan["status"] != "confirmed" {
        return Ok(json!({
            "status": "not-confirmed",
            "componentWidthMismatches": plan["componentWidthMismatches"],
            "assumptions": plan["assumptions"],
            "planId": plan["planId"],
            "questionRounds": question_rounds(array(&plan["questions"]), per_round(&plan)),
            "questions": ask_shape(array(&plan["questions"])),
            "askInstruction": ASK_INSTRUCTION,
            "nextAction": {"tool": "devup_screen_plan", "arguments": {"action": "confirm", "projectRoot": root, "planId": plan["planId"]}, "how": ASK_INSTRUCTION},
        }));
    }
    let mut plan = plan;
    let audit = match (
        &input.audit_plan_path,
        &input.audit_report_path,
        &input.tooling_unavailable,
    ) {
        (Some(plan_path), Some(report_path), None) => Some((
            super::runtime_audit::ingest_with_artifacts(policy, plan_path, report_path, artifacts)
                .await?,
            sha256(&policy.resolve(report_path)?.read_bounded(8 * 1024 * 1024)?),
        )),
        (Some(plan_path), None, Some(reason)) => {
            let bytes = policy.resolve(plan_path)?.read_bounded(MAX_PLAN_BYTES)?;
            let audit_plan: Value = serde_json::from_slice(&bytes)
                .map_err(|_| invalid("auditPlanPath is not a runtime audit plan."))?;
            for screen in array(&audit_plan["plan"]["screens"]) {
                if let Some(route) = screen["routePath"].as_str() {
                    plan["audit"]["inconclusive"][route] = json!(reason);
                }
            }
            write_plan(policy, root, &plan)?;
            None
        }
        (None, None, None) => None,
        _ => {
            return Err(invalid(
                "status takes auditPlanPath with auditReportPath, or auditPlanPath with toolingUnavailable, or neither.",
            ));
        }
    };
    let evidence = project_evidence(root).await?;
    let mut audit = audit;
    if let Some((audit, _)) = &mut audit {
        super::audit_plan::classify_geometry(audit, root, &evidence.sources, policy);
    }
    let round = match &audit {
        Some((audit, report_sha)) if audit["verdict"] != "rejected" => {
            let routes = array(&audit["screenTable"])
                .iter()
                .filter_map(|row| row["routePath"].as_str())
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if let Some(inconclusive) = plan["audit"]["inconclusive"].as_object_mut() {
                inconclusive.retain(|route, _| !routes.contains(route));
            }
            // The links each page holds, for auditPlan's next pass over the
            // dynamic routes.
            for (route, hrefs) in audit["links"].as_object().into_iter().flatten() {
                plan["audit"]["links"][route] = hrefs.clone();
            }
            let mut link_table = array(&plan["audit"]["linkTable"]).iter()
                .filter(|row| !row["from"].as_str().and_then(|from| url::Url::parse(from).ok()).and_then(|url| normalize_route(url.path())).is_some_and(|route| routes.contains(&route)))
                .cloned().collect::<Vec<_>>();
            link_table.extend(array(&audit["linkTable"]).iter().cloned());
            plan["audit"]["linkTable"] = json!(link_table);
            let max = plan["audit"]["maxFixAttempts"]
                .as_u64()
                .and_then(|max| u32::try_from(max).ok())
                .or(evidence.config.max_fix_attempts)
                .unwrap_or(3);
            let round = super::audit_plan::record_round(
                &mut plan,
                audit,
                report_sha,
                &input.visual_results,
                &evidence.figma_nodes,
                max,
            );
            write_plan(policy, root, &plan)?;
            Some(round)
        }
        _ => None,
    };
    let mut report = status_report(&plan, &evidence, audit.as_ref().map(|(audit, _)| audit));
    if let Some((audit, _)) = &audit {
        report["visualComparisons"] = audit["visualComparisons"].clone();
        report["missingNodes"] = audit["missingNodes"].clone();
    }
    if let Some(round) = round {
        report["pendingComparisons"] = round["pendingComparisons"].clone();
        report["inconclusiveComparisons"] = round["inconclusiveComparisons"].clone();
    }
    Ok(report)
}
