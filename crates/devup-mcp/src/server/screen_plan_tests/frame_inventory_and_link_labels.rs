/// Every listed frame stays in the plan with what became of it, and a menu
/// label with no URL is matched against those frames instead of being
/// pointed at `/`.
#[test]
fn frame_inventory_lists_every_frame_and_link_labels_are_matched() {
    let mut home = screen("40:1", "Home", 1440.0);
    home.links = vec![
        PlanLinkInput {
            label: "Core technology".into(),
            ..PlanLinkInput::default()
        },
        PlanLinkInput {
            label: "Pricing".into(),
            ..PlanLinkInput::default()
        },
        PlanLinkInput {
            label: "Company".into(),
            url: Some("/company".into()),
            ..PlanLinkInput::default()
        },
    ];
    let plan = draft_plan_with(
        &[home, screen("40:2", "Core Technology - 1440", 1440.0)],
        &[],
        &[],
        &BTreeMap::new(),
        &evidence(),
        json!({}),
        None,
    )
    .unwrap();

    let inventory = &plan["frameInventory"];
    assert_eq!(inventory["total"], 2);
    assert!(
        array(&inventory["frames"])
            .iter()
            .all(|frame| frame["disposition"] == "planned"),
        "{inventory}"
    );
    assert!(text(&inventory["rule"]).contains("unverified"));

    let check = &plan["linkLabelCheck"];
    let labels = array(&check["labels"]);
    assert_eq!(labels.len(), 2, "a link with a URL is not checked: {check}");
    let core = labels
        .iter()
        .find(|row| row["label"] == "Core technology")
        .unwrap();
    assert_eq!(core["state"], "frame-found");
    assert_eq!(core["frames"][0]["nodeId"], "40:2");
    let pricing = labels.iter().find(|row| row["label"] == "Pricing").unwrap();
    assert_eq!(pricing["state"], "unverified");
    assert!(text(&pricing["next"]).contains("devup_figma_search"));
    assert_eq!(
        check["unresolved"], 2,
        "a found frame still needs a confirmed route"
    );
    assert_eq!(plan["screens"][0]["links"][0]["resolved"]["screen"], "40:2");
}

#[test]
fn frame_inventory_retains_excluded_assets_and_out_of_scope_frames() {
    let screens = vec![
        sectioned("41:1", "Home", 1440.0, "Site"),
        PlanScreenInput {
            visible: Some(false),
            ..sectioned("41:2", "Hidden", 1440.0, "Site")
        },
        sectioned("41:3", "Logo", 300.0, "Site"),
        sectioned("41:4", "Other", 1440.0, "Elsewhere"),
    ];
    let plan = draft_plan_with(
        &screens,
        &[],
        &[],
        &BTreeMap::new(),
        &evidence(),
        json!({"nodeId": "section-Site"}),
        None,
    )
    .unwrap();
    let frames = array(&plan["frameInventory"]["frames"]);
    assert_eq!(frames.len(), 4);
    assert_eq!(
        frames
            .iter()
            .map(|frame| text(&frame["disposition"]))
            .collect::<Vec<_>>(),
        ["planned", "excluded", "siteAsset", "outOfScope"]
    );
    assert_eq!(
        status_report(&plan, &evidence(), None)["frameInventory"],
        plan["frameInventory"]
    );
    let md = markdown(&plan);
    for screen in screens {
        assert!(md.contains(&screen.node_id));
    }
}

#[test]
fn incomplete_or_duplicate_frame_inventories_are_refused() {
    for field in ["projection", "candidates", "atCeiling"] {
        let mut source = json!({"truncation": {}});
        source["truncation"][field] = json!(true);
        assert!(
            draft_plan(
                &[screen("42:1", "Home", 1440.0)],
                &BTreeMap::new(),
                &evidence(),
                source,
                None
            )
            .unwrap_err()
            .message
            .contains("incomplete")
        );
    }
    assert!(
        draft_plan(
            &[
                screen("42:1", "Home", 1440.0),
                screen("42:1", "Other", 390.0)
            ],
            &BTreeMap::new(),
            &evidence(),
            json!({}),
            None
        )
        .is_err()
    );
}

