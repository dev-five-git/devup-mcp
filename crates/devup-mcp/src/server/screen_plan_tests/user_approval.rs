// The tool confirms a screen plan only with the user's receipt, and only for
// the URLs the user saw.

fn receipt(via: &str, answer: &str, urls: &[&str]) -> UserApprovalInput {
    UserApprovalInput {
        via: via.into(),
        user_answer: answer.into(),
        approved_urls: urls.iter().map(|url| (*url).to_owned()).collect(),
    }
}

#[tokio::test]
async fn the_tool_confirm_refuses_without_a_user_receipt_and_writes_nothing() {
    let fixture = Fixture::new();
    let mut input = drafted(&fixture, &main_widths()).await;
    input.accept_recommended = true;
    let before = std::fs::read_to_string(fixture.0.join(PLAN_FILE)).unwrap();
    let error = confirm_approved(&input, &fixture.policy()).unwrap_err();
    assert!(error.message.contains("needs userApproval"), "{}", error.message);
    assert_eq!(error.details["approvalRequired"], true);
    assert!(error.details["markdownTable"].as_str().is_some_and(|table| table.contains('/')));
    assert!(text(&error.details["how"]).contains("question tool"));
    assert_eq!(std::fs::read_to_string(fixture.0.join(PLAN_FILE)).unwrap(), before);
    // A placeholder or an unknown channel is not an answer either.
    for bad in [receipt("question-tool", "<the label the user chose, verbatim>", &["/"]), receipt("assistant", "Accept", &["/"]), receipt("question-tool", "  ", &["/"])] {
        input.user_approval = Some(bad);
        assert_eq!(confirm_approved(&input, &fixture.policy()).unwrap_err().details["approvalRequired"], true);
    }
    assert_eq!(read_plan(fixture.root()).unwrap().unwrap()["status"], "draft");
}

#[tokio::test]
async fn approved_urls_must_be_exactly_the_urls_confirm_builds() {
    let fixture = Fixture::new();
    let mut input = drafted(&fixture, &main_widths()).await;
    input.accept_recommended = true;
    input.user_approval = Some(receipt("question-tool", "Accept the table (Recommended)", &["/", "/pricing"]));
    let error = confirm_approved(&input, &fixture.policy()).unwrap_err();
    assert!(error.message.contains("does not match"), "{}", error.message);
    assert_eq!(error.details["notInPlan"], json!(["/pricing"]));
    assert_eq!(error.details["expectedUrls"], json!(["/"]));
    assert_eq!(read_plan(fixture.root()).unwrap().unwrap()["status"], "draft");

    // The URLs the user saw: confirmed, and the receipt is recorded.
    input.user_approval = Some(receipt("question-tool", "Accept the table (Recommended)", &["/"]));
    let confirmed = confirm_approved(&input, &fixture.policy()).unwrap();
    assert_eq!(confirmed["status"], "confirmed");
    assert_eq!(confirmed["userApproval"]["reviewedByUser"], true);
    assert!(text(&confirmed["approval"]).starts_with("approved by the user (question-tool)"));
    let saved = read_plan(fixture.root()).unwrap().unwrap();
    assert_eq!(saved["userApproval"]["userAnswer"], "Accept the table (Recommended)");
    assert_eq!(saved["userApproval"]["approvedUrls"], json!(["/"]));
}

#[tokio::test]
async fn a_delegated_confirm_is_reported_as_not_reviewed_by_the_user() {
    let fixture = Fixture::new();
    let mut input = drafted(&fixture, &main_widths()).await;
    input.accept_recommended = true;
    input.user_approval = Some(receipt("delegated", "알아서 진행해 줘", &["/"]));
    confirm_approved(&input, &fixture.policy()).unwrap();
    let saved = read_plan(fixture.root()).unwrap().unwrap();
    assert_eq!(saved["userApproval"]["reviewedByUser"], false);
    assert!(approval_line(&saved["userApproval"]).starts_with("NOT reviewed by the user"));
    // A plan confirmed outside the tool carries no receipt, and says so.
    assert!(approval_line(&approval_record(None)).contains("no approval receipt"));
}

