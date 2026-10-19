type RouteDecision = (Vec<RouteHit>, Option<(String, &'static str)>);
type RouteChoiceFn<'a> = dyn Fn(&ProjectEvidence, &str, &str, Value) -> Choice + 'a;
type UnclaimedRoutesFn<'a> =
    dyn Fn(&ProjectEvidence, &[String], &BTreeSet<String>) -> Vec<(usize, String)> + 'a;

struct RouteGroupPlanning<'a, 'e> {
    evidence_for: &'a (dyn Fn(&[Frame]) -> &'e ProjectEvidence + 'a),
    is_modal: &'a (dyn Fn(&str) -> bool + 'a),
    route_choice: &'a RouteChoiceFn<'a>,
    unclaimed_for: &'a UnclaimedRoutesFn<'a>,
    decided: &'a [RouteDecision],
    claimed: &'a BTreeSet<String>,
    drafts: &'a [DraftScreen],
    targets: &'a BTreeMap<String, String>,
    screen_apps: &'a BTreeMap<String, super::framework::App>,
    kind_evidence: &'a str,
    page_screens: &'a [usize],
    frame_owner: &'a BTreeMap<String, usize>,
    screen_notes: &'a [Vec<String>],
    questions: &'a mut Vec<Value>,
    rows: &'a mut Vec<Value>,
    heuristics: &'a mut Vec<Value>,
    part_screens: &'a mut BTreeMap<String, Vec<String>>,
}

fn plan_route_group(context: RouteGroupPlanning<'_, '_>, index: usize, screen: &DraftScreen) {
    let RouteGroupPlanning {
        evidence_for,
        is_modal,
        route_choice,
        unclaimed_for,
        decided,
        claimed,
        drafts,
        targets,
        screen_apps,
        kind_evidence,
        page_screens,
        frame_owner,
        screen_notes,
        questions,
        rows,
        heuristics,
        part_screens,
    } = context;

    let evidence = evidence_for(&screen.frames);
    let (hits, proposed) = &decided[index];
    let modal = is_modal(screen.name());
    let id = screen.id().to_owned();
    let name = screen.name().to_owned();
    let widths = screen.widths();
    let mut decisions = Vec::new();
    let candidates = hits
        .iter()
        .map(|hit| hit.route.clone())
        .collect::<BTreeSet<_>>();
    let screen_slug = slug(&screen.words);
    let new_route = (!screen_slug.is_empty()).then(|| format!("/{screen_slug}"));
    // Questions about this screen as one page apply only if the group is
    // one page; when that is still being asked, they carry the condition.
    let group_id = screen.grouping_question().unwrap_or_default();
    let (single_label, separate_label) = match screen.grouping {
        Grouping::Sections { .. } => (SECTIONS_SINGLE, SECTIONS_SEPARATE),
        _ => (SINGLE, SEPARATE),
    };
    let mut single_when = screen.extra_when.clone();
    if !matches!(screen.grouping, Grouping::Fixed(_)) {
        single_when.push(when(&group_id, single_label, json!({"group": "single"})));
    }
    let push = |questions: &mut Vec<Value>, mut question: Value, extra: &[Value]| {
        let conditions = single_when.iter().chain(extra).cloned().collect::<Vec<_>>();
        if !conditions.is_empty() {
            question["when"] = json!(conditions);
        }
        questions.push(question);
    };
    let grouping_question = match &screen.grouping {
        Grouping::Fixed(_) => None,
        Grouping::Ask {
            recommend_single,
            reason,
        } => {
            let single = choice(
                SINGLE,
                "One URL; the widths become devup-ui responsive arrays (responsiveTsx).",
                json!({"group": "single"}),
            );
            let separate = choice(
                SEPARATE,
                "Each frame is its own page and gets its own route, asked next.",
                json!({"group": "separate"}),
            );
            Some(question(
                group_id.clone(),
                header("Pages", &name),
                format!(
                    "These frames share the name \"{name}\" and are {reason}: {}. One responsive page, or separate pages? A custom answer may list one route per frame, e.g. \"/a, /b\".",
                    describe_frames(&screen.frames)
                ),
                if *recommend_single {
                    vec![single, separate]
                } else {
                    vec![separate, single]
                },
                "group",
            ))
        }
        Grouping::Sections {
            all_device,
            sections,
            why,
        } => {
            let separate = choice(
                SECTIONS_SEPARATE,
                "Each frame is its own page; its URL is asked next.",
                json!({"group": "separate"}),
            );
            let single = choice(
                SECTIONS_SINGLE,
                if *all_device {
                    format!(
                        "Inferred from the section names ({}), which name only devices.",
                        sections.join(", ")
                    )
                } else {
                    "The sections would only split one page by device.".to_owned()
                },
                json!({"group": "single"}),
            );
            let fix = choice(
                FIX_FIGMA,
                "These frames are not built. They are listed under Design fixes requested; everything else goes on.",
                json!({"designFix": true}),
            );
            let mut asked = question(
                group_id.clone(),
                header("Sections", &name),
                format!(
                    include_str!("../text/screen_plan-draft_plan_with-140b58b5.md"),
                    sections.join(", "),
                    describe_frames(&screen.frames),
                    why = why
                ),
                if *all_device {
                    vec![single, separate, fix]
                } else {
                    vec![separate, single, fix]
                },
                "group",
            );
            asked["fix"] = json!({
                "frames": screen.frames.iter().map(Frame::json).collect::<Vec<_>>(),
                "text": format!(
                    "Frames {} share the name \"{name}\" but sit in sections {}. Move them into one section if they are one page, or rename them if they are different pages.",
                    describe_frames(&screen.frames),
                    sections.join(", ")
                ),
            });
            Some(asked)
        }
    };
    if let Some(asked) = grouping_question {
        decisions.push("one-page-or-separate");
        questions.push(with_when(asked, &screen.extra_when));
        for frame in &screen.frames {
            let mut frame_notes = Vec::new();
            let frame_hits = route_hits(
                std::slice::from_ref(frame),
                &screen.words,
                targets,
                evidence,
                &mut frame_notes,
            );
            let mut choices = Vec::new();
            for hit in frame_hits.iter().filter(|hit| hit.kind != NAME_MATCH) {
                choices.push(route_choice(
                    evidence,
                    &hit.route,
                    &format!("From {}.", hit.detail),
                    json!({}),
                ));
            }
            for hit in frame_hits.iter().filter(|hit| hit.kind == NAME_MATCH) {
                choices.push(route_choice(
                    evidence,
                    &hit.route,
                    "Inferred from the layer name; only one frame can take it.",
                    json!({}),
                ));
            }
            let section_slug = frame
                .section_name
                .as_deref()
                .map(|section| slug(&base_words(section)))
                .unwrap_or_default();
            if !section_slug.is_empty() && !screen_slug.is_empty() {
                choices.push(route_choice(
                    evidence,
                    &format!("/{section_slug}/{screen_slug}"),
                    "Named after its Figma section and layer.",
                    json!({}),
                ));
            }
            for (_, route) in unclaimed_for(evidence, &screen.words, &candidates)
                .iter()
                .filter(|(score, _)| *score > 0)
            {
                choices.push(route_choice(
                    evidence,
                    route,
                    "Existing route with a similar name.",
                    json!({}),
                ));
            }
            choices.truncate(MAX_OPTIONS - 1);
            choices.push(choice(
                SKIP,
                "Leave this frame out of this build.",
                json!({"skip": true}),
            ));
            if choices.len() < 2 {
                choices.push(choice(
                    NOT_A_PAGE,
                    "It is a section or state of another screen; it gets no URL.",
                    json!({"notPage": true}),
                ));
            }
            let asked = question(
                format!("split-route:{}", frame.node_id),
                header("Route", &format!("{} {}", frame.name, frame.width.round())),
                format!(
                    "As a separate page, which URL should \"{}\" ({}, {}px) be served at?",
                    frame.name,
                    frame.node_id,
                    frame.width.round()
                ),
                choices,
                "route",
            );
            let separate_when = [
                screen.extra_when.clone(),
                vec![when(
                    &group_id,
                    separate_label,
                    json!({"group": "separate"}),
                )],
            ]
            .concat();
            questions.push(with_when(asked, &separate_when));
        }
    }
    let route_evidence = match proposed {
        Some((_, kind)) if *kind == NAME_MATCH => json!(NAME_MATCH),
        Some(_) => json!(
            hits.iter()
                .map(|hit| hit.detail.clone())
                .collect::<Vec<_>>()
                .join("; ")
        ),
        None => json!("UNVERIFIED"),
    };
    if modal {
        decisions.push("modal-or-route");
        let mut hosts = page_screens
            .iter()
            .filter(|other| **other != index)
            .map(|other| &drafts[*other])
            .map(|host| {
                let shared = host
                    .words
                    .iter()
                    .filter(|word| screen.words.contains(word))
                    .count();
                (shared, host)
            })
            .collect::<Vec<_>>();
        hosts.sort_by_key(|host| std::cmp::Reverse(host.0));
        let state_choices = hosts
            .iter()
            .take(2)
            .map(|(_, host)| {
                choice(
                    format!("State on {}", truncate(host.name(), 40)),
                    format!(
                        "Open/close state inside the page built from {} ({}); no URL of its own.",
                        host.name(),
                        host.id()
                    ),
                    json!({"state": host.id()}),
                )
            })
            .collect::<Vec<_>>();
        let own_route = proposed
            .as_ref()
            .map(|(route, _)| route_choice(evidence, route, "A page with its own URL.", json!({})))
            .or_else(|| {
                new_route.as_ref().map(|route| {
                    route_choice(
                        evidence,
                        route,
                        "A page with its own URL, named after the layer.",
                        json!({}),
                    )
                })
            });
        let mut choices = Vec::new();
        if proposed
            .as_ref()
            .is_some_and(|(_, kind)| *kind != NAME_MATCH)
        {
            choices.extend(own_route);
            choices.extend(state_choices);
        } else {
            choices.extend(state_choices);
            choices.extend(own_route);
        }
        choices.push(choice(
            SKIP,
            "Leave it out of this build.",
            json!({"skip": true}),
        ));
        let why = if kind_evidence == NAME_MATCH {
            "Its layer name suggests a modal or overlay (name-match)"
        } else {
            "Its layer name matches the project's modalKeywords"
        };
        push(
            questions,
            question(
                format!("modal:{id}"),
                header("Modal", &name),
                format!(
                    "\"{name}\" ({id}): {why}. Is it a state of another screen, or a page with its own URL?"
                ),
                choices,
                "modal",
            ),
            &[],
        );
        if kind_evidence == NAME_MATCH {
            heuristics.push(json!({"screen": id, "name": name, "decision": "kind: modal", "evidence": NAME_MATCH, "askedIn": format!("modal:{id}")}));
        }
    } else if let Some((route, NAME_MATCH)) = proposed {
        decisions.push("route-from-name-match");
        heuristics.push(json!({"screen": id, "name": name, "decision": format!("route: {route}"), "evidence": NAME_MATCH, "askedIn": format!("review:{id}")}));
        let mut choices = vec![
            route_choice(
                evidence,
                route,
                "Keep the inferred route.",
                json!({"collision": "merge"}),
            ),
            choice(
                format!("Overwrite {route}"),
                "Keep the inferred route, but replace the existing page file outright.",
                json!({"route": route, "collision": "overwrite"}),
            ),
        ];
        if let Some(alternative) = new_route
            .as_ref()
            .filter(|candidate| *candidate != route && !claimed.contains(*candidate))
        {
            choices.push(route_choice(
                evidence,
                alternative,
                "Named after the layer.",
                json!({}),
            ));
        }
        choices.push(choice(
            SKIP,
            "Leave it out of this build.",
            json!({"skip": true}),
        ));
        push(
            questions,
            question(
                format!("review:{id}"),
                header("Check route", &name),
                format!(
                    "\"{name}\" ({id}) was matched to {route} by its layer name only. Keep it?"
                ),
                choices,
                "route",
            ),
            &[when("heuristics", REVIEW, json!({"heuristics": "review"}))],
        );
    } else if proposed.is_none() {
        decisions.push(if candidates.len() > 1 {
            "conflicting-route-evidence"
        } else {
            "no-route-evidence"
        });
        let mut choices = Vec::new();
        for route in &candidates {
            choices.push(route_choice(
                evidence,
                route,
                "Named by conflicting evidence.",
                json!({}),
            ));
        }
        let unclaimed = unclaimed_for(evidence, &screen.words, &candidates);
        for (_, route) in unclaimed.iter().filter(|(score, _)| *score > 0) {
            choices.push(route_choice(
                evidence,
                route,
                "Existing route with a similar name.",
                json!({}),
            ));
        }
        if let Some(route) = new_route.as_ref().filter(|route| !claimed.contains(*route)) {
            choices.push(route_choice(
                evidence,
                route,
                "Named after the layer.",
                json!({}),
            ));
        }
        for (_, route) in unclaimed.iter().filter(|(score, _)| *score == 0).take(1) {
            choices.push(route_choice(
                evidence,
                route,
                "Existing route not claimed by another screen.",
                json!({}),
            ));
        }
        choices.truncate(MAX_OPTIONS - 1);
        choices.push(choice(
            SKIP,
            "Leave it out of this build.",
            json!({"skip": true}),
        ));
        if choices.len() < 2 {
            choices.push(choice(
                NOT_A_PAGE,
                "It is a section or state of another screen; it gets no URL.",
                json!({"notPage": true}),
            ));
        }
        let reason = if candidates.len() > 1 {
            format!(
                "The evidence names {} different routes.",
                candidates.iter().cloned().collect::<Vec<_>>().join(", ")
            )
        } else {
            "No existing page, nav link, navigationTargets entry or routeAliases entry names it."
                .to_owned()
        };
        push(
            questions,
            question(
                format!("route:{id}"),
                header("Route", &name),
                format!(
                    "Which URL should the Figma screen \"{name}\" ({id}, drawn at {}) be served at? {reason}",
                    width_list(&widths)
                ),
                choices,
                "route",
            ),
            &[],
        );
    }
    let route = proposed.as_ref().map(|(route, _)| route.as_str());
    let existing = route.and_then(|route| existing_page(route, evidence));
    let name_matched = proposed
        .as_ref()
        .is_some_and(|(_, kind)| *kind == NAME_MATCH);
    if !modal
        && !name_matched
        && let (Some(route), Some(page)) = (route, existing)
    {
        decisions.push("collides-with-existing-page");
        push(
            questions,
            question(
                format!("collision:{id}"),
                header("Existing page", route),
                format!(
                    "{route} already has a page ({page}). What should building \"{name}\" ({id}) do to it?"
                ),
                vec![
                    choice(
                        "Merge into the existing page",
                        "Keep the page file and its data wiring; replace its markup with the screen.",
                        json!({"collision": "merge"}),
                    ),
                    choice(
                        "Overwrite the existing page",
                        "Replace the page file outright; existing wiring is lost.",
                        json!({"collision": "overwrite"}),
                    ),
                    choice(
                        SKIP,
                        "Leave the existing page as it is.",
                        json!({"collision": "skip"}),
                    ),
                ],
                "collision",
            ),
            &[],
        );
    }
    if let Some(route) = route.filter(|route| route.contains('[')) {
        decisions.push("dynamic-segment");
        push(
            questions,
            question(
                format!("dynamic:{id}"),
                header("Dynamic route", route),
                format!(
                    "{route} has a dynamic segment. Where does \"{name}\" get the record it shows?"
                ),
                vec![
                    choice(
                        "Sample data from the design",
                        "Render the Figma content as fixture data for one id; wire real data later.",
                        json!({"data": "sample"}),
                    ),
                    choice(
                        "Wire to the API now",
                        "Fetch the record by the segment value; needs the endpoint from devup_project_context scope api.",
                        json!({"data": "api"}),
                    ),
                ],
                "dynamic",
            ),
            &[],
        );
    }
    if widths.len() > 1 {
        decisions.push("several-widths");
    }
    let mut links = Vec::new();
    for (link_index, link) in screen.links.iter().enumerate() {
        let resolved = link
            .url
            .as_deref()
            .map(|url| json!({"url": url, "evidence": "figma-url"}))
            .or_else(|| {
                let inferred = if link.destination_node_id.is_none() {
                    let owners = drafts
                        .iter()
                        .enumerate()
                        .filter(|(_, draft)| {
                            screen_apps.get(draft.id()).map(|app| &app.dir)
                                == screen_apps.get(screen.id()).map(|app| &app.dir)
                                && draft.frames.iter().any(|frame| {
                                    label_names_frame(&link.label, &frame.name)
                                })
                        })
                        .map(|(index, _)| index)
                        .collect::<Vec<_>>();
                    (owners.len() == 1).then(|| owners[0])
                } else {
                    None
                };
                let destination = link
                    .destination_node_id
                    .as_deref()
                    .or_else(|| inferred.map(|owner| drafts[owner].id()))?;
                frame_owner.get(destination).map(|owner| {
                    json!({"screen": drafts[*owner].id(),
                        "nodeId": link.destination_node_id,
                        "evidence": if inferred.is_some() { "label matches a frame in this plan" } else { "destination frame in this plan" }})
                }).or_else(|| {
                    targets.get(destination).map(|url| {
                        json!({"url": url, "evidence": format!("navigationTargets[{destination}]")})
                    })
                })
            });
        if resolved.is_none() {
            decisions.push("unresolved-link");
            let mut choices = vec![choice(
                "Leave unlinked",
                "Render it without an href; wire it once the destination exists.",
                json!({"link": null}),
            )];
            for (route, _) in evidence.routes.iter().take(MAX_OPTIONS - 1) {
                choices.push(choice(
                    route.as_str(),
                    format!("Link to the existing route {route}."),
                    json!({"link": route}),
                ));
            }
            if choices.len() < 2 {
                choices.push(choice(
                    "Remove the link",
                    "Render the label as plain text.",
                    json!({"link": "remove"}),
                ));
            }
            questions.push(with_when(question(
            format!("link:{id}:{link_index}"),
            header("Link", &link.label),
            format!(
                "\"{}\" on \"{name}\" ({id}) links somewhere this plan cannot see{}. Where should it go?",
                link.label,
                link.destination_node_id
                    .as_deref()
                    .map(|node| format!(" (Figma destination {node})"))
                    .unwrap_or_default()
            ),
            choices,
            "link",
        ), &screen.extra_when));
        }
        links.push(json!({"label": link.label, "destinationNodeId": link.destination_node_id, "resolved": resolved}));
    }
    let mut shared = evidence.layout_parts.clone();
    for part in &screen.parts {
        part_screens
            .entry(part.clone())
            .or_default()
            .push(id.clone());
        if !shared.contains(part) {
            shared.push(part.clone());
        }
    }
    rows.push(json!({
    "id": id,
    "name": name,
    "kind": if modal { "modal" } else { "page" },
    "kindEvidence": if modal { json!(kind_evidence) } else { Value::Null },
    "frames": screen.frames.iter().map(Frame::json).collect::<Vec<_>>(),
    "drawnWidths": widths,
    "app": screen_apps.get(screen.id()).map(|app| app.dir.clone()),
    "appDir": screen_apps.get(screen.id()).map(|app| app.path(app.app_router.as_deref().unwrap_or("src/app"))),
    "groupingQuestion": screen.grouping_question(),
    "structureQuestions": screen.structure,
    "grouping": match &screen.grouping {
        Grouping::Fixed(evidence) => json!({"mode": "decided", "evidence": evidence}),
        Grouping::Sections { all_device, sections, why } => json!({"mode": "ask-sections", "question": group_id,
            "sections": sections, "allDeviceNames": all_device, "why": why}),
        Grouping::Ask { recommend_single, reason } => json!({"mode": "ask", "question": group_id,
            "recommended": if *recommend_single { "single" } else { "separate" }, "reason": reason}),
    },
    "proposedRoute": route,
    "routeEvidence": route_evidence,
    "routeCandidates": candidates,
    "existingFile": existing,
    "sharedParts": shared,
    "links": links,
    "openDecisions": decisions,
    "notes": screen_notes[index],
}));
}
