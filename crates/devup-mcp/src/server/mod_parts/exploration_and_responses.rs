/// Cuts the ranked candidate list to `limit` and reports the two truncations
/// apart.
///
/// `limit` is applied here rather than inside the projection so that it stays
/// a property of the answer alone: the projection is always asked for the same
/// ceiling, so no choice of `limit` can change how much of the design was read.
/// `truncated` arrives meaning "the snapshot itself was incomplete" and is
/// widened back to "something was cut" only after the two are recorded apart.
fn apply_explore_limit(mut response: Value, limit: usize) -> Value {
    let Some(Value::Array(mut candidates)) = response.get_mut("candidates").map(Value::take) else {
        return response;
    };
    let projection_truncated = response["truncated"].as_bool().unwrap_or(false);
    let found = candidates.len();
    candidates.truncate(limit);
    let returned = candidates.len();
    let candidates_truncated = found > returned;
    let at_ceiling = found >= MATCH_CEILING;

    let reason = match (projection_truncated, candidates_truncated) {
        (false, false) => {
            "Nothing was cut: every screen candidate found is in this answer.".to_owned()
        }
        (false, true) => format!(
            "{found} screen candidates were found and {returned} returned, because limit is \
             {limit}. Nothing is missing from the design - raise limit to see the rest."
        ),
        (true, false) => include_str!("../text/mod-apply_explore_limit-f9b94793.md").to_owned(),
        (true, true) => format!(
            include_str!("../text/mod-apply_explore_limit-7dacda8e.md"),
            found = found,
            limit = limit,
            returned = returned
        ),
    };
    let mut truncation = json!({
        "projection": projection_truncated,
        "candidates": candidates_truncated,
        "returned": returned,
        "found": found,
        "atCeiling": at_ceiling,
        "reason": reason,
    });
    if candidates_truncated {
        truncation["nextAction"] = json!({
            "limit": found.max(returned).min(MATCH_CEILING),
            "note": if at_ceiling {
                "This is the highest limit there is; a Section holding more screens than that \
                 has to be exported in parts."
            } else {
                "The ranked order is stable, so a larger limit extends this list rather than \
                 reshuffling it."
            },
        });
    }
    response["candidates"] = Value::Array(candidates);
    response["count"] = json!(returned);
    // There is no cursor to hand back: the projection is rebuilt per call and
    // has no position to resume from. `truncation` says which lever moves it.
    response["truncated"] = json!(projection_truncated || candidates_truncated);
    response["truncation"] = truncation;
    response
}

#[path = "../coerce_args.rs"]
mod coerce_args;
#[path = "../handlers/devup_feature_trace.rs"]
mod devup_feature_trace_handler;
#[path = "../handlers/devup_figma_auth.rs"]
mod devup_figma_auth_handler;
#[path = "../handlers/devup_figma_explore.rs"]
mod devup_figma_explore_handler;
#[path = "../handlers/devup_figma_export.rs"]
mod devup_figma_export_handler;
#[path = "../handlers/devup_figma_measure.rs"]
mod devup_figma_measure_handler;
#[path = "../handlers/devup_figma_search.rs"]
mod devup_figma_search_handler;
#[path = "../handlers/devup_project_context.rs"]
mod devup_project_context_handler;
#[path = "../handlers/devup_project_plan.rs"]
mod devup_project_plan_handler;
#[path = "../handlers/devup_runtime_audit.rs"]
mod devup_runtime_audit_handler;
#[path = "../handlers/devup_screen_plan.rs"]
mod devup_screen_plan_handler;
#[path = "../handlers/devup_skills.rs"]
mod devup_skills_handler;
#[path = "../handlers/devup_stack_diff.rs"]
mod devup_stack_diff_handler;
#[path = "../handlers/devup_ui_validate.rs"]
mod devup_ui_validate_handler;
#[path = "../handlers/devup_visual_compare.rs"]
mod devup_visual_compare_handler;

impl DevupServer {
    fn tool_router() -> ToolRouter<Self> {
        Self::devup_figma_measure_router()
            + Self::devup_skills_router()
            + Self::devup_figma_auth_router()
            + Self::devup_figma_search_router()
            + Self::devup_figma_explore_router()
            + Self::devup_figma_export_router()
            + Self::devup_project_context_router()
            + Self::devup_ui_validate_router()
            + Self::devup_stack_diff_router()
            + Self::devup_visual_compare_router()
            + Self::devup_feature_trace_router()
            + Self::devup_runtime_audit_router()
            + Self::devup_project_plan_router()
            + Self::devup_screen_plan_router()
    }
}