#[test]
fn explicit_destinations_override_labels_and_ambiguous_labels_stay_unresolved() {
    let mut home = screen("43:1", "Home", 1440.0);
    home.links = vec![
        PlanLinkInput {
            label: "Pricing".into(),
            destination_node_id: Some("missing".into()),
            ..PlanLinkInput::default()
        },
        PlanLinkInput {
            label: "Pricing".into(),
            ..PlanLinkInput::default()
        },
    ];
    let plan = draft_plan(
        &[
            home,
            sectioned("43:2", "Pricing", 1440.0, "A"),
            sectioned("43:3", "Pricing", 390.0, "B"),
        ],
        &BTreeMap::new(),
        &evidence(),
        json!({}),
        None,
    )
    .unwrap();
    assert_eq!(
        plan["linkLabelCheck"]["labels"][0]["state"],
        "destination-not-listed"
    );
    assert!(array(&plan["linkLabelCheck"]["labels"][0]["frames"]).is_empty());
    assert!(plan["screens"][0]["links"][0]["resolved"].is_null());
    for link in array(&plan["screens"][0]["links"]) {
        assert!(link["resolved"]["url"].is_null());
    }
    assert_eq!(plan["linkLabelCheck"]["unresolved"], 2);
}

#[tokio::test]
async fn link_labels_follow_confirmed_routes_and_inventory_survives_status() {
    let fixture = Fixture::new();
    let mut home = screen("44:1", "Home", 1440.0);
    home.links = vec![PlanLinkInput {
        label: "Core technology".into(),
        ..PlanLinkInput::default()
    }];
    let mut draft_input = input(&fixture, "draft");
    draft_input.navigation_targets = BTreeMap::from([("44:1".into(), "/".into())]);
    let result = draft_with(
        &draft_input,
        &[home, screen("44:2", "Core Technology - 1440", 1440.0)],
        &[],
        json!({}),
        &fixture.policy(),
    )
    .await
    .unwrap();
    assert_eq!(result["frameInventory"]["total"], 2);
    let plan = read_plan(fixture.root()).unwrap().unwrap();
    let mut confirm_input = input(&fixture, "confirm");
    confirm_input.plan_id = Some(text(&plan["planId"]).into());
    confirm_input.answers = array(&plan["questions"])
        .iter()
        .map(|q| {
            (
                text(&q["id"]).into(),
                AnswerInput::One(first_label(q).into()),
            )
        })
        .collect();
    confirm_input
        .answers
        .insert("route:44:2".into(), AnswerInput::One("/technology".into()));
    let confirmed = confirm(&confirm_input, &fixture.policy()).unwrap();
    let check = &confirmed["linkLabelCheck"];
    assert_eq!(check["labels"][0]["state"], "route-bound");
    assert_eq!(check["labels"][0]["resolved"]["url"], "/technology");
    assert_eq!(check["unresolved"], 0);
    let plan = read_plan(fixture.root()).unwrap().unwrap();
    assert_eq!(plan["frameInventory"]["frames"][1]["route"], "/technology");
    assert_eq!(
        status_report(&plan, &evidence(), None)["linkLabelCheck"],
        *check
    );
}

#[tokio::test]
async fn prototype_links_follow_the_selected_frame_when_a_group_is_split() {
    let fixture = Fixture::new();
    let mut home = screen("45:1", "Landing", 1440.0);
    home.links = vec![
        PlanLinkInput {
            label: "Pricing".into(),
            destination_node_id: Some("45:3".into()),
            ..PlanLinkInput::default()
        },
        PlanLinkInput {
            label: "Pricing".into(),
            ..PlanLinkInput::default()
        },
        PlanLinkInput {
            label: "Unknown".into(),
            ..PlanLinkInput::default()
        },
    ];
    let draft_input = input(&fixture, "draft");
    draft_with(
        &draft_input,
        &[
            home,
            screen("45:2", "Pricing", 1440.0),
            screen("45:3", "Pricing", 390.0),
        ],
        &[],
        json!({}),
        &fixture.policy(),
    )
    .await
    .unwrap();
    let plan = read_plan(fixture.root()).unwrap().unwrap();
    let mut confirm_input = input(&fixture, "confirm");
    confirm_input.plan_id = Some(text(&plan["planId"]).into());
    confirm_input.answers = array(&plan["questions"])
        .iter()
        .map(|q| {
            (
                text(&q["id"]).into(),
                AnswerInput::One(first_label(q).into()),
            )
        })
        .collect();
    confirm_input.answers.insert(
        "group:45:2".into(),
        AnswerInput::One("/desktop, /mobile".into()),
    );
    let confirmed = confirm(&confirm_input, &fixture.policy()).unwrap();
    assert_eq!(
        confirmed["linkLabelCheck"]["labels"][0]["resolved"]["url"],
        "/mobile"
    );
    assert_eq!(
        confirmed["linkLabelCheck"]["labels"][2]["state"],
        "left-unlinked"
    );
    assert!(confirmed["linkLabelCheck"]["labels"][1]["resolved"]["url"].is_null());
    assert_eq!(confirmed["linkLabelCheck"]["unresolved"], 1);
}

