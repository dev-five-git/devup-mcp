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