/// Adds what `devup_ui_validate` checked beyond its rules, kept apart from
/// `violations` so `ok`, `okReason` and `strict` mean what they meant: the
/// static layout risks, the export node behind each violation when the caller
/// passed the export's `sourceMap`, and which part was static and which was
/// never rendered.
fn attach_layout_and_source_links(result: &mut Value, input: &UiValidateInput) {
    let sources: Vec<(Option<&str>, &str)> = match &input.files {
        Some(files) => files
            .iter()
            .filter(|file| {
                [".tsx", ".jsx"]
                    .iter()
                    .any(|extension| file.path.ends_with(extension))
            })
            .map(|file| (Some(file.path.as_str()), file.content.as_str()))
            .collect(),
        None => vec![(None, input.tsx.as_str())],
    };
    let mut risks = Vec::new();
    let mut unmeasured = 0;
    let mut layout = Value::Null;
    for (path, source) in sources {
        layout = devup_mcp_devup_ui::ui_layout::layout_risks(source, input.reference_width);
        unmeasured += layout["groupsWithoutReferenceWidth"].as_u64().unwrap_or(0);
        for mut risk in layout["risks"].as_array().cloned().unwrap_or_default() {
            if let Some(path) = path {
                risk["path"] = json!(path);
            }
            risks.push(risk);
        }
    }
    result["layoutRisks"] = json!(risks);
    let mut rules = devup_mcp_devup_ui::ui_validate::rule_names()
        .into_iter()
        .collect::<Vec<_>>();
    if input.project_root.is_some() {
        rules.push(mask_guard::RULE);
        rules.push(asset_guard::RULE);
    }
    if input.section_coverage.is_some() {
        rules.push(asset_guard::COVERAGE_RULE);
    }
    if input.files.is_some() {
        rules.extend([
            "file-placement",
            "one-component-per-file",
            "client-boundary",
            "inline-style",
            "react-query-states",
        ]);
    }
    result["verificationScope"] = json!({
        "static": {
            "rules": rules,
            "sectionCoverage": {
                "state": if input.section_coverage.is_some() { "checked" } else { "unverified" },
                "scope": "Supplied JSX identities and asset references; runtime use and visibility remain unverified."
            },
            "unknownTokenCheckRan": result["tokens"]["unknownTokenCheckRan"],
            "layout": layout["checked"],
            "referenceWidth": input.reference_width,
            "groupsWithoutReferenceWidth": unmeasured,
        },
        "runtime": {
            "state": "not-performed",
            "unverified": layout["runtimeUnverified"],
            "note": "Nothing was rendered. devup_runtime_audit plans a consumer-side browser run whose report this server can ingest.",
        },
    });
    let Some(source_map) = &input.source_map else {
        return;
    };
    if input.files.is_some() {
        result["sourceMapLink"] = json!({"state": "unavailable",
            "reason": "A sourceMap describes the one TSX it was exported with, and files numbers the elements of each file from 0, so a violation cannot be tied to it. Pass the sourceMap with tsx."});
        return;
    }
    let entries = source_map
        .entries
        .iter()
        .chain(&source_map.tsx)
        .collect::<Vec<_>>();
    if entries.is_empty() {
        result["sourceMapLink"] = json!({"state": "unavailable",
            "reason": "The sourceMap holds no entries: pass the sourceMap devup_figma_export returned with this tsx, whose entries are under tsx (a screen) or entries (a Section frame)."});
        return;
    }
    if !entries.iter().any(|entry| entry.element_index.is_some()) {
        result["sourceMapLink"] = json!({"state": "unavailable",
            "reason": "No sourceMap entry carries elementIndex; it was produced by a build before elementIndex existed. Export again to link violations."});
        return;
    }
    let mut linked = 0;
    for violation in result["violations"].as_array_mut().into_iter().flatten() {
        let (Some(element), Some(attribute)) = (
            violation["elementIndex"].as_u64(),
            violation["attribute"].as_str().map(str::to_owned),
        ) else {
            continue;
        };
        let matches = entries
            .iter()
            .filter(|entry| entry.element_index == usize::try_from(element).ok())
            .filter(|entry| {
                entry
                    .generated_property
                    .as_deref()
                    .is_some_and(|generated| {
                        generated == attribute || generated.starts_with(&format!("{attribute}="))
                    })
            })
            .take(3)
            .map(|entry| {
                json!({"nodeId": entry.node_id, "property": entry.property,
                    "generatedProperty": entry.generated_property, "resolution": entry.resolution})
            })
            .collect::<Vec<_>>();
        if !matches.is_empty() {
            linked += 1;
            violation["source"] = json!(matches);
        }
    }
    let unlinked = result["violations"].as_array().map_or(0, Vec::len) - linked;
    result["sourceMapLink"] = json!({"state": "linked", "linkedViolations": linked,
        "unlinkedViolations": unlinked, "joinedOn": "elementIndex + attribute name"});
    if unlinked > 0 {
        result["sourceMapLink"]["unlinkedReason"] = json!(include_str!(
            "../text/mod-attach_layout_and_source_links-a7298619.md"
        ));
    }
}

