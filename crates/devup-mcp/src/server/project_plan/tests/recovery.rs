use super::*;

#[test]
fn empty_prefix_header_preserves_thirty_character_capacity() {
    let name = "123456789012345678901234567890";
    assert_eq!(screen_plan::header("", name), name);
    assert_eq!(screen_plan::header("Page", "Home"), "Page: Home");
}

#[tokio::test]
async fn stale_status_id_returns_current_plan_and_executable_recovery() {
    let dir = TestDir::new();
    let (mut request, policy, draft) = drafted(dir.path(), false).await;
    request.action = "status".into();
    request.plan_id = Some("stale".into());
    let error = run(&request, &policy).await.unwrap_err();
    assert_eq!(error.details["planId"], draft["planId"]);
    assert_eq!(error.details["state"], "draft");
    let recovery: ProjectPlanInput = serde_json::from_value(error.details["nextAction"]["arguments"].clone()).unwrap();
    assert_eq!(run(&recovery, &policy).await.unwrap()["planId"], draft["planId"]);
}
