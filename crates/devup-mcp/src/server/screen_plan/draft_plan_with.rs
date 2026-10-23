/// [`draft_plan`] with Figma nodes that may become the favicon or share
/// image.
pub(super) fn draft_plan_with(
    screens: &[PlanScreenInput],
    assets: &[PlanAssetInput],
    extra: &[ExtraQuestionInput],
    targets: &BTreeMap<String, String>,
    evidence: &ProjectEvidence,
    source: Value,
    notes: Option<&str>,
) -> Result<Value, DevupError> {
    let hit = ["projection", "candidates", "atCeiling"]
        .iter()
        .filter(|field| source["truncation"][**field] == true)
        .copied()
        .collect::<Vec<_>>();
    if !hit.is_empty() {
        let mut sections = Vec::<String>::new();
        for screen in screens {
            let label = match (screen.section_name.as_deref(), screen.section.as_deref()) {
                (Some(name), Some(id)) => format!("\"{name}\" ({id})"),
                (Some(name), None) => format!("\"{name}\""),
                (None, Some(id)) => id.to_owned(),
                _ => continue,
            };
            if !sections.contains(&label) {
                sections.push(label);
            }
        }
        let listed = if sections.is_empty() {
            "none of the returned frames sits in a Section".to_owned()
        } else {
            sections.join(", ")
        };
        let section_calls = screens.iter().filter_map(|screen| screen.section.as_ref().map(|id| (id.clone(), screen.section_name.clone())))
            .collect::<BTreeMap<_, _>>().into_iter().map(|(id, name)| {
                let url = source["url"].as_str().and_then(|url| url.split('?').next()).map(|base| format!("{base}?node-id={}", id.replace(':', "-")));
                json!({"nodeId": id, "name": name, "draftCall": {"tool": "devup_screen_plan", "arguments": {"action": "draft", "url": url}}})
            }).collect::<Vec<_>>();
        return Err(DevupError::with_details(ErrorCode::DevupInvalidInput, format!(
            "The frame inventory is incomplete: exploration was truncated or reached its ceiling ({}). Sections seen so far: {listed}. Call draft with one Section link at a time (devup_figma_search lists the page's top-level SECTION/FRAME nodes) or supply the complete frame list before drafting.",
            hit.join(", ")
        ), false, json!({"frameCount": screens.len(), "limits": {"maxScreens": MAX_SCREENS, "triggers": hit},
            "sections": section_calls, "nextAction": {"tool":"devup_screen_plan", "arguments": {"action":"draft", "screens":"complete combined frame inventory from the listed Sections"}},
            "merge": "Collect all Section inventories, combine screens, and draft once; separate drafts replace the saved plan."})));
    }
    let mut ids = BTreeSet::new();
    if screens
        .iter()
        .any(|screen| screen.node_id.trim().is_empty() || !ids.insert(&screen.node_id))
    {
        return Err(invalid(
            "Frame inventory requires a non-empty, unique nodeId for every frame.",
        ));
    }
    if screens.is_empty() {
        return Err(invalid(
            "draft found no screens. Link a Section or frame, or pass screens from devup_figma_explore.",
        ));
    }
    if screens.len() > MAX_SCREENS {
        return Err(invalid(format!(
            "draft plans at most {MAX_SCREENS} screens; link one Section at a time."
        )));
    }
    // Only what the user pointed at, and never what is marked as not for
    // implementation. Both are listed, not dropped.
    let anchor_id = source["nodeId"].as_str();
    let anchor_name = source["anchorName"].as_str();
    let linked = array(&source["anchorBreadcrumb"])
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .chain(anchor_name.map(str::to_owned))
        .collect::<Vec<_>>();
    let mut out_of_scope = Vec::new();
    let mut excluded = Vec::new();
    let mut kept = Vec::new();
    for screen in screens {
        if !in_scope(screen, anchor_id, anchor_name) {
            out_of_scope.push(json!({"nodeId": screen.node_id, "name": screen.name,
                "reason": "not under the linked node; never a candidate"}));
            continue;
        }
        match exclusion_reason(screen, &linked, &evidence.config) {
            Some(reason) => excluded.push((screen, reason)),
            None => kept.push(screen.clone()),
        }
    }
    if kept.is_empty() && excluded.is_empty() {
        return Err(invalid(
            "No screen is under the linked node. Link the Section or frame to plan; the plan never widens past it.",
        ));
    }
    let mut questions = Vec::new();
    let mut base_when = Vec::new();
    let linked_names = linked
        .iter()
        .map(|name| ("linked node", name.as_str()))
        .collect::<Vec<_>>();
    if let Some(reason) = excluded_by(&linked_names, &evidence.config) {
        questions.push(question(
            "scope".into(),
            "Linked area: build it?".into(),
            format!(
                "The link you gave points inside an area whose name marks it as not for implementation: {reason}. Plan the screens under it anyway?"
            ),
            vec![
                choice(
                    PLAN_ANYWAY,
                    "You linked this area on purpose; plan its screens.",
                    json!({"scope": "use"}),
                ),
                choice(
                    NOTHING_HERE,
                    "Build nothing from this link.",
                    json!({"scope": "none"}),
                ),
            ],
            "scope",
        ));
        base_when.push(when("scope", PLAN_ANYWAY, json!({"scope": "use"})));
    }
    if !excluded.is_empty() {
        let listed = excluded
            .iter()
            .take(8)
            .map(|(screen, reason)| format!("\"{}\" ({}): {reason}", screen.name, screen.node_id))
            .collect::<Vec<_>>()
            .join("; ");
        let more = if excluded.len() > 8 {
            format!("; and {} more listed in the plan", excluded.len() - 8)
        } else {
            String::new()
        };
        questions.push(with_when(
            question(
                "exclusions".into(),
                "Excluded areas".into(),
                format!(
                    "These {} frames were excluded as trash, archive, draft, copy or hidden, and will not be built: {listed}{more}. Accept the exclusions?",
                    excluded.len()
                ),
                vec![
                    choice(
                        ACCEPT_EXCLUSIONS,
                        "None of them is built; they stay listed in the plan.",
                        json!({"exclusions": "accept"}),
                    ),
                    choice(
                        REVIEW,
                        "Ask about each one in the next round.",
                        json!({"exclusions": "review"}),
                    ),
                ],
                "exclusions",
            ),
            &base_when,
        ));
        let review = when("exclusions", REVIEW, json!({"exclusions": "review"}));
        for (screen, reason) in &excluded {
            let id = &screen.node_id;
            let conditions = [base_when.clone(), vec![review.clone()]].concat();
            questions.push(with_when(
                question(
                    format!("exclusion:{id}"),
                    header("Excluded", &screen.name),
                    format!(
                        "\"{}\" ({id}) was excluded: {reason}. Keep it out of the build?",
                        screen.name
                    ),
                    vec![
                        choice(
                            KEEP_EXCLUDED,
                            "Not built; listed as excluded.",
                            json!({"build": false}),
                        ),
                        choice(
                            BUILD_IT,
                            "Build it; its URL is asked next.",
                            json!({"build": true}),
                        ),
                    ],
                    "exclusion",
                ),
                &conditions,
            ));
            let screen_slug = slug(&base_words(&screen.name));
            let mut choices = Vec::new();
            if !screen_slug.is_empty() {
                choices.push(choice(
                    format!("/{screen_slug}"),
                    "Named after the layer.",
                    json!({"route": format!("/{screen_slug}")}),
                ));
            }
            choices.push(choice(
                SKIP,
                "Leave it out after all.",
                json!({"skip": true}),
            ));
            if choices.len() < 2 {
                choices.push(choice(
                    NOT_A_PAGE,
                    "It is a section or state of another screen; it gets no URL.",
                    json!({"notPage": true}),
                ));
            }
            let built = [
                conditions,
                vec![when(
                    &format!("exclusion:{id}"),
                    BUILD_IT,
                    json!({"build": true}),
                )],
            ]
            .concat();
            questions.push(with_when(
                question(
                    format!("exclusion-route:{id}"),
                    header("Route", &screen.name),
                    format!("Which URL should \"{}\" ({id}) be served at?", screen.name),
                    choices,
                    "route",
                ),
                &built,
            ));
        }
    }
    let excluded_rows = excluded
        .iter()
        .map(|(screen, reason)| {
            json!({"nodeId": screen.node_id, "name": screen.name, "width": screen.width,
                "section": screen.section_name.clone().or_else(|| screen.section.clone()),
                "pageId": screen.page_id,
                "pageName": screen.page_name.clone().or_else(|| screen.breadcrumb.first().cloned()),
                "reason": reason})
        })
        .collect::<Vec<_>>();
    // A frame named like a logo or a share image is a site asset, not a page.
    let mut site_assets = assets
        .iter()
        .filter_map(|asset| {
            asset_kind(&asset.name).map(|kind| (asset.node_id.clone(), asset.name.clone(), kind))
        })
        .collect::<Vec<_>>();
    kept.retain(|screen| match asset_kind(&screen.name) {
        Some(kind) => {
            site_assets.push((screen.node_id.clone(), screen.name.clone(), kind));
            false
        }
        None => true,
    });
    let mobile_menu_frame = kept.iter().any(|screen| {
        let name = screen.name.to_lowercase();
        name.contains("drawer") || (name.contains("mobile") && name.contains("menu"))
    });
    let navigation_risk = kept.iter().any(|screen| screen.navigation_content_width.is_some_and(|width| width > 390.0))
        || evidence.sources.iter().filter_map(|path| std::fs::read_to_string(evidence.root.join(path)).ok())
            .any(|source| array(&devup_mcp_devup_ui::ui_layout::layout_risks(&source, None)["risks"])
                .iter().any(|risk| risk["rule"] == "navigation-fit"));
    if navigation_risk && !mobile_menu_frame {
        questions.push(with_when(question("mobile-menu".into(), "Mobile menu".into(),
            "The measured header navigation exceeds the narrow viewport and no mobile menu frame was supplied. Which mobile navigation should be built?".into(),
            vec![choice("Drawer behind a menu button (Recommended)", "Below the navigation content-fit breakpoint, show a menu button opening an accessible drawer.", json!({"mobileMenu":"drawer", "breakpoint":"navigation-content-fit"})),
                choice("Request a mobile menu design", "Leave mobile navigation pending a Figma menu frame.", json!({"mobileMenu":"design-required"}))], "mobile-menu"), &base_when));
    }
    for asked in extra_questions(extra)? {
        questions.push(with_when(asked, &base_when));
    }
    let (screen_apps, app_questions, app_default) = assign_apps(evidence, &kept);
    for asked in app_questions {
        questions.push(with_when(asked, &base_when));
    }
    // One app for the whole plan: its routes are the only route evidence.
    let chosen = screen_apps
        .values()
        .map(|app| app.dir.clone())
        .collect::<BTreeSet<_>>();
    let scoped;
    let evidence = match (chosen.len(), screen_apps.values().next()) {
        (1, Some(app))
            if app_default
                .as_ref()
                .is_some_and(|item| item["mechanism"] == "decided") =>
        {
            let mut narrowed = evidence.clone();
            narrowed
                .routes
                .retain(|(_, page)| page.starts_with(&format!("{}/", app.dir)));
            narrowed.app_dir = app.path(app.app_router.as_deref().unwrap_or("src/app"));
            narrowed.target_app = Some(app.clone());
            scoped = narrowed;
            &scoped
        }
        _ => evidence,
    };
    // A mixed file builds into several apps: each screen's routes come from
    // its own app, never from another app's pages.
    let per_app = if chosen.len() > 1 {
        chosen
            .iter()
            .filter_map(|dir| {
                let app = screen_apps.values().find(|app| &app.dir == dir)?;
                let mut narrowed = evidence.clone();
                narrowed
                    .routes
                    .retain(|(_, page)| page.starts_with(&format!("{dir}/")));
                narrowed.app_dir = app.path(app.app_router.as_deref().unwrap_or("src/app"));
                narrowed.target_app = Some(app.clone());
                Some((dir.clone(), narrowed))
            })
            .collect::<BTreeMap<_, _>>()
    } else {
        BTreeMap::new()
    };
    let evidence_for = |frames: &[Frame]| -> &ProjectEvidence {
        frames
            .first()
            .and_then(|frame| screen_apps.get(&frame.node_id))
            .and_then(|app| per_app.get(&app.dir))
            .unwrap_or(evidence)
    };
    for asked in stack_questions(evidence) {
        questions.push(with_when(asked, &base_when));
    }
    let defaults = [
        app_default.into_iter().collect::<Vec<_>>(),
        stack_defaults(evidence),
        site_defaults(evidence, &kept),
    ]
    .concat();
    let cover = kept
        .first()
        .map(|screen| (screen.node_id.clone(), screen.name.clone()));
    let (site, site_asked) = site_questions(
        evidence,
        &site_assets,
        cover
            .as_ref()
            .map(|(node, name)| (node.as_str(), name.as_str())),
    );
    for asked in site_asked {
        questions.push(with_when(asked, &base_when));
    }
    let linked_page = source["nodePage"]["id"].as_str().or_else(|| {
        screens.iter().find(|screen| Some(screen.node_id.as_str()) == anchor_id)
            .and_then(|screen| screen.page_id.as_deref())
    });
    let mut pages = kept.iter().filter_map(|screen| {
        Some((screen.page_id.clone()?, screen.page_name.clone().or_else(|| screen.breadcrumb.first().cloned())?))
    }).collect::<BTreeMap<_, _>>().into_iter().collect::<Vec<_>>();
    pages.sort_by(|a, b| {
        let rank = |name: &str| {
            let lower = name.to_lowercase();
            let preferred = ["시안", "확정", "final", "최종"].iter().any(|word| lower.contains(word));
            let date = name.chars().filter(char::is_ascii_digit).collect::<String>();
            (preferred, date)
        };
        rank(&b.1).cmp(&rank(&a.1)).then_with(|| a.0.cmp(&b.0))
    });
    let competing_pages = kept.iter().any(|screen| kept.iter().any(|other| {
        screen.page_id != other.page_id && base_words(&screen.name) == base_words(&other.name)
    }));
    if linked_page.is_none() && competing_pages {
        questions.insert(0, question("base-page".into(), "Base Figma page".into(),
            "Screens span several Figma pages. Which page is the implementation source?".into(),
            pages.iter().map(|(id, name)| choice(name, "Use frames from this owning page only.", json!({"pageId": id}))).collect(), "base-page"));
    }
    let page_sets = if pages.len() > 1 {
        pages.iter().filter(|(id, _)| linked_page.is_none_or(|linked| linked == id))
            .map(|(id, _)| kept.iter().filter(|screen| screen.page_id.as_deref() == Some(id)).cloned().collect::<Vec<_>>())
            .collect::<Vec<_>>()
    } else { vec![kept.clone()] };
    let mut drafts = page_sets.iter().flat_map(|screens| group_screens(screens, &evidence.config)).collect::<Vec<_>>();
    if linked_page.is_none() && competing_pages {
        for draft in &mut drafts {
            if let Some(page) = draft.frames.first().and_then(|frame| frame.page_id.as_ref()) {
                draft.extra_when.push(when("base-page", "", json!({"pageId": page})));
            }
        }
    }
    for draft in &mut drafts {
        draft.extra_when.extend(base_when.iter().cloned());
    }
    for oddity in structure_questions(&drafts) {
        let first = drafts[oddity.screens[0]].id().to_owned();
        let id = format!("structure:{}:{first}", oddity.kind);
        let frames = oddity
            .screens
            .iter()
            .flat_map(|index| drafts[*index].frames.iter().map(Frame::json))
            .collect::<Vec<_>>();
        let mut asked = question(
            id.clone(),
            header("Figma structure", drafts[oddity.screens[0]].name()),
            format!(
                "{} Is that intended, or should the Figma file be fixed? This plan does not decide which frame is right.",
                oddity.facts
            ),
            structure_choices(&oddity),
            "structure",
        );
        asked["fix"] = json!({"frames": frames, "text": oddity.fix});
        questions.push(with_when(asked, &base_when));
        for index in &oddity.screens {
            let placed = oddity.states.is_some() && *index == oddity.screens[0];
            drafts[*index].extra_when.push(when(
                &id,
                oddity.keep,
                json!({"structure": if placed { "placed" } else { "keep" }}),
            ));
            drafts[*index].structure.push(id.clone());
        }
    }
    let mut screen_notes = vec![Vec::new(); drafts.len()];
    let mut decided = drafts
        .iter()
        .enumerate()
        .map(|(index, screen)| {
            let hits = route_hits(
                &screen.frames,
                &screen.words,
                targets,
                evidence_for(&screen.frames),
                &mut screen_notes[index],
            );
            let proposed = agreed(&hits).map(|hit| (hit.route.clone(), hit.kind));
            (hits, proposed)
        })
        .collect::<Vec<_>>();
    // Two screens on one URL is a decision, not a fact: neither keeps it unasked.
    let mut owners = BTreeMap::<String, Vec<usize>>::new();
    for (index, (_, proposed)) in decided.iter().enumerate() {
        if let Some((route, _)) = proposed {
            owners.entry(route.clone()).or_default().push(index);
        }
    }
    let mut route_conflicts = Vec::new();
    for (route, indices) in owners.iter().filter(|(_, indices)| indices.len() > 1) {
        if pages.len() > 1 && indices.iter().filter_map(|index| drafts[*index].frames.first()?.page_id.as_ref()).collect::<BTreeSet<_>>().len() == indices.len() {
            continue;
        }
        route_conflicts.push(json!({"url": route, "screens": indices.iter().map(|index| drafts[*index].id().to_owned()).collect::<Vec<_>>(), "names": indices.iter().map(|index| drafts[*index].name().to_owned()).collect::<Vec<_>>()}));
        for index in indices {
            if let Some((route, _)) = decided[*index].1.take() {
                screen_notes[*index].push(format!(
                    "{route} is also the route of another screen in this plan."
                ));
            }
        }
    }
    let claimed = decided
        .iter()
        .filter_map(|(_, proposed)| proposed.as_ref().map(|(route, _)| route.clone()))
        .collect::<BTreeSet<_>>();
    let modal_words = evidence
        .config
        .modal_keywords
        .clone()
        .unwrap_or_else(|| MODAL_WORDS.iter().map(|word| (*word).to_owned()).collect());
    let kind_evidence = if evidence.config.modal_keywords.is_some() {
        "config"
    } else {
        NAME_MATCH
    };
    let is_modal = |name: &str| {
        let lowered = name.to_lowercase();
        modal_words
            .iter()
            .any(|word| lowered.contains(&word.to_lowercase()))
    };
    let page_screens = drafts
        .iter()
        .enumerate()
        .filter(|(_, screen)| !is_modal(screen.name()))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let frame_owner = drafts
        .iter()
        .enumerate()
        .flat_map(|(index, screen)| {
            screen
                .frames
                .iter()
                .map(move |frame| (frame.node_id.clone(), index))
        })
        .collect::<BTreeMap<_, _>>();
    let route_choice = |evidence: &ProjectEvidence, route: &str, why: &str, extra: Value| {
        let description = match existing_page(route, evidence) {
            Some(page) => format!("{why} Exists at {page}; the screen is merged into it."),
            None => format!(
                "{why} New page at {}.",
                planned_page(route, &evidence.app_dir)
            ),
        };
        let mut action = json!({"route": route});
        if let (Some(object), Some(extra)) = (action.as_object_mut(), extra.as_object()) {
            object.extend(extra.clone());
        }
        choice(route, description, action)
    };
    let unclaimed_for =
        |evidence: &ProjectEvidence, words: &[String], exclude: &BTreeSet<String>| {
            let mut unclaimed = evidence
                .routes
                .iter()
                .filter(|(route, _)| !claimed.contains(route) && !exclude.contains(route))
                .map(|(route, _)| {
                    let score = route
                        .trim_matches('/')
                        .split(['/', '-'])
                        .filter(|segment| words.iter().any(|word| word == segment))
                        .count();
                    (score, route.clone())
                })
                .collect::<Vec<_>>();
            unclaimed.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            unclaimed
        };
    let mut rows = Vec::new();
    let mut heuristics = Vec::new();
    let mut part_screens = BTreeMap::<String, Vec<String>>::new();
    for (index, screen) in drafts.iter().enumerate() {
        plan_route_group(
            RouteGroupPlanning {
                evidence_for: &evidence_for,
                is_modal: &is_modal,
                route_choice: &route_choice,
                unclaimed_for: &unclaimed_for,
                decided: &decided,
                claimed: &claimed,
                drafts: &drafts,
                targets,
                screen_apps: &screen_apps,
                kind_evidence,
                page_screens: &page_screens,
                frame_owner: &frame_owner,
                screen_notes: &screen_notes,
                questions: &mut questions,
                rows: &mut rows,
                heuristics: &mut heuristics,
                part_screens: &mut part_screens,
            },
            index,
            screen,
        );
    }
    let route_heuristics = heuristics
        .iter()
        .filter(|item| text(&item["askedIn"]).starts_with("review:"))
        .collect::<Vec<_>>();
    if !route_heuristics.is_empty() {
        let listed = route_heuristics
            .iter()
            .map(|item| format!("{} -> {}", text(&item["name"]), text(&item["decision"])))
            .collect::<Vec<_>>()
            .join(", ");
        questions.push(question(
            "heuristics".into(),
            "Inferred from layer names".into(),
            format!(
                "These were inferred from layer names only: {listed}. Accept all (merged into the existing pages), or review each one?"
            ),
            vec![
                choice(
                    ACCEPT_ALL,
                    "Use every inferred route; existing pages are merged into, not overwritten.",
                    json!({"heuristics": "accept"}),
                ),
                choice(
                    REVIEW,
                    "Ask one question per inferred route, in the next round.",
                    json!({"heuristics": "review"}),
                ),
            ],
            "heuristics",
        ));
    }
    for (part, owners) in part_screens.iter().filter(|(part, owners)| {
        owners.len() > 1
            && !evidence
                .layout_parts
                .iter()
                .any(|existing| compact(existing) == compact(part))
    }) {
        questions.push(question(
            format!("layout:{}", compact(part)),
            header("Shared part", part),
            format!(
                "\"{part}\" appears on {} screens and no layout renders it yet. Extract it once into a shared layout?",
                owners.len()
            ),
            vec![
                choice(
                    "Extract into a shared layout",
                    "Build it once and render it from the App Router layout.",
                    json!({"layout": "shared"}),
                ),
                choice(
                    "Keep it in each page",
                    "Every page renders its own copy.",
                    json!({"layout": "per-page"}),
                ),
            ],
            "layout",
        ));
    }
    // Questions about the whole plan wait on the linked area being planned.
    for asked in questions
        .iter_mut()
        .filter(|asked| asked["id"] == "heuristics" || text(&asked["id"]).starts_with("layout:"))
    {
        *asked = with_when(asked.clone(), &base_when);
    }
    let inventory = frame_inventory(screens, &out_of_scope, &excluded_rows, &kept);
    let labels = link_label_check(screens, &out_of_scope, &excluded_rows, &kept);
    let body = json!({"source": source, "screens": rows, "questions": questions, "notes": notes,
        "frameInventory": inventory, "linkLabelCheck": labels});
    let plan_id = format!(
        "sp-{}",
        &sha256(&serde_json::to_vec(&body).unwrap_or_default())[..16]
    );
    let mut plan = json!({
        "kind": PLAN_KIND,
        "schemaVersion": SCHEMA_VERSION,
        "planId": plan_id,
        "status": "draft",
        "source": body["source"],
        "notes": notes,
        "appDir": evidence.app_dir,
        "inventoryNote": evidence.inventory_note,
        "config": evidence.config_file,
        "screens": body["screens"],
        "heuristics": heuristics,
        "excluded": excluded_rows,
        "outOfScope": out_of_scope,
        "frameInventory": body["frameInventory"],
        "linkLabelCheck": body["linkLabelCheck"],
        "site": site,
        "defaults": defaults,
        "apps": evidence.apps.iter().map(super::framework::App::json).collect::<Vec<_>>(),
        "targetApp": evidence.target_app.as_ref().map(super::framework::App::json),
        "siteAssets": site_assets.iter().map(|(node, name, kind)| json!({"nodeId": node, "name": name, "kind": kind})).collect::<Vec<_>>(),
        "questions": body["questions"],
        "routeConflicts": route_conflicts,
        "answers": {},
        "routes": [],
        "assumptions": [],
    });
    refresh_inventory_checks(&mut plan);
    Ok(plan)
}