#[test]
fn page_fields_reach_inventory_and_screens() {
    let mut home = screen("50:1", "Home", 1440.0);
    home.page_id = Some("0:1".into());
    home.page_name = Some("Final".into());
    let plan = draft_plan(&[home], &BTreeMap::new(), &evidence(), json!({}), None).unwrap();
    assert_eq!(plan["frameInventory"]["frames"][0]["pageId"], "0:1");
    assert_eq!(plan["frameInventory"]["frames"][0]["pageName"], "Final");
    assert_eq!(plan["screens"][0]["frames"][0]["pageName"], "Final");
    assert_eq!(distinct_pages(array(&plan["screens"][0]["frames"]))[0]["id"], "0:1");
}

#[test]
fn a_frame_on_a_trash_page_is_excluded() {
    let mut old = screen("51:1", "Home", 1440.0);
    old.page_name = Some("Trash".into());
    let mut live = screen("51:2", "About", 1440.0);
    live.page_name = Some("Final".into());
    let plan = draft_plan(&[old, live], &BTreeMap::new(), &evidence(), json!({}), None).unwrap();
    let excluded = array(&plan["excluded"]);
    assert_eq!(excluded.len(), 1, "{excluded:?}");
    assert_eq!(excluded[0]["nodeId"], "51:1");
    assert!(text(&excluded[0]["reason"]).contains("page \"Trash\""));
    assert_eq!(excluded[0]["pageName"], "Trash");
}

#[test]
fn status_warns_when_an_assumption_names_an_unmapped_frame() {
    let mut plan = confirmed_plan();
    plan["screens"] = json!([{"id": "10:9", "frames": [{"nodeId": "10:9"}]}]);
    plan["assumptions"] = json!([{"about": "10:9", "decision": "use this frame instead"}]);
    let report = status_report(&plan, &evidence(), None);
    assert_eq!(array(&report["warnings"]).len(), 1);
    assert!(text(&report["summary"]).contains("WARNING"));
    let clean = status_report(&confirmed_plan(), &evidence(), None);
    assert!(array(&clean["warnings"]).is_empty());
}

#[tokio::test]
async fn confirmed_routes_carry_figma_pages() {
    let fixture = Fixture::new();
    let mut pricing = screen("52:1", "Pricing", 1440.0);
    pricing.page_id = Some("0:7".into());
    pricing.page_name = Some("Final".into());
    let mut confirm_input = drafted(&fixture, &[pricing]).await;
    confirm_input.answers = answers(&[("route:52:1", "/pricing (Recommended)")]);
    let confirmed = confirm(&confirm_input, &fixture.policy()).unwrap();
    assert_eq!(confirmed["routes"][0]["pages"][0]["name"], "Final");
    assert!(text(&confirmed["markdownTable"]).contains("Figma page"));
    let plan = read_plan(fixture.root()).unwrap().unwrap();
    let report = status_report(&plan, &evidence(), None);
    assert_eq!(report["rows"][0]["figmaPages"][0]["id"], "0:7");
}
#[test]
fn device_only_names_do_not_group_across_sections() {
    let plan = draft_plan(
        &[
            sectioned("60:1", "Desktop", 1440.0, "Home"),
            sectioned("60:2", "Mobile", 360.0, "About"),
        ],
        &BTreeMap::new(),
        &evidence(),
        json!({}),
        None,
    )
    .unwrap();
    assert_eq!(array(&plan["screens"]).len(), 2);
    assert!(!question_ids(&plan).iter().any(|id| id.starts_with("sections:")));
}