#[tokio::test]
async fn the_draft_tells_the_agent_to_wait_for_the_user_before_confirm() {
    let fixture = Fixture::new();
    let drafted = draft(&input(&fixture, "draft"), &main_widths(), json!({}), &fixture.policy())
        .await
        .unwrap();
    let next = &drafted["nextAction"];
    assert_eq!(next["mustAskUser"], true);
    assert_eq!(next["arguments"]["userApproval"]["via"], "question-tool");
    assert!(next["arguments"]["userApproval"]["approvedUrls"].is_array());
}

#[test]
fn a_confirmed_plan_without_the_users_approval_writes_no_page_until_it_is_approved() {
    let fixture = Fixture::new();
    let mut plan = confirmed_plan();
    plan["userApproval"] = Value::Null;
    write_plan_file(&fixture, &plan);
    let request = ExportRequest {
        project_root: fixture.root(),
        url: None,
        frames: vec!["10:2".into()],
        all_screens: false,
        scaffold_write: true,
        scaffold_route: Some(""),
        output_paths: Vec::new(),
    };
    let Gate::Blocked(response) = export_gate(&request).unwrap() else {
        panic!("a plan the user never approved must not write a page");
    };
    assert_eq!(response["status"], "approval_required");
    assert_eq!(response["written"], false);
    assert_eq!(response["approvalRequired"]["expectedUrls"], json!(["/", "/about"]));
    assert_eq!(response["nextAction"]["mustAskUser"], true);
    // A preview that writes nothing is never blocked.
    let preview = ExportRequest { scaffold_write: false, scaffold_route: None, ..request };
    assert!(matches!(export_gate(&preview).unwrap(), Gate::Open));

    // The user approves the existing plan; the receipt is recorded and pages open.
    let mut approve = input(&fixture, "confirm");
    approve.plan_id = Some("sp-test".into());
    approve.user_approval = Some(receipt("question-tool", "승인", &["/about"]));
    assert!(confirm_approved(&approve, &fixture.policy()).unwrap_err().message.contains("does not match"));
    approve.user_approval = Some(receipt("question-tool", "승인", &["/", "/about"]));
    let approved = confirm_approved(&approve, &fixture.policy()).unwrap();
    assert_eq!(approved["alreadyConfirmed"], true);
    assert_eq!(approved["userApproval"]["userAnswer"], "승인");
    let request = ExportRequest { scaffold_write: true, scaffold_route: Some(""), ..preview };
    assert!(matches!(export_gate(&request).unwrap(), Gate::Route(_)));
}

#[tokio::test]
async fn the_procedure_names_one_step_and_waits_for_the_user_at_the_question() {
    let fixture = Fixture::new();
    let none = procedure(fixture.root());
    assert_eq!(none["step"], "1/5 draft");
    assert_eq!(none["next"]["arguments"]["action"], "draft");
    assert_eq!(none["waitForUser"], false);

    let mut input = drafted(&fixture, &main_widths()).await;
    let asking = procedure(fixture.root());
    assert_eq!(asking["step"], "2/5 ask-user");
    assert_eq!(asking["waitForUser"], true);
    assert_eq!(asking["next"]["tool"], "question-tool");
    assert!(array(&asking["forbiddenNow"]).iter().any(|rule| text(rule).contains("same turn")));

    input.accept_recommended = true;
    input.user_approval = Some(receipt("question-tool", "Accept", &["/"]));
    confirm_approved(&input, &fixture.policy()).unwrap();
    let building = procedure(fixture.root());
    assert_eq!(building["step"], "4/5 implement");
    assert_eq!(building["waitForUser"], false);

    let mut plan = read_plan(fixture.root()).unwrap().unwrap();
    plan["userApproval"] = Value::Null;
    write_plan_file(&fixture, &plan);
    let approve = procedure(fixture.root());
    assert_eq!(approve["step"], "3/5 confirm-with-approval");
    assert_eq!(approve["next"]["arguments"]["userApproval"]["approvedUrls"], json!(["/"]));
}
