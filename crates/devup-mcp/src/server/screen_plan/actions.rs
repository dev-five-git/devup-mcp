/// `confirm`: every question that applies answered, in one call, or nothing
/// changes.
pub(super) fn confirm(input: &ScreenPlanInput, policy: &OutputPolicy) -> Result<Value, DevupError> {
    let root = require_root(input)?;
    let mut plan = read_plan(root)?.ok_or_else(|| {
        invalid("No screen plan exists in this project; run devup_screen_plan action draft first.")
    })?;
    let plan_id = input
        .plan_id
        .as_deref()
        .ok_or_else(|| invalid("action confirm needs planId."))?;
    if plan["planId"] != plan_id {
        return Err(DevupError::with_details(
            ErrorCode::DevupInvalidInput,
            "planId does not match the plan on disk; it was redrafted. Ask the questions of the current draft.",
            false,
            json!({"planId": plan_id, "currentPlanId": plan["planId"]}),
        ));
    }
    let mut questions = array(&plan["questions"]).to_vec();
    let ordered = questions.clone();
    questions.sort_by_key(|question| depth(question, &ordered));
    let known = questions
        .iter()
        .map(|question| text(&question["id"]).to_owned())
        .collect::<BTreeSet<_>>();
    let unknown = input
        .answers
        .keys()
        .filter(|id| !known.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    let mut actions = BTreeMap::new();
    let mut custom = Vec::new();
    let mut missing = Vec::new();
    for question in &questions {
        let id = text(&question["id"]).to_owned();
        if !required(question, &actions) {
            continue;
        }
        let Some(answer) = input.answers.get(&id) else {
            if input.accept_recommended
                && let Some(recommended) = array(&question["resolve"]).first()
            {
                actions.insert(id, recommended["action"].clone());
            } else {
                missing.push(id);
            }
            continue;
        };
        let (action, note) = resolve_answer(question, answer)?;
        if let Some(note) = note {
            custom.push(json!({"question": id, "answer": note}));
        }
        actions.insert(id, action);
    }
    if !missing.is_empty() || !unknown.is_empty() {
        let pending = questions
            .iter()
            .filter(|question| missing.iter().any(|id| question["id"] == id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        return Err(DevupError::with_details(
            ErrorCode::DevupInvalidInput,
            format!(
                "confirm needs an answer to every question that applies. Missing: {}.",
                if missing.is_empty() {
                    "none".to_owned()
                } else {
                    missing.join(", ")
                }
            ),
            false,
            json!({"missing": missing, "unknown": unknown, "askInstruction": ASK_INSTRUCTION,
                "questionRounds": question_rounds(&pending, per_round(&plan))}),
        ));
    }
    let accepted = actions
        .get("heuristics")
        .is_none_or(|action| action["heuristics"] == "accept");
    let nothing_here = actions
        .get("scope")
        .is_some_and(|action| action["scope"] == "none");
    // Every "the Figma structure is wrong" answer: its frames are not built,
    // and the fix is reported. The work goes on without them.
    let design_fixes = questions
        .iter()
        .filter(|question| {
            actions
                .get(text(&question["id"]))
                .is_some_and(|action| action["designFix"] == true)
        })
        .map(|question| {
            json!({"question": question["id"], "frames": question["fix"]["frames"],
                "fix": question["fix"]["text"]})
        })
        .collect::<Vec<_>>();
    let fixed = |ids: &[&str]| {
        ids.iter().any(|id| {
            actions
                .get(*id)
                .is_some_and(|action| action["designFix"] == true)
        })
    };
    let app_dir = actions
        .get("site:app")
        .and_then(|action| action["app"]["appDir"].as_str())
        .unwrap_or(text(&plan["appDir"]))
        .to_owned();
    plan["appDir"] = json!(app_dir);
    let folds = state_folds(&questions, &actions);
    let folded = folded_ids(&folds);
    let mut routes = Vec::new();
    let mut screen_routes = BTreeMap::new();
    let screens = array(&plan["screens"]).to_vec();
    for screen in &screens {
        let id = text(&screen["id"]).to_owned();
        let frames = array(&screen["frames"]).to_vec();
        if let Some(page) = actions.get("base-page").and_then(|action| action["pageId"].as_str())
            && !frames.iter().any(|frame| frame["pageId"] == page) {
            continue;
        }
        let mut deciding = array(&screen["structureQuestions"])
            .iter()
            .map(text)
            .collect::<Vec<_>>();
        deciding.extend(screen["groupingQuestion"].as_str());
        if nothing_here || fixed(&deciding) || folded.contains(&id) {
            continue;
        }
        let chosen_app = actions
            .get(&format!("app:{id}"))
            .or_else(|| actions.get("site:app"))
            .map(|action| action["app"].clone());
        let screen_app = chosen_app
            .as_ref()
            .and_then(|app| app["dir"].as_str())
            .or(screen["app"].as_str())
            .map(str::to_owned);
        let screen_app_dir = chosen_app
            .as_ref()
            .and_then(|app| app["appDir"].as_str())
            .or(screen["appDir"].as_str())
            .unwrap_or(&app_dir)
            .to_owned();
        let group = screen["groupingQuestion"]
            .as_str()
            .and_then(|question| actions.get(question));
        let separate = group.is_some_and(|action| action["group"] == "separate");
        let data = actions
            .get(&format!("dynamic:{id}"))
            .map(|action| action["data"].clone());
        // Each placement is (frames, implementation, route, host, collision).
        let mut placements = Vec::new();
        if separate {
            let explicit = group.and_then(|action| action["routes"].as_array());
            if explicit.is_some_and(|routes| routes.len() != frames.len()) {
                return Err(invalid(format!(
                    "{}: list exactly one route per frame ({} frames).",
                    text(&screen["groupingQuestion"]),
                    frames.len()
                )));
            }
            for (index, frame) in frames.iter().enumerate() {
                let node = text(&frame["nodeId"]);
                let action = explicit
                    .map(|routes| json!({"route": routes[index]}))
                    .or_else(|| actions.get(&format!("split-route:{node}")).cloned())
                    .unwrap_or(Value::Null);
                placements.push((vec![frame.clone()], action, None));
            }
        } else {
            let placed = group
                .filter(|action| action["route"].is_string())
                .or_else(|| actions.get(&format!("route:{id}")))
                .or_else(|| actions.get(&format!("modal:{id}")))
                .or_else(|| actions.get(&format!("review:{id}")))
                .cloned();
            let action = match placed {
                Some(action) => action,
                None if screen["routeEvidence"] == NAME_MATCH && accepted => {
                    json!({"route": screen["proposedRoute"], "collision": "merge", "acceptedHeuristic": true})
                }
                None => json!({"route": screen["proposedRoute"]}),
            };
            let collision = actions
                .get(&format!("collision:{id}"))
                .map(|action| text(&action["collision"]).to_owned());
            placements.push((frames.clone(), action, collision));
        }
        for (group_frames, action, collision) in placements {
            let collision = collision.or_else(|| action["collision"].as_str().map(str::to_owned));
            let (implementation, route, host) =
                if action["skip"] == true || collision.as_deref() == Some("skip") {
                    ("skip", None, None)
                } else if action["notPage"] == true {
                    ("not-a-page", None, None)
                } else if let Some(host) = action["state"].as_str() {
                    ("state", None, Some(host.to_owned()))
                } else {
                    ("page", action["route"].as_str().map(str::to_owned), None)
                };
            let first = text(&group_frames[0]["nodeId"]).to_owned();
            if implementation == "page" && route.is_none() {
                return Err(invalid(format!(
                    "{first} has no route after the answers; answer its route question with a route."
                )));
            }
            if let Some(url) = &route {
                screen_routes
                    .entry(id.clone())
                    .or_insert_with(|| url.clone());
            }
            let existing = route.as_deref().and_then(|url| {
                (screen["proposedRoute"] == url && !screen["existingFile"].is_null())
                    .then(|| screen["existingFile"].clone())
            });
            routes.push(json!({
                "url": route,
                "screen": if separate { first.clone() } else { id.clone() },
                "name": group_frames[0]["name"],
                "nodeIds": group_frames.iter().map(|frame| frame["nodeId"].clone()).collect::<Vec<_>>(),
                "pages": distinct_pages(&group_frames),
                "drawnWidths": group_frames.iter().filter_map(|frame| frame["width"].as_f64()).map(|width| width.round() as i64).collect::<Vec<_>>(),
                "implementation": implementation,
                "hostScreen": host,
                "app": screen_app,
                "file": existing.clone().unwrap_or_else(|| route.as_deref().map_or(Value::Null, |url| json!(planned_page(url, &screen_app_dir)))),
                "fileExists": existing.is_some(),
                "collisionPolicy": collision,
                "data": data,
                "routeEvidence": if action["acceptedHeuristic"] == true { json!("name-match (accepted)") } else { screen["routeEvidence"].clone() },
            }));
        }
    }
    record_states(&mut routes, &folds);
    let mut excluded = Vec::new();
    for row in array(&plan["excluded"]) {
        let node = text(&row["nodeId"]).to_owned();
        let built = !nothing_here
            && actions
                .get(&format!("exclusion:{node}"))
                .is_some_and(|action| action["build"] == true);
        let action = actions.get(&format!("exclusion-route:{node}"));
        match action.and_then(|action| action["route"].as_str()) {
            Some(route) if built => routes.push(json!({
                "url": route, "screen": node, "name": row["name"], "nodeIds": [node],
                "drawnWidths": [row["width"].as_f64().map(|width| width.round() as i64)],
                "implementation": "page", "hostScreen": null,
                "file": planned_page(route, &app_dir), "fileExists": false,
                "collisionPolicy": null, "data": null,
                "routeEvidence": "confirmed answer (was excluded)",
            })),
            _ => {
                let mut row = row.clone();
                row["decision"] = json!("excluded");
                excluded.push(row);
            }
        }
    }
    if nothing_here {
        for screen in &screens {
            for frame in array(&screen["frames"]) {
                excluded.push(json!({"nodeId": frame["nodeId"], "name": frame["name"], "width": frame["width"],
                    "section": frame["sectionName"], "reason": "the linked area is not for implementation (answered)",
                    "decision": "excluded"}));
            }
        }
    }
    // One URL per app: the public site and the console may both have `/`.
    let mut seen = BTreeMap::<(String, String), String>::new();
    let mut collisions = Vec::new();
    for row in &routes {
        if let Some(url) = row["url"].as_str()
            && let Some(other) = seen.insert(
                (text(&row["app"]).to_owned(), url.to_owned()),
                text(&row["screen"]).to_owned(),
            )
            && other != text(&row["screen"])
        {
            collisions.push((url.to_owned(), other, row["screen"].clone(), row["name"].clone()));
        }
    }
    if !collisions.is_empty() {
        return Err(duplicate_routes(&plan, &collisions));
    }
    for row in routes
        .iter_mut()
        .filter(|row| row["implementation"] == "state")
    {
        row["hostUrl"] = json!(screen_routes.get(text(&row["hostScreen"])));
    }
    let node_routes = routes
        .iter()
        .flat_map(|route| {
            array(&route["nodeIds"])
                .iter()
                .map(move |node| (text(node).to_owned(), route["url"].clone()))
        })
        .collect::<BTreeMap<_, _>>();
    if let Some(rows) = plan["screens"].as_array_mut() {
        for screen in rows {
            let id = text(&screen["id"]).to_owned();
            let mut resolved = Vec::new();
            for (index, link) in array(&screen["links"]).iter().enumerate() {
                let mut link = link.clone();
                if let Some(action) = actions.get(&format!("link:{id}:{index}")) {
                    link["resolved"] =
                        json!({"url": action["link"], "evidence": "confirmed answer"});
                } else if let Some(target) = link["resolved"]["screen"].as_str() {
                    link["resolved"]["url"] =
                        if let Some(node) = link["resolved"]["nodeId"].as_str() {
                            node_routes.get(node).cloned().unwrap_or(Value::Null)
                        } else {
                            let urls = routes
                                .iter()
                                .filter(|route| {
                                    screens
                                        .iter()
                                        .find(|screen| screen["id"] == target)
                                        .is_some_and(|screen| {
                                            array(&screen["frames"]).iter().any(|frame| {
                                                array(&route["nodeIds"]).contains(&frame["nodeId"])
                                            })
                                        })
                                })
                                .filter_map(|route| route["url"].as_str())
                                .collect::<BTreeSet<_>>();
                            if urls.len() == 1 {
                                json!(urls.first())
                            } else {
                                Value::Null
                            }
                        };
                }
                resolved.push(link);
            }
            screen["links"] = json!(resolved);
        }
    }
    plan["mobileNavigation"] = actions.get("mobile-menu").cloned().unwrap_or(Value::Null);
    plan["answers"] = json!(input.answers);
    plan["customAnswers"] = json!(custom);
    plan["heuristicsAccepted"] = json!(accepted);
    plan["layout"] = json!(
        actions
            .iter()
            .filter(|(id, _)| id.starts_with("layout:"))
            .map(|(id, action)| (id.clone(), action["layout"].clone()))
            .collect::<BTreeMap<_, _>>()
    );
    for asked in questions
        .iter()
        .filter(|asked| text(&asked["id"]).starts_with("site:"))
    {
        let field = text(&asked["field"]).to_owned();
        if let Some(action) = actions.get(text(&asked["id"])) {
            plan["site"][&field]["state"] = json!("answered");
            plan["site"][&field]["answer"] = action[&field].clone();
        }
    }
    plan["extraAnswers"] = json!(
        actions
            .iter()
            .filter(|(id, _)| id.starts_with("extra:"))
            .map(|(id, action)| (id.clone(), action["extra"].clone()))
            .collect::<BTreeMap<_, _>>()
    );
    plan["routes"] = json!(routes);
    plan["excluded"] = json!(excluded);
    plan["designFixes"] = json!(design_fixes);
    plan["confirmedPage"] = confirmed_page(&plan);
    plan["status"] = json!("confirmed");
    plan["confirmedAt"] = json!(now());
    refresh_inventory_checks(&mut plan);
    if !plan["assumptions"].is_array() {
        plan["assumptions"] = json!([]);
    }
    let written = write_plan(policy, root, &plan)?;
    let work_queue = routes
        .iter()
        .filter(|route| route["url"].is_string())
        .map(|route| {
            json!({
                "do": "build",
                "url": route["url"],
                "implementation": route["implementation"],
                "file": route["file"],
                "nodeIds": route["nodeIds"],
                "drawnWidths": route["drawnWidths"],
            })
        })
        .collect::<Vec<_>>();
    let urls = work_queue
        .iter()
        .map(|item| text(&item["url"]).to_owned())
        .collect::<Vec<_>>();
    Ok(json!({
        "status": "confirmed",
        "planId": plan["planId"],
        "workQueue": work_queue,
        "confirmedPage": plan["confirmedPage"],
        "routes": routes,
        "markdownTable": route_markdown(&routes),
        "frameInventory": plan["frameInventory"],
        "linkLabelCheck": plan["linkLabelCheck"],
        "excluded": excluded,
        "designFixes": design_fixes,
        "site": plan["site"],
        "defaults": plan["defaults"],
        "extraAnswers": plan["extraAnswers"],
        "mobileNavigation": plan["mobileNavigation"],
        "customAnswers": custom,
        "rule": IMPLEMENT_INSTRUCTION,
        "writtenTo": written,
        "nextAction": {
            "how": IMPLEMENT_INSTRUCTION,
            "buildAll": urls,
            "until": "every URL in workQueue is built; do not stop after the first route. Check progress with action status.",
        },
    }))
}

/// `assume`: what implementation decided without asking, appended to the
/// confirmed plan so status and the final report carry it.
pub(super) fn assume(input: &ScreenPlanInput, policy: &OutputPolicy) -> Result<Value, DevupError> {
    let root = require_root(input)?;
    let mut plan = read_plan(root)?.ok_or_else(|| {
        invalid("No screen plan exists in this project; run devup_screen_plan action draft first.")
    })?;
    if plan["status"] != "confirmed" {
        return Err(invalid(
            "Assumptions belong to implementation, after confirm. Before confirm, put the decision to the user in the question rounds instead.",
        ));
    }
    if input.assumptions.is_empty() && input.dependency_update.is_none() && input.navigation_targets.is_empty() {
        return Err(invalid(
            "action assume needs assumptions, dependencyUpdate or navigationTargets.",
        ));
    }
    let recorded_at = now();
    // An assumption about a pending decision takes it, unless it is
    // destructive: that one stays unbuilt and goes into the final report.
    let mut decisions = array(&plan["pendingDecisions"]).to_vec();
    for decision in &mut decisions {
        let Some(taken) = input
            .assumptions
            .iter()
            .find(|assumption| decision["id"] == assumption.about.as_str())
        else {
            continue;
        };
        if decision["destructive"] == true {
            decision["status"] = json!("left-unbuilt");
        } else {
            decision["status"] = json!("taken");
            decision["taken"] = json!(taken.decision);
        }
        decision["takenAt"] = json!(recorded_at);
    }
    if !decisions.is_empty() {
        plan["pendingDecisions"] = json!(decisions);
    }
    if let Some(update) = &input.dependency_update {
        plan["dependencies"] = json!({"updatedAt": recorded_at, "bumped": update.bumped,
            "pins": update.pins, "command": UPDATE_STEP});
    }
    let mut list = array(&plan["assumptions"]).to_vec();
    for pin in input
        .dependency_update
        .iter()
        .flat_map(|update| &update.pins)
    {
        list.push(json!({"about": format!("dependency {}", pin.package),
            "decision": format!("pinned to {}", pin.version), "reason": pin.reason, "recordedAt": recorded_at}));
    }
    for assumption in &input.assumptions {
        if assumption.about.trim().is_empty() || assumption.decision.trim().is_empty() {
            return Err(invalid("Every assumption needs about and decision."));
        }
        list.push(
            json!({"about": assumption.about, "decision": assumption.decision,
            "reason": assumption.reason, "recordedAt": recorded_at}),
        );
    }
    let mut remapped = Vec::new();
    for (node, url) in &input.navigation_targets {
        let route = normalize_route(url).filter(|route| valid_route(route)).ok_or_else(|| invalid(format!("navigationTargets[{node}] = {url:?} is not an app route.")))?;
        remap_frame(&mut plan, node, &route, &mut remapped)?;
        list.push(json!({"about": node, "decision": format!("remapped to {route}"), "reason": "navigationTargets in assume", "recordedAt": recorded_at}));
    }
    if !remapped.is_empty() {
        refresh_inventory_checks(&mut plan);
        let mut all = array(&plan["remaps"]).to_vec();
        all.extend(remapped.iter().cloned());
        plan["remaps"] = json!(all);
    }
    plan["assumptions"] = json!(list);
    // An assumption never builds what the plan excluded or sent back for a
    // design fix; the export gate refuses those frames whatever is recorded.
    let blocked = array(&plan["excluded"])
        .iter()
        .map(|row| text(&row["nodeId"]).to_owned())
        .chain(array(&plan["designFixes"]).iter().flat_map(|fix| {
            array(&fix["frames"])
                .iter()
                .map(|frame| text(&frame["nodeId"]).to_owned())
                .collect::<Vec<_>>()
        }))
        .filter(|id| {
            !id.is_empty()
                && input
                    .assumptions
                    .iter()
                    .any(|assumption| assumption.about.contains(id.as_str()))
        })
        .collect::<BTreeSet<_>>();
    let written = write_plan(policy, root, &plan)?;
    Ok(json!({
        "status": "confirmed",
        "planId": plan["planId"],
        "pendingDecisions": plan["pendingDecisions"],
        "stillNotBuilt": blocked.iter().map(|id| json!({"frame": id,
            "why": "Excluded or sent back for a design fix; an assumption cannot build it, and the export gate refuses it."})).collect::<Vec<_>>(),
        "assumptions": plan["assumptions"],
        "remapped": remapped,
        "dependencies": plan["dependencies"],
        "writtenTo": written,
        "how": "Keep implementing. Every assumption appears in status and must be in your final report.",
    }))
}

/// Records a decision the work needs without stopping it.
pub(super) fn defer(input: &ScreenPlanInput, policy: &OutputPolicy) -> Result<Value, DevupError> {
    let root = require_root(input)?;
    let mut plan = read_plan(root)?.ok_or_else(|| {
        invalid("No screen plan exists in this project; run devup_screen_plan action draft first.")
    })?;
    if plan["status"] != "confirmed" {
        return Err(invalid(
            "A decision before confirm is a question: put it in extraQuestions on draft.",
        ));
    }
    let decision = input
        .decision
        .as_ref()
        .ok_or_else(|| invalid("action defer needs decision."))?;
    for (field, value) in [
        ("id", &decision.id),
        ("about", &decision.about),
        ("question", &decision.question),
        ("recommended", &decision.recommended),
    ] {
        if value.trim().is_empty() {
            return Err(invalid(format!("decision.{field} is empty.")));
        }
    }
    let mut decisions = array(&plan["pendingDecisions"]).to_vec();
    decisions.retain(|existing| existing["id"] != decision.id.as_str());
    decisions.push(
        json!({"id": decision.id, "about": decision.about, "question": decision.question,
        "options": decision.options, "recommended": decision.recommended,
        "destructive": decision.destructive, "status": "open", "raisedAt": now()}),
    );
    plan["pendingDecisions"] = json!(decisions);
    let written = write_plan(policy, root, &plan)?;
    Ok(json!({
        "status": "confirmed",
        "pendingDecisions": plan["pendingDecisions"],
        "writtenTo": written,
        "how": if decision.destructive {
            "Recorded. Do not build what it touches; carry on with everything else. It stays unbuilt and is listed in the final report."
        } else {
            include_str!("../text/screen_plan-defer-3281174c.md")
        },
    }))
}

/// Writes one runtime-audit plan per confirmed URL and records where.
pub(super) async fn audit_plan(
    input: &ScreenPlanInput,
    policy: &OutputPolicy,
    artifacts: &ArtifactStore,
) -> Result<Value, DevupError> {
    let root = require_root(input)?;
    let mut plan = read_plan(root)?.ok_or_else(|| {
        invalid("No screen plan exists in this project; run devup_screen_plan action draft first.")
    })?;
    if plan["status"] != "confirmed" {
        return Err(invalid(
            "auditPlan needs the confirmed plan: answer the questions and confirm first.",
        ));
    }
    let config = read_config(root)?.unwrap_or_default();
    let thresholds = config.visual_thresholds.clone().unwrap_or_default();
    let mut result = super::audit_plan::write(
        &plan,
        &super::audit_plan::AuditRequest {
            root,
            base_url: input.base_url.as_deref().unwrap_or("http://localhost:3000"),
            base_urls: &input.base_urls,
            only: &input.urls,
            artifact_ids: &input.artifacts,
            thresholds: &thresholds,
        },
        policy,
        artifacts,
    )
    .await?;
    let mut entries = array(&plan["audit"]["plans"]).to_vec();
    for written in array(&result["plans"]) {
        entries.retain(|entry| entry["url"] != written["url"]);
        entries.push(json!({"url": written["url"], "screen": written["screen"],
            "missingReferences": written["missingReferences"], "planPath": written["planPath"], "reportPath": format!("{}/report.json", text(&written["reportDirectory"]).replace('\\', "/"))}));
    }
    // A dynamic route sampled from its Figma frame is an assumption, and is
    // reported as one.
    let mut assumptions = array(&plan["assumptions"]).to_vec();
    for written in array(&result["plans"]) {
        if let Some(assumption) = written["dynamic"]["assumption"].as_str()
            && !assumptions
                .iter()
                .any(|existing| existing["decision"] == assumption)
        {
            assumptions.push(json!({"about": written["dynamic"]["pattern"], "decision": assumption,
                "reason": "no generateStaticParams and no link to the route on an audited page yet", "recordedAt": now()}));
        }
    }
    plan["assumptions"] = json!(assumptions);
    let max_fix_attempts = config.max_fix_attempts.unwrap_or(3);
    plan["audit"]["plans"] = json!(entries);
    plan["audit"]["maxFixAttempts"] = json!(max_fix_attempts);
    plan["audit"]["generatedAt"] = json!(now());
    result["framePages"] = json!(frame_page_table(&plan));
    plan["audit"]["framePages"] = result["framePages"].clone();
    result["writtenTo"] = write_plan(policy, root, &plan)?;
    result["maxFixAttempts"] = json!(max_fix_attempts);
    result["doneWhen"] = json!(DONE_RULE);
    Ok(result)
}

pub(super) const DONE_RULE: &str = include_str!("../text/screen_plan-DONE_RULE-95793a55.md");

/// The question ids that place `screen` (a screen id, or a frame node id for
/// a frame split into its own page).
fn placing_questions(plan: &Value, screen: &str) -> Vec<String> {
    array(&plan["questions"])
        .iter()
        .map(|question| text(&question["id"]))
        .filter(|id| {
            ["route:", "review:", "modal:", "group:", "split-route:", "collision:"]
                .iter()
                .any(|prefix| id.strip_prefix(prefix) == Some(screen))
        })
        .map(str::to_owned)
        .collect()
}

fn suggested_route(name: &str, screen: &str, taken: &str) -> String {
    let words = name
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let base = if words.is_empty() {
        format!("/page-{}", screen.replace([':', ';'], "-"))
    } else {
        format!("/{}", words.join("-"))
    };
    if base == taken {
        format!("{base}-{}", screen.replace([':', ';'], "-"))
    } else {
        base
    }
}

/// Screens on one URL: the error names every colliding pair, the questions
/// that place each, and a distinct URL for the later screen, so the caller can
/// retry confirm with all changed answers at once instead of one per call.
fn duplicate_routes(plan: &Value, collisions: &[(String, String, Value, Value)]) -> DevupError {
    let mut conflicts = Vec::new();
    let mut retry = serde_json::Map::new();
    let mut taken = collisions.iter().map(|c| c.0.clone()).collect::<BTreeSet<_>>();
    for (url, first, second, name) in collisions {
        let second = text(second);
        let first_name = array(&plan["screens"])
            .iter()
            .find(|screen| screen["id"] == first.as_str())
            .map(|screen| screen["name"].clone())
            .unwrap_or(Value::Null);
        let mut suggestion = suggested_route(text(name), second, url);
        while !taken.insert(suggestion.clone()) {
            suggestion = format!("{suggestion}-{}", second.replace([':', ';'], "-"));
        }
        let questions = placing_questions(plan, second);
        if let Some(id) = questions.iter().find(|id| !id.starts_with("collision:")) {
            retry.insert(id.clone(), json!(suggestion));
        }
        conflicts.push(json!({
            "url": url,
            "screens": [
                {"screen": first, "name": first_name, "questions": placing_questions(plan, first)},
                {"screen": second, "name": name, "questions": questions, "suggestedUrl": suggestion},
            ],
        }));
    }
    let (url, first, second, _) = &collisions[0];
    let retry = Value::Object(retry);
    DevupError::with_details(
        ErrorCode::DevupInvalidInput,
        format!(
            "Two screens resolve to {url} ({first} and {}); {} URL conflict(s) in total. Nothing was written. Retry confirm with the same answers plus {retry}, or show the user the conflicting screens and ask which one owns each URL.",
            text(second),
            collisions.len()
        ),
        false,
        json!({
            "url": url,
            "conflict": conflicts[0]["screens"],
            "conflicts": conflicts,
            "retryAnswers": retry,
        }),
    )
}
fn screen_table(plan: &Value) -> Vec<Value> {
    array(&plan["screens"])
        .iter()
        .map(|screen| {
            json!({
                "id": screen["id"],
                "name": screen["name"],
                "kind": screen["kind"],
                "drawnWidths": screen["drawnWidths"],
                "proposedRoute": screen["proposedRoute"],
                "routeEvidence": screen["routeEvidence"],
                "existingFile": screen["existingFile"],
                "sharedParts": screen["sharedParts"],
                "openDecisions": screen["openDecisions"],
            })
        })
        .collect()
}

fn cell(value: &Value) -> String {
    let rendered = match value {
        Value::Null => "-".to_owned(),
        Value::String(text) => text.clone(),
        Value::Array(items) if items.is_empty() => "-".to_owned(),
        Value::Array(items) => items.iter().map(cell).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    };
    rendered.replace('|', "\\|").replace('\n', " ")
}

fn route_markdown(routes: &[Value]) -> String {
    let mut out = String::from(
        "| URL | Figma screen | Figma page | nodeIds | widths | file | implementation | app |\n|---|---|---|---|---|---|---|---|\n",
    );
    for row in routes {
        let url = if row["implementation"] == "state" {
            format!("(state on {})", cell(&row["hostUrl"]))
        } else {
            cell(&row["url"])
        };
        out.push_str(&format!(
            "| {url} | {} | {} | {} | {} | {} | {} | {} |\n",
            cell(&row["name"]),
            cell(&row["pages"]),
            cell(&row["nodeIds"]),
            cell(&row["drawnWidths"]),
            cell(&row["file"]),
            cell(&row["implementation"]),
            cell(&row["app"]),
        ));
    }
    out
}

/// The Markdown twin of the plan: the same content, for a person.
fn markdown(plan: &Value) -> String {
    let mut out = format!(
        "# Screen plan {}\n\nStatus: **{}**. Source: {}.\n",
        cell(&plan["planId"]),
        cell(&plan["status"]),
        cell(&plan["source"]["url"]),
    );
    if let Some(notes) = plan["notes"].as_str() {
        out.push_str(&format!("\nNotes: {notes}\n"));
    }
    if let Some(note) = plan["inventoryNote"].as_str() {
        out.push_str(&format!("\n> {note}\n"));
    }
    out.push_str("\n## Screens\n\n| Figma screen | nodeId | widths | proposed route | evidence | existing file | shared parts | open decisions |\n|---|---|---|---|---|---|---|---|\n");
    for screen in array(&plan["screens"]) {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            cell(&screen["name"]),
            cell(&screen["id"]),
            cell(&screen["drawnWidths"]),
            cell(&screen["proposedRoute"]),
            cell(&screen["routeEvidence"]),
            cell(&screen["existingFile"]),
            cell(&screen["sharedParts"]),
            cell(&screen["openDecisions"]),
        ));
    }
    if plan["frameInventory"].is_object() {
        out.push_str(
            "\n## Frame inventory\n\n| Frame | nodeId | disposition | route |\n|---|---|---|---|\n",
        );
        for frame in array(&plan["frameInventory"]["frames"]) {
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                cell(&frame["name"]),
                cell(&frame["nodeId"]),
                cell(&frame["disposition"]),
                cell(&frame["route"])
            ));
        }
        out.push_str(&format!("\n{}\n", text(&plan["frameInventory"]["rule"])));
        out.push_str(
            "\n## Link labels\n\n| On frame | Label | state | resolved |\n|---|---|---|---|\n",
        );
        for label in array(&plan["linkLabelCheck"]["labels"]) {
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                cell(&label["on"]),
                cell(&label["label"]),
                cell(&label["state"]),
                cell(&label["resolved"])
            ));
        }
    }
    let questions = array(&plan["questions"]);
    if !questions.is_empty() {
        out.push_str(&format!("\n## Open questions\n\n{ASK_INSTRUCTION}\n\n"));
        for question in questions {
            let id = text(&question["id"]);
            out.push_str(&format!(
                "- **{}** (`{id}`): {}\n",
                cell(&question["header"]),
                text(&question["question"])
            ));
            for option in array(&question["options"]) {
                out.push_str(&format!(
                    "  - {}: {}\n",
                    text(&option["label"]),
                    text(&option["description"])
                ));
            }
            if let Some(answer) = plan["answers"].get(id) {
                out.push_str(&format!("  - Answer: {}\n", cell(answer)));
            }
        }
    }
    let routes = array(&plan["routes"]);
    if !routes.is_empty() {
        out.push_str("\n## Confirmed routes\n\n");
        out.push_str(&route_markdown(routes));
    }
    let lists = trailing_lists(plan);
    if !lists.is_empty() {
        out.push_str("\n## Not built, and why\n");
        out.push_str(&lists);
    }
    out
}

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
    let root = require_root(input)?;
    let plan = read_plan(root)?.ok_or_else(|| {
        invalid("No screen plan exists in this project; run devup_screen_plan action draft first.")
    })?;
    if plan["status"] != "confirmed" {
        return Ok(json!({
            "status": "not-confirmed",
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
