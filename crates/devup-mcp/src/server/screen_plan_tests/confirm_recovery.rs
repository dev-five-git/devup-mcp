#[tokio::test]
async fn a_superseded_plan_id_names_the_current_plan_and_the_next_call() {
    let fixture = Fixture::new();
    let old = drafted(&fixture, &[screen("1:2", "Pricing", 1440.0)]).await;
    let current = drafted(&fixture, &[screen("1:2", "Pricing", 1440.0), screen("1:3", "Careers", 1440.0)]).await;
    assert_ne!(old.plan_id, current.plan_id);
    let saved = read_plan(fixture.root()).unwrap().unwrap();
    assert_eq!(saved["previousPlanIds"], json!([old.plan_id]));
    let mut stale = old;
    stale.accept_recommended = true;
    let error = confirm(&stale, &fixture.policy()).unwrap_err();
    assert!(error.message.contains("superseded"), "{}", error.message);
    assert_eq!(error.details["supersededBy"], json!(current.plan_id));
    assert_eq!(error.details["existingPlans"][0]["status"], "draft");
    assert_eq!(error.details["nextAction"]["arguments"]["action"], "confirm");
    assert_eq!(error.details["nextAction"]["arguments"]["planId"], json!(current.plan_id));
}

#[tokio::test]
async fn confirming_a_confirmed_plan_again_changes_nothing() {
    let fixture = Fixture::new();
    let mut confirm_input = drafted(&fixture, &main_widths()).await;
    confirm_input.accept_recommended = true;
    confirm(&confirm_input, &fixture.policy()).unwrap();
    let before = std::fs::read_to_string(fixture.0.join(PLAN_FILE)).unwrap();
    let again = confirm(&confirm_input, &fixture.policy()).unwrap();
    assert_eq!(again["alreadyConfirmed"], true);
    assert_eq!(again["routes"][0]["url"], "/");
    assert_eq!(again["nextAction"]["arguments"]["action"], "status");
    assert_eq!(std::fs::read_to_string(fixture.0.join(PLAN_FILE)).unwrap(), before);
}

#[test]
fn a_state_host_is_never_more_specific_than_its_state() {
    let plan = draft_plan(
        &[screen("3:1", "Pricing", 1440.0), screen("3:2", "Pricing detail", 1440.0)],
        &BTreeMap::new(),
        &evidence(),
        json!({}),
        None,
    )
    .unwrap();
    let labels = |id: &str| {
        array(&find(&plan, id)["options"])
            .iter()
            .map(|option| text(&option["label"]).to_owned())
            .collect::<Vec<_>>()
    };
    assert!(labels("route:3:2").iter().any(|label| label.starts_with("State on Pricing")), "{:?}", labels("route:3:2"));
    assert!(!labels("route:3:1").iter().any(|label| label.starts_with("State on")), "{:?}", labels("route:3:1"));
}