#[test]
fn device_only_frames_take_their_screen_name_from_the_section() {
    let plan = draft_plan(
        &[
            sectioned("62:1", "Desktop", 1440.0, "Main"),
            sectioned("62:2", "Mobile", 360.0, "Main"),
            sectioned("62:3", "Desktop", 1440.0, "About"),
            sectioned("62:4", "Mobile", 360.0, "About"),
        ],
        &BTreeMap::new(),
        &evidence(),
        json!({}),
        None,
    )
    .unwrap();
    assert_eq!(array(&plan["screens"]).len(), 2);
    assert!(!question_ids(&plan).iter().any(|id| id.starts_with("sections:")));
}

#[test]
fn a_navigation_target_settles_the_route_without_a_question() {
    let targets = BTreeMap::from([("73:8589".to_owned(), "/ko/technology/patents".to_owned())]);
    let plan = draft_plan(
        &[screen("73:8589", "Patents", 1440.0)],
        &targets,
        &evidence(),
        json!({}),
        None,
    )
    .unwrap();
    assert!(
        !question_ids(&plan).iter().any(|id| id.starts_with("route:")),
        "{:?}",
        question_ids(&plan)
    );
}

#[test]
fn a_truncated_inventory_names_the_limit_and_the_next_step() {
    let error = draft_plan_with(
        &[screen("61:1", "Home", 1440.0)],
        &[],
        &[],
        &BTreeMap::new(),
        &evidence(),
        json!({"truncation": {"candidates": true}}),
        None,
    )
    .unwrap_err();
    let message = format!("{error:?}");
    assert!(message.contains("candidates"), "{message}");
    assert!(message.contains("devup_figma_search"), "{message}");
    assert!(message.contains("Sections seen so far"), "{message}");
}
#[test]
fn draft_reports_route_conflicts() {
    let targets = BTreeMap::from([
        ("70:1".to_owned(), "/shared".to_owned()),
        ("70:2".to_owned(), "/shared".to_owned()),
    ]);
    let plan = draft_plan(
        &[screen("70:1", "Alpha", 1440.0), screen("70:2", "Beta", 1440.0)],
        &targets,
        &evidence(),
        json!({}),
        None,
    )
    .unwrap();
    let conflicts = array(&plan["routeConflicts"]);
    assert_eq!(conflicts.len(), 1, "{conflicts:?}");
    assert_eq!(conflicts[0]["url"], "/shared");
    assert_eq!(array(&conflicts[0]["screens"]).len(), 2);
}