/// The target a tool's url names. No url is the file the attached Devup Bridge
/// plugin has open, which [`DevupServer::bind_to_bridge`] then resolves.
fn parse_link(url: Option<&str>) -> Result<FigmaTarget, DevupError> {
    url.map_or_else(|| Ok(FigmaTarget::bridge_current()), FigmaTarget::parse)
}

fn section_index_from_payload(payload: &CollectedPayload) -> Option<SectionIndex> {
    serde_json::from_value(payload.metadata.get("sectionIndex")?.clone()).ok()
}

/// The skills the code in this response is written in, named when this runtime
/// would not load them.
///
/// On the export rather than only on `devup_ui_validate`, because the agent
/// that has never seen devup-ui does not call the validator either: the gap
/// was only ever shown to callers who had already written the code and then
/// chosen to check it. This is the response every implementing agent reads,
/// and it arrives before the code is written rather than after it is refused.
///
/// The same sentence is in `devup_figma_export`'s own tool description,
/// because that is the only channel that reaches an agent which never sees the
/// server's `instructions` - a subagent handed tool schemas alone, or Codex,
/// where `instructions` becomes one namespace blurb rather than prompt text.
const CODE_SKILLS: [&str; 2] = ["devup-ui", "devfive-frontend"];

fn with_skill_gap(mut result: Value, lookup: &skills::Lookup) -> Value {
    if let Some(gap) = skill_gap(CODE_SKILLS.into_iter(), lookup, CODE_GAP_WHY) {
        result["skillGap"] = gap;
    }
    result
}

const CODE_GAP_WHY: &str = include_str!("../text/mod-CODE_GAP_WHY-6866def7.md");

/// The gap between the skills a response's subject needs and what this runtime
/// would actually load, or `None` when there is none.
///
/// Missing and outdated are reported apart because they need different words
/// from the caller. One is a skill the agent has never seen. The other is one
/// it is reading right now, and being taught rules this build no longer emits -
/// which reads exactly like a correctly installed skill until someone compares
/// the bytes.
fn skill_gap<'a>(
    names: impl Iterator<Item = &'a str>,
    lookup: &skills::Lookup,
    why: &str,
) -> Option<Value> {
    let mut missing = Vec::new();
    let mut outdated = Vec::new();
    for skill in names.filter_map(skills::find_by_name) {
        let entry = json!({
            "skill": skill.record.name,
            "whyYouNeedIt": skill.record.used_for,
            "install": skill.install_action(lookup),
        });
        match skill.installed_freshness(lookup) {
            None => missing.push(entry),
            Some(state) if state.should_refresh() => {
                let mut entry = entry;
                entry["revision"] = json!(state.as_str());
                outdated.push(entry);
            }
            Some(_) => {}
        }
    }
    if missing.is_empty() && outdated.is_empty() {
        return None;
    }
    // The same install writes /devup, the one word that starts the whole
    // workflow; say so where the agent is already being told to install.
    let command = skills::command_state(lookup);
    Some(json!({
        "missing": missing,
        "outdated": outdated,
        "command": if command["installed"] == true { Value::Null } else { json!({
            "invocation": command["invocation"],
            "supported": command["supported"],
            "how": command["how"],
        }) },
        "why": why,
        "how": include_str!("../text/mod-skill_gap-bd8a6ae8.md"),
        "runtime": lookup.runtime.as_str(),
        "workspace": lookup.project.display().to_string(),
    }))
}