/// What became of one listed frame: planned, excluded, outOfScope or
/// siteAsset (a logo or share image taken out of the page list).
fn disposition_of(
    node_id: &str,
    out_of_scope: &[Value],
    excluded_rows: &[Value],
    kept: &[PlanScreenInput],
) -> &'static str {
    let listed = |rows: &[Value]| rows.iter().any(|row| text(&row["nodeId"]) == node_id);
    if listed(out_of_scope) {
        "outOfScope"
    } else if listed(excluded_rows) {
        "excluded"
    } else if kept.iter().any(|screen| screen.node_id == node_id) {
        "planned"
    } else {
        "siteAsset"
    }
}

/// The rule every inventory and label check carries, so an agent reading
/// either one alone still meets it.
const ABSENCE_RULE: &str = "Say a screen does not exist only after searching frameInventory and running devup_figma_search for its name. A name missing from this list was not seen, which is not the same as absent: report it as unverified. outOfScope frames exist in the file but sit outside the linked node.";

/// Every frame the explore listed and what became of it, so "there is no
/// screen for X" is answered from a list that stays in the plan file and in
/// `status`, never from memory of an earlier call.
fn frame_inventory(
    screens: &[PlanScreenInput],
    out_of_scope: &[Value],
    excluded_rows: &[Value],
    kept: &[PlanScreenInput],
) -> Value {
    let frames = screens
        .iter()
        .map(|screen| {
            json!({"nodeId": screen.node_id, "name": screen.name, "width": screen.width,
                "section": screen.section_name.clone().or_else(|| screen.section.clone()),
                "pageId": screen.page_id,
                "pageName": screen.page_name.clone().or_else(|| screen.breadcrumb.first().cloned()),
                "disposition": disposition_of(&screen.node_id, out_of_scope, excluded_rows, kept)})
        })
        .collect::<Vec<_>>();
    json!({"total": frames.len(), "frames": frames, "rule": ABSENCE_RULE})
}

