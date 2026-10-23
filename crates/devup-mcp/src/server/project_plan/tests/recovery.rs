use super::*;

#[tokio::test]
async fn confirm_refuses_to_drop_existing_columns_without_writing_models() {
    let dir = TestDir::new();
    let (request, policy, draft) = drafted(dir.path(), true).await;
    let mut post = array(&draft["plan"]["entities"]).iter().find(|entity| entity["name"] == "post").unwrap().clone();
    post["columns"].as_array_mut().unwrap().push(json!({"name":"category","type":"text","nullable":false}));
    std::fs::create_dir_all(dir.path().join("models")).unwrap();
    let path = dir.path().join("models/post.json");
    let original = serde_json::to_vec(&post).unwrap();
    std::fs::write(&path, &original).unwrap();
    let error = run(&request, &policy).await.unwrap_err();
    assert_eq!(error.details["removedColumns"], json!([{"table":"post","column":"category"}]));
    assert_eq!(error.details["decision"]["destructive"], true);
    assert_eq!(std::fs::read(path).unwrap(), original);
    assert!(!dir.path().join("models/user.json").exists());
    assert_eq!(read(&request.project_root).unwrap()["state"], "draft");
}

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