#[tokio::test]
async fn confirm_reports_every_colliding_pair() {
    let fixture = Fixture::new();
    let screens = [
        screen("71:1", "Alpha", 1440.0),
        screen("71:2", "Beta", 1440.0),
        screen("71:3", "Gamma", 1440.0),
        screen("71:4", "Delta", 1440.0),
    ];
    let mut confirm_input = drafted(&fixture, &screens).await;
    confirm_input.answers = answers(&[
        ("route:71:1", "/x"),
        ("route:71:2", "/x"),
        ("route:71:3", "/y"),
        ("route:71:4", "/y"),
    ]);
    let error = confirm(&confirm_input, &fixture.policy()).unwrap_err();
    assert_eq!(array(&error.details["conflicts"]).len(), 2, "{:?}", error.details);
    assert_eq!(error.details["retryAnswers"].as_object().unwrap().len(), 2);
}
#[tokio::test]
async fn confirm_returns_a_work_queue_for_every_url() {
    let fixture = Fixture::new();
    let screens = [
        screen("80:1", "Alpha", 1440.0),
        screen("80:2", "Beta", 1440.0),
    ];
    let mut confirm_input = drafted(&fixture, &screens).await;
    confirm_input.answers = answers(&[("route:80:1", "/alpha"), ("route:80:2", "/beta")]);
    let confirmed = confirm(&confirm_input, &fixture.policy()).unwrap();
    let queue = array(&confirmed["workQueue"]);
    assert_eq!(queue.len(), 2, "{queue:?}");
    assert_eq!(queue[0]["do"], "build");
    assert_eq!(array(&confirmed["nextAction"]["buildAll"]).len(), 2);
}
#[tokio::test]
async fn confirmed_page_defaults_and_status_notes_other_pages() {
    let fixture = Fixture::new();
    let mut a = screen("90:1", "Alpha", 1440.0);
    a.page_id = Some("0:1".into());
    a.page_name = Some("Final".into());
    let mut b = screen("90:2", "Beta", 1440.0);
    b.page_id = Some("0:1".into());
    b.page_name = Some("Final".into());
    let mut c = screen("90:3", "Gamma", 1440.0);
    c.page_id = Some("0:9".into());
    c.page_name = Some("Scratch".into());
    let mut confirm_input = drafted(&fixture, &[a, b, c]).await;
    confirm_input.answers = answers(&[("route:90:1", "/alpha"), ("route:90:2", "/beta"), ("route:90:3", "/gamma")]);
    let confirmed = confirm(&confirm_input, &fixture.policy()).unwrap();
    assert_eq!(confirmed["confirmedPage"]["name"], "Final");
    let plan = read_plan(fixture.root()).unwrap().unwrap();
    let report = status_report(&plan, &evidence(), None);
    let warnings = array(&report["warnings"]);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(text(&warnings[0]).contains("90:3"));
}
#[tokio::test]
async fn route_question_offers_state_on_a_similar_page() {
    let fixture = Fixture::new();
    let screens = [
        screen("95:1", "Settings", 1440.0),
        screen("95:2", "Settings Edit", 1440.0),
    ];
    let mut confirm_input = drafted(&fixture, &screens).await;
    confirm_input.answers = answers(&[
        ("route:95:1", "/settings"),
        ("route:95:2", "State on Settings"),
    ]);
    let confirmed = confirm(&confirm_input, &fixture.policy()).unwrap();
    let routes = array(&confirmed["routes"]);
    assert!(
        routes.iter().any(|route| route["implementation"] == "state"),
        "{routes:?}"
    );
}
#[test]
fn r14_navigation_targets_are_routes_not_route_questions() {
    let targets = BTreeMap::from([
        ("73:1".to_owned(), "/ko/technology/patents".to_owned()),
        ("73:3".to_owned(), "/ko/news/company".to_owned()),
    ]);
    let plan = draft_plan(
        &[
            sectioned("73:1", "Patents", 1440.0, "Tech"),
            sectioned("73:3", "Company news", 1440.0, "News"),
        ],
        &targets,
        &evidence(),
        json!({}),
        None,
    )
    .unwrap();
    let rows = array(&plan["screens"]);
    assert_eq!(rows[0]["proposedRoute"], "/ko/technology/patents");
    assert_eq!(rows[0]["routeEvidence"], "navigationTargets[73:1]");
    assert_eq!(rows[1]["proposedRoute"], "/ko/news/company");
    let ids = question_ids(&plan);
    assert!(!ids.iter().any(|id| id.starts_with("route:")), "{ids:?}");
}

#[test]
fn r14_explicit_routes_do_not_offer_an_occupied_home_route() {
    let targets = BTreeMap::from([
        ("114:1".into(), "/".into()),
        ("114:2".into(), "/ko/technology/patents".into()),
        ("114:3".into(), "/ko/news/company".into()),
    ]);
    let plan = draft_plan(
        &[
            sectioned("114:1", "Main", 1440.0, "Main"),
            sectioned("114:2", "Patents", 1440.0, "Technology"),
            sectioned("114:3", "Company news", 1440.0, "News"),
        ],
        &targets,
        &evidence(),
        json!({}),
        None,
    ).unwrap();
    for row in array(&plan["screens"]) {
        let node = text(&row["id"]);
        assert_eq!(row["proposedRoute"], targets[node]);
        assert!(text(&row["routeEvidence"]).contains("navigationTargets"));
        assert!(!question_ids(&plan).contains(&format!("route:{node}")));
    }
}

#[test]
fn r15_six_sections_with_device_names_remain_six_screens() {
    let screens = ["Main", "Company", "Vision", "Location", "Notice", "Contact"]
        .iter().enumerate().flat_map(|(index, section)| {
            [sectioned(&format!("115:{}", index * 2), "Desktop", 1440.0, section),
             sectioned(&format!("115:{}", index * 2 + 1), "Mobile", 360.0, section)]
        }).collect::<Vec<_>>();
    let plan = draft_plan(&screens, &BTreeMap::new(), &evidence(), json!({}), None).unwrap();
    assert_eq!(array(&plan["screens"]).len(), 6);
    assert!(!question_ids(&plan).iter().any(|id| id.starts_with("sections:")));
    for row in array(&plan["screens"]) {
        assert_eq!(array(&row["frames"]).len(), 2);
        assert_eq!(row["drawnWidths"], json!([1440, 360]));
    }
}