/// Whether a link label names a frame: the same words with widths and
/// device words dropped, or the same letters once spacing is removed.
fn label_names_frame(label: &str, frame: &str) -> bool {
    let label_compact = compact(label);
    if label_compact.is_empty() {
        return false;
    }
    let label_words = base_words(label);
    let frame_words = base_words(frame);
    (!label_words.is_empty() && label_words == frame_words)
        || compact(&frame_words.join("")) == compact(&label_words.join(""))
        || compact(frame) == label_compact
}

/// Link and menu labels with no URL. A tab "Core technology" with no
/// destination hints that a frame for it exists somewhere in the file, so
/// each label is matched against every frame the explore listed, in scope or
/// not. An unresolved label is never pointed at `/`.
fn link_label_check(
    screens: &[PlanScreenInput],
    out_of_scope: &[Value],
    excluded_rows: &[Value],
    kept: &[PlanScreenInput],
) -> Value {
    let mut rows = Vec::new();
    for screen in screens {
        for link in screen.links.iter().filter(|link| link.url.is_none()) {
            if compact(&link.label).is_empty() {
                continue;
            }
            let matches = screens
                .iter()
                .filter(|other| {
                    match link.destination_node_id.as_deref() {
                        Some(destination) => destination == other.node_id,
                        None => label_names_frame(&link.label, &other.name),
                    }
                })
                .map(|other| {
                    json!({"nodeId": other.node_id, "name": other.name,
                        "disposition": disposition_of(&other.node_id, out_of_scope, excluded_rows, kept)})
                })
                .collect::<Vec<_>>();
            let state = if !matches.is_empty() {
                "frame-found"
            } else if link.destination_node_id.is_some() {
                "destination-not-listed"
            } else {
                "unverified"
            };
            let next = match state {
                "frame-found" => {
                    "Plan the matched frame, or bind the label to its route with navigationTargets."
                }
                "destination-not-listed" => {
                    "The prototype names a destination the explore did not list: export or search that node before deciding anything about it."
                }
                _ => {
                    "Run devup_figma_search for this label before saying no screen exists; until then report it as unverified and leave the link unresolved."
                }
            };
            rows.push(json!({"on": screen.node_id, "onName": screen.name,
                "label": link.label, "destinationNodeId": link.destination_node_id,
                "state": state, "frames": matches, "next": next}));
        }
    }
    let unresolved = rows
        .iter()
        .filter(|row| row["state"] != "route-bound")
        .count();
    json!({"labels": rows, "unresolved": unresolved,
        "rule": "A link or menu label with no URL remains unresolved until it is bound to a route or explicitly left unlinked. Search for a matching frame with devup_figma_search before claiming it is absent. Not listed means unverified, not absent. Never point an unresolved label at / or any other guessed route."})
}