/// Names the conventions for the files this scope just read, when this runtime
/// would not load them.
///
/// The export gap covers the code devup-mcp writes; this covers the files it
/// reads. `devup.json` is devup-ui's theme format, the `openapi.json` under
/// scope api is generated by vespera routes, and the `models/*.json` under
/// scope db are vespertide schemas - so an agent about to edit any of them
/// needs the same rules, and had no way to learn it needed them.
///
/// Gated on the scope having found something. A project with no `openapi.json`
/// has no use for the vespera skill, and naming it there is the noise that
/// teaches a reader to skip the field.
fn with_context_skill_gap(mut result: Value, scope: &str, lookup: &skills::Lookup) -> Value {
    let found = |key: &str| {
        // `all` nests one object per scope; a single scope answers at the top
        // level. Both say `found`.
        result
            .get(key)
            .unwrap_or(&result)
            .get("found")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    let mut names: Vec<&str> = Vec::new();
    if matches!(scope, "theme" | "all") && found("theme") {
        names.push("devup-ui");
    }
    if matches!(scope, "api" | "all") && found("api") {
        names.push("vespera");
    }
    if matches!(scope, "db" | "all") && found("db") {
        names.push("vespertide");
    }
    if scope == "ui" && found("ui") {
        names.extend(CODE_SKILLS);
    }
    if let Some(gap) = skill_gap(
        names.into_iter(),
        lookup,
        include_str!("../text/mod-with_context_skill_gap-fc39c774.md"),
    ) {
        result["skillGap"] = gap;
    }
    result
}

/// Says whether the TSX just generated fits the project it is going into.
///
/// Tokens are named after the Figma variables and text styles the screen
/// uses. For a project with no `devup.json` that is the right answer, and it
/// is what the `devupJson` output is for. For a project that already has one,
/// the names are its own and the export had no way to know them: a Braillify
/// Studio login screen came back referencing `$background`, `$innerBg`,
/// `$kakaoLogo`, `$title`, `$textSub`, `$borderLight` and the typography
/// `mainSubText`, `titleSmMed`, `bodyMed`, and that project defines none of
/// them. `devup_ui_validate` refused the same code against the same root, so
/// the export was handing back TSX this server's own validator rejects, and
/// saying nothing about it.
///
/// Reported rather than rewritten. Which token was meant is the caller's
/// decision — `$background` is one edit from `$bg` and also from any other
/// name a project happens to use — and silently substituting a colour is a
/// worse failure than naming the mismatch.
///
/// Without a root the theme half is skipped and nothing is read from disk, but
/// the skill half still runs: see [`with_skill_gap`] for why it cannot be
/// gated on an argument the unaware caller is the one least likely to send.
fn with_project_checks(
    mut result: Value,
    project_root: Option<&str>,
    lookup: &skills::Lookup,
) -> Value {
    if result.get("interactionCoverage").is_some()
        && let Some(root) = project_root
    {
        project_context::attach_export_context(&mut result, root);
    }
    review::attach(&mut result);
    const OUTPUTS: [&str; 3] = ["tsx", "componentTsx", "responsiveTsx"];
    let mut generated: Vec<(String, String)> = OUTPUTS
        .into_iter()
        .filter_map(|key| {
            result
                .get(key)
                .and_then(Value::as_str)
                .map(|tsx| (key.to_owned(), tsx.to_owned()))
        })
        .collect();
    if let Some(frames) = result.get("frames").and_then(Value::as_array) {
        for frame in frames {
            let node = frame
                .get("nodeId")
                .and_then(Value::as_str)
                .unwrap_or("frame")
                .to_owned();
            for key in OUTPUTS {
                if let Some(tsx) = frame.get(key).and_then(Value::as_str) {
                    generated.push((format!("{node}:{key}"), tsx.to_owned()));
                }
            }
        }
    }
    // Before the theme half, and before its `project_root` gate: a response
    // that handed back code has to name the conventions that code is written
    // in whether or not the caller told us where the project is.
    let produced_code = !generated.is_empty() || result.get("deliverable").is_some();
    if produced_code {
        result = with_skill_gap(result, lookup);
    }
    validation_guidance::attach_export_formatting(&mut result, &generated);
    let Some(project_root) = project_root else {
        return result;
    };
    match screen_plan::export_progress(project_root) {
        Ok(progress) if !progress.is_null() => result["planProgress"] = progress,
        Err(error) => result["planProgress"] = json!({"state": "unavailable", "reason": error.message}),
        _ => {}
    }
    if generated.is_empty() {
        // Resource delivery hands the code back as a link, so there is no
        // source here to check. Saying so is the point: an absent verdict
        // reads exactly like a clean one, and this export is neither.
        if result.get("resources").is_some_and(|r| !r.is_null()) {
            result["projectThemeValidation"] = json!({
                "state": "not-checked",
                "reason": "The outputs were delivered as resources, so no source was present to check.",
                "note": "Request delivery=inline to have the generated tokens checked against this project.",
            });
        }
        return result;
    }
    let lookup = match project_context::theme_for_validation(Some(project_root)) {
        Ok(lookup) => lookup,
        Err(error) => {
            result["projectThemeValidation"] = json!({
                "state": "unavailable",
                "reason": error.message,
                "note": "The generated tokens could not be checked against this project.",
            });
            return result;
        }
    };
    let mut outputs = serde_json::Map::new();
    for (key, tsx) in generated {
        let report = devup_mcp_devup_ui::ui_validate::validate_devup_ui_tsx(
            &tsx,
            lookup.theme.as_ref(),
            false,
        );
        // Only the findings a caller has to act on. The hardcoded-value
        // information is already in `fidelity`, and repeating twenty of them
        // here would bury the handful that stop the code compiling into a
        // theme.
        let unknown = report
            .violations
            .iter()
            .filter(|violation| violation.rule == "unknown-token")
            .map(|violation| {
                json!({
                    "message": violation.message,
                    "suggestion": violation.suggestion,
                })
            })
            .collect::<Vec<_>>();
        outputs.insert(
            key.to_owned(),
            json!({
                "ok": report.ok,
                "unknownTokenCount": unknown.len(),
                "unknownTokens": unknown,
            }),
        );
    }
    let any_unknown = outputs
        .values()
        .any(|output| output["unknownTokenCount"].as_u64().unwrap_or(0) > 0);
    result["projectThemeValidation"] = json!({
        "state": if lookup.theme.is_some() { "checked" } else { "no-theme" },
        "themeAvailable": lookup.theme.is_some(),
        "guardrail": lookup.guardrail,
        "outputs": outputs,
        "note": if any_unknown {
            include_str!("../text/mod-with_project_checks-2c4cf346.md")
        } else {
            "Every token the generated TSX references exists in this project's devup.json."
        },
    });
    if any_unknown {
        let action = theme_sync_action(&result, project_root);
        result["projectThemeValidation"]["nextAction"] = action.clone();
        if result.get("nextAction").is_none_or(Value::is_null) {
            result["nextAction"] = action;
        }
    }
    result
}
fn theme_sync_action(result: &Value, project_root: &str) -> Value {
    let mut arguments = json!({
        "outputs": ["devupJson"],
        "devupJsonMode": "missing-only",
        "projectRoot": project_root,
        "scope": "file",
    });
    if let Some(url) = result.get("url").filter(|url| url.is_string()) {
        arguments["url"] = url.clone();
    }
    json!({
        "tool": "devup_figma_export",
        "arguments": arguments,
        "how": "The generated code references tokens missing from this project's devup.json. Sync devup.json with this call (additions only; existing tokens are kept) before using the tsx.",
    })
}

fn section_candidate_as_explore(candidate: &SectionCandidate) -> ExploreCandidate {
    ExploreCandidate {
        node: ExploreNode {
            node_id: candidate.node_id.clone(),
            name: candidate.name.clone(),
            node_type: candidate.node_type.clone(),
            bounds: candidate.bounds,
            child_count: candidate.direct_child_count,
            text_preview: candidate.text_preview.clone(),
            text_preview_state: candidate.text_preview_state.clone(),
            navigation_content_width: candidate.navigation_content_width,
            parent_id: candidate.parent_id.clone(),
            kind: ExploreKind::Screen,
            visible: candidate.visible,
            breadcrumb: candidate.breadcrumb.clone(),
            page_child_index: None,
        },
        canonical_url: candidate.canonical_url.clone(),
        score: 900,
        selection_reasons: candidate.selection_reasons.clone(),
    }
}

fn format_epoch_rfc3339(epoch_seconds: u64) -> String {
    let days = (epoch_seconds / 86_400) as i64;
    let seconds = epoch_seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds / 3_600;
    let minute = (seconds % 3_600) / 60;
    let second = seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let days = days_since_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

fn parse_scope(scope: &str) -> Result<ThemeScope, DevupError> {
    match scope {
        "node" => Ok(ThemeScope::Node),
        "page" => Ok(ThemeScope::Page),
        "file" => Ok(ThemeScope::File),
        _ => Err(DevupError::new(
            ErrorCode::DevupThemeConflict,
            "scope must be node, page, or file.",
            false,
        )),
    }
}

fn with_error_identity(mut error: ErrorData) -> ErrorData {
    let mut data = error.data.take().unwrap_or_else(|| json!({}));
    if !data.is_object() {
        data = json!({"details":data});
    }
    data["server"] = delivery::server_identity();
    error.data = Some(data);
    error
}

/// The one automatic recovery for `DEVUP_SECTION_REQUIRED`: re-aim the same
/// request at the ancestor Section the plugin reported, keeping the requested
/// frame as the selection. Returns `None` when the plugin named no Section.
fn section_retry(
    error: &DevupError,
    operation: &PendingOperation,
    request: &CollectionRequest,
) -> Option<(PendingOperation, CollectionRequest, Value)> {
    if error.details["pluginCode"] != "DEVUP_SECTION_REQUIRED" {
        return None;
    }
    let section_id = error.details["sectionId"].as_str()?;
    let requested = request.target.node_id.clone()?;
    let PendingOperation::Export {
        frame_ids,
        all_screens,
        ..
    } = operation
    else {
        return None;
    };
    let mut operation = operation.clone();
    let mut request = request.clone();
    let selection = if frame_ids.is_empty() && !all_screens {
        vec![requested.clone()]
    } else {
        frame_ids.clone()
    };
    if let PendingOperation::Export { frame_ids, .. } = &mut operation {
        frame_ids.clone_from(&selection);
    }
    request.target.node_id = Some(section_id.to_owned());
    request.section = Some(SectionReadOptions {
        frame_ids: selection.clone(),
        all_screens: *all_screens,
    });
    Some((
        operation,
        request,
        json!({"requestedNodeId":requested,"sectionId":section_id,"frameIds":selection,
            "note":"The url named a node inside a SECTION; the export was re-run against that SECTION with the node selected."}),
    ))
}

/// Maps a [`DevupError`] onto the JSON-RPC error the caller actually sees.
///
/// The protocol code is not decoration here: the caller is usually an agent
/// choosing between "fix the arguments and call again" and "stop and report",
/// and every error used to arrive as INTERNAL_ERROR, which says the second
/// thing about both. A mistake in the call itself is INVALID_PARAMS, so that
/// decision can be made without parsing the message. `data` keeps carrying
/// the exact `code` and `retryable`, unchanged.
fn to_mcp_error(mut error: DevupError) -> ErrorData {
    if error.code == ErrorCode::DevupFigmaHandoffExpired
        && error.details.get("exportJob").is_none()
        && error.details.get("assetJob").is_none()
    {
        if !error.details.is_object() {
            error.details = json!({"upstreamDetails":error.details});
        }
        if error.details.get("stage").is_none() {
            error.details["stage"] = json!("artifact-lookup");
        }
        if error.details.get("artifactState").is_none() {
            error.details["artifactState"] = json!("unknown");
        }
        let recovery = error.details.get("recoveryArguments").cloned();
        error.details["recoveryState"] = json!(if recovery.is_some() {
            "available"
        } else {
            "unrecoverable"
        });
        let another_process = error.details["artifactState"] == "issued-by-another-process";
        if recovery.is_none() {
            error.details["recoveryReason"] = json!(if another_process {
                include_str!("../text/mod-to_mcp_error-784239dd.md")
            } else {
                "Original URL/selection cannot be recovered from this server. The ID may be unknown, evicted beyond metadata retention, or lost on restart; the cause cannot be determined."
            });
        }
        error.details["recovery"] = json!({
            "mode": "recollect",
            "cause": match error.details["artifactState"].as_str() {
                Some("expired") => "artifact-expired",
                Some("evicted") => "artifact-evicted",
                Some("issued-by-another-process") => "issued-by-another-process",
                _ => "artifact-unknown",
            },
            "reprojectable": false,
            "requiresOriginalUrl": recovery.is_none(),
        });
        error.details["nextAction"] = json!({"tool":"devup_figma_export",
            "arguments":{"refresh":true},"requiredArguments":["url"],"omitArguments":["artifactId"],
            "how":include_str!("../text/mod-to_mcp_error-20941350.md")});
        if let Some(arguments) = recovery {
            error.details["nextAction"]["arguments"] = arguments;
        }
    }
    if error.details["recoveryArguments"]["nodeIds"].is_array() {
        error.details["nextAction"] = json!({"tool":"devup_figma_measure","arguments":error.details["recoveryArguments"],
            "how":"Repeat the explicit measured node list; a measurement receipt never recovers by exporting the current selection."});
    }
    let mcp_code = if validation::is_caller_mistake(&error) {
        McpErrorCode::INVALID_PARAMS
    } else {
        McpErrorCode::INTERNAL_ERROR
    };
    ErrorData::new(
        mcp_code,
        error.message,
        Some(
            json!({ "code": error.code, "retryable": error.retryable, "details": error.details, "server":delivery::server_identity() }),
        ),
    )
}

fn structured_tool_error(error: ErrorData) -> rmcp::model::CallToolResult {
    let mut data = error.data.unwrap_or_else(|| json!({}));
    if !data.is_object() {
        data = json!({"details":data});
    }
    data.as_object_mut().unwrap().remove("server");
    if data.get("code").is_none() {
        data["code"] = json!("DEVUP_INVALID_INPUT");
    }
    if data.get("retryable").is_none() {
        data["retryable"] = json!(false);
    }
    if data.get("details").is_none() {
        data["details"] = json!({});
    }
    data["message"] = json!(error.message);
    data["rpcCode"] = json!(error.code);
    let mut result = tool_result(json!({"error":data}));
    result.is_error = Some(true);
    result
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for DevupServer {
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, ErrorData> {
        // Read here rather than by overriding `initialize`, because the peer
        // already recorded it there and this is the first place the value is
        // needed. `set` after the first call is a no-op, so one session keeps
        // one answer.
        if self.client_name.get().is_none()
            && let Some(peer) = context.peer.peer_info()
        {
            let _ = self.client_name.set(peer.client_info.name.clone());
        }
        let mut request = request;
        if let (Some(tool), Some(arguments)) =
            (self.tool_router.get(request.name.as_ref()), request.arguments.as_mut())
        {
            coerce_args::coerce_stringified_arguments(&tool.input_schema, arguments);
        }
        let call = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        let mut response = match self.tool_router.call(call).await {
            Ok(response) => response,
            Err(error) if error.data.as_ref().and_then(|d| d.get("code")).is_some() => {
                return Ok(structured_tool_error(error).into());
            }
            Err(error) => return Err(with_error_identity(error)),
        };
        if let rmcp::model::CallToolResponse::Complete(result) = &mut response
            && result.is_error == Some(true)
        {
            // Router argument decoding failures arrive as content-only tool
            // errors. Give them the same error envelope as execution errors.
            if result
                .structured_content
                .as_ref()
                .and_then(|v| v.get("error"))
                .is_none()
            {
                *result = structured_tool_error(ErrorData::invalid_params(
                    "Invalid tool arguments.",
                    Some(json!({"details":{"content":result.content}})),
                ));
            }
        }
        Ok(response)
    }

    fn get_info(&self) -> ServerInfo {
        // Initialize is the first moment a real session exists, which makes it
        // the right place to start the release lookup: server construction is
        // not, because `self_check()` constructs a server on a thread with no
        // Tokio runtime and `--self-check` must stay network-free. `spawn` is
        // idempotent and returns immediately when disabled or when there is no
        // runtime, so this costs nothing here.
        release_check::spawn();
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_resources()
                .enable_tools()
                .build(),
        )
        .with_server_info(Implementation::new("devup-mcp", env!("CARGO_PKG_VERSION")))
        .with_instructions(guide::INSTRUCTIONS)
    }

    async fn list_resources(
        &self,
        request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::ListResourcesResult, ErrorData> {
        resources::list_output_resources(
            &self.artifacts,
            request
                .as_ref()
                .and_then(|request| request.cursor.as_deref()),
        )
        .await
        .map_err(to_mcp_error)
    }

    async fn list_resource_templates(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::ListResourceTemplatesResult, ErrorData> {
        Ok(resources::resource_templates())
    }

    async fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::ReadResourceResponse, ErrorData> {
        resources::read_output_resource(&self.artifacts, &request.uri)
            .await
            .map(Into::into)
            .map_err(|error| with_error_identity(resource_read_error(error)))
    }
}

/// A failed resource read, still `resource not found` on the wire, carrying
/// which part was missing and whether the artifact behind it is alive: a live
/// artifact can project the output again, a gone one has to be recollected.
fn resource_read_error(error: DevupError) -> ErrorData {
    let details = &error.details;
    let live = details["artifactState"] == "live";
    let (mode, cause) = match details["resourceKind"].as_str() {
        Some("chunk") => ("reread-manifest", "chunk-out-of-range"),
        Some("output") => ("reproject", "output-not-attached"),
        None if error.code == ErrorCode::DevupFigmaHandoffInvalid => ("fix-uri", "invalid-uri"),
        _ => (
            "recollect",
            match details["artifactState"].as_str() {
                Some("expired") => "artifact-expired",
                Some("evicted") => "artifact-evicted",
                Some("issued-by-another-process") => "issued-by-another-process",
                _ => "artifact-unknown",
            },
        ),
    };
    let recovery = json!({
        "mode": mode,
        "cause": cause,
        "reprojectable": live,
        "requiresOriginalUrl": !live && details.get("recoveryArguments").is_none(),
        "recoveryArguments": details.get("recoveryArguments"),
    });
    ErrorData::resource_not_found(
        "resource not found",
        Some(json!({
            "code": error.code,
            "retryable": error.retryable,
            "message": error.message,
            "details": error.details,
            "recovery": recovery,
        })),
    )
}

#[cfg(test)]
#[path = "../export_skill_gap_tests.rs"]
mod export_skill_gap_tests;

#[cfg(test)]
mod p3_error_tests {
    use super::*;

    #[test]
    fn p3_expired_artifact_is_state_failure_with_original_message() {
        let error = to_mcp_error(DevupError::new(
            ErrorCode::DevupFigmaHandoffExpired,
            "The Figma artifact is missing or expired.",
            true,
        ));
        assert_eq!(error.code, McpErrorCode::INTERNAL_ERROR);
        assert_eq!(error.message, "The Figma artifact is missing or expired.");
    }

    #[test]
    fn p3_output_root_violation_is_invalid_params() {
        let policy = OutputPolicy::from_roots(vec![std::env::current_dir().unwrap()]).unwrap();
        let error = policy.resolve("../outside.svg").err().unwrap();
        assert_eq!(to_mcp_error(error).code, McpErrorCode::INVALID_PARAMS);
    }
}

#[cfg(test)]
mod r7_error_tests {
    use super::*;
    #[test]
    fn r7_errors_have_the_same_build_identity_as_success() {
        let success = serde_json::to_value(tool_result(json!({"status":"complete"}))).unwrap();
        let success: Value =
            serde_json::from_str(success["content"][0]["text"].as_str().unwrap()).unwrap();
        for code in [
            ErrorCode::DevupInvalidInput,
            ErrorCode::DevupSnapshotUnsupported,
            ErrorCode::DevupFigmaHandoffExpired,
        ] {
            let error = to_mcp_error(DevupError::new(code, "failure", false));
            assert_eq!(error.data.as_ref().unwrap()["server"], success["server"]);
        }
    }
    #[test]
    fn r7_plugin_expiry_has_recovery_and_keeps_its_stage() {
        let raw = json!({"isError":true,"content":[{"type":"text","text":"DEVUP_FIGMA_HANDOFF_EXPIRED"}]});
        let error = to_mcp_error(operation::upstream_error(&raw).unwrap());
        let data = error.data.unwrap();
        assert_eq!(data["code"], "DEVUP_FIGMA_HANDOFF_EXPIRED");
        assert_eq!(data["details"]["stage"], "plugin-execution");
        assert_eq!(data["details"]["nextAction"]["arguments"]["refresh"], true);
    }

    #[test]
    fn r7_expired_artifact_explains_refresh_arguments() {
        let error = to_mcp_error(DevupError::new(
            ErrorCode::DevupFigmaHandoffExpired,
            "expired",
            true,
        ));
        let d = &error.data.as_ref().unwrap()["details"];
        assert_eq!(d["stage"], "artifact-lookup");
        assert_eq!(d["nextAction"]["tool"], "devup_figma_export");
        assert_eq!(d["nextAction"]["arguments"]["refresh"], true);
        assert!(
            d["nextAction"]["omitArguments"]
                .as_array()
                .unwrap()
                .contains(&json!("artifactId"))
        );
    }
}

#[cfg(test)]
mod r8_recovery_tests {
    use super::*;
    #[test]
    fn r8_expired_artifact_returns_known_url_or_explicit_unrecoverable_state() {
        for known in [true, false] {
            let details = if known {
                json!({"artifactState":"expired","recoveryArguments":{"url":"https://www.figma.com/design/test?node-id=1-2","frameIds":["2:1"],"refresh":true}})
            } else {
                json!({})
            };
            let e = to_mcp_error(DevupError::with_details(
                ErrorCode::DevupFigmaHandoffExpired,
                "expired",
                true,
                details,
            ));
            let d = &e.data.unwrap()["details"];
            if known {
                assert_eq!(
                    d["nextAction"]["arguments"]["url"],
                    "https://www.figma.com/design/test?node-id=1-2"
                );
            } else {
                assert_eq!(d["artifactState"], "unknown");
                assert_eq!(d["recoveryState"], "unrecoverable");
            }
        }
    }
}
