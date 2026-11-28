/// Routes that statically import a component, from the UI inventory.
fn component_routes(ui: &Value, path: &str) -> Option<(BTreeSet<String>, usize)> {
    let path = normalize_path(path);
    let component = array(&ui["components"])
        .iter()
        .find(|c| text(&c["path"]) == path)?;
    let sites = array(&component["importedBy"]);
    let routes = sites
        .iter()
        .flat_map(|site| {
            array(&site["routes"])
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    Some((routes, sites.len()))
}

fn looks_like_file(target: &str) -> bool {
    target.contains('/')
        || [".tsx", ".ts", ".jsx", ".js"]
            .iter()
            .any(|e| target.ends_with(e))
}

/// Who is affected when a component changes: every screen importing it.
fn change_impact(ctx: &MatrixContext<'_>, paths: &BTreeSet<String>) -> Vec<Value> {
    paths
        .iter()
        .map(|path| match component_routes(ctx.ui, path) {
            Some((routes, sites)) => {
                let scope = match routes.len() {
                    0 if sites == 0 => "unreferenced",
                    0 => "shared-layout-or-unrouted",
                    1 if sites <= 1 => "screen-only",
                    _ => "shared",
                };
                json!({"path":path,"scope":scope,"affectedScreens":routes,"importSites":sites,
                    "basis":"static import reachability; rendering on each screen is unverified until browser-checked",
                    "action":if scope == "shared" {"A change here applies to every affected screen: verify each of them, or make the change a screen-level prop/variant."} else {"A change here stays on its screen."}})
            }
            None => json!({"path":path,"scope":"unknown","affectedScreens":[],
                "basis":"Not in the bounded UI inventory; affected screens are unknown, not none."}),
        })
        .collect()
}

/// Shared configuration (applies to every screen) versus screen-only files.
fn config_scope(ctx: &MatrixContext<'_>) -> Value {
    let mut owners = BTreeSet::from([String::new()]);
    for route in array(&ctx.ui["routes"]) {
        if let Some(owner) = route["appliesTo"].as_str().filter(|o| *o != ".") {
            owners.insert(owner.to_owned());
        }
    }
    let mut shared = vec![];
    for owner in &owners {
        let prefix = if owner.is_empty() {
            String::new()
        } else {
            format!("{owner}/")
        };
        for (file, kind) in [
            ("devup.json", "theme tokens"),
            ("src/app/layout.tsx", "root layout"),
            ("app/layout.tsx", "root layout"),
            ("src/app/globals.css", "global styles"),
            ("app/globals.css", "global styles"),
            (".devup/screen-plan.config.json", "screen-plan config"),
        ] {
            let path = format!("{prefix}{file}");
            if ctx.root.join(&path).is_file() {
                shared
                    .push(json!({"path":path,"kind":kind,"appliesTo":"every screen of this app"}));
            }
        }
    }
    let screen_only = array(&ctx.ui["routes"])
        .iter()
        .map(|r| json!({"path":r["page"],"route":r["route"],"appliesTo":"this screen only"}))
        .collect::<Vec<_>>();
    json!({"shared":shared,"screenOnly":screen_only,
        "rule":"Put a value that only one screen needs in that screen's files; change shared files only when every listed screen should change."})
}

fn evidence_row(e: &RequirementEvidence) -> Value {
    json!({"source":e.source.as_str(),"status":e.status,"detail":e.detail,"routePath":e.route_path,"reference":e.reference})
}

/// Verdict for one requirement: verified, unverified or not-applicable, with its evidence and scope.
fn evaluate_requirement(
    ctx: &MatrixContext<'_>,
    requirement: &TracedRequirement,
    states: &[Value],
    gaps: &[Value],
) -> Value {
    let mut routes = BTreeSet::new();
    let mut components = BTreeSet::new();
    let mut shared = vec![];
    let mut scope_notes = vec![];
    let mut target_states = vec![];
    for target in &requirement.targets {
        if let Some(route) = present(&target.route_path) {
            routes.insert(route.to_owned());
        }
        if let Some(path) = present(&target.component_path) {
            let path = normalize_path(path);
            match component_routes(ctx.ui, &path) {
                Some((found, _)) => {
                    if found.len() > 1 {
                        shared.push(json!({"component":path,"screens":found}));
                    }
                    routes.extend(found);
                }
                None => scope_notes.push(format!(
                    "{path} is not in the UI inventory; screens using it are unknown."
                )),
            }
            components.insert(path);
        }
        if let Some(state) = present(&target.state) {
            target_states.push((target.clone(), normalize_state(state)));
        }
    }
    let nodes = requirement
        .targets
        .iter()
        .filter_map(|t| present(&t.figma_node_id))
        .chain(present(&requirement.figma_node_id))
        .collect::<BTreeSet<_>>();
    let blocking_gaps = nodes
        .iter()
        .flat_map(|node| gaps_for(ctx.payload, gaps, node))
        .collect::<Vec<_>>();
    // States linked by target (component path or screen) and state name.
    let linked = states
        .iter()
        .filter(|s| {
            target_states.iter().any(|(t, state)| {
                text(&s["state"]) == state
                    && (present(&t.component_path).is_some_and(|p| normalize_path(p) == normalize_path(text(&s["target"])))
                        || present(&t.route_path).is_some_and(|r| s["routePath"] == r)
                        || present(&t.figma_node_id).is_some_and(|n| s["figmaNodeId"] == n))
            })
        })
        .map(|s| json!({"target":s["target"],"routePath":s["routePath"],"figmaNodeId":s["figmaNodeId"],"state":s["state"],"verdict":s["verdict"],"source":s["implementationSource"],"visual":s["visual"]["verdict"]}))
        .collect::<Vec<_>>();
    let unlinked_states = target_states
        .iter()
        .filter(|(t, state)| {
            !linked.iter().any(|l| {
                l["state"] == state.as_str()
                    && (present(&t.component_path)
                        .is_some_and(|p| normalize_path(p) == normalize_path(text(&l["target"])))
                        || present(&t.route_path).is_some_and(|r| l["routePath"] == r)
                        || present(&t.figma_node_id).is_some_and(|n| l["figmaNodeId"] == n))
            })
        })
        .map(|(_, s)| s.clone())
        .collect::<Vec<_>>();
    let evidence = requirement
        .evidence
        .iter()
        .map(evidence_row)
        .collect::<Vec<_>>();
    let failing = requirement
        .evidence
        .iter()
        .any(|e| e.status == EvidenceStatus::Fail)
        || linked.iter().any(|l| l["verdict"] == "failed");
    let accepted = |e: &&RequirementEvidence| {
        e.status == EvidenceStatus::Pass
            && match requirement.check {
                RequirementCheck::Interaction | RequirementCheck::Visual => e.source.observed(),
                _ => true,
            }
    };
    let passing = requirement
        .evidence
        .iter()
        .filter(accepted)
        .collect::<Vec<_>>();
    let static_pass_only = requirement
        .evidence
        .iter()
        .any(|e| e.status == EvidenceStatus::Pass && !e.source.observed())
        && passing.is_empty();
    let states_ok = !linked.is_empty() && linked.iter().all(|l| l["verdict"] == "verified");
    let proven = (!passing.is_empty() || states_ok)
        && unlinked_states.is_empty()
        && linked.iter().all(|l| l["verdict"] == "verified");
    // Every screen in scope needs its own evidence once the scope has several.
    let covered = routes
        .iter()
        .filter(|route| {
            routes.len() == 1
                || passing
                    .iter()
                    .any(|e| present(&e.route_path) == Some(route.as_str()))
                || linked
                    .iter()
                    .any(|l| l["routePath"] == route.as_str() && l["verdict"] == "verified")
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    let uncovered = routes.difference(&covered).cloned().collect::<Vec<_>>();
    let needs_browser = matches!(
        requirement.check,
        RequirementCheck::Interaction | RequirementCheck::Visual
    ) && passing.is_empty()
        && !states_ok;
    let (verdict, outcome, reason) = if let Some(reason) =
        present(&requirement.not_applicable_reason)
    {
        ("not-applicable", "not-applicable", reason.to_owned())
    } else if requirement.targets.is_empty() && requirement.figma_node_id.is_none() {
        (
            "unverified",
            "unlinked",
            "Not linked to any screen, component, state or design node.".to_owned(),
        )
    } else if failing {
        (
            "unverified",
            "failed",
            "Evidence or a linked state failed; fix it and verify again.".to_owned(),
        )
    } else if !blocking_gaps.is_empty() {
        ("unverified", "design-data-incomplete", "The linked Figma data is incomplete, so the requirement cannot be judged against the design.".to_owned())
    } else if needs_browser {
        ("unverified", "browser-verification-required", if static_pass_only {
            "Only static evidence was supplied; this user-visible behaviour needs browser or automated-test evidence."
        } else {
            "No observed evidence; this user-visible behaviour needs browser or automated-test evidence."
        }.to_owned())
    } else if !proven {
        (
            "unverified",
            "missing-evidence",
            if unlinked_states.is_empty() {
                "No passing evidence was supplied.".to_owned()
            } else {
                format!(
                    "Linked states have no state check: {}.",
                    unlinked_states.join(", ")
                )
            },
        )
    } else if !uncovered.is_empty() {
        (
            "unverified",
            "scope-not-covered",
            format!(
                "Evidence does not cover every screen in scope: {}.",
                uncovered.join(", ")
            ),
        )
    } else {
        (
            "verified",
            "verified",
            "Passing evidence covers every linked screen and state.".to_owned(),
        )
    };
    json!({"id":requirement.id,"text":requirement.text,"origin":requirement.origin,"figmaNodeId":requirement.figma_node_id,
        "check":requirement.check,"verdict":verdict,"outcome":outcome,"reason":reason,
        "browserVerificationRequired":outcome == "browser-verification-required",
        "scope":{"screens":routes,"components":components,"states":target_states.iter().map(|(_, s)| s).collect::<BTreeSet<_>>(),
            "sharedComponents":shared,"multiScreen":routes.len() > 1,"uncoveredScreens":uncovered,"notes":scope_notes},
        "linkedStates":linked,"evidence":evidence,"designDataGaps":blocking_gaps,
        "attempts":requirement.attempts,"blocker":requirement.blocker})
}

/// The usual way to put a control into a state, for a review step.
fn state_step(target: &str, state: &str) -> String {
    match state {
        "default" => format!("Look at {target} without interacting"),
        "hover" => format!("Move the pointer over {target}"),
        "active" => format!("Press and hold {target}"),
        "focus" => format!("Press Tab until {target} has keyboard focus"),
        "selected" => format!("Select {target}"),
        "disabled" => format!("Bring {target} into its disabled condition"),
        other => format!("Bring {target} into the {other} state"),
    }
}

/// One review entry per change: where to look, in what order, what to expect,
/// and the results separated by who produced them.
fn review_report(input: &MatrixInput<'_>, requirements: &[Value], states: &[Value]) -> Value {
    let derived;
    let changes = if input.changes.is_empty() {
        derived = input
            .requirements
            .iter()
            .map(|r| ReviewChange {
                id: Some(r.id.clone()),
                summary: r.text.clone(),
                route_path: r.targets.iter().find_map(|t| t.route_path.clone()),
                requirement_ids: vec![r.id.clone()],
                ..ReviewChange::default()
            })
            .collect::<Vec<_>>();
        &derived
    } else {
        input.changes
    };
    let entries = changes.iter().map(|change| {
        let linked = requirements
            .iter()
            .filter(|r| change.requirement_ids.iter().any(|id| r["id"] == id.as_str()))
            .collect::<Vec<_>>();
        let route = present(&change.route_path).map(str::to_owned).or_else(|| {
            linked.iter().find_map(|r| array(&r["scope"]["screens"]).first().and_then(Value::as_str).map(str::to_owned))
        });
        let mut steps = change.steps.clone();
        if steps.is_empty() {
            if let Some(route) = &route {
                steps.push(format!("Open {route}"));
            }
            for state in linked.iter().flat_map(|r| array(&r["linkedStates"])) {
                steps.push(state_step(text(&state["target"]), text(&state["state"])));
            }
        }
        let expected = present(&change.expected).map(str::to_owned).unwrap_or_else(|| {
            let texts = linked.iter().map(|r| text(&r["text"])).collect::<Vec<_>>();
            if texts.is_empty() { change.summary.clone() } else { texts.join("; ") }
        });
        let mut results = BTreeMap::<&str, Vec<Value>>::from([("automated", vec![]), ("browser", vec![]), ("manual", vec![]), ("static", vec![])]);
        for requirement in &linked {
            for e in array(&requirement["evidence"]) {
                let bucket = match text(&e["source"]) {
                    "automated-test" => "automated",
                    "browser" => "browser",
                    "manual" => "manual",
                    _ => "static",
                };
                results.get_mut(bucket).unwrap().push(json!({"requirement":requirement["id"],"status":e["status"],"detail":e["detail"],"reference":e["reference"]}));
            }
            for state in array(&requirement["linkedStates"]) {
                let bucket = if state["source"] == "static" { "static" } else { "browser" };
                results.get_mut(bucket).unwrap().push(json!({"requirement":requirement["id"],"state":state["state"],"target":state["target"],"status":state["verdict"]}));
            }
        }
        let captures = states
            .iter()
            .filter(|s| s["verdict"] == "failed" && s["capture"]["diffPath"].is_string())
            .filter(|s| linked.iter().any(|r| array(&r["linkedStates"]).iter().any(|l| l["state"] == s["state"] && l["target"] == s["target"])))
            .map(|s| json!({"state":s["state"],"target":s["target"],"diffPath":s["capture"]["diffPath"]}))
            .collect::<Vec<_>>();
        let verdict = if linked.iter().any(|r| r["verdict"] == "unverified") {
            "unverified"
        } else if !linked.is_empty() && linked.iter().all(|r| r["verdict"] == "not-applicable") {
            "not-applicable"
        } else if linked.is_empty() {
            "unverified"
        } else {
            "verified"
        };
        json!({"id":change.id,"summary":change.summary,"path":route,"files":change.files,"steps":steps,"expected":expected,
            "verdict":verdict,"results":results,"captures":captures,
            "missingReviewPath":route.is_none() && change.steps.is_empty()})
    }).collect::<Vec<_>>();
    json!({"changes":entries,
        "rules":["Give the user each change's path, steps and expected result; keep automated, browser, manual and static results apart.",
            "Attach a capture only for a visual comparison or a failure analysis; write a separate capture summary only when asked."]})
}

const FOLLOW_UP_ORDER: &[&str] = &[
    "1. Find the cause; fix what is inside this request's scope and verify again.",
    "2. Finish any other work that does not depend on the blocked item.",
    "3. Only when a user permission or an external environment change is required: state the unfinished verification and its cause, the recommended fix and what it would verify, and the action or permission needed; ask once.",
    "4. Keep doing unaffected work while waiting for the answer.",
    "5. Report what cannot be resolved with the attempts made and the remaining constraint.",
];

/// Sort every unfinished item into the follow-up order.
fn follow_up(states: &[Value], requirements: &[Value], gaps: &[Value]) -> Value {
    let mut fix = vec![];
    let mut ask = vec![];
    let mut unresolved = vec![];
    let items = states
        .iter()
        .filter(|s| s["verdict"] != "verified")
        .map(|s| {
            (
                format!("state {} {}", text(&s["target"]), text(&s["state"])),
                s,
            )
        })
        .chain(
            requirements
                .iter()
                .filter(|r| r["verdict"] == "unverified")
                .map(|r| (format!("requirement {}", text(&r["id"])), r)),
        );
    for (item, row) in items {
        let attempts = row["attempts"].clone();
        let blocker = &row["blocker"];
        match text(&blocker["kind"]) {
            "permission" | "environment" | "external" => {
                let missing = ["recommendation", "verifies", "userAction"]
                    .into_iter()
                    .filter(|k| blocker[*k].as_str().is_none_or(|s| s.trim().is_empty()))
                    .collect::<Vec<_>>();
                ask.push(json!({"item":item,"kind":blocker["kind"],"cause":blocker["cause"],"recommendation":blocker["recommendation"],
                    "verifies":blocker["verifies"],"userAction":blocker["userAction"],"attempts":attempts,"incompleteRequest":missing}));
            }
            "unresolvable" => unresolved.push(
                json!({"item":item,"attempts":attempts,"remainingConstraint":blocker["cause"]}),
            ),
            _ => {
                let next = if row["designDataGaps"]
                    .as_array()
                    .is_some_and(|g| !g.is_empty())
                {
                    "Re-export the missing nodes with devup_figma_export (or devup_figma_measure for geometry), then rerun this trace."
                } else if row["verdict"] == "failed" || row["outcome"] == "failed" {
                    "Fix the implementation in scope, then capture again and rerun this trace."
                } else if row["browserVerificationRequired"] == true
                    || row["outcome"] == "browser-verification-required"
                {
                    "Observe it in a browser (devup_runtime_audit plan/ingest, or computed styles of the rendered state) and rerun with implementationSource or evidence source browser."
                } else {
                    "Supply the missing design reference or capture; if it cannot exist, record a blocker with its cause and recommendation."
                };
                fix.push(json!({"item":item,"reason":row["reason"],"nextAction":next,"attempts":attempts}));
            }
        }
    }
    json!({"order":FOLLOW_UP_ORDER,"fixInScope":fix,"askUser":ask,"unresolved":unresolved,
        "continueWhileWaiting":!ask.is_empty(),"designDataGapCount":gaps.len()})
}

/// Whether design-match and completion may be claimed, and why not.
fn claims(states: &[Value], requirements: &[Value], gaps: &[Value]) -> Value {
    let mut design = vec![];
    if !gaps.is_empty() {
        design.push(format!(
            "{} Figma node/property gaps: incomplete design data cannot establish a design match.",
            gaps.len()
        ));
    }
    let open_states = states.iter().filter(|s| s["verdict"] != "verified").count();
    if open_states > 0 {
        design.push(format!("{open_states} states are failed or unverified."));
    }
    let mut complete = design.clone();
    let open_requirements = requirements
        .iter()
        .filter(|r| r["verdict"] == "unverified")
        .count();
    if open_requirements > 0 {
        complete.push(format!("{open_requirements} requirements are unverified."));
    }
    let design_match = if states.is_empty() && gaps.is_empty() {
        "not-evaluated"
    } else if design.is_empty() {
        "allowed"
    } else {
        "blocked"
    };
    json!({"designMatch":design_match,"verificationComplete":if complete.is_empty() {"allowed"} else {"blocked"},
        "designMatchBlockedBy":design,"completionBlockedBy":complete})
}

/// The whole matrix, or `None` when the caller supplied none of its inputs.
pub(super) async fn build(ctx: &MatrixContext<'_>, input: &MatrixInput<'_>) -> Option<Value> {
    if input.is_empty() {
        return None;
    }
    let focus = input
        .state_checks
        .iter()
        .filter_map(|c| present(&c.figma_node_id))
        .chain(input.requirements.iter().flat_map(|r| {
            r.targets
                .iter()
                .filter_map(|t| present(&t.figma_node_id))
                .chain(present(&r.figma_node_id))
        }))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let gaps = collect_gaps(ctx, input, &focus);
    let groups = state_groups(ctx, input, &gaps).await;
    let states = groups
        .iter()
        .flat_map(|g| array(&g["states"]).iter().cloned())
        .collect::<Vec<_>>();
    let requirements = input
        .requirements
        .iter()
        .map(|r| evaluate_requirement(ctx, r, &states, &gaps))
        .collect::<Vec<_>>();
    let mut components = input
        .state_checks
        .iter()
        .map(|c| c.target.as_str())
        .filter(|t| looks_like_file(t))
        .chain(
            input
                .requirements
                .iter()
                .flat_map(|r| r.targets.iter().filter_map(|t| present(&t.component_path))),
        )
        .chain(
            input
                .changes
                .iter()
                .flat_map(|c| c.files.iter().map(String::as_str))
                .filter(|t| looks_like_file(t)),
        )
        .map(normalize_path)
        .collect::<BTreeSet<_>>();
    if let Some(path) = input.component_path.filter(|p| !p.trim().is_empty()) {
        components.insert(normalize_path(path));
    }
    let affected_by_gaps = gaps
        .iter()
        .map(|g| {
            let node = text(&g["nodeId"]);
            let hit = |n: &Value| {
                n.as_str().is_some_and(|n| {
                    node == "*"
                        || within(ctx.payload, node, n)
                        || g["referencedBy"]
                            .as_str()
                            .is_some_and(|p| within(ctx.payload, p, n))
                })
            };
            let screens = states
                .iter()
                .filter(|s| hit(&s["figmaNodeId"]))
                .filter_map(|s| s["routePath"].as_str())
                .chain(
                    requirements
                        .iter()
                        .filter(|r| {
                            hit(&r["figmaNodeId"])
                                || array(&r["designDataGaps"])
                                    .iter()
                                    .any(|x| x["nodeId"] == g["nodeId"])
                        })
                        .flat_map(|r| {
                            array(&r["scope"]["screens"])
                                .iter()
                                .filter_map(Value::as_str)
                        }),
                )
                .collect::<BTreeSet<_>>();
            let reqs = requirements
                .iter()
                .filter(|r| {
                    array(&r["designDataGaps"])
                        .iter()
                        .any(|x| x["nodeId"] == g["nodeId"])
                })
                .map(|r| r["id"].clone())
                .collect::<Vec<_>>();
            let state_rows = states
                .iter()
                .filter(|s| hit(&s["figmaNodeId"]))
                .map(|s| json!({"target":s["target"],"state":s["state"]}))
                .collect::<Vec<_>>();
            let mut row = g.clone();
            row["cannotJudge"] = json!({"screens":screens,"requirements":reqs,"states":state_rows});
            row
        })
        .collect::<Vec<_>>();
    let static_results = states
        .iter()
        .filter(|s| s["implementationSource"] == "static")
        .count()
        + requirements
            .iter()
            .flat_map(|r| array(&r["evidence"]))
            .filter(|e| e["source"] == "static")
            .count();
    let observed_results = states
        .iter()
        .filter(|s| s["implementationSource"] != "static" && s["missing"] != true)
        .count()
        + requirements
            .iter()
            .flat_map(|r| array(&r["evidence"]))
            .filter(|e| e["source"] != "static")
            .count();
    let review = review_report(input, &requirements, &states);
    Some(json!({
        "claims":claims(&states, &requirements, &gaps),
        "stateGroups":groups,
        "requirements":requirements,
        "designData":{"complete":gaps.is_empty(),"gaps":affected_by_gaps,
            "rule":"Never declare a design match or a finished verification from incomplete Figma data."},
        "changeImpact":change_impact(ctx, &components),
        "configScope":config_scope(ctx),
        "resultsBySource":{"static":static_results,"observed":observed_results,
            "meaning":"static = read from source; observed = browser, automated test or manual. Static results never prove rendered behaviour."},
        "reviewReport":review,
        "followUp":follow_up(&states, &requirements, &gaps),
    }))
}