/// Reconcile draft diagnostics with confirmed placements and actual link URLs.
fn refresh_inventory_checks(plan: &mut Value) {
    if !plan["frameInventory"].is_object() {
        return;
    }
    let confirmed = plan["status"] == "confirmed";
    let screens = array(&plan["screens"]).to_vec();
    let routes = array(&plan["routes"]).to_vec();
    let excluded = array(&plan["excluded"]).to_vec();
    let fixes = array(&plan["designFixes"]).to_vec();
    if confirmed && let Some(frames) = plan["frameInventory"]["frames"].as_array_mut() {
        for frame in frames {
            let node = frame["nodeId"].clone();
            if frame["disposition"] == "outOfScope" || frame["disposition"] == "siteAsset" {
                continue;
            }
            let placement = routes
                .iter()
                .find(|route| array(&route["nodeIds"]).contains(&node));
            frame["disposition"] = json!(if let Some(route) = placement {
                text(&route["implementation"])
            } else if excluded.iter().any(|row| row["nodeId"] == node) {
                "excluded"
            } else if fixes.iter().any(|fix| array(&fix["frames"])
                .iter()
                .any(|row| row["nodeId"] == node))
            {
                "designFix"
            } else {
                "notBuilt"
            });
            frame["route"] = placement.map_or(Value::Null, |route| route["url"].clone());
        }
    }
    let inventory = array(&plan["frameInventory"]["frames"]).to_vec();
    if let Some(labels) = plan["linkLabelCheck"]["labels"].as_array_mut() {
        for label in labels.iter_mut() {
            let mut matches = array(&label["frames"]).to_vec();
            for frame in &mut matches {
                if let Some(current) = inventory
                    .iter()
                    .find(|row| row["nodeId"] == frame["nodeId"])
                {
                    frame["disposition"] = current["disposition"].clone();
                    frame["route"] = current["route"].clone();
                }
            }
            label["frames"] = json!(matches);
            label["state"] = json!(if !array(&label["frames"]).is_empty() {
                "frame-found"
            } else if label["destinationNodeId"].is_string() {
                "destination-not-listed"
            } else {
                "unverified"
            });
            label["next"] = json!(
                "Bind the label to a verified route or leave it unlinked. Run devup_figma_search before claiming that no destination frame exists."
            );
            let link = screens
                .iter()
                .find(|screen| {
                    array(&screen["frames"])
                        .iter()
                        .any(|frame| frame["nodeId"] == label["on"])
                })
                .and_then(|screen| {
                    array(&screen["links"]).iter().find(|link| {
                        link["label"] == label["label"]
                            && link["destinationNodeId"] == label["destinationNodeId"]
                    })
                });
            label["resolved"] = link.map_or(Value::Null, |link| link["resolved"].clone());
            if confirmed
                && link.is_some_and(|link| link["resolved"]["evidence"] == "confirmed answer")
                && (label["resolved"]["url"].is_null() || label["resolved"]["url"] == "remove")
            {
                label["state"] = json!("left-unlinked");
                label["next"] = json!("Keep this label unlinked as confirmed.");
            } else if label["resolved"]["url"].is_string() {
                label["state"] = json!("route-bound");
                label["next"] = json!("Use the resolved URL for this label.");
            }
        }
        let unresolved = labels
            .iter()
            .filter(|label| label["state"] != "route-bound" && label["state"] != "left-unlinked")
            .count();
        plan["linkLabelCheck"]["unresolved"] = json!(unresolved);
    }
}
